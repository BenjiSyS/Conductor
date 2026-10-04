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

pub fn tier_hint(kind: &ProviderKind, model_id: &str) -> Tier {
    let m = model_id.to_lowercase();
    if matches!(kind, ProviderKind::OpenaiCompatible)
        && !m.contains("gpt")
        && !m.contains("claude")
        && !m.contains("gemini")
    {
        return Tier::Local;
    }
    if [
        "mini", "nano", "flash", "haiku", "lite", "small", "8b", "7b", "3b",
    ]
    .iter()
    .any(|k| m.contains(k))
    {
        return Tier::Fast;
    }
    if ["opus", "pro", "ultra", "large", "max"]
        .iter()
        .any(|k| m.contains(k))
    {
        return Tier::Frontier;
    }
    Tier::Strong
}

pub fn profiles(providers: &[ProviderConfig]) -> Vec<ModelProfile> {
    let mut out = Vec::new();
    for p in providers.iter().filter(|p| p.enabled) {
        for m in &p.models {
            let tier = tier_hint(&p.kind, &m.id);
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
                cost: match tier {
                    Tier::Local => 1,
                    Tier::Fast => 2,
                    Tier::Strong => 4,
                    Tier::Frontier => 8,
                },
                latency: match tier {
                    Tier::Local | Tier::Fast => 1,
                    Tier::Strong => 3,
                    Tier::Frontier => 5,
                },
                deprecated: false,
                replacement: None,
                subscription: false,
                local: tier == Tier::Local,
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
}
