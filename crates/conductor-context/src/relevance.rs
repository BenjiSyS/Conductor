//! Deterministic file relevance scoring.
//!
//! Signals, strongest first: explicit path mention, explicit symbol mention,
//! changed-file status, identifier word overlap with path / symbols /
//! summary. Every score carries human-readable reasons for the Context
//! Inspector.

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use crate::index::{FileEntry, RepoIndex};
use crate::symbols::words;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scored {
    pub path: String,
    pub score: f32,
    pub reasons: Vec<String>,
    /// Symbols explicitly matched (for snippet extraction).
    pub matched_symbols: Vec<(String, usize)>,
}

const STOP: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "from", "into", "please", "can", "you", "how",
    "what", "why", "when", "where", "make", "add", "fix", "use", "using", "should", "would",
    "could", "about", "all", "any", "are", "was", "were", "has", "have", "had", "not", "but",
    "our", "its", "it's", "then", "than", "also", "just", "like", "some", "more", "new", "get",
    "set", "let", "out", "file", "files", "code", "need", "want", "does", "work", "working",
    "there", "here", "them", "they", "will", "issue", "problem", "change", "update",
];

/// Extract normalised query terms from free text.
pub fn query_terms(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for raw in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if raw.len() < 3 {
            continue;
        }
        for w in words(raw) {
            if w.len() >= 3 && !STOP.contains(&w.as_str()) {
                out.insert(stem(&w));
            }
        }
    }
    out
}

/// Very small stemmer: plural/gerund/past suffixes only.
fn stem(w: &str) -> String {
    for suf in ["ing", "ers", "ies", "es", "ed", "er", "s"] {
        if w.len() > suf.len() + 3 {
            if let Some(s) = w.strip_suffix(suf) {
                return s.to_string();
            }
        }
    }
    w.to_string()
}

fn terms_of(text: &str) -> HashSet<String> {
    words(text).into_iter().map(|w| stem(&w)).collect()
}

/// Score all files in the index for a task description.
pub fn score(index: &RepoIndex, task: &str, changed: &[String]) -> Vec<Scored> {
    let terms = query_terms(task);
    let task_lower = task.to_lowercase().replace('\\', "/");
    // When most of the tree is "changed" (fresh repo, nothing committed) the
    // signal carries no information, so ignore it.
    let changed: HashSet<&str> = if changed.len() > 20 && changed.len() * 2 > index.len() {
        HashSet::new()
    } else {
        changed.iter().map(String::as_str).collect()
    };
    let mut out = Vec::new();
    for e in index.entries() {
        if let Some(s) = score_one(e, &terms, &task_lower, &changed) {
            out.push(s);
        }
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.path.cmp(&b.path)));
    out
}

