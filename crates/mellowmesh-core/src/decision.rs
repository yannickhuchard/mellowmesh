use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What selecting an option means for the action a decision proposes.
///
/// Absent (the default) means the option carries no approve/reject semantics,
/// so answering with it records the decision as `answered` rather than
/// `approved`. This is what keeps a human's *rejection* from being persisted
/// as an approval: a "Reject" option is marked [`DecisionOutcome::Reject`] and
/// resolves to the `rejected` status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DecisionOutcome {
    Approve,
    Reject,
    Neutral,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub pros: Vec<String>,
    #[serde(default)]
    pub cons: Vec<String>,
    /// Approve/reject semantics of choosing this option. Optional for backward
    /// compatibility; `None` is treated as [`DecisionOutcome::Neutral`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<DecisionOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Decision {
    pub id: String, // e.g., "decision_01HZ..."
    pub title: String,
    pub question: String,
    pub created_by: String,       // e.g., "agent://architecture-reviewer"
    pub required_decider: String, // e.g., "human://yannick"
    pub status: String, // "requested", "discussed", "approved", "rejected", "deferred", etc.
    pub options: Vec<DecisionOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_option_id: Option<String>, // Chosen option ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_timestamp: Option<DateTime<Utc>>,
    /// Authenticated principal that answered the decision (audit trail).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responded_by: Option<String>,
}

impl Decision {
    /// Resolve the terminal status a response should record for `option_id`,
    /// derived from that option's [`DecisionOutcome`]. Returns `None` when the
    /// id does not name an option of this decision, so callers reject invalid
    /// responses instead of recording a meaningless answer.
    pub fn status_for_option(&self, option_id: &str) -> Option<&'static str> {
        let option = self.options.iter().find(|o| o.id == option_id)?;
        Some(match option.outcome {
            Some(DecisionOutcome::Approve) => "approved",
            Some(DecisionOutcome::Reject) => "rejected",
            Some(DecisionOutcome::Neutral) | None => "answered",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(id: &str, outcome: Option<DecisionOutcome>) -> DecisionOption {
        DecisionOption {
            id: id.to_string(),
            label: id.to_string(),
            pros: vec![],
            cons: vec![],
            outcome,
        }
    }

    fn decision(options: Vec<DecisionOption>) -> Decision {
        Decision {
            id: "decision_x".into(),
            title: "t".into(),
            question: "q".into(),
            created_by: "agent://a".into(),
            required_decider: "human://y".into(),
            status: "requested".into(),
            options,
            response_option_id: None,
            response_timestamp: None,
            responded_by: None,
        }
    }

    #[test]
    fn reject_option_resolves_to_rejected_not_approved() {
        let d = decision(vec![
            opt("approve", Some(DecisionOutcome::Approve)),
            opt("reject", Some(DecisionOutcome::Reject)),
        ]);
        assert_eq!(d.status_for_option("approve"), Some("approved"));
        assert_eq!(d.status_for_option("reject"), Some("rejected"));
    }

    #[test]
    fn option_without_outcome_is_answered() {
        let d = decision(vec![opt("keep", None)]);
        assert_eq!(d.status_for_option("keep"), Some("answered"));
    }

    #[test]
    fn unknown_option_is_rejected() {
        let d = decision(vec![opt("keep", None)]);
        assert_eq!(d.status_for_option("nope"), None);
    }
}
