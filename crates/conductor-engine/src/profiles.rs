//! Configured provider models → orchestration model profiles.
//!
//! Provider catalog endpoints rarely publish capabilities, so capability and
//! tier values here are **conservative inferences** from what the provider
//! declares (effort levels, context window) plus well-known naming
//! conventions. They only steer routing preferences; users can override any
//! assignment in the Combo editor, and nothing is invented about effort: only
//! levels the model declares are used.

use conductor_core::domain::{ProviderConfig, ProviderKind};
use conductor_orchestrator::model::{Capability, EffortLevel, ModelProfile, Tier};

/// Whether an OpenAI-compatible endpoint runs on this machine or the local
/// network (Ollama, LM Studio, …). Only these count as local/offline models.
pub fn is_local_endpoint(base_url: &str) -> bool {
    let Some(host) = url::Url::parse(base_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.trim_matches(['[', ']']).to_string()))
    else {
        return false;
    };
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => false,
    }
}

pub fn tier_hint(kind: &ProviderKind, model_id: &str) -> Tier {
    let _ = kind;
    let m = model_id.to_lowercase();
    if m == "default" {
        return Tier::Strong;
    }
    // Whole words only: "gemini" must not match "mini".
    let words: Vec<&str> = m.split(|c: char| !c.is_ascii_alphanumeric()).collect();
    let has = |k: &&str| words.contains(k);
    if [
        "mini", "nano", "flash", "haiku", "lite", "small", "fast", "8b", "7b", "3b",
    ]
    .iter()
    .any(has)
    {
        return Tier::Fast;
    }
    if ["opus", "pro", "ultra", "large", "max"].iter().any(has) {
        return Tier::Frontier;
    }
    Tier::Strong
}

pub fn profiles(providers: &[ProviderConfig]) -> Vec<ModelProfile> {
    let mut out = Vec::new();
    for p in providers.iter().filter(|p| p.enabled) {
        // Signed-in apps (CLI bridge) use the user's prepaid plan.
        let bridged = crate::cli_bridge::Cli::from_provider_id(&p.id);
        let local = p.kind == ProviderKind::OpenaiCompatible
            && bridged.is_none()
            && is_local_endpoint(&p.base_url);
        for m in &p.models {
            let tier = if local {
                Tier::Local
            } else {
                tier_hint(&p.kind, &m.id)
            };
            let mut caps = vec![
                Capability::Coding,
                Capability::Reasoning,
                Capability::Streaming,
            ];
            if m.vision {
                caps.push(Capability::Vision);
            }
            if m.tools {
                caps.push(Capability::ToolUse);
            }
            if m.context_window.unwrap_or(0) >= 500_000 {
                caps.push(Capability::LongContext);
            }
            let efforts: Vec<EffortLevel> = m
                .efforts
                .iter()
                .filter_map(|e| EffortLevel::parse(e))
                .collect();
            out.push(ModelProfile {
                provider: p.id.clone(),
                model: m.id.clone(),
                display: format!("{} · {}", p.name, m.name),
                tier,
                capabilities: caps,
                efforts,
                context_window: m.context_window.unwrap_or(32_000),
                cost: if bridged.is_some() {
                    1
                } else {
                    match tier {
                        Tier::Local => 1,
                        Tier::Fast => 2,
                        Tier::Strong => 4,
                        Tier::Frontier => 8,
                    }
                },
                latency: match tier {
                    Tier::Local | Tier::Fast => 1,
                    Tier::Strong => 3,
                    Tier::Frontier => 5,
                } + if bridged.is_some() { 3 } else { 0 },
                deprecated: false,
                replacement: None,
                subscription: bridged.is_some(),
                local,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use conductor_core::domain::Model;

    #[test]
    fn maps_providers_conservatively() {
        let p = ProviderConfig {
            id: "openai".into(),
            name: "OpenAI".into(),
            kind: ProviderKind::Openai,
            base_url: "https://api.openai.com/v1".into(),
            models: vec![
                Model {
                    id: "gpt-x-mini".into(),
                    name: "Mini".into(),
                    efforts: vec!["low".into(), "medium".into(), "bogus".into()],
                    context_window: Some(128_000),
                    tools: true,
                    vision: false,
                },
                Model {
                    id: "gpt-x".into(),
                    name: "X".into(),
                    efforts: vec![],
                    context_window: None,
                    tools: false,
                    vision: true,
                },
            ],
            enabled: true,
        };
        let local = ProviderConfig {
            id: "ollama".into(),
            name: "Ollama".into(),
            kind: ProviderKind::OpenaiCompatible,
            base_url: "http://localhost:11434/v1".into(),
            models: vec![Model {
                id: "llama3".into(),
                name: "Llama".into(),
                efforts: vec![],
                context_window: None,
                tools: false,
                vision: false,
            }],
            enabled: true,
        };
        let mut disabled = local.clone();
        disabled.id = "off".into();
        disabled.enabled = false;
        let v = profiles(&[p, local, disabled]);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].tier, Tier::Fast);
        assert_eq!(
            v[0].efforts,
            vec![EffortLevel::Low, EffortLevel::Medium],
            "unknown levels dropped, none invented"
        );
        assert!(v[1].efforts.is_empty());
        assert!(v[1].has(Capability::Vision));
        assert_eq!(v[2].tier, Tier::Local);
        assert!(v[2].local);
    }

    #[test]
    fn cloud_compatible_endpoints_and_bridges_are_not_local() {
        let mk = |id: &str, url: &str, model: &str| ProviderConfig {
            id: id.into(),
            name: id.into(),
            kind: ProviderKind::OpenaiCompatible,
            base_url: url.into(),
            models: vec![Model {
                id: model.into(),
                name: model.into(),
                efforts: vec![],
                context_window: None,
                tools: false,
                vision: false,
            }],
            enabled: true,
        };
        let v = profiles(&[
            mk("xai", "https://api.x.ai/v1", "grok-4"),
            mk(
                "cli-agy",
                "http://127.0.0.1:5555/agy/v1",
                "gemini-3.1-pro-high",
            ),
            mk("lan", "http://192.168.1.20:11434/v1", "qwen3"),
            mk("cli-claude", "http://127.0.0.1:5555/claude/v1", "default"),
        ]);
        assert!(!v[0].local && v[0].tier == Tier::Strong && !v[0].subscription);
        assert!(!v[1].local && v[1].tier == Tier::Frontier && v[1].subscription && v[1].cost == 1);
        assert!(v[2].local && v[2].tier == Tier::Local);
        assert!(!v[3].local && v[3].tier == Tier::Strong && v[3].subscription);
        assert!(
            is_local_endpoint("http://[::1]:8080/v1") && !is_local_endpoint("https://example.com")
        );
        assert_eq!(
            tier_hint(&ProviderKind::Gemini, "gemini-2.5-pro"),
            Tier::Frontier
        );
        assert_eq!(
            tier_hint(&ProviderKind::Gemini, "gemini-2.5-flash"),
            Tier::Fast
        );
        assert_eq!(tier_hint(&ProviderKind::Openai, "gpt-5-mini"), Tier::Fast);
        assert_eq!(tier_hint(&ProviderKind::Openai, "o4-mini"), Tier::Fast);
    }
}
