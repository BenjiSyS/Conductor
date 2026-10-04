//! Secret detection and redaction.
//!
//! The scanner is deliberately conservative about *what* it reports (each
//! finding names a kind and a byte range, never the secret itself) and
//! aggressive about *redacting*. False positives cost a few tokens of context;
//! false negatives leak credentials to a cloud provider.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    PrivateKey,
    AwsAccessKey,
    OpenAiKey,
    AnthropicKey,
    GoogleApiKey,
    GithubToken,
    SlackToken,
    StripeKey,
    Jwt,
    BearerToken,
    PasswordAssignment,
    EnvAssignment,
    ConnectionString,
}

impl SecretKind {
    pub fn label(self) -> &'static str {
        match self {
            SecretKind::PrivateKey => "private key",
            SecretKind::AwsAccessKey => "AWS access key",
            SecretKind::OpenAiKey => "OpenAI key",
            SecretKind::AnthropicKey => "Anthropic key",
            SecretKind::GoogleApiKey => "Google API key",
            SecretKind::GithubToken => "GitHub token",
            SecretKind::SlackToken => "Slack token",
            SecretKind::StripeKey => "Stripe key",
            SecretKind::Jwt => "JWT",
            SecretKind::BearerToken => "bearer token",
            SecretKind::PasswordAssignment => "password",
            SecretKind::EnvAssignment => ".env value",
            SecretKind::ConnectionString => "connection string credentials",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub kind: SecretKind,
    /// Byte range of the secret value (not the surrounding key name).
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

struct Rule {
    kind: SecretKind,
    re: Regex,
    /// Capture group holding the secret value (0 = whole match).
    group: usize,
}

fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let r = |kind, pat: &str, group| Rule {
            kind,
            re: Regex::new(pat).expect("valid secret regex"),
            group,
        };
        vec![
            r(
                SecretKind::PrivateKey,
                r"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----[\s\S]*?(?:-----END (?:[A-Z0-9 ]+ )?PRIVATE KEY-----|\z)",
                0,
            ),
            r(SecretKind::AwsAccessKey, r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", 0),
            r(SecretKind::AnthropicKey, r"\bsk-ant-[A-Za-z0-9_\-]{20,}", 0),
            r(SecretKind::OpenAiKey, r"\bsk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_\-]{20,}", 0),
            r(SecretKind::GoogleApiKey, r"\bAIza[0-9A-Za-z_\-]{35}\b", 0),
            r(
                SecretKind::GithubToken,
                r"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{40,})",
                0,
            ),
            r(SecretKind::SlackToken, r"\bxox[abprs]-[A-Za-z0-9\-]{10,}", 0),
            r(SecretKind::StripeKey, r"\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}", 0),
            r(
                SecretKind::Jwt,
                r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
                0,
            ),
            r(
                SecretKind::BearerToken,
                r"(?i)\bauthorization\s*[:=]\s*(?:bearer|token|basic)\s+([A-Za-z0-9._~+/=\-]{12,})",
                1,
            ),
            r(
                SecretKind::ConnectionString,
                r"(?i)\b[a-z][a-z0-9+.\-]*://[^\s:/@]+:([^\s@/]{3,})@",
                1,
            ),
            r(
                SecretKind::PasswordAssignment,
                r#"(?i)\b(?:password|passwd|pwd|secret|api[_\-]?key|access[_\-]?token|auth[_\-]?token|client[_\-]?secret|private[_\-]?key)\b["']?\s*[:=]\s*["']?([^\s"',;]{6,})"#,
                1,
            ),
            r(
                SecretKind::EnvAssignment,
                r"(?m)^\s*(?:export\s+)?[A-Z][A-Z0-9_]*(?:KEY|TOKEN|SECRET|PASSWORD|PASS|CREDENTIALS?)\s*=\s*([^\s#]{6,})",
                1,
            ),
        ]
    })
}

/// Values that look like assignments but are obviously placeholders.
fn is_placeholder(v: &str) -> bool {
    let l = v
        .trim_matches(|c| c == '"' || c == '\'')
        .to_ascii_lowercase();
    l.starts_with("${")
        || l.starts_with("$(")
        || l.starts_with('<')
        || l.starts_with("{{")
        || l.starts_with("process.env")
        || l.starts_with("env(")
        || l.starts_with("std::env")
        || l.starts_with("os.environ")
        || l.contains("[redacted")
        || l == "changeme"
        || l.chars().all(|c| c == 'x' || c == '*' || c == '.')
        || matches!(
            l.as_str(),
            "none"
                | "null"
                | "true"
                | "false"
                | "example"
                | "password"
                | "your_api_key"
                | "your-api-key"
        )
}

