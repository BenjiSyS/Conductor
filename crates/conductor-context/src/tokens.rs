//! Deterministic token estimation.
//!
//! Real tokenizers differ per provider, so Conductor never claims exact
//! counts unless the provider reports usage. This heuristic tracks common BPE
//! tokenizers within roughly ±15% on English prose and source code: ASCII
//! text averages ~4 characters per token, punctuation-dense code a bit less,
//! and non-ASCII scripts closer to one token per character.

/// Estimate the number of tokens in `text`.
pub fn estimate(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let mut ascii_word_chars = 0usize;
    let mut punct = 0usize;
    let mut non_ascii = 0usize;
    let mut whitespace_runs = 0usize;
    let mut prev_ws = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            ascii_word_chars += 1;
            prev_ws = false;
        } else if c.is_whitespace() {
            if !prev_ws {
                whitespace_runs += 1;
            }
            prev_ws = true;
        } else if c.is_ascii() {
            punct += 1;
            prev_ws = false;
        } else {
            non_ascii += 1;
            prev_ws = false;
        }
    }
    let words = ascii_word_chars.div_ceil(4);
    // Punctuation often merges with neighbours; count ~2 per 3.
    let p = (punct * 2).div_ceil(3);
    // Newlines/indentation runs are usually folded into neighbouring tokens.
    let ws = whitespace_runs / 4;
    (words + p + non_ascii + ws).max(1)
}

/// Format an estimate for display, e.g. "~1.2k tokens".
pub fn display(n: usize) -> String {
    if n >= 1_000_000 {
        format!("~{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("~{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("~{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasonable_estimates() {
        assert_eq!(estimate(""), 0);
        let prose = "The quick brown fox jumps over the lazy dog. ".repeat(20);
        let e = estimate(&prose);
        // ~10 tokens per sentence on cl100k-like tokenizers
        assert!((150..=260).contains(&e), "{e}");
        let code = "fn main() { let x = vec![1, 2, 3]; println!(\"{:?}\", x); }\n".repeat(10);
        let e = estimate(&code);
        assert!((150..=330).contains(&e), "{e}");
        assert!(estimate("日本語のテキスト") >= 7);
    }

    #[test]
    fn display_formats() {
        assert_eq!(display(12), "~12");
        assert_eq!(display(1234), "~1.2k");
        assert_eq!(display(2_500_000), "~2.5M");
    }
}
