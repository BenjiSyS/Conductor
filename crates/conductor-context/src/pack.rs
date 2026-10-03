//! The context compression pipeline.
//!
//! Priority order: current task → changed files → directly related files and
//! symbols → project rules → recent decisions → older history only when
//! needed. Each stage records what it did so the Context Inspector can show
//! exactly what will be sent, what was omitted, and why.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::compact::{self, Turn};
use crate::index::RepoIndex;
use crate::logs::{self, LogOptions};
use crate::relevance;
use crate::tokens;
use conductor_security::{injection, secrets};

#[derive(Debug, Clone, Default)]
pub struct PackRequest {
    pub task: String,
    pub conversation: Vec<Turn>,
    pub changed_files: Vec<String>,
    pub always_include: Vec<String>,
    /// Total soft budget for the assembled context.
    pub budget_tokens: usize,
    pub project_rules: Vec<String>,
    pub decisions: Vec<String>,
    /// (tool name, raw output)
    pub tool_outputs: Vec<(String, String)>,
    pub include_structure: bool,
    pub secret_scanning: bool,
    /// path -> content hash already sent to this provider earlier in the
    /// session (Smart Context Cache reuse across turns).
    pub previously_sent: HashMap<String, String>,
    /// Allow sending sensitive files (explicit user override).
    pub allow_sensitive: bool,
    pub max_files: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Rules,
    Decisions,
    Structure,
    File,
    Snippet,
    Outline,
    Summary,
    History,
    ToolOutput,
    Reference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextItem {
    pub kind: ItemKind,
    pub source: String,
    pub reason: String,
    pub tokens: usize,
    pub content: String,
    /// True when content is a summary/snippet rather than the original.
    pub reduced: bool,
    /// True when this item references content sent earlier instead of
    /// resending it.
    pub cached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Omitted {
    pub source: String,
    pub reason: String,
    pub tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageReport {
    pub stage: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPack {
    pub items: Vec<ContextItem>,
    pub omitted: Vec<Omitted>,
    pub stages: Vec<StageReport>,
    pub budget_tokens: usize,
    /// Estimated tokens of the assembled context.
    pub total_tokens: usize,
    /// Estimated tokens a naive approach would send (all candidate files in
    /// full + full conversation + raw tool output).
    pub naive_tokens: usize,
    pub cache_reused_tokens: usize,
    pub duplicates_removed: usize,
    pub redactions: usize,
    pub injection_warnings: usize,
    pub compression_active: bool,
    /// Always true: every number here is an estimate.
    pub estimated: bool,
}

impl ContextPack {
    pub fn compression_ratio(&self) -> f32 {
        if self.naive_tokens == 0 {
            1.0
        } else {
            self.total_tokens as f32 / self.naive_tokens as f32
        }
    }

    /// Estimated tokens avoided versus the naive approach.
    pub fn tokens_avoided(&self) -> usize {
        self.naive_tokens.saturating_sub(self.total_tokens)
    }

    /// Render the provider-ready context block.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let any_untrusted = self.items.iter().any(|i| {
            matches!(
                i.kind,
                ItemKind::File | ItemKind::Snippet | ItemKind::ToolOutput
            )
        });
        if any_untrusted {
            out.push_str(injection::POLICY);
            out.push_str("\n\n");
        }
        for it in &self.items {
            match it.kind {
                ItemKind::Rules => out.push_str(&format!("## Project rules\n{}\n\n", it.content)),
                ItemKind::Decisions => out.push_str(&format!("## Decisions\n{}\n\n", it.content)),
                ItemKind::Structure => {
                    out.push_str(&format!("## Project structure\n{}\n\n", it.content))
                }
                ItemKind::Summary | ItemKind::History => {
                    out.push_str(&format!("{}\n\n", it.content))
                }
                ItemKind::Reference => out.push_str(&format!("- {} — {}\n", it.source, it.content)),
                _ => {
                    out.push_str(&it.content);
                    out.push_str("\n\n");
                }
            }
        }
        out.trim_end().to_string()
    }
}

const DEFAULT_BUDGET: usize = 24_000;

/// (path, score, reasons, matched symbols)
type Candidate = (String, f32, Vec<String>, Vec<(String, usize)>);

pub fn build(index: &RepoIndex, req: &PackRequest) -> ContextPack {
    let budget = if req.budget_tokens == 0 {
        DEFAULT_BUDGET
    } else {
        req.budget_tokens
    };
    let max_files = if req.max_files == 0 {
        12
    } else {
        req.max_files
    };
    let mut items: Vec<ContextItem> = Vec::new();
    let mut omitted: Vec<Omitted> = Vec::new();
    let mut stages: Vec<StageReport> = Vec::new();
    let mut redactions = 0usize;
    let mut injection_warnings = 0usize;
    let mut naive = tokens::estimate(&req.task);
    let mut used = tokens::estimate(&req.task);
    let mut cache_reused = 0usize;

    // 1. Repository filtering already happened in the index.
    stages.push(StageReport {
        stage: "repository filtering".into(),
        detail: format!(
            "{} files indexed (gitignore + vendor/build dirs excluded, {} large and {} binary skipped)",
            index.len(),
            index.last_stats.skipped_large,
            index.last_stats.skipped_binary
        ),
    });

    // Project rules and decisions are small and authoritative: always first.
    if !req.project_rules.is_empty() {
        let content = dedupe_lines(&req.project_rules).join("\n");
        let t = tokens::estimate(&content);
        naive += t;
        used += t;
        items.push(item(
            ItemKind::Rules,
            "project rules",
            "authoritative project instructions",
            content,
            false,
        ));
    }
    if !req.decisions.is_empty() {
        let content = dedupe_lines(&req.decisions)
            .iter()
            .map(|d| format!("- {d}"))
            .collect::<Vec<_>>()
            .join("\n");
        let t = tokens::estimate(&content);
        naive += t;
        used += t;
        items.push(item(
            ItemKind::Decisions,
            "decision memory",
            "user decisions — do not re-ask",
            content,
            false,
        ));
    }

    // 7. Conversation compaction (budgeted to ~25% of the total).
    let convo_budget = budget / 4;
    let compacted = compact::compact(&req.conversation, 6, convo_budget);
    naive += compacted.tokens_before;
    if compacted.compacted_turns > 0 {
        stages.push(StageReport {
            stage: "conversation compaction".into(),
            detail: format!(
                "{} older turn(s) summarised: {} → {} tokens (est.)",
                compacted.compacted_turns, compacted.tokens_before, compacted.tokens_after
            ),
        });
        let t = tokens::estimate(&compacted.summary);
        used += t;
        items.push(item(
            ItemKind::Summary,
            "conversation summary",
            "older turns compacted",
            compacted.summary.clone(),
            true,
        ));
    }
    // Recent turns themselves are sent as messages by the caller; we account
    // for their size so the file budget is realistic.
    used += compacted
        .kept
        .iter()
        .map(|t| tokens::estimate(&t.text))
        .sum::<usize>();

    // 8. Tool output: dedupe + log reduction.
    let (tool_outputs, dupes) = compact::dedupe(req.tool_outputs.clone(), |(_, t)| t.as_str());
    if dupes > 0 {
        stages.push(StageReport {
            stage: "duplicate removal".into(),
            detail: format!("{dupes} duplicate tool output(s) dropped"),
        });
    }
    for (name, raw) in &tool_outputs {
        let raw_t = tokens::estimate(raw);
        naive += raw_t;
        let reduced = logs::reduce(raw, &LogOptions::default());
        let mut content = reduced.text.clone();
        if req.secret_scanning {
            let r = secrets::redact(&content);
            redactions += r.findings.len();
            content = r.text;
        }
        injection_warnings += injection::detect(&content).len();
        let body = injection::fence(injection::Source::ToolOutput, name, &content);
        let t = tokens::estimate(&body);
        used += t;
        let was_reduced = reduced.kept_lines < reduced.original_lines;
        if was_reduced {
            stages.push(StageReport {
                stage: "build/log reduction".into(),
                detail: format!(
                    "{name}: {} → {} lines",
                    reduced.original_lines, reduced.kept_lines
                ),
            });
        }
        items.push(item(
            ItemKind::ToolOutput,
            name,
            if was_reduced {
                "tool output (reduced)"
            } else {
                "tool output"
            },
            body,
            was_reduced,
        ));
    }

    // 2-4. Relevance + changed-file priority + symbol relevance.
    let scored = relevance::score(index, &req.task, &req.changed_files);
    let mut candidates: Vec<Candidate> = scored
        .into_iter()
        .map(|s| (s.path, s.score, s.reasons, s.matched_symbols))
        .collect();
    // Always-include files go first.
    for p in req.always_include.iter().rev() {
        if index.get(p).is_some() {
            candidates.retain(|c| &c.0 != p);
            candidates.insert(
                0,
                (
                    p.clone(),
                    f32::MAX,
                    vec!["always included (conductor.toml)".into()],
                    vec![],
                ),
            );
        }
    }
    // Neighbour expansion: imports of the top hits get a smaller boost.
    let top: Vec<String> = candidates.iter().take(3).map(|c| c.0.clone()).collect();
    let present: HashSet<String> = candidates.iter().map(|c| c.0.clone()).collect();
    for t in &top {
        for n in index.neighbors(t) {
            if !present.contains(&n) {
                candidates.push((n, 3.0, vec![format!("imported by/imports {t}")], vec![]));
            }
        }
    }
    stages.push(StageReport {
        stage: "relevance scoring".into(),
        detail: format!(
            "{} candidate file(s) scored for this task",
            candidates.len()
        ),
    });

    let per_file_cap = (budget / 4).max(800);
    let mut included = 0usize;
    let mut seen_hashes: BTreeMap<String, String> = BTreeMap::new();
    for (path, score, reasons, matched) in candidates {
        let Some(entry) = index.get(&path) else {
            continue;
        };
        naive += entry.tokens;
        let reason = reasons.join("; ");
        if included >= max_files {
            omitted.push(Omitted {
                source: path,
                reason: "beyond file limit".into(),
                tokens: entry.tokens,
            });
            continue;
        }
        if score < 2.5 && included >= 3 {
            omitted.push(Omitted {
                source: path,
                reason: "low relevance".into(),
                tokens: entry.tokens,
            });
            continue;
        }
        if entry.sensitive && !req.allow_sensitive {
            omitted.push(Omitted {
                source: path,
                reason: "sensitive file blocked (override in Context Inspector)".into(),
                tokens: entry.tokens,
            });
            continue;
        }
        // Duplicate content (e.g. vendored copies).
        if let Some(other) = seen_hashes.get(&entry.hash) {
            omitted.push(Omitted {
                source: path,
                reason: format!("identical to {other}"),
                tokens: entry.tokens,
            });
            continue;
        }
        // 6. Stale-context removal / cache reuse.
        if req.previously_sent.get(&path) == Some(&entry.hash) {
            cache_reused += entry.tokens;
            let it = ContextItem {
                kind: ItemKind::Reference,
                source: path.clone(),
                reason: "unchanged since it was last sent".into(),
                tokens: 12,
                content: "unchanged since provided earlier in this conversation".into(),
                reduced: true,
                cached: true,
                hash: Some(entry.hash.clone()),
            };
            used += it.tokens;
            items.push(it);
            seen_hashes.insert(entry.hash.clone(), path);
            included += 1;
            continue;
        }
        let remaining = budget.saturating_sub(used);
        if remaining < 200 {
            omitted.push(Omitted {
                source: path,
                reason: "context budget reached".into(),
                tokens: entry.tokens,
            });
            continue;
        }
        let Some(text) = index.read(&path) else {
            continue;
        };
        let cap = per_file_cap.min(remaining);
        let (kind, mut body, reduced) = if entry.tokens <= cap {
            (ItemKind::File, text, false)
        } else if !matched.is_empty() {
            (ItemKind::Snippet, snippets(&text, &matched, cap), true)
        } else {
            (ItemKind::Outline, outline_view(entry, &text, cap), true)
        };
        if req.secret_scanning {
            let r = secrets::redact(&body);
            redactions += r.findings.len();
            body = r.text;
        }
        injection_warnings += injection::detect(&body).len();
        let fenced = injection::fence(
            injection::Source::RepositoryFile,
            &path,
            &format!(
                "{}{}\n{}",
                path,
                if reduced { " (excerpt)" } else { "" },
                body
            ),
        );
        let t = tokens::estimate(&fenced);
        if t > remaining {
            omitted.push(Omitted {
                source: path,
                reason: "context budget reached".into(),
                tokens: entry.tokens,
            });
            continue;
        }
        used += t;
        let mut it = item(kind, &path, &reason, fenced, reduced);
        it.hash = Some(entry.hash.clone());
        items.push(it);
        seen_hashes.insert(entry.hash.clone(), path);
        included += 1;
    }

    if req.include_structure && budget.saturating_sub(used) > 400 {
        let s = index.structure(40);
        let t = tokens::estimate(&s);
        used += t;
        items.push(item(
            ItemKind::Structure,
            "project structure",
            "directory overview instead of a full tree",
            s,
            true,
        ));
    }

    stages.push(StageReport {
        stage: "provider prompt construction".into(),
        detail: format!(
            "{} item(s), {} omitted, ~{} of {} budget tokens",
            items.len(),
            omitted.len(),
            used,
            budget
        ),
    });
    if redactions > 0 {
        stages.push(StageReport {
            stage: "secret scanning".into(),
            detail: format!("{redactions} likely secret(s) redacted"),
        });
    }
    let total_tokens: usize =
        items.iter().map(|i| i.tokens).sum::<usize>() + tokens::estimate(&req.task);
    ContextPack {
        compression_active: naive > total_tokens,
        items,
        omitted,
        stages,
        budget_tokens: budget,
        total_tokens,
        naive_tokens: naive.max(total_tokens),
        cache_reused_tokens: cache_reused,
        duplicates_removed: dupes,
        redactions,
        injection_warnings,
        estimated: true,
    }
}

fn item(kind: ItemKind, source: &str, reason: &str, content: String, reduced: bool) -> ContextItem {
    ContextItem {
        kind,
        source: source.to_string(),
        reason: reason.to_string(),
        tokens: tokens::estimate(&content),
        content,
        reduced,
        cached: false,
        hash: None,
    }
}

fn dedupe_lines(lines: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    lines
        .iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && seen.insert(l.to_lowercase()))
        .collect()
}

