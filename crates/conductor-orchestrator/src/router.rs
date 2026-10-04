//! Provider-aware routing.
//!
//! Inputs: the Combo (members, roles, strategy, reserves, phase rules), model
//! capability profiles, live usage/health, and the task. Output: one model
//! plus ordered fallbacks and a human-readable reason.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::combo::{Combo, ComboMember, Phase, Strategy};
use crate::model::{Capability, EffortLevel, ModelProfile};
use crate::roles::Role;
use crate::usage::UsageTracker;

#[derive(Debug, Clone, Default)]
pub struct RouteRequest {
    pub role: Option<Role>,
    pub phase: Option<Phase>,
    pub needs: Vec<Capability>,
    /// Models that already failed on this task (keys).
    pub exclude: Vec<String>,
    pub min_context: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteDecision {
    pub model: String,
    pub provider: String,
    /// Member-configured effort (None = decide automatically).
    pub member_effort: Option<EffortLevel>,
    pub reason: String,
    pub fallbacks: Vec<String>,
    /// True when a usage reserve had to be dipped into because nothing else
    /// was available.
    pub used_reserve: bool,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RouteError {
    #[error("no model available: {0}")]
    NoModel(String),
}

pub fn route(
    combo: &Combo,
    models: &HashMap<String, ModelProfile>,
    usage: &UsageTracker,
    req: &RouteRequest,
    now: u64,
) -> Result<RouteDecision, RouteError> {
    let mut rejected: Vec<String> = Vec::new();
    let mut cands: Vec<(&ComboMember, &ModelProfile)> = Vec::new();
    for m in combo.enabled_members() {
        let Some(p) = models.get(&m.model) else {
            rejected.push(format!("{} not configured", m.model));
            continue;
        };
        if p.deprecated {
            rejected.push(format!("{} deprecated", m.model));
            continue;
        }
        if req.exclude.contains(&m.model) {
            rejected.push(format!("{} already failed on this task", m.model));
            continue;
        }
        if !usage.available(&p.provider, now) {
            let msg = usage
                .get(&p.provider)
                .map(|u| u.health.message(&p.provider))
                .unwrap_or_else(|| format!("{} unavailable", p.provider));
            rejected.push(msg);
            continue;
        }
        let mut needs: Vec<Capability> = req.needs.clone();
        if let Some(r) = req.role {
            needs.extend_from_slice(r.required());
        }
        if let Some(missing) = needs.iter().find(|c| !p.has(**c)) {
            rejected.push(format!("{} lacks {:?}", m.model, missing));
            continue;
        }
        if req.min_context > 0 && p.context_window < req.min_context {
            rejected.push(format!("{} context too small", m.model));
            continue;
        }
        cands.push((m, p));
    }
    if cands.is_empty() {
        rejected.dedup();
        return Err(RouteError::NoModel(if rejected.is_empty() {
            "Combo has no enabled models".into()
        } else {
            rejected.join("; ")
        }));
    }

    // Usage reserves: hold back capacity unless the role is allowed.
    let reserve_blocked = |p: &ModelProfile| -> bool {
        combo.reserves.iter().any(|r| {
            r.provider == p.provider
                && !req.role.is_some_and(|role| r.for_roles.contains(&role))
                && usage
                    .get(&p.provider)
                    .and_then(|u| u.remaining(now))
                    .is_some_and(|rem| rem.fraction <= r.fraction)
        })
    };
    let unreserved: Vec<_> = cands
        .iter()
        .copied()
        .filter(|(_, p)| !reserve_blocked(p))
        .collect();
    let used_reserve = unreserved.is_empty();
    let pool = if used_reserve {
        cands.clone()
    } else {
        unreserved
    };

    // Explicit phase rule wins when its model is eligible.
    if let Some(phase) = req.phase {
        if let Some(rule) = combo.phases.iter().find(|r| r.phase == phase) {
            if let Some(want) = &rule.model {
                if let Some((m, p)) = pool.iter().find(|(m, _)| &m.model == want) {
                    let fallbacks = pool
                        .iter()
                        .filter(|(x, _)| x.model != m.model)
                        .map(|(x, _)| x.model.clone())
                        .collect();
                    return Ok(RouteDecision {
                        model: m.model.clone(),
                        provider: p.provider.clone(),
                        member_effort: rule.effort.or(m.effort),
                        reason: format!("{phase:?} phase rule"),
                        fallbacks,
                        used_reserve,
                    });
                }
            }
        }
    }

    let mut scored: Vec<(f32, &ComboMember, &ModelProfile, String)> = pool
        .iter()
        .map(|(m, p)| {
            let (s, why) = score(combo, m, p, req, usage, now);
            (s, *m, *p, why)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.model.cmp(&b.1.model)));
    let (_, m, p, why) = &scored[0];
    Ok(RouteDecision {
        model: m.model.clone(),
        provider: p.provider.clone(),
        member_effort: m.effort,
        reason: if used_reserve {
            format!("{why}; using reserved capacity (no other model available)")
        } else {
            why.clone()
        },
        fallbacks: scored.iter().skip(1).map(|s| s.1.model.clone()).collect(),
        used_reserve,
    })
}

fn score(
    combo: &Combo,
    m: &ComboMember,
    p: &ModelProfile,
    req: &RouteRequest,
    usage: &UsageTracker,
    now: u64,
) -> (f32, String) {
    let mut s = 0.0f32;
    let mut why: Vec<String> = Vec::new();
    if let Some(role) = req.role {
        if m.roles.contains(&role) {
            s += 50.0;
            why.push(format!("assigned {}", role.label()));
        } else if m.roles.is_empty() {
            s += 10.0;
        }
        if p.tier >= role.preferred_tier() {
            s += 10.0;
        }
    }
    let tier = p.tier as i32 as f32;
    match combo.strategy {
        Strategy::Balanced => s += tier * 5.0 - p.cost as f32 * 2.0,
        Strategy::MaximumQuality => {
            s += tier * 20.0;
            why.push("highest quality".into());
        }
        Strategy::LowestCost => {
            s -= p.cost as f32 * 10.0;
            if p.local {
                s += 5.0;
            }
            why.push("lowest cost".into());
        }
        Strategy::Fastest => {
            s -= p.latency as f32 * 10.0;
            why.push("fastest".into());
        }
        Strategy::SubscriptionFirst => {
            if p.subscription {
                s += 30.0;
                why.push("subscription first".into());
            }
            s += tier * 3.0;
        }
        Strategy::PreferProvider => {
            if combo.strategy_provider.as_deref() == Some(p.provider.as_str()) {
                s += 40.0;
                why.push(format!("prefer {}", p.provider));
            }
            s += tier * 3.0;
        }
        Strategy::PreserveProvider => {
            let reserved_role = matches!(
                req.role,
                Some(Role::Reviewer | Role::SecurityReviewer | Role::Architect)
            );
            if combo.strategy_provider.as_deref() == Some(p.provider.as_str()) && !reserved_role {
                s -= 40.0;
                why.push(format!("preserving {}", p.provider));
            }
            s += tier * 3.0;
        }
        Strategy::ManualWeights => {
            s += tier;
        }
    }
    s *= m.weight.max(0.01);
    // Gentle preference for providers with more (estimated) headroom.
    if let Some(rem) = usage.get(&p.provider).and_then(|u| u.remaining(now)) {
        s += rem.fraction * 5.0;
    }
    if why.is_empty() {
        why.push("best match".into());
    }
    (s, format!("{} ({})", m.model, why.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combo::{PhaseRule, Reserve};
    use crate::model::{test_model, Tier};
    use crate::usage::CallOutcome;

    fn setup() -> (Combo, HashMap<String, ModelProfile>) {
        let models: HashMap<_, _> = [
            test_model("openai", "coder", Tier::Frontier),
            test_model("anthropic", "reviewer", Tier::Frontier),
            test_model("google", "flash", Tier::Fast),
        ]
        .into_iter()
        .map(|m| (m.key(), m))
        .collect();
        let combo = Combo::new(
            "t",
            vec![
                ComboMember {
                    model: "openai/coder".into(),
                    roles: vec![Role::Coder],
                    effort: None,
                    enabled: true,
                    weight: 1.0,
                },
                ComboMember {
                    model: "anthropic/reviewer".into(),
                    roles: vec![Role::Reviewer],
                    effort: Some(EffortLevel::High),
                    enabled: true,
                    weight: 1.0,
                },
                ComboMember {
                    model: "google/flash".into(),
                    roles: vec![Role::Researcher, Role::Planner],
                    effort: None,
                    enabled: true,
                    weight: 1.0,
                },
            ],
        );
        (combo, models)
    }

    fn req(role: Role) -> RouteRequest {
        RouteRequest {
            role: Some(role),
            ..Default::default()
        }
    }

    #[test]
    fn routes_by_role() {
        let (c, m) = setup();
        let u = UsageTracker::default();
        assert_eq!(
            route(&c, &m, &u, &req(Role::Coder), 0).unwrap().model,
            "openai/coder"
        );
        let r = route(&c, &m, &u, &req(Role::Reviewer), 0).unwrap();
        assert_eq!(r.model, "anthropic/reviewer");
        assert_eq!(r.member_effort, Some(EffortLevel::High));
        assert_eq!(
            route(&c, &m, &u, &req(Role::Researcher), 0).unwrap().model,
            "google/flash"
        );
    }

    #[test]
    fn role_moves_when_provider_exhausted() {
        // Spec example: coder provider runs out; another model takes the role.
        let (c, m) = setup();
        let mut u = UsageTracker::default();
        u.record_outcome(
            "openai",
            CallOutcome::UsageExhausted {
                retry_after_secs: 3600,
            },
            0,
        );
        let r = route(&c, &m, &u, &req(Role::Coder), 10).unwrap();
        assert_ne!(r.provider, "openai");
        assert!(!r.fallbacks.contains(&"openai/coder".to_string()));
    }

    #[test]
    fn exclusion_and_capabilities() {
        let (c, mut m) = setup();
        let u = UsageTracker::default();
        let mut rq = req(Role::Coder);
        rq.exclude = vec!["openai/coder".into()];
        assert_ne!(route(&c, &m, &u, &rq, 0).unwrap().model, "openai/coder");
        m.get_mut("google/flash")
            .unwrap()
            .capabilities
            .retain(|c| *c != Capability::Coding);
        m.get_mut("anthropic/reviewer")
            .unwrap()
            .capabilities
            .retain(|c| *c != Capability::Coding);
        let e = route(&c, &m, &u, &rq, 0).unwrap_err();
        assert!(matches!(e, RouteError::NoModel(msg) if msg.contains("already failed")));
    }

    #[test]
    fn usage_reserve_holds_capacity_for_reviewer() {
        let (mut c, m) = setup();
        c.members[0].enabled = false; // only anthropic + google left
        c.reserves.push(Reserve {
            provider: "anthropic".into(),
            fraction: 0.3,
            for_roles: vec![Role::Reviewer],
        });
        let mut u = UsageTracker::default();
        u.set_limit("anthropic", Some(1000), 0);
        u.record_tokens("anthropic", 800, 0); // 20% left, below 30% reserve
        let coder = route(&c, &m, &u, &req(Role::Coder), 0).unwrap();
        assert_eq!(
            coder.provider, "google",
            "coder must not eat Claude's reserve"
        );
        let rev = route(&c, &m, &u, &req(Role::Reviewer), 0).unwrap();
        assert_eq!(rev.provider, "anthropic");
    }

    #[test]
    fn strategies_and_phase_rules() {
        let (mut c, m) = setup();
        let u = UsageTracker::default();
        c.strategy = Strategy::LowestCost;
        let r = route(&c, &m, &u, &RouteRequest::default(), 0).unwrap();
        assert_eq!(r.model, "google/flash");
        c.strategy = Strategy::PreferProvider;
        c.strategy_provider = Some("anthropic".into());
        assert_eq!(
            route(&c, &m, &u, &RouteRequest::default(), 0)
                .unwrap()
                .provider,
            "anthropic"
        );
        c.phases.push(PhaseRule {
            phase: Phase::RepetitiveFixes,
            model: Some("google/flash".into()),
            min_tier: None,
            max_tier: None,
            effort: Some(EffortLevel::Low),
        });
        let r = route(
            &c,
            &m,
            &u,
            &RouteRequest {
                phase: Some(Phase::RepetitiveFixes),
                role: Some(Role::Coder),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        assert_eq!(r.model, "google/flash");
        assert_eq!(r.member_effort, Some(EffortLevel::Low));
    }
}
