//! Per-provider usage and health tracking.
//!
//! Most providers do not expose exact remaining subscription quota. When a
//! provider reports remaining capacity (e.g. rate-limit headers) we use it and
//! mark it exact; otherwise remaining capacity is *estimated* from observed
//! usage against a user-configured or learned limit, and labelled as such.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Connected,
    SignedOut,
    RateLimited,
    Unavailable,
    Degraded,
    #[default]
    Unknown,
    InvalidKey,
    ModelMissing,
    UsageExhausted,
}

impl Health {
    pub fn usable(self) -> bool {
        matches!(self, Health::Connected | Health::Degraded | Health::Unknown)
    }
    /// Plain-language status for the UI.
    pub fn message(self, provider: &str) -> String {
        match self {
            Health::Connected => format!("{provider} connected"),
            Health::SignedOut => format!("{provider} signed out. Reconnect to continue."),
            Health::RateLimited => format!("{provider} is rate-limited. Waiting before retrying."),
            Health::Unavailable => format!("{provider} is unavailable right now."),
            Health::Degraded => format!("{provider} is responding slowly or with errors."),
            Health::Unknown => format!("{provider} status unknown"),
            Health::InvalidKey => format!("{provider} API key is invalid. Update it in Settings."),
            Health::ModelMissing => format!("{provider} no longer offers this model."),
            Health::UsageExhausted => format!("{provider} usage unavailable."),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Remaining {
    pub fraction: f32,
    /// False only when the provider itself reported the remaining amount.
    pub estimated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub provider: String,
    pub health: Health,
    pub window_start: u64,
    pub window_secs: u64,
    pub tokens_in_window: u64,
    pub requests_in_window: u64,
    pub total_tokens: u64,
    /// User-configured or learned soft limit for the window.
    pub limit_tokens: Option<u64>,
    /// Exact remaining reported by provider headers (tokens) and when.
    pub reported_remaining: Option<(u64, u64)>,
    pub reported_limit: Option<u64>,
    pub blocked_until: Option<u64>,
    pub consecutive_failures: u32,
    pub next_health_check: u64,
}

impl ProviderUsage {
    pub fn remaining(&self, now: u64) -> Option<Remaining> {
        if let (Some((rem, at)), Some(limit)) = (self.reported_remaining, self.reported_limit) {
            if now.saturating_sub(at) < 120 && limit > 0 {
                return Some(Remaining {
                    fraction: (rem as f32 / limit as f32).clamp(0.0, 1.0),
                    estimated: false,
                });
            }
        }
        self.limit_tokens.filter(|l| *l > 0).map(|limit| Remaining {
            fraction: (1.0 - self.tokens_in_window as f32 / limit as f32).clamp(0.0, 1.0),
            estimated: true,
        })
    }

    pub fn available(&self, now: u64) -> bool {
        if let Some(until) = self.blocked_until {
            if now < until {
                return false;
            }
        }
        match self.health {
            Health::RateLimited | Health::UsageExhausted | Health::Unavailable => {
                // A block that has expired makes the provider eligible again.
                self.blocked_until.is_some_and(|u| now >= u)
            }
            h => h.usable(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageTracker {
    pub providers: BTreeMap<String, ProviderUsage>,
}

/// Outcome of a provider call, as classified by the integrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    Ok,
    RateLimited { retry_after_secs: u64 },
    UsageExhausted { retry_after_secs: u64 },
    SignedOut,
    InvalidKey,
    ModelMissing,
    Unavailable,
    OtherError,
}

impl UsageTracker {
    fn entry(&mut self, provider: &str, now: u64) -> &mut ProviderUsage {
        self.providers
            .entry(provider.to_string())
            .or_insert_with(|| ProviderUsage {
                provider: provider.to_string(),
                window_start: now,
                window_secs: 5 * 3600,
                ..Default::default()
            })
    }

    pub fn get(&self, provider: &str) -> Option<&ProviderUsage> {
        self.providers.get(provider)
    }

    pub fn set_limit(&mut self, provider: &str, limit_tokens: Option<u64>, now: u64) {
        self.entry(provider, now).limit_tokens = limit_tokens;
    }

    pub fn record_tokens(&mut self, provider: &str, tokens: u64, now: u64) {
        let e = self.entry(provider, now);
        if now.saturating_sub(e.window_start) >= e.window_secs {
            e.window_start = now;
            e.tokens_in_window = 0;
            e.requests_in_window = 0;
        }
        e.tokens_in_window += tokens;
        e.requests_in_window += 1;
        e.total_tokens += tokens;
    }

    pub fn record_reported(&mut self, provider: &str, remaining: u64, limit: u64, now: u64) {
        let e = self.entry(provider, now);
        e.reported_remaining = Some((remaining, now));
        e.reported_limit = Some(limit);
    }

    /// Record a call result and update health with adaptive backoff.
    pub fn record_outcome(&mut self, provider: &str, outcome: CallOutcome, now: u64) {
        let e = self.entry(provider, now);
        match outcome {
            CallOutcome::Ok => {
                e.health = Health::Connected;
                e.consecutive_failures = 0;
                e.blocked_until = None;
                e.next_health_check = now + 15 * 60;
            }
            CallOutcome::RateLimited { retry_after_secs } => {
                e.health = Health::RateLimited;
                e.blocked_until = Some(now + retry_after_secs.max(5));
            }
            CallOutcome::UsageExhausted { retry_after_secs } => {
                e.health = Health::UsageExhausted;
                e.blocked_until = Some(now + retry_after_secs.max(60));
            }
            CallOutcome::SignedOut => e.health = Health::SignedOut,
            CallOutcome::InvalidKey => e.health = Health::InvalidKey,
            CallOutcome::ModelMissing => e.health = Health::ModelMissing,
            CallOutcome::Unavailable | CallOutcome::OtherError => {
                e.consecutive_failures += 1;
                let backoff = (30u64 << e.consecutive_failures.min(6)).min(30 * 60);
                if e.consecutive_failures >= 3 {
                    e.health = Health::Unavailable;
                    e.blocked_until = Some(now + backoff);
                } else {
                    e.health = Health::Degraded;
                }
                e.next_health_check = now + backoff;
            }
        }
    }

    /// Should the integrator poll this provider's health now? Adaptive: no
    /// polling while healthy and recently used; back off while failing.
    pub fn needs_health_check(&self, provider: &str, now: u64) -> bool {
        self.providers
            .get(provider)
            .is_none_or(|e| now >= e.next_health_check)
    }

    pub fn available(&self, provider: &str, now: u64) -> bool {
        self.providers
            .get(provider)
            .is_none_or(|e| e.available(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimated_vs_reported_remaining() {
        let mut t = UsageTracker::default();
        t.set_limit("anthropic", Some(1000), 0);
        t.record_tokens("anthropic", 300, 10);
        let r = t.get("anthropic").unwrap().remaining(10).unwrap();
        assert!((r.fraction - 0.7).abs() < 1e-3);
        assert!(r.estimated);
        t.record_reported("anthropic", 900, 1000, 20);
        let r = t.get("anthropic").unwrap().remaining(30).unwrap();
        assert!(!r.estimated);
        assert!((r.fraction - 0.9).abs() < 1e-3);
        // stale report falls back to estimate
        assert!(
            t.get("anthropic")
                .unwrap()
                .remaining(1000)
                .unwrap()
                .estimated
        );
        // no limit known -> no claim at all
        t.record_tokens("openai", 5, 0);
        assert!(t.get("openai").unwrap().remaining(0).is_none());
    }

    #[test]
    fn window_resets() {
        let mut t = UsageTracker::default();
        t.record_tokens("p", 100, 0);
        t.record_tokens("p", 100, 5 * 3600 + 1);
        assert_eq!(t.get("p").unwrap().tokens_in_window, 100);
        assert_eq!(t.get("p").unwrap().total_tokens, 200);
    }

    #[test]
    fn rate_limit_blocks_then_recovers() {
        let mut t = UsageTracker::default();
        t.record_outcome(
            "p",
            CallOutcome::RateLimited {
                retry_after_secs: 30,
            },
            100,
        );
        assert!(!t.available("p", 110));
        assert!(t.available("p", 131));
        t.record_outcome("p", CallOutcome::Ok, 140);
        assert_eq!(t.get("p").unwrap().health, Health::Connected);
    }

    #[test]
    fn repeated_failures_back_off() {
        let mut t = UsageTracker::default();
        t.record_outcome("p", CallOutcome::Unavailable, 0);
        assert!(t.available("p", 0), "one failure only degrades");
        t.record_outcome("p", CallOutcome::Unavailable, 1);
        t.record_outcome("p", CallOutcome::Unavailable, 2);
        assert!(!t.available("p", 3));
        assert!(!t.needs_health_check("p", 3));
        assert!(t.needs_health_check("p", 10_000));
        t.record_outcome("q", CallOutcome::SignedOut, 0);
        assert!(!t.available("q", 0));
        assert_eq!(
            Health::SignedOut.message("Claude"),
            "Claude signed out. Reconnect to continue."
        );
    }
}