/// Windows of lines around matched symbol definitions.
fn snippets(text: &str, matched: &[(String, usize)], cap_tokens: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut ranges: Vec<(usize, usize)> = matched
        .iter()
        .map(|(_, line)| {
            let start = line.saturating_sub(4);
            (start, (start + 60).min(lines.len()))
        })
        .collect();
    ranges.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.0 <= last.1 => last.1 = last.1.max(r.1),
            _ => merged.push(r),
        }
    }
    let mut out = String::new();
    for (a, b) in merged {
        let chunk: String = lines[a..b]
            .iter()
            .enumerate()
            .map(|(i, l)| format!("{:>5} {}\n", a + i + 1, l))
            .collect();
        if tokens::estimate(&out) + tokens::estimate(&chunk) > cap_tokens {
            out.push_str("…\n");
            break;
        }
        out.push_str(&chunk);
        out.push_str("     …\n");
    }
    out
}

/// Outline + head of a large file when no specific symbol matched.
fn outline_view(entry: &crate::index::FileEntry, text: &str, cap_tokens: usize) -> String {
    let mut out = String::new();
    if !entry.summary.is_empty() {
        out.push_str(&format!("Summary: {}\n", entry.summary));
    }
    out.push_str("Symbols:\n");
    for s in entry.outline.symbols.iter().take(80) {
        out.push_str(&format!("  {:?} {} (line {})\n", s.kind, s.name, s.line));
    }
    out.push_str("Head:\n");
    for (i, l) in text.lines().take(60).enumerate() {
        let line = format!("{:>5} {}\n", i + 1, l);
        if tokens::estimate(&out) + tokens::estimate(&line) > cap_tokens {
            break;
        }
        out.push_str(&line);
    }
    out
}

