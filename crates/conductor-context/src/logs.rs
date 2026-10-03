//! Build / test log reduction.
//!
//! Keeps the actual failure, its preceding context, stack traces and relevant
//! warnings; collapses repeated noise; caps the result. The full log stays on
//! disk and is referenced, never sent wholesale.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct LogOptions {
    pub max_lines: usize,
    pub context_before: usize,
    pub context_after: usize,
    /// Always keep this many trailing lines (summaries live at the end).
    pub tail: usize,
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            max_lines: 200,
            context_before: 6,
            context_after: 12,
            tail: 15,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReducedLog {
    pub text: String,
    pub original_lines: usize,
    pub kept_lines: usize,
    pub error_lines: usize,
    pub warning_lines: usize,
    /// First failure line, for one-line status display.
    pub headline: Option<String>,
}

fn error_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?i)(\berror\b|\berror\[|\bfailed\b|\bfailure\b|\bfail:|^FAIL\b|panicked at|traceback \(most recent call last\)|exception\b|\bfatal\b|segmentation fault|assertion.*failed|undefined reference|cannot find|not found:|(?-i:\bE\d{4}\b)|(?-i:\bTS\d{4}\b)|exit code [1-9]|npm ERR!)",
        )
        .expect("valid regex")
    })
}

fn warn_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)(\bwarning\b|\bwarn\b|deprecated)").expect("valid regex"))
}

fn stack_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"^\s+(at |File "|\d+: |--> |\|)|^\s*[A-Za-z_][\w.]*Error:|^\s*note:|^\s*help:|^\s*= note"#)
            .expect("valid regex")
    })
}

fn noise_key(line: &str) -> String {
    // Normalise digits/hex/paths so "Compiling foo v1.2" lines collapse.
    static R: OnceLock<Regex> = OnceLock::new();
    let r =
        R.get_or_init(|| Regex::new(r"[0-9a-f]{6,}|\d+(\.\d+)*|\S+[/\\]\S+").expect("valid regex"));
    let first_word = line.split_whitespace().next().unwrap_or("");
    format!("{first_word}|{}", r.replace_all(line.trim(), "#"))
}

pub fn reduce(log: &str, opts: &LogOptions) -> ReducedLog {
    let lines: Vec<&str> = log.lines().collect();
    let n = lines.len();
    let mut keep = vec![false; n];
    let mut error_lines = 0;
    let mut warning_lines = 0;
    let mut headline = None;

    for (i, l) in lines.iter().enumerate() {
        if error_re().is_match(l) && !l.contains("0 failed") && !l.contains("errors: 0") {
            error_lines += 1;
            if headline.is_none() {
                headline = Some(l.trim().chars().take(200).collect());
            }
            let a = i.saturating_sub(opts.context_before);
            let b = (i + opts.context_after + 1).min(n);
            for k in keep.iter_mut().take(b).skip(a) {
                *k = true;
            }
        } else if warn_re().is_match(l) {
            warning_lines += 1;
            keep[i] = true;
        } else if stack_re().is_match(l) {
            keep[i] = true;
        }
    }
    for k in keep.iter_mut().skip(n.saturating_sub(opts.tail)) {
        *k = true;
    }
    if n <= opts.max_lines && error_lines == 0 {
        // Short, clean logs pass through (still deduped below).
        keep.iter_mut().for_each(|k| *k = true);
    }

    // Emit kept lines with gap markers and collapse consecutive repeats.
    let mut out: Vec<String> = Vec::new();
    let mut last_kept: Option<usize> = None;
    let mut repeat_key = String::new();
    let mut repeat_count = 0usize;
    let flush_repeat = |out: &mut Vec<String>, count: &mut usize| {
        if *count > 0 {
            out.push(format!("    … {count} similar line(s) omitted"));
            *count = 0;
        }
    };
    for i in 0..n {
        if !keep[i] {
            continue;
        }
        if let Some(prev) = last_kept {
            if i > prev + 1 {
                flush_repeat(&mut out, &mut repeat_count);
                out.push(format!("[… {} line(s) omitted …]", i - prev - 1));
                repeat_key.clear();
            }
        } else if i > 0 {
            out.push(format!("[… {i} line(s) omitted …]"));
        }
        last_kept = Some(i);
        let key = noise_key(lines[i]);
        if key == repeat_key && !error_re().is_match(lines[i]) {
            repeat_count += 1;
            continue;
        }
        flush_repeat(&mut out, &mut repeat_count);
        repeat_key = key;
        out.push(lines[i].to_string());
    }
    flush_repeat(&mut out, &mut repeat_count);
    if let Some(prev) = last_kept {
        if prev + 1 < n {
            out.push(format!("[… {} line(s) omitted …]", n - prev - 1));
        }
    }

    // Hard cap: keep the head of failures and the tail summary.
    if out.len() > opts.max_lines {
        let head = opts.max_lines.saturating_sub(opts.tail + 1);
        let dropped = out.len() - head - opts.tail;
        let mut capped: Vec<String> = out[..head].to_vec();
        capped.push(format!(
            "[… {dropped} reduced line(s) omitted to fit budget …]"
        ));
        capped.extend_from_slice(&out[out.len() - opts.tail..]);
        out = capped;
    }
    let kept_lines = out.len();
    ReducedLog {
        text: out.join("\n"),
        original_lines: n,
        kept_lines,
        error_lines,
        warning_lines,
        headline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_failure_and_context_drops_noise() {
        let mut log = String::new();
        for i in 0..5000 {
            log.push_str(&format!("   Compiling crate{i} v0.1.{i}\n"));
        }
        log.push_str("error[E0308]: mismatched types\n --> src/main.rs:4:5\n  |\n4 |     let x: u32 = \"a\";\n  |                  ^^^ expected `u32`\n");
        for i in 0..3000 {
            log.push_str(&format!("noise line {i}\n"));
        }
        log.push_str("error: could not compile `app` due to 1 previous error\n");
        let r = reduce(&log, &LogOptions::default());
        assert!(r.text.contains("error[E0308]: mismatched types"));
        assert!(r.text.contains("src/main.rs:4:5"));
        assert!(r.text.contains("could not compile"));
        assert!(r.kept_lines <= 200, "{}", r.kept_lines);
        assert!(r.original_lines > 8000);
        assert_eq!(
            r.headline.as_deref(),
            Some("error[E0308]: mismatched types")
        );
    }

    #[test]
    fn python_traceback_kept() {
        let log = "collecting...\n".repeat(300)
            + "Traceback (most recent call last):\n  File \"app.py\", line 3, in <module>\n    main()\nValueError: bad value\n";
        let r = reduce(&log, &LogOptions::default());
        assert!(r.text.contains("Traceback"));
        assert!(r.text.contains("ValueError: bad value"));
        assert!(r.text.contains("File \"app.py\""));
    }

    #[test]
    fn short_clean_log_passthrough() {
        let log = "running 3 tests\ntest a ... ok\ntest result: ok. 3 passed; 0 failed";
        let r = reduce(log, &LogOptions::default());
        assert_eq!(r.error_lines, 0);
        assert!(r.text.contains("test result: ok"));
    }

    #[test]
    fn repeated_lines_collapse() {
        let log = "warning: unused variable `x`\n".repeat(50);
        let r = reduce(&log, &LogOptions::default());
        assert!(r.kept_lines < 10, "{}", r.text);
        assert!(r.text.contains("similar line(s) omitted"));
    }
}
