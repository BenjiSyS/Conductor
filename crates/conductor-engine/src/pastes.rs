//! Large pasted text. The composer keeps a short chip such as
//! `[Pasted text #1 · 5,000,000 chars · paste:<id>]` instead of megabytes of
//! text; the text itself is saved here. When a message is sent, chips are
//! expanded: inline when everything fits the model's budget, otherwise the
//! start and end of each paste plus the saved file's path (Agent mode can
//! read the rest in parts). Conversations store only the chip, so a huge
//! paste never bloats history or later requests.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Largest single paste accepted (UTF-8 bytes).
pub const MAX_PASTE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct PasteInfo {
    pub id: String,
    pub chars: usize,
    pub lines: usize,
    pub path: String,
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn path_for(dir: &Path, id: &str) -> Option<PathBuf> {
    valid_id(id).then(|| dir.join(format!("{id}.txt")))
}

pub fn save(dir: &Path, text: &str) -> Result<PasteInfo, String> {
    if text.len() > MAX_PASTE_BYTES {
        return Err(format!(
            "That paste is {} MB; the limit is {} MB. Save it as a file in the project and ask about the file instead.",
            text.len() / (1024 * 1024),
            MAX_PASTE_BYTES / (1024 * 1024)
        ));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = dir.join(format!("{id}.txt"));
    std::fs::write(&path, text).map_err(|e| format!("Could not save the paste: {e}"))?;
    Ok(PasteInfo {
        id,
        chars: text.chars().count(),
        lines: text.lines().count().max(1),
        path: path.display().to_string(),
    })
}

/// Remove pastes older than `days`.
pub fn prune(dir: &Path, days: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let limit = std::time::Duration::from_secs(days * 86_400);
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > limit);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Chip spans in `text`: (start, end, id).
fn chips(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = vec![];
    let mut from = 0;
    while let Some(rel) = text[from..].find("[Pasted text #") {
        let start = from + rel;
        let Some(close) = text[start..].find(']') else {
            break;
        };
        let end = start + close + 1;
        let inner = &text[start..end];
        if let Some(id) = inner
            .rsplit_once("paste:")
            .map(|(_, rest)| rest.trim_end_matches(']').trim())
            .filter(|id| valid_id(id))
        {
            out.push((start, end, id.to_string()));
        }
        from = end;
    }
    out
}

fn char_prefix(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}
fn char_suffix(s: &str, n: usize) -> &str {
    let total = s.chars().count();
    if n >= total {
        return s;
    }
    match s.char_indices().nth(total - n) {
        Some((i, _)) => &s[i..],
        None => s,
    }
}

/// Replace chips with their text, keeping the result within `budget_chars`.
pub fn expand(text: &str, dir: &Path, budget_chars: usize) -> String {
    let found = chips(text);
    if found.is_empty() {
        return text.to_string();
    }
    let loaded: Vec<(usize, usize, String, Option<String>, PathBuf)> = found
        .into_iter()
        .map(|(s, e, id)| {
            let path = path_for(dir, &id).unwrap_or_default();
            let body = std::fs::read_to_string(&path).ok();
            (s, e, id, body, path)
        })
        .collect();
    let total: usize = loaded
        .iter()
        .filter_map(|l| l.3.as_ref())
        .map(|b| b.chars().count())
        .sum();
    let per = budget_chars / loaded.len().max(1);
    let mut out = String::with_capacity(text.len() + total.min(budget_chars) + 512);
    let mut at = 0;
    for (start, end, id, body, path) in &loaded {
        out.push_str(&text[at..*start]);
        at = *end;
        let Some(body) = body else {
            out.push_str(&format!("[Pasted text {id} is no longer available]"));
            continue;
        };
        let chars = body.chars().count();
        if total <= budget_chars {
            out.push_str(&format!(
                "<pasted_text chars=\"{chars}\">\n{body}\n</pasted_text>"
            ));
        } else {
            let half = per / 2;
            out.push_str(&format!(
                "<pasted_text chars=\"{chars}\" excerpt=\"true\" saved_at=\"{}\">\n{}\n\n[… {} characters omitted. The full text is saved at the path above; read it in parts if you need the middle …]\n\n{}\n</pasted_text>",
                path.display(),
                char_prefix(body, half),
                chars.saturating_sub(half * 2),
                char_suffix(body, half),
            ));
        }
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_pastes_expand_inline_and_text_around_is_kept() {
        let d = tempfile::tempdir().unwrap();
        let a = save(d.path(), "alpha\nbeta").unwrap();
        assert_eq!((a.chars, a.lines), (10, 2));
        let msg = format!(
            "Look at this: [Pasted text #1 · 10 chars · paste:{}] thanks",
            a.id
        );
        let out = expand(&msg, d.path(), 1_000);
        assert_eq!(
            out,
            "Look at this: <pasted_text chars=\"10\">\nalpha\nbeta\n</pasted_text> thanks"
        );
    }

    #[test]
    fn five_million_characters_become_bounded_excerpts() {
        let d = tempfile::tempdir().unwrap();
        let big: String = "é".repeat(2_500_000) + &"z".repeat(2_500_000);
        let a = save(d.path(), &big).unwrap();
        assert_eq!(a.chars, 5_000_000);
        let msg = format!("[Pasted text #1 · 5,000,000 chars · paste:{}]", a.id);
        let out = expand(&msg, d.path(), 200_000);
        assert!(out.chars().count() < 201_000, "{}", out.chars().count());
        assert!(out.contains("4800000 characters omitted"));
        assert!(out.contains(&a.path));
        assert!(out.contains("éé") && out.contains("zz"));
    }

    #[test]
    fn rejects_oversize_and_ignores_forged_or_missing_chips() {
        let d = tempfile::tempdir().unwrap();
        assert!(save(d.path(), &"x".repeat(MAX_PASTE_BYTES + 1))
            .unwrap_err()
            .contains("limit"));
        // Not a valid id: left untouched (no path traversal).
        let forged = "[Pasted text #1 · paste:../../secrets]";
        assert_eq!(expand(forged, d.path(), 100), forged);
        let gone = format!("[Pasted text #2 · paste:{}]", "a".repeat(32));
        assert!(expand(&gone, d.path(), 100).contains("no longer available"));
        assert_eq!(
            expand("plain [text] here", d.path(), 100),
            "plain [text] here"
        );
    }

    #[test]
    fn prune_removes_only_old_files() {
        let d = tempfile::tempdir().unwrap();
        let a = save(d.path(), "keep").unwrap();
        prune(d.path(), 1);
        assert!(std::path::Path::new(&a.path).exists());
        prune(d.path(), 0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        prune(d.path(), 0);
        assert!(!std::path::Path::new(&a.path).exists());
    }
}
