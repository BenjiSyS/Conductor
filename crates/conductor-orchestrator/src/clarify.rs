//! Clarifying questions.
//!
//! Rules enforced here (not just suggested to the model):
//! * at most 7 questions per round, fewer preferred, zero when the task is
//!   already specified;
//! * only blocking and important questions are surfaced by default;
//! * questions already answered by project decisions or settings are dropped;
//! * routine permission questions ("may I run tests?") are never asked;
//! * every question supports "Decide for me", which picks the default.

use serde::{Deserialize, Serialize};

pub const MAX_PER_ROUND: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Blocking,
    Important,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub priority: Priority,
    #[serde(default)]
    pub options: Vec<String>,
    /// Sensible default used by "Decide for me".
    #[serde(default)]
    pub default: Option<String>,
    /// Short topic key used to match against decision memory (e.g. "engine").
    #[serde(default)]
    pub topic: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionRound {
    pub round: u32,
    pub questions: Vec<Question>,
    /// Questions filtered out and why (for transparency in advanced view).
    pub dropped: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum Answer {
    Text(String),
    Option(String),
    DecideForMe,
}

/// A remembered project decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub topic: String,
    pub value: String,
}

const ROUTINE: &[&str] = &[
    "may i install",
    "can i install",
    "may i create",
    "can i create a",
    "may i run",
    "can i run",
    "should i run the tests",
    "may i inspect",
    "can i inspect",
    "may i read",
    "can i read",
    "do you want me to proceed",
    "shall i proceed",
    "is it ok if i",
    "is it okay if i",
];

/// Build a question round from candidate questions (typically parsed from a
/// planner model's JSON), applying all rules.
pub fn build_round(
    round: u32,
    candidates: Vec<Question>,
    decisions: &[Decision],
    include_optional: bool,
) -> QuestionRound {
    let mut dropped = Vec::new();
    let mut kept: Vec<Question> = Vec::new();
    for q in candidates {
        let lower = q.text.to_lowercase();
        if q.text.trim().is_empty() {
            continue;
        }
        if ROUTINE.iter().any(|r| lower.contains(r)) {
            dropped.push((
                q.text,
                "routine permission question — governed by permission policy".into(),
            ));
            continue;
        }
        if let Some(d) = decisions.iter().find(|d| {
            (!q.topic.is_empty() && d.topic.eq_ignore_ascii_case(&q.topic))
                || (!d.topic.is_empty() && lower.contains(&d.topic.to_lowercase()))
        }) {
            dropped.push((
                q.text,
                format!("already decided: {} = {}", d.topic, d.value),
            ));
            continue;
        }
        if q.priority == Priority::Optional && !include_optional {
            dropped.push((q.text, "optional preference — sensible default used".into()));
            continue;
        }
        if kept.iter().any(|k| similar(&k.text, &q.text)) {
            dropped.push((q.text, "duplicate".into()));
            continue;
        }
        kept.push(q);
    }
    kept.sort_by_key(|q| q.priority);
    if kept.len() > MAX_PER_ROUND {
        for q in kept.drain(MAX_PER_ROUND..) {
            dropped.push((q.text, "over the 7-question limit — default used".into()));
        }
    }
    QuestionRound {
        round,
        questions: kept,
        dropped,
    }
}

fn similar(a: &str, b: &str) -> bool {
    let wa: std::collections::HashSet<String> = a
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    let wb: std::collections::HashSet<String> = b
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    let inter = wa.intersection(&wb).count() as f32;
    let union = wa.union(&wb).count().max(1) as f32;
    inter / union > 0.8
}

/// Resolve answers into decisions; "Decide for me" uses the default or the
/// first option. Questions without any usable answer are skipped.
pub fn resolve(round: &QuestionRound, answers: &[(String, Answer)]) -> Vec<Decision> {
    let mut out = Vec::new();
    for q in &round.questions {
        let ans = answers.iter().find(|(id, _)| id == &q.id).map(|(_, a)| a);
        let value = match ans {
            Some(Answer::Text(t)) | Some(Answer::Option(t)) if !t.trim().is_empty() => {
                Some(t.trim().to_string())
            }
            Some(Answer::DecideForMe) | None => {
                q.default.clone().or_else(|| q.options.first().cloned())
            }
            _ => None,
        };
        if let Some(v) = value {
            out.push(Decision {
                topic: if q.topic.is_empty() {
                    q.text.clone()
                } else {
                    q.topic.clone()
                },
                value: v,
            });
        }
    }
    out
}