fn score_one(
    e: &FileEntry,
    terms: &BTreeSet<String>,
    task_lower: &str,
    changed: &HashSet<&str>,
) -> Option<Scored> {
    let mut score = 0.0f32;
    let mut reasons = Vec::new();
    let mut matched_symbols = Vec::new();
    let path_lower = e.path.to_lowercase();
    let file_name = path_lower.rsplit('/').next().unwrap_or(&path_lower);

    if task_lower.contains(&path_lower) {
        score += 100.0;
        reasons.push("path mentioned in request".into());
    } else if file_name.len() > 4 && file_name.contains('.') && task_lower.contains(file_name) {
        score += 60.0;
        reasons.push("file name mentioned in request".into());
    }

    if changed.contains(e.path.as_str()) {
        score += 25.0;
        reasons.push("changed in working tree".into());
    }

    // Exact symbol mentions (identifier appears verbatim in the request).
    for s in &e.outline.symbols {
        if s.name.len() >= 4 && contains_ident(task_lower, &s.name.to_lowercase()) {
            score += 30.0;
            matched_symbols.push((s.name.clone(), s.line));
        }
    }
    if !matched_symbols.is_empty() {
        let names: Vec<_> = matched_symbols
            .iter()
            .take(3)
            .map(|(n, _)| n.as_str())
            .collect();
        reasons.push(format!("defines {}", names.join(", ")));
    }

    if !terms.is_empty() {
        let pt = terms_of(&e.path);
        let path_hits = terms.iter().filter(|t| pt.contains(*t)).count();
        if path_hits > 0 {
            score += 8.0 * path_hits as f32;
            reasons.push(format!("path matches {path_hits} term(s)"));
        }
        let mut sym_terms = HashSet::new();
        for s in &e.outline.symbols {
            sym_terms.extend(terms_of(&s.name));
        }
        let sym_hits = terms.iter().filter(|t| sym_terms.contains(*t)).count();
        if sym_hits > 0 {
            score += 5.0 * sym_hits as f32;
            reasons.push(format!("symbols match {sym_hits} term(s)"));
        }
        let st = terms_of(&e.summary);
        let sum_hits = terms.iter().filter(|t| st.contains(*t)).count();
        if sum_hits > 0 {
            score += 2.0 * sum_hits as f32;
            reasons.push(format!("summary matches {sum_hits} term(s)"));
        }
    }

    if score <= 0.0 {
        return None;
    }
    // Prefer code over docs/config slightly, and penalise huge files so the
    // budget isn't consumed by one generated blob.
    if e.lang.is_code() {
        score *= 1.1;
    }
    if e.tokens > 8_000 {
        score *= 0.8;
    }
    if e.sensitive {
        score *= 0.1;
        reasons.push("sensitive file (redacted, low priority)".into());
    }
    Some(Scored {
        path: e.path.clone(),
        score,
        reasons,
        matched_symbols,
    })
}

fn contains_ident(hay: &str, ident: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = hay[start..].find(ident) {
        let a = start + pos;
        let b = a + ident.len();
        let before_ok = a == 0
            || !hay.as_bytes()[a - 1].is_ascii_alphanumeric() && hay.as_bytes()[a - 1] != b'_';
        let after_ok = b >= hay.len()
            || !hay.as_bytes()[b].is_ascii_alphanumeric() && hay.as_bytes()[b] != b'_';
        if before_ok && after_ok {
            return true;
        }
        start = a + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::IndexOptions;

    fn idx(files: &[(&str, &str)]) -> (tempfile::TempDir, RepoIndex) {
        let d = tempfile::tempdir().unwrap();
        for (p, t) in files {
            let path = d.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, t).unwrap();
        }
        let mut i = RepoIndex::open(d.path(), None);
        i.refresh(&IndexOptions::default()).unwrap();
        (d, i)
    }

    #[test]
    fn ranks_relevant_files_first() {
        let (_d, i) = idx(&[
            (
                "src/auth/login.rs",
                "/// Handles user login\npub fn verify_password() {}\n",
            ),
            ("src/render/mesh.rs", "pub fn draw_mesh() {}\n"),
            ("README.md", "# Project\nGeneral docs\n"),
        ]);
        let s = score(
            &i,
            "Login fails when verify_password gets an empty string",
            &[],
        );
        assert_eq!(s[0].path, "src/auth/login.rs");
        assert!(s[0].reasons.iter().any(|r| r.contains("verify_password")));
        assert!(!s.iter().any(|x| x.path == "src/render/mesh.rs"));
    }

    #[test]
    fn explicit_path_and_changed_boost() {
        let (_d, i) = idx(&[
            ("a/b.ts", "export const x = 1;\n"),
            ("c.ts", "export const y = 1;\n"),
        ]);
        let s = score(&i, "look at a/b.ts", &["c.ts".into()]);
        assert_eq!(s[0].path, "a/b.ts");
        assert_eq!(s[1].path, "c.ts");
    }

    #[test]
    fn terms_drop_stopwords_and_stem() {
        let t = query_terms("Please fix the parsing of configs for the HttpServer");
        assert!(t.contains("pars"));
        assert!(t.contains("config"));
        assert!(t.contains("http"));
        assert!(!t.contains("the"));
        assert!(!t.contains("fix"));
    }
}
