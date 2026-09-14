//! P11: persistence-aware feedback classification.
//!
//! MindMemOS insight: users often correct the *agent*, not the memory store.
//! Before writing durable memory, classify how long a correction should live:
//!
//! - `TaskTemporary` — only the current task; do NOT store as durable memory.
//! - `ScenarioSpecific` — keep with explicit applicability conditions.
//! - `LongTerm` — durable facts / preferences / constraints.
//!
//! Classification is keyword-first (deterministic); an LLM can refine later.

use serde::{Deserialize, Serialize};

use crate::text::keyword_hit;

/// How long a corrective / preference signal should persist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceScope {
    TaskTemporary,
    ScenarioSpecific,
    LongTerm,
}

impl PersistenceScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TaskTemporary => "task_temporary",
            Self::ScenarioSpecific => "scenario_specific",
            Self::LongTerm => "long_term",
        }
    }

    /// Only these scopes may create durable observations / concept revisions.
    pub fn is_durable(&self) -> bool {
        matches!(self, Self::LongTerm | Self::ScenarioSpecific)
    }
}

impl std::str::FromStr for PersistenceScope {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "task_temporary" => Ok(Self::TaskTemporary),
            "scenario_specific" => Ok(Self::ScenarioSpecific),
            "long_term" => Ok(Self::LongTerm),
            _ => Err(()),
        }
    }
}

/// A corrective signal extracted from dialogue (implicit or explicit).
#[derive(Debug, Clone, PartialEq)]
pub struct CorrectionSignal {
    pub text: String,
    pub scope: PersistenceScope,
    /// True when the user is correcting prior agent output (implicit feedback).
    pub implicit: bool,
}

/// Keyword tables (zh/en), boundary-aware via `keyword_hit`.
const LONG_TERM_MARKERS: &[&str] = &[
    "以后",
    "一直",
    "永远",
    "默认",
    "以后都",
    "always",
    "from now on",
    "going forward",
    "by default",
    "长期",
    "记住",
];

const TASK_TEMPORARY_MARKERS: &[&str] = &[
    "这次",
    "本次",
    "先这样",
    "临时",
    "just this once",
    "for this task",
    "this time only",
    "暂时",
    "这回",
    "就这一次",
];

const SCENARIO_MARKERS: &[&str] = &[
    "在这个项目",
    "本项目",
    "该仓库",
    "这个仓库",
    "in this repo",
    "in this project",
    "for this codebase",
    "场景",
];

/// Implicit correction cues (user is fixing the agent's output).
const IMPLICIT_CORRECTION_CUES: &[&str] = &[
    "不对",
    "错了",
    "不是这样",
    "应该是",
    "你搞错",
    "搞错了",
    "wrong",
    "that's incorrect",
    "not quite",
    "actually",
    "更正",
    "纠正",
];

/// Classify persistence scope from free text. Order: temporary wins over
/// long-term (a sentence can contain both); long-term over scenario default.
pub fn classify_persistence(text: &str) -> PersistenceScope {
    let lower = text.to_lowercase();
    if TASK_TEMPORARY_MARKERS.iter().any(|k| keyword_hit(&lower, k)) {
        return PersistenceScope::TaskTemporary;
    }
    if LONG_TERM_MARKERS.iter().any(|k| keyword_hit(&lower, k)) {
        return PersistenceScope::LongTerm;
    }
    if SCENARIO_MARKERS.iter().any(|k| keyword_hit(&lower, k)) {
        return PersistenceScope::ScenarioSpecific;
    }
    // Default: scenario-specific — durable enough to help, but not global law.
    PersistenceScope::ScenarioSpecific
}

/// Detect whether text looks like a correction aimed at the agent.
pub fn is_implicit_correction(text: &str) -> bool {
    let lower = text.to_lowercase();
    IMPLICIT_CORRECTION_CUES
        .iter()
        .any(|k| keyword_hit(&lower, k))
}

/// Build a correction signal from a user message (implicit feedback path).
pub fn detect_correction(user_text: &str) -> Option<CorrectionSignal> {
    if !is_implicit_correction(user_text) {
        return None;
    }
    let scope = classify_persistence(user_text);
    Some(CorrectionSignal {
        text: user_text.to_string(),
        scope,
        implicit: true,
    })
}

/// Explicit feedback path: user is talking to the memory system.
pub fn explicit_feedback_signal(feedback_text: &str) -> CorrectionSignal {
    CorrectionSignal {
        text: feedback_text.to_string(),
        scope: classify_persistence(feedback_text),
        implicit: false,
    }
}

/// Gate: should this signal produce a durable write?
pub fn should_persist(signal: &CorrectionSignal) -> bool {
    signal.scope.is_durable()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_markers_win() {
        assert_eq!(
            classify_persistence("这次先用 Python，以后再说"),
            PersistenceScope::TaskTemporary
        );
    }

    #[test]
    fn long_term_markers() {
        assert_eq!(
            classify_persistence("以后都用 cargo test 验收"),
            PersistenceScope::LongTerm
        );
    }

    #[test]
    fn project_scope() {
        assert_eq!(
            classify_persistence("在这个项目里用 Windows 路径"),
            PersistenceScope::ScenarioSpecific
        );
    }

    #[test]
    fn default_is_scenario() {
        assert_eq!(
            classify_persistence("测试要过"),
            PersistenceScope::ScenarioSpecific
        );
    }

    #[test]
    fn implicit_correction_detected() {
        assert!(is_implicit_correction("不对，应该是 not_has"));
        assert!(!is_implicit_correction("帮我看看这个文件"));
    }

    #[test]
    fn temporary_correction_not_durable() {
        let s = detect_correction("错了，这次先别写测试").unwrap();
        assert!(!should_persist(&s));
    }

    #[test]
    fn long_term_correction_is_durable() {
        let s = detect_correction("不对，以后都必须先跑 clippy").unwrap();
        assert!(should_persist(&s));
        assert_eq!(s.scope, PersistenceScope::LongTerm);
    }
}