/// Parse the planner's JSON (`{"questions":[...]}` or a bare array). Lenient:
/// extracts the first JSON value from surrounding prose/code fences.
pub fn parse_questions(model_output: &str) -> Vec<Question> {
    #[derive(Deserialize)]
    struct Wrapper {
        questions: Vec<RawQ>,
    }
    #[derive(Deserialize)]
    struct RawQ {
        #[serde(default)]
        id: Option<String>,
        #[serde(alias = "question")]
        text: String,
        #[serde(default)]
        priority: Option<String>,
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        default: Option<String>,
        #[serde(default)]
        topic: Option<String>,
    }
    let Some(json) = crate::ledger::extract_json(model_output) else {
        return Vec::new();
    };
    let raws: Vec<RawQ> = serde_json::from_str::<Wrapper>(&json)
        .map(|w| w.questions)
        .or_else(|_| serde_json::from_str::<Vec<RawQ>>(&json))
        .unwrap_or_default();
    raws.into_iter()
        .enumerate()
        .map(|(i, r)| Question {
            id: r.id.unwrap_or_else(|| format!("q{}", i + 1)),
            priority: match r
                .priority
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("blocking") | Some("1") => Priority::Blocking,
                Some("optional") | Some("3") => Priority::Optional,
                _ => Priority::Important,
            },
            text: r.text,
            options: r.options,
            default: r.default,
            topic: r.topic.unwrap_or_default(),
        })
        .collect()
}

/// Prompt fragment instructing a planner how to ask questions.
pub const PLANNER_INSTRUCTIONS: &str = "Before planning, decide whether anything blocks a good result. Ask only questions whose answers change the product or design; never ask about things you can detect from the project, nor for permission to run tests, install normal dependencies, create folders or read files. Ask zero questions if the task is clear. At most 7. Reply with JSON: {\"questions\":[{\"id\":\"q1\",\"text\":\"...\",\"priority\":\"blocking|important|optional\",\"options\":[\"...\"],\"default\":\"...\",\"topic\":\"short-key\"}]}";

#[cfg(test)]
mod tests {
    use super::*;

    fn q(id: &str, text: &str, p: Priority, topic: &str) -> Question {
        Question {
            id: id.into(),
            text: text.into(),
            priority: p,
            options: vec!["A".into(), "B".into()],
            default: None,
            topic: topic.into(),
        }
    }

    #[test]
    fn limits_filters_and_sorts() {
        let mut cands = vec![
            q("1", "May I run the tests?", Priority::Blocking, ""),
            q(
                "2",
                "Which engine: Godot or Unity?",
                Priority::Important,
                "engine",
            ),
            q(
                "3",
                "Is multiplayer required?",
                Priority::Blocking,
                "multiplayer",
            ),
            q("4", "Preferred indentation?", Priority::Optional, "style"),
        ];
        for i in 0..10 {
            cands.push(q(
                &format!("x{i}"),
                &format!("Important question number {i} about scope"),
                Priority::Important,
                &format!("t{i}"),
            ));
        }
        let decisions = vec![Decision {
            topic: "engine".into(),
            value: "Godot".into(),
        }];
        let r = build_round(1, cands, &decisions, false);
        assert_eq!(r.questions.len(), MAX_PER_ROUND);
        assert_eq!(r.questions[0].id, "3", "blocking first");
        assert!(!r
            .questions
            .iter()
            .any(|q| q.id == "1" || q.id == "2" || q.id == "4"));
        assert!(r
            .dropped
            .iter()
            .any(|(_, why)| why.contains("already decided")));
        assert!(r.dropped.iter().any(|(_, why)| why.contains("routine")));
    }

    #[test]
    fn zero_questions_is_valid() {
        let r = build_round(1, vec![], &[], false);
        assert!(r.questions.is_empty());
    }

    #[test]
    fn decide_for_me_uses_default() {
        let mut a = q("a", "Web or native?", Priority::Blocking, "platform");
        a.default = Some("Web".into());
        let b = q("b", "Visual style?", Priority::Important, "style");
        let r = build_round(1, vec![a, b], &[], false);
        let d = resolve(
            &r,
            &[
                ("a".into(), Answer::DecideForMe),
                ("b".into(), Answer::Option("B".into())),
            ],
        );
        assert_eq!(
            d,
            vec![
                Decision {
                    topic: "platform".into(),
                    value: "Web".into()
                },
                Decision {
                    topic: "style".into(),
                    value: "B".into()
                }
            ]
        );
    }

    #[test]
    fn parses_model_output() {
        let out = "Sure!\n```json\n{\"questions\":[{\"text\":\"Which platforms?\",\"priority\":\"blocking\",\"options\":[\"Windows\",\"Web\"],\"topic\":\"platforms\"}]}\n```";
        let qs = parse_questions(out);
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0].priority, Priority::Blocking);
        assert_eq!(qs[0].id, "q1");
        assert!(parse_questions("no json here").is_empty());
        assert_eq!(parse_questions("[{\"question\":\"x?\"}]").len(), 1);
    }
}