/// Scan text for likely secrets.
pub fn scan(text: &str) -> Vec<Finding> {
    let mut found: Vec<Finding> = Vec::new();
    for rule in rules() {
        for caps in rule.re.captures_iter(text) {
            let Some(m) = caps.get(rule.group) else {
                continue;
            };
            if matches!(
                rule.kind,
                SecretKind::PasswordAssignment
                    | SecretKind::EnvAssignment
                    | SecretKind::ConnectionString
            ) && is_placeholder(m.as_str())
            {
                continue;
            }
            // Skip overlaps with an earlier, more specific rule.
            if found.iter().any(|f| m.start() < f.end && f.start < m.end()) {
                continue;
            }
            let line = text[..m.start()].bytes().filter(|b| *b == b'\n').count() + 1;
            found.push(Finding {
                kind: rule.kind,
                start: m.start(),
                end: m.end(),
                line,
            });
        }
    }
    found.sort_by_key(|f| f.start);
    found
}

/// Replace every detected secret with `[REDACTED:<kind>]`.
pub fn redact(text: &str) -> Redacted {
    let findings = scan(text);
    if findings.is_empty() {
        return Redacted {
            text: text.to_string(),
            findings,
        };
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for f in &findings {
        out.push_str(&text[last..f.start]);
        out.push_str("[REDACTED:");
        out.push_str(f.kind.label());
        out.push(']');
        last = f.end;
    }
    out.push_str(&text[last..]);
    Redacted {
        text: out,
        findings,
    }
}

#[derive(Debug, Clone)]
pub struct Redacted {
    pub text: String,
    pub findings: Vec<Finding>,
}

impl Redacted {
    pub fn changed(&self) -> bool {
        !self.findings.is_empty()
    }
}

/// Files that should never be sent to a cloud provider by default.
pub fn is_sensitive_path(path: &str) -> bool {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let name = p.rsplit('/').next().unwrap_or(&p);
    name == ".env"
        || (name.starts_with(".env.")
            && !name.ends_with(".example")
            && !name.ends_with(".sample")
            && !name.ends_with(".template"))
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || name.ends_with(".keystore")
        || name.ends_with(".jks")
        || matches!(
            name,
            "id_rsa"
                | "id_ed25519"
                | "id_ecdsa"
                | "id_dsa"
                | ".npmrc"
                | ".pypirc"
                | ".netrc"
                | "credentials"
                | "credentials.json"
                | ".git-credentials"
        )
        || p.contains("/.ssh/")
        || p.starts_with(".ssh/")
        || p.contains("/.aws/")
        || p.starts_with(".aws/")
        || p.contains("/.gnupg/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_keys() {
        let cases = [
            (
                "key = sk-proj-abcdefghijklmnopqrstuvwx1234",
                SecretKind::OpenAiKey,
            ),
            (
                "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123",
                SecretKind::AnthropicKey,
            ),
            ("AKIAIOSFODNN7EXAMPLE", SecretKind::AwsAccessKey),
            (
                "AIzaSyA1234567890abcdefghijklmnopqrstuv",
                SecretKind::GoogleApiKey,
            ),
            (
                "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
                SecretKind::GithubToken,
            ),
            (
                "Authorization: Bearer abcdef123456789xyz",
                SecretKind::BearerToken,
            ),
            (
                "postgres://admin:hunter22pass@db.local/x",
                SecretKind::ConnectionString,
            ),
        ];
        for (text, kind) in cases {
            let f = scan(text);
            assert!(
                f.iter().any(|f| f.kind == kind),
                "expected {kind:?} in {text:?}, got {f:?}"
            );
        }
    }

    #[test]
    fn private_key_block_redacted_entirely() {
        let t = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAAB3Nza\nmore\n-----END OPENSSH PRIVATE KEY-----\nafter";
        let r = redact(t);
        assert!(!r.text.contains("AAAAB3Nza"));
        assert!(r.text.starts_with("before\n[REDACTED:private key]"));
        assert!(r.text.ends_with("after"));
    }

    #[test]
    fn env_and_password_assignments() {
        let t =
            "DATABASE_PASSWORD=s3cr3tvalue\nAPI_KEY=${FROM_VAULT}\npassword: \"correcthorse\"\n";
        let r = redact(t);
        assert!(!r.text.contains("s3cr3tvalue"));
        assert!(!r.text.contains("correcthorse"));
        assert!(r.text.contains("${FROM_VAULT}"), "placeholders survive");
        // the key name is kept so the model still understands the config
        assert!(r.text.contains("DATABASE_PASSWORD="));
    }

    #[test]
    fn clean_code_untouched() {
        let t = "fn main() { let password_len = 12; println!(\"hi\"); }";
        assert!(scan(t).is_empty());
        assert_eq!(redact(t).text, t);
    }

    #[test]
    fn findings_report_lines() {
        let t = "a\nb\nghp_abcdefghijklmnopqrstuvwxyz0123456789\n";
        assert_eq!(scan(t)[0].line, 3);
    }

    #[test]
    fn sensitive_paths() {
        assert!(is_sensitive_path(".env"));
        assert!(is_sensitive_path("config/.env.production"));
        assert!(!is_sensitive_path(".env.example"));
        assert!(is_sensitive_path("home/u/.ssh/config"));
        assert!(is_sensitive_path("certs/server.pem"));
        assert!(!is_sensitive_path("src/main.rs"));
    }
}