/// Compress MCP/tool descriptions: first sentence, capped length. Full
/// descriptions remain available on demand.
pub fn compress_tool_description(desc: &str, max_chars: usize) -> String {
    let first = desc
        .split_inclusive(['.', '\n'])
        .next()
        .unwrap_or(desc)
        .trim();
    let mut s: String = first.chars().take(max_chars).collect();
    if first.chars().count() > max_chars {
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::IndexOptions;

    fn repo() -> (tempfile::TempDir, RepoIndex) {
        let d = tempfile::tempdir().unwrap();
        let w = |p: &str, t: &str| {
            let path = d.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, t).unwrap();
        };
        w(
            "src/auth.rs",
            "/// Auth\npub fn login(user: &str) -> bool { !user.is_empty() }\n",
        );
        w(
            "src/db.rs",
            "pub fn connect() {}\nconst KEY: &str = \"sk-proj-abcdefghijklmnopqrstuvwx1234\";\n",
        );
        let mut big = String::from("//! Huge renderer\n");
        for i in 0..3000 {
            big.push_str(&format!("fn helper_{i}() {{ let v = {i}; }}\n"));
        }
        big.push_str("pub fn render_scene() {}\n");
        w("src/render.rs", &big);
        w(".env", "SECRET_TOKEN=abcdef123456\n");
        w("docs/readme.md", "# Readme\nlogin docs\n");
        let mut i = RepoIndex::open(d.path(), None);
        i.refresh(&IndexOptions::default()).unwrap();
        (d, i)
    }

    #[test]
    fn selects_relevant_redacts_and_reports() {
        let (_d, idx) = repo();
        let req = PackRequest {
            task: "Why does login fail? Also check connect in db".into(),
            secret_scanning: true,
            budget_tokens: 6000,
            project_rules: vec!["Never use unwrap in runtime code".into()],
            decisions: vec!["Target Windows 10".into(), "target windows 10".into()],
            ..Default::default()
        };
        let p = build(&idx, &req);
        let srcs: Vec<_> = p.items.iter().map(|i| i.source.as_str()).collect();
        assert!(srcs.contains(&"src/auth.rs"), "{srcs:?}");
        assert!(srcs.contains(&"src/db.rs"));
        assert!(!srcs.contains(&".env"));
        assert!(p.redactions >= 1);
        let rendered = p.render();
        assert!(!rendered.contains("sk-proj-abcdefghijklmnopqrstuvwx1234"));
        assert!(rendered.contains("<<<UNTRUSTED"));
        assert!(rendered.contains("Never use unwrap"));
        // decisions deduped case-insensitively
        assert_eq!(rendered.matches("indows 10").count(), 1);
        assert!(p.estimated);
        assert!(p.total_tokens <= 6000 + 100);
    }

    #[test]
    fn large_file_becomes_snippet() {
        let (_d, idx) = repo();
        let req = PackRequest {
            task: "render_scene is slow".into(),
            budget_tokens: 4000,
            ..Default::default()
        };
        let p = build(&idx, &req);
        let r = p
            .items
            .iter()
            .find(|i| i.source == "src/render.rs")
            .unwrap();
        assert_eq!(r.kind, ItemKind::Snippet);
        assert!(r.content.contains("render_scene"));
        assert!(r.tokens < idx.get("src/render.rs").unwrap().tokens / 4);
        assert!(p.compression_active);
        assert!(p.tokens_avoided() > 0);
    }

    #[test]
    fn previously_sent_files_are_referenced_not_resent() {
        let (_d, idx) = repo();
        let hash = idx.get("src/auth.rs").unwrap().hash.clone();
        let mut prev = HashMap::new();
        prev.insert("src/auth.rs".to_string(), hash);
        let req = PackRequest {
            task: "login bug".into(),
            previously_sent: prev,
            ..Default::default()
        };
        let p = build(&idx, &req);
        let a = p.items.iter().find(|i| i.source == "src/auth.rs").unwrap();
        assert!(a.cached);
        assert_eq!(a.kind, ItemKind::Reference);
        assert!(p.cache_reused_tokens > 0);
    }

    #[test]
    fn tool_output_reduced_and_deduped() {
        let (_d, idx) = repo();
        let log = "ok line\n".repeat(2000) + "error: boom at src/auth.rs:2\n";
        let req = PackRequest {
            task: "fix build".into(),
            tool_outputs: vec![
                ("cargo build".into(), log.clone()),
                ("cargo build".into(), log),
            ],
            ..Default::default()
        };
        let p = build(&idx, &req);
        assert_eq!(p.duplicates_removed, 1);
        let t = p
            .items
            .iter()
            .find(|i| i.kind == ItemKind::ToolOutput)
            .unwrap();
        assert!(t.reduced);
        assert!(t.content.contains("error: boom"));
        assert!(p.stages.iter().any(|s| s.stage == "build/log reduction"));
    }

    #[test]
    fn tool_description_compression() {
        let d = "Reads a file from disk. Supports offsets and limits. Returns text.";
        assert_eq!(compress_tool_description(d, 100), "Reads a file from disk.");
        assert_eq!(compress_tool_description(d, 5), "Reads…");
    }
}
