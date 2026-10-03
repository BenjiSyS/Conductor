//! Provider-reported rate limits, read from response headers.
//!
//! OpenAI sends `x-ratelimit-*` and Anthropic sends `anthropic-ratelimit-*`
//! on every API response. Gemini reports no limit headers, so nothing is
//! recorded for it. Values are kept in memory per provider id and are only
//! what the provider last reported — never an estimate.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::domain::ProviderKind;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    /// As reported: an RFC 3339 time (Anthropic) or a duration such as
    /// `6m0s` (OpenAI).
    pub reset: Option<String>,
}

impl Window {
    fn is_empty(&self) -> bool {
        self.limit.is_none() && self.remaining.is_none() && self.reset.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RateLimits {
    pub requests: Window,
    pub tokens: Window,
    /// Anthropic also splits token limits into input and output.
    pub input_tokens: Window,
    pub output_tokens: Window,
    /// Unix seconds when these headers were seen.
    pub observed_at: u64,
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)?
        .to_str()
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v.len() <= 64)
}

fn window(headers: &HeaderMap, limit: &str, remaining: &str, reset: &str) -> Window {
    let num = |name: &str| header(headers, name).and_then(|v| v.parse().ok());
    Window {
        limit: num(limit),
        remaining: num(remaining),
        reset: header(headers, reset),
    }
}

/// Parse the limit headers a provider of `kind` sends. `None` when the
/// response carried none.
pub fn parse(kind: &ProviderKind, headers: &HeaderMap, now: u64) -> Option<RateLimits> {
    let anthropic = |what: &str| {
        window(
            headers,
            &format!("anthropic-ratelimit-{what}-limit"),
            &format!("anthropic-ratelimit-{what}-remaining"),
            &format!("anthropic-ratelimit-{what}-reset"),
        )
    };
    let openai = |what: &str| {
        window(
            headers,
            &format!("x-ratelimit-limit-{what}"),
            &format!("x-ratelimit-remaining-{what}"),
            &format!("x-ratelimit-reset-{what}"),
        )
    };
    let limits = match kind {
        ProviderKind::Anthropic => RateLimits {
            requests: anthropic("requests"),
            tokens: anthropic("tokens"),
            input_tokens: anthropic("input-tokens"),
            output_tokens: anthropic("output-tokens"),
            observed_at: now,
        },
        ProviderKind::Openai | ProviderKind::OpenaiCompatible => RateLimits {
            requests: openai("requests"),
            tokens: openai("tokens"),
            observed_at: now,
            ..Default::default()
        },
        ProviderKind::Gemini => return None,
    };
    let empty = limits.requests.is_empty()
        && limits.tokens.is_empty()
        && limits.input_tokens.is_empty()
        && limits.output_tokens.is_empty();
    (!empty).then_some(limits)
}

fn table() -> &'static Mutex<HashMap<String, RateLimits>> {
    static TABLE: OnceLock<Mutex<HashMap<String, RateLimits>>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

pub fn record(provider_id: &str, limits: RateLimits) {
    if let Ok(mut t) = table().lock() {
        t.insert(provider_id.to_string(), limits);
    }
}

/// The limits `provider_id` last reported, if any.
pub fn get(provider_id: &str) -> Option<RateLimits> {
    table().lock().ok()?.get(provider_id).cloned()
}

pub(crate) fn observe(provider_id: &str, kind: &ProviderKind, headers: &HeaderMap) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Some(limits) = parse(kind, headers, now) {
        record(provider_id, limits);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn parses_openai_headers() {
        let h = headers(&[
            ("x-ratelimit-limit-requests", "500"),
            ("x-ratelimit-remaining-requests", "499"),
            ("x-ratelimit-reset-requests", "120ms"),
            ("x-ratelimit-limit-tokens", "30000"),
            ("x-ratelimit-remaining-tokens", "29000"),
            ("x-ratelimit-reset-tokens", "2s"),
        ]);
        let l = parse(&ProviderKind::Openai, &h, 7).unwrap();
        assert_eq!(l.requests.limit, Some(500));
        assert_eq!(l.requests.remaining, Some(499));
        assert_eq!(l.tokens.remaining, Some(29000));
        assert_eq!(l.tokens.reset.as_deref(), Some("2s"));
        assert_eq!(l.observed_at, 7);
    }

    #[test]
    fn parses_anthropic_headers_including_split_tokens() {
        let h = headers(&[
            ("anthropic-ratelimit-requests-limit", "50"),
            ("anthropic-ratelimit-requests-remaining", "49"),
            ("anthropic-ratelimit-requests-reset", "2026-10-04T12:00:00Z"),
            ("anthropic-ratelimit-input-tokens-limit", "40000"),
            ("anthropic-ratelimit-input-tokens-remaining", "39000"),
            ("anthropic-ratelimit-output-tokens-limit", "8000"),
            ("anthropic-ratelimit-output-tokens-remaining", "7900"),
        ]);
        let l = parse(&ProviderKind::Anthropic, &h, 0).unwrap();
        assert_eq!(l.requests.remaining, Some(49));
        assert_eq!(l.input_tokens.remaining, Some(39000));
        assert_eq!(l.output_tokens.limit, Some(8000));
        assert!(l.tokens.limit.is_none());
    }

    #[test]
    fn no_headers_or_gemini_records_nothing() {
        assert!(parse(&ProviderKind::Openai, &HeaderMap::new(), 0).is_none());
        let h = headers(&[("x-ratelimit-limit-requests", "5")]);
        assert!(parse(&ProviderKind::Gemini, &h, 0).is_none());
        // Malformed numbers are ignored rather than guessed.
        let h = headers(&[("x-ratelimit-limit-requests", "lots")]);
        assert!(parse(&ProviderKind::Openai, &h, 0).is_none());
    }

    #[test]
    fn record_and_get_by_provider() {
        record(
            "limits-test-provider",
            RateLimits {
                observed_at: 3,
                ..Default::default()
            },
        );
        assert_eq!(get("limits-test-provider").unwrap().observed_at, 3);
        assert!(get("limits-missing-provider").is_none());
    }
}
