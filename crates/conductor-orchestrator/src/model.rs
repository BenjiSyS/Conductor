//! Model profiles: what a model can do, independent of the vendor schema.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffortLevel {
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl EffortLevel {
    pub fn is_max_tier(self) -> bool {
        matches!(self, EffortLevel::XHigh | EffortLevel::Max)
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => EffortLevel::None,
            "minimal" => EffortLevel::Minimal,
            "low" => EffortLevel::Low,
            "medium" | "med" => EffortLevel::Medium,
            "high" => EffortLevel::High,
            "xhigh" | "extra_high" | "extra-high" => EffortLevel::XHigh,
            "max" | "ultra" => EffortLevel::Max,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            EffortLevel::None => "none",
            EffortLevel::Minimal => "minimal",
            EffortLevel::Low => "low",
            EffortLevel::Medium => "medium",
            EffortLevel::High => "high",
            EffortLevel::XHigh => "xhigh",
            EffortLevel::Max => "max",
        }
    }
}

/// Coarse quality/cost tier used by adaptive presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Small/local models.
    Local,
    /// Fast, cheap cloud models.
    Fast,
    /// Strong general models.
    Strong,
    /// Highest-quality models.
    Frontier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Coding,
    Reasoning,
    Vision,
    ImageOutput,
    ToolUse,
    ComputerUse,
    Browser,
    StructuredOutput,
    LongContext,
    Audio,
    Streaming,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProfile {
    pub provider: String,
    pub model: String,
    pub display: String,
    pub tier: Tier,
    pub capabilities: Vec<Capability>,
    /// Effort levels this model actually supports (empty = no control).
    pub efforts: Vec<EffortLevel>,
    pub context_window: u32,
    /// Relative cost hint (1 = cheapest). Used by LowestCost strategy only.
    #[serde(default = "one")]
    pub cost: u8,
    /// Relative latency hint (1 = fastest).
    #[serde(default = "one")]
    pub latency: u8,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub replacement: Option<String>,
    /// True when access comes from a subscription rather than metered API.
    #[serde(default)]
    pub subscription: bool,
    #[serde(default)]
    pub local: bool,
}

fn one() -> u8 {
    1
}

impl ModelProfile {
    pub fn key(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
    pub fn has(&self, c: Capability) -> bool {
        self.capabilities.contains(&c)
    }
    /// Clamp a desired effort to the nearest supported level not above it
    /// (or the lowest supported if none is below).
    pub fn clamp_effort(&self, want: EffortLevel) -> Option<EffortLevel> {
        if self.efforts.is_empty() {
            return None;
        }
        let mut best: Option<EffortLevel> = None;
        for e in &self.efforts {
            if *e <= want && best.is_none_or(|b| *e > b) {
                best = Some(*e);
            }
        }
        best.or_else(|| self.efforts.iter().min().copied())
    }
    pub fn max_effort(&self) -> Option<EffortLevel> {
        self.efforts.iter().max().copied()
    }
}

#[cfg(test)]
pub(crate) fn test_model(provider: &str, model: &str, tier: Tier) -> ModelProfile {
    ModelProfile {
        provider: provider.into(),
        model: model.into(),
        display: model.into(),
        tier,
        capabilities: vec![
            Capability::Coding,
            Capability::Reasoning,
            Capability::ToolUse,
        ],
        efforts: vec![
            EffortLevel::Low,
            EffortLevel::Medium,
            EffortLevel::High,
            EffortLevel::Max,
        ],
        context_window: 200_000,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_effort_respects_supported_levels() {
        let mut m = test_model("p", "m", Tier::Strong);
        m.efforts = vec![EffortLevel::Low, EffortLevel::High];
        assert_eq!(m.clamp_effort(EffortLevel::Medium), Some(EffortLevel::Low));
        assert_eq!(m.clamp_effort(EffortLevel::Max), Some(EffortLevel::High));
        assert_eq!(m.clamp_effort(EffortLevel::Minimal), Some(EffortLevel::Low));
        m.efforts.clear();
        assert_eq!(m.clamp_effort(EffortLevel::High), None);
    }

    #[test]
    fn parse_effort() {
        assert_eq!(EffortLevel::parse("Ultra"), Some(EffortLevel::Max));
        assert_eq!(EffortLevel::parse("medium"), Some(EffortLevel::Medium));
        assert_eq!(EffortLevel::parse("weird"), None);
    }
}
