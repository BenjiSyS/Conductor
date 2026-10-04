//! Bundled provider catalog: capability hints for models whose provider
//! listing endpoint doesn't publish them.
//!
//! This is data, refreshable without an app release (a signed catalog file
//! in the data folder overrides it — see `conductor_security::integrity`).
//! Entries only declare effort levels the provider documents for that model
//! family. If a provider rejects a declared level at runtime, the engine
//! retries without effort and removes the level for that model, so a stale
//! entry degrades gracefully instead of breaking requests.

use conductor_core::domain::{Model, ProviderConfig, ProviderKind};
use conductor_security::integrity::{SignedEnvelope, TrustStore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogRule {
    pub provider: ProviderKind,
    /// Model id prefix (lowercase), unless `exact` is set.
    pub prefix: String,
    /// Match only this alias or exact snapshot id.
    #[serde(default)]
    pub exact: bool,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub efforts: Vec<String>,
    #[serde(default)]
    pub vision: Option<bool>,
    #[serde(default)]
    pub tools: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub version: String,
    pub rules: Vec<CatalogRule>,
}

pub fn builtin() -> Catalog {
    let exact = |model: &str, efforts: &[&str]| CatalogRule {
        provider: ProviderKind::Openai,
        prefix: model.into(),
        exact: true,
        exclude: vec![],
        efforts: efforts.iter().map(|s| s.to_string()).collect(),
        vision: Some(true),
        tools: Some(true),
    };
    Catalog {
        version: "2026.10-builtin".into(),
        rules: vec![
            // Exact aliases and documented snapshots only. Unknown future IDs
            // retain only capabilities returned by the provider.
            exact("gpt-5", &["minimal", "low", "medium", "high"]),
            exact("gpt-5-2025-08-07", &["minimal", "low", "medium", "high"]),
            exact("gpt-5-mini", &[]),
            exact("gpt-5-mini-2025-08-07", &[]),
            exact("gpt-5-nano", &[]),
            exact("gpt-5-nano-2025-08-07", &[]),
            exact("gpt-5-pro", &["high"]),
            exact("gpt-5-pro-2025-10-06", &["high"]),
            exact("gpt-5.1", &["none", "low", "medium", "high"]),
            exact("gpt-5.1-2025-11-13", &["none", "low", "medium", "high"]),
            // The model pages document image input and function calling, but
            // do not establish allowed effort lists for these models.
            exact("o3", &[]),
            exact("o3-2025-04-16", &[]),
            exact("o4-mini", &[]),
            exact("o4-mini-2025-04-16", &[]),
        ],
    }
}

/// Load a signed catalog override if present and valid, else the built-in.
pub fn load(data_dir: &std::path::Path, trust: &TrustStore) -> Catalog {
    let p = data_dir.join("catalog.signed.json");
    if let Ok(b) = std::fs::read(&p) {
        if let Ok(env) = serde_json::from_slice::<SignedEnvelope>(&b) {
            match env.verify(trust) {
                Ok(payload) => {
                    if let Ok(c) = serde_json::from_slice::<Catalog>(&payload) {
                        return c;
                    }
                }
                Err(e) => tracing::warn!(error = %e, "ignoring catalog with invalid signature"),
            }
        }
    }
    builtin()
}

/// Fill in effort levels / capabilities the provider didn't report.
pub fn enrich(catalog: &Catalog, config: &mut ProviderConfig) {
    for m in &mut config.models {
        apply(catalog, &config.kind, m);
    }
}

fn apply(catalog: &Catalog, kind: &ProviderKind, m: &mut Model) {
    let id = m.id.to_lowercase();
    for rule in &catalog.rules {
        let matches = if rule.exact {
            id == rule.prefix
        } else {
            id.starts_with(&rule.prefix)
        };
        if &rule.provider == kind
            && matches
            && !rule.exclude.iter().any(|x| id.contains(x.as_str()))
        {
            if m.efforts.is_empty() {
                m.efforts = rule.efforts.clone();
            }
            if let Some(v) = rule.vision {
                m.vision |= v;
            }
            if let Some(t) = rule.tools {
                m.tools |= t;
            }
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(kind: ProviderKind, ids: &[&str]) -> ProviderConfig {
        ProviderConfig {
            id: "p".into(),
            name: "P".into(),
            kind,
            base_url: String::new(),
            models: ids
                .iter()
                .map(|i| Model {
                    id: i.to_string(),
                    name: i.to_string(),
                    efforts: vec![],
                    context_window: None,
                    tools: false,
                    vision: false,
                })
                .collect(),
            enabled: true,
        }
    }

    #[test]
    fn enriches_only_matching_models() {
        let mut c = cfg(
            ProviderKind::Openai,
            &["gpt-5", "gpt-5-chat-latest", "gpt-4o"],
        );
        enrich(&builtin(), &mut c);
        assert_eq!(
            c.models[0].efforts,
            vec!["minimal", "low", "medium", "high"]
        );
        assert!(c.models[0].vision);
        assert!(c.models[0].tools);
        assert!(c.models[1].efforts.is_empty(), "chat variant excluded");
        assert!(!c.models[1].vision, "ChatGPT-only alias is not enriched");
        assert!(!c.models[1].tools, "unknown model tools remain unknown");
        assert!(
            c.models[2].efforts.is_empty(),
            "non-reasoning model untouched"
        );
        let mut a = cfg(ProviderKind::Anthropic, &["gpt-5"]);
        enrich(&builtin(), &mut a);
        assert!(
            a.models[0].efforts.is_empty(),
            "rules are provider-specific"
        );
    }

    #[test]
    fn exact_gpt5_rules_distinguish_efforts_snapshots_and_unknown_ids() {
        let mut c = cfg(
            ProviderKind::Openai,
            &[
                "gpt-5-mini",
                "gpt-5-pro-2025-10-06",
                "gpt-5.1-2025-11-13",
                "gpt-5.10",
                "gpt-5-future",
                "gpt-5-codex",
            ],
        );
        enrich(&builtin(), &mut c);

        assert!(
            c.models[0].efforts.is_empty(),
            "no mini effort rule without explicit model documentation"
        );
        assert_eq!(c.models[1].efforts, vec!["high"]);
        assert_eq!(c.models[2].efforts, vec!["none", "low", "medium", "high"]);
        assert!(c.models[0].vision && c.models[0].tools);
        for model in &c.models[3..] {
            assert!(model.efforts.is_empty());
            assert!(!model.vision);
            assert!(!model.tools);
        }
    }

    #[test]
    fn enrichment_preserves_provider_reported_metadata() {
        let mut c = cfg(ProviderKind::Openai, &["gpt-5", "gpt-5-pro"]);
        c.models[0].efforts = vec!["provider-level".into()];
        c.models[0].vision = true;
        c.models[0].tools = true;
        enrich(&builtin(), &mut c);
        assert_eq!(c.models[0].efforts, vec!["provider-level"]);
        assert!(c.models[0].vision && c.models[0].tools);
        assert_eq!(c.models[1].efforts, vec!["high"]);
    }

    #[test]
    fn o3_and_o4_mini_rules_match_only_documented_ids() {
        let mut c = cfg(
            ProviderKind::Openai,
            &[
                "o3",
                "o3-2025-04-16",
                "o3-future",
                "o3-pro-unlisted",
                "o4-mini",
                "o4-mini-2025-04-16",
                "o4-mini-future",
            ],
        );
        enrich(&builtin(), &mut c);

        for index in [0, 1, 4, 5] {
            assert!(c.models[index].efforts.is_empty());
            assert!(c.models[index].vision);
            assert!(c.models[index].tools);
        }
        for index in [2, 3, 6] {
            assert!(c.models[index].efforts.is_empty());
            assert!(!c.models[index].vision);
            assert!(!c.models[index].tools);
        }
    }

    #[test]
    fn signed_override_verified() {
        let d = tempfile::tempdir().unwrap();
        let (sk, pk) = conductor_security::integrity::generate_keypair();
        let key = conductor_security::integrity::signing_key_from_b64(&sk).unwrap();
        let custom = Catalog {
            version: "x".into(),
            rules: vec![],
        };
        let env = SignedEnvelope::sign("k", &key, &serde_json::to_vec(&custom).unwrap());
        std::fs::write(
            d.path().join("catalog.signed.json"),
            serde_json::to_vec(&env).unwrap(),
        )
        .unwrap();
        let trust = TrustStore::default().with_key("k", &pk);
        assert_eq!(load(d.path(), &trust).version, "x");
        assert_eq!(
            load(d.path(), &TrustStore::default()).version,
            builtin().version,
            "untrusted override ignored"
        );
    }
}
