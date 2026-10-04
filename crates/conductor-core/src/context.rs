use crate::{store::Store, Error, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Read, path::Path, sync::LazyLock};

static SECRETS: LazyLock<Vec<Regex>> =
    LazyLock::new(|| {
        [r"(?i)(?:sk-|AIza|ghp_|github_pat_)[A-Za-z0-9_-]{16,}",
     r"(?im)((?:api[_-]?key|access[_-]?token|password|secret|authorization)\s*[:=]\s*)[^\r\n]+",
     r"(?s)-----BEGIN (?:[A-Z ]+)?PRIVATE KEY-----.*?-----END (?:[A-Z ]+)?PRIVATE KEY-----"]
        .iter().filter_map(|s| Regex::new(s).ok()).collect()
    });

pub fn redact(text: &str) -> String {
    let mut result = conductor_security::secrets::redact(text).text;
    for (index, regex) in SECRETS.iter().enumerate() {
        result = regex
            .replace_all(
                &result,
                if index == 1 {
                    "${1}[REDACTED]"
                } else {
                    "[REDACTED]"
                },
            )
            .into_owned();
    }
    result
}

pub fn estimated_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}
pub fn truncate_utf8(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFile {
    pub path: String,
    pub reason: String,
    pub tokens: usize,
    pub cached: bool,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextReport {
    pub files: Vec<ContextFile>,
    pub omitted: Vec<String>,
    pub estimated_tokens: usize,
    pub cache_hits: usize,
    pub compression: bool,
    pub truncated_scan: bool,
}

fn excluded(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let name = name.as_str();
    [
        ".git",
        "node_modules",
        "target",
        "dist",
        ".conductor",
        ".ssh",
        ".aws",
        ".gnupg",
    ]
    .contains(&name)
        || conductor_security::secrets::is_sensitive_path(name)
        || name.starts_with(".env")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || ["credentials", "auth.json"].contains(&name)
}

pub fn select(
    root: &Path,
    prompt: &str,
    budget: usize,
    compression: bool,
    store: &Store,
) -> Result<ContextReport> {
    let root = root.canonicalize()?;
    let keywords: Vec<String> = prompt
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
        .filter(|s| s.len() > 2)
        .take(64)
        .map(str::to_lowercase)
        .collect();
    let mut candidates = Vec::new();
    let mut stack = vec![root.clone()];
    let mut visited = 0;
    let mut truncated_scan = false;
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries {
            visited += 1;
            if visited > 5000 {
                truncated_scan = true;
                break;
            }
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if excluded(&name) {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            // Junctions and other filesystem aliases must never broaden the
            // project scope, even if their file type is reported as ordinary.
            if !entry.path().canonicalize()?.starts_with(&root) {
                continue;
            }
            if kind.is_dir() {
                stack.push(entry.path());
                continue;
            }
            let path = entry.path();
            if !kind.is_file() || entry.metadata()?.len() > 1_000_000 {
                continue;
            }
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ![
                "rs", "ts", "tsx", "js", "jsx", "json", "toml", "md", "py", "java", "gd", "cs",
                "cpp", "c", "h", "css", "html", "yaml", "yml", "txt",
            ]
            .contains(&extension)
            {
                continue;
            }
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| Error::Denied("File outside project".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            let lower = relative.to_lowercase();
            let mut score = keywords
                .iter()
                .filter(|word| lower.contains(word.as_str()))
                .count()
                * 20;
            if ["AGENTS.md", "conductor.toml"].contains(&relative.as_str()) {
                score += 100;
            }
            if ["Cargo.toml", "package.json", "README.md"].contains(&relative.as_str()) {
                score += 5;
            }
            if score > 0 {
                candidates.push((score, relative, path));
            }
        }
        if truncated_scan {
            break;
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut report = ContextReport {
        files: vec![],
        omitted: vec![],
        estimated_tokens: 0,
        cache_hits: 0,
        compression,
        truncated_scan,
    };
    let mut used = 0;
    let mut hashes = HashSet::new();
    for (score, relative, path) in candidates {
        if used >= budget {
            report.omitted.push(relative);
            continue;
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)?
            .take(1_000_001)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 1_000_000 {
            report.omitted.push(relative);
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            report.omitted.push(relative);
            continue;
        };
        let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
        if !hashes.insert(hash.clone()) {
            report.omitted.push(format!("{relative} (duplicate)"));
            continue;
        }
        let cached = store.cache_get(&path.to_string_lossy(), &hash)?;
        let cache_hit = cached.is_some();
        let clean = cached.unwrap_or_else(|| redact(&text));
        if !cache_hit {
            store.cache_put(&path.to_string_lossy(), &hash, &clean)?;
        }
        let file_limit = if compression { 12_000 } else { 64_000 };
        let allowance = (budget - used).min(file_limit);
        const NOTICE: &str = "\n[File truncated; inspect locally for remaining content]";
        let selected = if clean.len() > allowance {
            if allowance < NOTICE.len() {
                report.omitted.push(relative);
                continue;
            }
            let head = truncate_utf8(&clean, allowance - NOTICE.len());
            format!("{head}{NOTICE}")
        } else {
            clean
        };
        let tokens = estimated_tokens(&selected);
        used += selected.len();
        report.estimated_tokens += tokens;
        if cache_hit {
            report.cache_hits += 1;
        }
        report.files.push(ContextFile {
            path: relative,
            reason: if score >= 100 {
                "Project rules (untrusted file content)".into()
            } else {
                "Prompt relevance / project manifest".into()
            },
            tokens,
            cached: cache_hit,
            text: selected,
        });
    }
    Ok(report)
}

pub fn reduce_log(log: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = log.lines().collect();
    if lines.len() <= max_lines {
        return redact(log);
    }
    let mut selected = std::collections::BTreeSet::new();
    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        if lower.contains("error")
            || lower.contains("failed")
            || lower.contains("panic")
            || lower.contains("traceback")
        {
            for index in i.saturating_sub(4)..(i + 12).min(lines.len()) {
                selected.insert(index);
            }
        }
    }
    for index in 0..5.min(lines.len()) {
        selected.insert(index);
    }
    for index in lines.len().saturating_sub(10)..lines.len() {
        selected.insert(index);
    }
    redact(
        &selected
            .into_iter()
            .take(max_lines)
            .map(|i| lines[i])
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_credentials_and_preserves_unicode_boundaries() {
        let input = "api_key=very-secret\nhello sk-abcdefghijklmnop123456\n-----BEGIN PRIVATE KEY-----\nkey\n-----END PRIVATE KEY-----";
        let clean = redact(input);
        assert!(!clean.contains("very-secret"));
        assert!(!clean.contains("abcdefghijklmnop"));
        assert!(!clean.contains("BEGIN PRIVATE"));
        assert_eq!(truncate_utf8("åβc", 3), "å");
    }
    #[test]
    fn context_is_bounded_cached_and_incrementally_invalidated() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open(&temp.path().join(".conductor/state.db"))?;
        std::fs::write(
            temp.path().join("widget.rs"),
            "fn widget() {}\napi_key=private",
        )?;
        std::fs::write(temp.path().join(".env"), "API_KEY=private")?;
        let first = select(temp.path(), "widget", 1000, true, &store)?;
        assert_eq!(first.files.len(), 1);
        assert!(!first.files[0].text.contains("private"));
        assert_eq!(first.cache_hits, 0);
        let second = select(temp.path(), "widget", 1000, true, &store)?;
        assert_eq!(second.cache_hits, 1);
        std::fs::write(temp.path().join("widget.rs"), "fn widget_new() {}")?;
        let third = select(temp.path(), "widget", 1000, true, &store)?;
        assert_eq!(third.cache_hits, 0);
        assert!(third.files[0].text.contains("widget_new"));
        Ok(())
    }
    #[test]
    fn context_budget_includes_notices_and_hashes_the_complete_file() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open(&temp.path().join(".conductor/state.db"))?;
        let source = format!("{}å tail-one", "a".repeat(63_999));
        std::fs::write(temp.path().join("widget.rs"), &source)?;
        // A byte limit must neither overflow nor discard a file simply because
        // the selection boundary cuts a multi-byte character.
        for budget in [0, 1, 54, 55, 64, 1000, 64_000, 100_000] {
            let report = select(temp.path(), "widget", budget, false, &store)?;
            let used: usize = report.files.iter().map(|file| file.text.len()).sum();
            assert!(used <= budget, "used {used} with budget {budget}");
            if budget >= 1000 {
                assert_eq!(report.files.len(), 1);
                assert!(report.files[0]
                    .text
                    .ends_with("[File truncated; inspect locally for remaining content]"));
            }
        }
        std::fs::write(
            temp.path().join("widget.rs"),
            source.replace("tail-one", "tail-two"),
        )?;
        let changed = select(temp.path(), "widget", 1000, true, &store)?;
        assert_eq!(
            changed.cache_hits, 0,
            "changes beyond the selected prefix invalidate the cache"
        );
        for name in [".ENV", "credentials", "AUTH.JSON", "secret.PEM"] {
            assert!(excluded(name), "{name}");
        }
        Ok(())
    }
    #[test]
    fn reduced_logs_keep_failure_context() {
        let mut log = "noise\n".repeat(100);
        log.push_str("preceding context\nerror: build failed\nstack trace\n");
        log.push_str(&"noise\n".repeat(100));
        let reduced = reduce_log(&log, 40);
        assert!(reduced.contains("preceding context"));
        assert!(reduced.contains("error: build failed"));
        assert!(reduced.contains("stack trace"));
        assert!(reduced.lines().count() <= 40);
    }
}
