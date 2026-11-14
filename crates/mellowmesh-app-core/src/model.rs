//! FFI view models: plain records the shells render directly, and the pure
//! conversions from hub types (`mellowmesh_core`) into them. Every string that
//! originates on the fabric is sanitized here, once, so shells never see
//! terminal escapes or bidi overrides.

use std::collections::HashMap;

use mellowmesh_client::sanitize::{sanitize_line, sanitize_text};
use mellowmesh_core::decision::{Decision, DecisionOutcome};
use mellowmesh_core::message::Message;
use mellowmesh_core::task::Task;

use crate::conversation::{ConversationKind, ConversationRef};
use crate::identity::{
    accent_of, participant, short_name, stable_hash, Accent, ParticipantRef, Principal,
    PrincipalKind,
};
use crate::meshui::{card_from_value, is_valid_action_id, MeshUiCard};
use crate::presence::Presence;

/// Payload key carrying a MeshUI card.
pub const MESH_UI_KEY: &str = "mesh_ui";
/// Payload key carrying a MeshUI action reply.
pub const UI_ACTION_KEY: &str = "ui_action";
/// Content type of a MeshUI action reply.
pub const UI_ACTION_CONTENT_TYPE: &str = "application/vnd.mellowmesh.ui-action+json";
/// Longest conversation preview, in characters.
const PREVIEW_CHARS: usize = 140;

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AgentCard {
    pub id: String,
    pub display_name: String,
    /// Owner URI (`human://…`).
    pub owner: String,
    /// True when `owner` is the session's principal: these are "my agents",
    /// the center of the experience. Sorted first in `HomeSnapshot.agents`.
    pub is_mine: bool,
    /// `human-piloted`, `autonomous`, ...
    pub mode: String,
    pub capabilities: Vec<String>,
    pub presence: Presence,
    pub last_seen_ms: Option<i64>,
    pub current_task_title: Option<String>,
    /// Stable seed for a generative avatar.
    pub avatar_seed: u64,
    pub accent: Accent,
    /// The 1:1 conversation id with this agent (`_agent.<owner>.<name>`).
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum OptionOutcome {
    Approve,
    Reject,
    Neutral,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OptionCard {
    pub id: String,
    pub label: String,
    pub outcome: OptionOutcome,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DecisionCard {
    pub id: String,
    pub title: String,
    pub question: String,
    pub options: Vec<OptionCard>,
    pub created_by: String,
    pub required_decider: String,
    /// `requested`, `discussed`, `approved`, `rejected`, `answered`, ...
    pub status: String,
    /// When the decision was requested (from its `_decision.<id>.requested`
    /// event); 0 when that event is outside the fetched history window.
    pub created_ms: i64,
    /// True when the session's principal is the `required_decider`.
    pub for_me: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TaskCard {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub created_by: String,
    pub claimed_by: Option<String>,
    pub claim_expires_ms: Option<i64>,
}

/// A reply produced by tapping a MeshUI action.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UiActionReply {
    /// Id of the message whose card carried the action.
    pub card_message_id: String,
    pub action_id: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ChatMessage {
    pub id: String,
    pub topic: String,
    pub from: String,
    pub from_kind: PrincipalKind,
    pub from_display: String,
    /// Sanitized body (escape sequences, control and bidi characters removed).
    pub body: String,
    pub content_type: String,
    pub ts_ms: i64,
    /// Sent by the session's principal.
    pub is_mine: bool,
    pub parent_id: Option<String>,
    /// A validated MeshUI card from `payload.mesh_ui` (dropped if invalid).
    pub ui: Option<MeshUiCard>,
    /// Set when this message is a MeshUI action reply.
    pub ui_action: Option<UiActionReply>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ConversationSummary {
    /// Topic root (see `conversation_for_topic`).
    pub id: String,
    pub kind: ConversationKind,
    pub title: String,
    pub participants: Vec<ParticipantRef>,
    pub last_message_preview: Option<String>,
    /// 0 when the conversation has no message in the fetched window.
    pub last_activity_ms: i64,
    /// Messages from others newer than the read marker (see `mark_read`).
    pub unread: u32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct HomeSnapshot {
    pub me: Principal,
    /// My agents first, then everyone else's; each group sorted by presence
    /// then name.
    pub agents: Vec<AgentCard>,
    /// Decisions still awaiting an answer, mine (`for_me`) first.
    pub pending_decisions: Vec<DecisionCard>,
    /// Tasks not in a terminal state.
    pub active_tasks: Vec<TaskCard>,
    /// Most recent activity first.
    pub conversations: Vec<ConversationSummary>,
    pub generated_ms: i64,
}

/// Display names known to the session (agent registry), keyed by lowercase
/// URI.
#[derive(Debug, Clone, Default)]
pub(crate) struct Directory {
    names: HashMap<String, String>,
}

impl Directory {
    pub(crate) fn insert(&mut self, id: &str, name: &str) {
        if !name.trim().is_empty() {
            self.names
                .insert(id.to_lowercase(), sanitize_line(name.trim()));
        }
    }

    pub(crate) fn name_of(&self, id: &str) -> String {
        self.names
            .get(&id.to_lowercase())
            .cloned()
            .unwrap_or_else(|| short_name(id))
    }

    pub(crate) fn participant(&self, id: &str) -> ParticipantRef {
        participant(id, Some(&self.name_of(id)))
    }
}

pub(crate) fn ts_ms(msg: &Message) -> i64 {
    msg.timestamp.timestamp_millis()
}

pub(crate) fn is_terminal_task(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "completed" | "cancelled" | "canceled" | "failed" | "done"
    )
}

pub(crate) fn is_pending_decision(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "requested" | "discussed"
    )
}

pub(crate) fn same_principal(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub(crate) fn decision_card(d: &Decision, me: &str, created_ms: i64) -> DecisionCard {
    DecisionCard {
        id: d.id.clone(),
        title: sanitize_line(&d.title),
        question: sanitize_text(&d.question),
        options: d
            .options
            .iter()
            .map(|o| OptionCard {
                id: o.id.clone(),
                label: sanitize_line(&o.label),
                outcome: match o.outcome {
                    Some(DecisionOutcome::Approve) => OptionOutcome::Approve,
                    Some(DecisionOutcome::Reject) => OptionOutcome::Reject,
                    Some(DecisionOutcome::Neutral) | None => OptionOutcome::Neutral,
                },
                pros: o.pros.iter().map(|s| sanitize_line(s)).collect(),
                cons: o.cons.iter().map(|s| sanitize_line(s)).collect(),
            })
            .collect(),
        created_by: d.created_by.clone(),
        required_decider: d.required_decider.clone(),
        status: sanitize_line(&d.status),
        created_ms,
        for_me: same_principal(&d.required_decider, me),
    }
}

pub(crate) fn task_card(t: &Task) -> TaskCard {
    TaskCard {
        id: t.id.clone(),
        title: sanitize_line(&t.title),
        status: sanitize_line(&t.status),
        priority: sanitize_line(&t.priority),
        created_by: t.created_by.clone(),
        claimed_by: t.claimed_by.clone(),
        claim_expires_ms: t
            .claim_expires_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis()),
    }
}

pub(crate) fn chat_message(msg: &Message, me: &str, dir: &Directory) -> ChatMessage {
    let payload = msg.payload.as_ref();
    let ui = payload
        .and_then(|p| p.get(MESH_UI_KEY))
        .and_then(|v| card_from_value(v).ok());
    let ui_action = payload.and_then(|p| p.get(UI_ACTION_KEY)).and_then(|v| {
        let card_message_id = v.get("card_message_id")?.as_str()?;
        let action_id = v.get("action_id")?.as_str()?;
        (is_valid_action_id(action_id) && card_message_id.len() <= 128).then(|| UiActionReply {
            card_message_id: sanitize_line(card_message_id),
            action_id: action_id.to_string(),
        })
    });
    ChatMessage {
        id: msg.id.clone(),
        topic: msg.topic.clone(),
        from: msg.from.clone(),
        from_kind: PrincipalKind::of(&msg.from),
        from_display: dir.name_of(&msg.from),
        body: sanitize_text(&msg.body),
        content_type: sanitize_line(&msg.content_type),
        ts_ms: ts_ms(msg),
        is_mine: same_principal(&msg.from, me),
        parent_id: msg.parent_id.clone(),
        ui,
        ui_action,
    }
}

pub(crate) fn preview(msg: &ChatMessage) -> String {
    let text = if msg.body.trim().is_empty() {
        msg.ui.as_ref().map(|c| c.title.clone()).unwrap_or_default()
    } else {
        msg.body.clone()
    };
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > PREVIEW_CHARS {
        let mut s: String = line.chars().take(PREVIEW_CHARS).collect();
        s.push('…');
        s
    } else {
        line
    }
}

/// Title of a conversation as shown in the list.
pub(crate) fn conversation_title(conv: &ConversationRef, me: &str, dir: &Directory) -> String {
    match conv.kind {
        ConversationKind::Agent => conv
            .agent_id
            .as_deref()
            .map(|a| dir.name_of(a))
            .unwrap_or_else(|| conv.id.clone()),
        ConversationKind::Person => {
            let me_handle = short_name(me).to_lowercase();
            let other = conv
                .dm_handles
                .iter()
                .find(|h| **h != me_handle)
                .cloned()
                .unwrap_or_else(|| conv.dm_handles.join(" & "));
            dir.name_of(&format!("human://{other}"))
        }
        _ => {
            let name = conv.id.rsplit('.').next().unwrap_or(&conv.id);
            pretty(name)
        }
    }
}

/// `release-train_v2` → `Release train v2`.
fn pretty(name: &str) -> String {
    let spaced = sanitize_line(&name.replace(['-', '_'], " "));
    let mut chars = spaced.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => spaced,
    }
}

/// Stable avatar/accent for anything with an id (re-exported for graph use).
pub(crate) fn visual(id: &str) -> (u64, Accent) {
    (stable_hash(id), accent_of(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn msg(topic: &str, from: &str, body: &str, payload: Option<serde_json::Value>) -> Message {
        Message {
            id: "msg_1".into(),
            topic: topic.into(),
            from: from.into(),
            owner: None,
            timestamp: chrono::Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
            content_type: "text/plain".into(),
            body: body.into(),
            headers: None,
            payload,
            parent_id: None,
        }
    }

    #[test]
    fn chat_message_sanitizes_and_parses_cards() {
        let dir = Directory::default();
        let m = msg(
            "_agent.y.coder.outbox",
            "agent://y/coder",
            "done \u{1b}[2J\u{202e}",
            Some(serde_json::json!({
                "mesh_ui": {"version": 1, "title": "Build", "blocks": [{"type": "text", "markdown": "ok"}]}
            })),
        );
        let c = chat_message(&m, "human://y", &dir);
        assert_eq!(c.body, "done ");
        assert_eq!(c.from_kind, PrincipalKind::Agent);
        assert_eq!(c.from_display, "coder");
        assert!(!c.is_mine);
        assert_eq!(c.ts_ms, 1_700_000_000_000);
        assert_eq!(c.ui.unwrap().title, "Build");

        // An invalid card is dropped, the message is kept.
        let bad = msg(
            "_project.x",
            "human://y",
            "hi",
            Some(
                serde_json::json!({"mesh_ui": {"version": 1, "blocks": [{"type": "text", "markdown": "<script>"}]}}),
            ),
        );
        let c = chat_message(&bad, "HUMAN://y", &dir);
        assert!(c.ui.is_none());
        assert!(c.is_mine);
    }

    #[test]
    fn ui_action_replies_are_recognized() {
        let m = msg(
            "_project.x",
            "human://y",
            "Retry",
            Some(
                serde_json::json!({"ui_action": {"card_message_id": "msg_9", "action_id": "retry"}}),
            ),
        );
        let c = chat_message(&m, "human://y", &Directory::default());
        assert_eq!(
            c.ui_action,
            Some(UiActionReply {
                card_message_id: "msg_9".into(),
                action_id: "retry".into()
            })
        );
    }

    #[test]
    fn titles() {
        let mut dir = Directory::default();
        dir.insert("agent://y/coder", "Coder Bot");
        let a = crate::conversation::classify_topic("_agent.y.coder.inbox").unwrap();
        assert_eq!(conversation_title(&a, "human://y", &dir), "Coder Bot");
        let p = crate::conversation::classify_topic("_project.release-train").unwrap();
        assert_eq!(conversation_title(&p, "human://y", &dir), "Release train");
        let dm = crate::conversation::classify_topic("_chat.dm.y.zoe").unwrap();
        assert_eq!(conversation_title(&dm, "human://y", &dir), "zoe");
    }
}
