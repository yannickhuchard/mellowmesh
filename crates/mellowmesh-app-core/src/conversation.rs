//! Conversations derived from topics.
//!
//! The hub has no "conversation" object: a conversation is a convention over
//! topic names. The conversation **id is the topic root** (see the crate
//! README for the full table):
//!
//! | Topic                                   | Kind           | Conversation id (root)      |
//! |-----------------------------------------|----------------|-----------------------------|
//! | `_agent.<owner>.<name>.inbox` / `.outbox` | `Agent`      | `_agent.<owner>.<name>`     |
//! | `_project.<name>[.**]`                  | `Squad`/`Group`| `_project.<name>`           |
//! | `_forum.<name>[.**]`                    | `Squad`/`Group`| `_forum.<name>`             |
//! | `_chat.dm.<a>.<b>`                      | `Person`       | `_chat.dm.<a>.<b>` (sorted) |
//! | `_chat.group.<name>[.**]`               | `Group`        | `_chat.group.<name>`        |
//! | `_org.<org>[.**]`                       | `Organization` | `_org.<org>`                |
//!
//! Anything else (`_task.*`, `_decision.*`, `_system.*`, application topics)
//! is not a conversation and surfaces through events / cards instead.

use crate::error::MeshError;
use crate::identity::{agent_id_from_path, agent_topic_path};

/// What kind of conversation a topic root represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ConversationKind {
    /// 1:1 with one agent (its inbox; the agent answers on its outbox).
    Agent,
    /// Several agents working on one topic/project (no other human posts).
    Squad,
    /// 1:1 between two humans.
    Person,
    /// Humans and agents together.
    Group,
    /// Organization-wide channel.
    Organization,
}

/// A topic classified as part of a conversation.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConversationRef {
    /// The topic root; stable id of the conversation.
    pub id: String,
    pub kind: ConversationKind,
    /// The agent URI for `Agent` conversations (`agent://<owner>/<name>`).
    pub agent_id: Option<String>,
    /// For `Person` conversations: the two human handles, sorted.
    pub dm_handles: Vec<String>,
}

/// Map a topic to the conversation it belongs to, if any.
///
/// Squad vs Group cannot be decided from the topic alone for `_project` and
/// `_forum` (it depends on who participates); this returns `Squad` and the
/// session upgrades it to `Group` when another human has posted.
#[uniffi::export]
pub fn conversation_for_topic(topic: String) -> Option<ConversationRef> {
    classify_topic(&topic)
}

pub(crate) fn classify_topic(topic: &str) -> Option<ConversationRef> {
    let lower = topic.trim().to_lowercase();
    let segs: Vec<&str> = lower.split('.').collect();
    if segs.iter().any(|s| s.is_empty() || is_wildcard(s)) {
        return None;
    }
    let simple = |id: String, kind| ConversationRef {
        id,
        kind,
        agent_id: None,
        dm_handles: vec![],
    };
    match segs.as_slice() {
        // _agent.<owner>.<name...>.inbox|outbox (at least owner + name).
        ["_agent", path @ .., leaf]
            if path.len() >= 2 && (*leaf == "inbox" || *leaf == "outbox") =>
        {
            Some(ConversationRef {
                id: format!("_agent.{}", path.join(".")),
                kind: ConversationKind::Agent,
                agent_id: Some(agent_id_from_path(path)),
                dm_handles: vec![],
            })
        }
        ["_project", name, ..] => Some(simple(format!("_project.{name}"), ConversationKind::Squad)),
        ["_forum", name, ..] => Some(simple(format!("_forum.{name}"), ConversationKind::Squad)),
        ["_chat", "dm", a, b] if a != b => {
            let mut handles = [a.to_string(), b.to_string()];
            handles.sort();
            Some(ConversationRef {
                id: format!("_chat.dm.{}.{}", handles[0], handles[1]),
                kind: ConversationKind::Person,
                agent_id: None,
                dm_handles: handles.to_vec(),
            })
        }
        ["_chat", "group", name, ..] => Some(simple(
            format!("_chat.group.{name}"),
            ConversationKind::Group,
        )),
        ["_org", org, ..] => Some(simple(
            format!("_org.{org}"),
            ConversationKind::Organization,
        )),
        _ => None,
    }
}

fn is_wildcard(seg: &str) -> bool {
    matches!(seg, "*" | "**" | ">")
}

/// Validate a conversation id coming from a shell: it must be exactly a
/// conversation root (as produced by [`classify_topic`]).
pub(crate) fn parse_conversation_id(id: &str) -> Result<ConversationRef, MeshError> {
    // For agents the root itself is not a chat topic; probe with `.inbox`.
    let probe = if id.trim().to_lowercase().starts_with("_agent.") {
        format!("{id}.inbox")
    } else {
        id.to_string()
    };
    match classify_topic(&probe) {
        Some(c) if c.id == id.trim().to_lowercase() => Ok(c),
        _ => Err(MeshError::invalid(format!(
            "'{id}' is not a conversation id"
        ))),
    }
}

/// Server-side history pattern covering a conversation.
pub(crate) fn history_pattern(conv: &ConversationRef) -> String {
    match conv.kind {
        // inbox + outbox (+ status/heartbeat, filtered out by `is_chat_topic`).
        ConversationKind::Agent => format!("{}.*", conv.id),
        ConversationKind::Person => conv.id.clone(),
        _ => format!("{}.**", conv.id),
    }
}

/// Whether a topic is chat traffic of `conv` (excludes agent heartbeats etc).
pub(crate) fn is_chat_topic(conv: &ConversationRef, topic: &str) -> bool {
    classify_topic(topic).is_some_and(|c| c.id == conv.id)
}

/// Topic a message sent by the user into `conv` is published on.
pub(crate) fn send_topic(conv: &ConversationRef) -> String {
    match conv.kind {
        ConversationKind::Agent => format!("{}.inbox", conv.id),
        _ => conv.id.clone(),
    }
}

/// Root id of the 1:1 agent conversation for an agent URI.
pub fn agent_conversation_id(agent_id: &str) -> Option<String> {
    agent_topic_path(agent_id)
        .filter(|p| p.contains('.'))
        .map(|p| format!("_agent.{p}"))
}

/// Canonical DM topic between two human handles (order-independent).
#[uniffi::export]
pub fn dm_conversation_id(a: String, b: String) -> Result<String, MeshError> {
    let clean = |h: &str| -> Result<String, MeshError> {
        let h = h
            .strip_prefix("human://")
            .unwrap_or(h)
            .trim()
            .to_lowercase();
        let ok = !h.is_empty()
            && h.len() <= 64
            && h.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if ok {
            Ok(h)
        } else {
            Err(MeshError::invalid(format!("invalid human handle '{h}'")))
        }
    };
    let (a, b) = (clean(&a)?, clean(&b)?);
    if a == b {
        return Err(MeshError::invalid("a DM needs two different people"));
    }
    let (x, y) = if a < b { (a, b) } else { (b, a) };
    Ok(format!("_chat.dm.{x}.{y}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(topic: &str) -> Option<ConversationKind> {
        classify_topic(topic).map(|c| c.kind)
    }

    #[test]
    fn agent_inbox_and_outbox() {
        let c = classify_topic("_agent.yannick.coder.inbox").unwrap();
        assert_eq!(c.kind, ConversationKind::Agent);
        assert_eq!(c.id, "_agent.yannick.coder");
        assert_eq!(c.agent_id.as_deref(), Some("agent://yannick/coder"));
        assert_eq!(
            classify_topic("_agent.yannick.coder.outbox").unwrap().id,
            "_agent.yannick.coder"
        );
        // Heartbeats/status are not chat.
        assert_eq!(kind("_agent.yannick.coder.heartbeat"), None);
        assert_eq!(kind("_agent.coder.inbox"), None); // needs owner + name
        assert_eq!(
            agent_conversation_id("agent://yannick/coder").as_deref(),
            Some("_agent.yannick.coder")
        );
    }

    #[test]
    fn squads_people_groups_orgs() {
        assert_eq!(kind("_project.apollo"), Some(ConversationKind::Squad));
        assert_eq!(
            classify_topic("_project.apollo.build.logs").unwrap().id,
            "_project.apollo"
        );
        assert_eq!(kind("_forum.general"), Some(ConversationKind::Squad));
        assert_eq!(kind("_chat.group.launch"), Some(ConversationKind::Group));
        assert_eq!(
            kind("_org.acme.announcements"),
            Some(ConversationKind::Organization)
        );
        let dm = classify_topic("_chat.dm.zoe.adam").unwrap();
        assert_eq!(dm.kind, ConversationKind::Person);
        assert_eq!(dm.id, "_chat.dm.adam.zoe");
        assert_eq!(dm.dm_handles, vec!["adam", "zoe"]);
        assert_eq!(kind("_chat.dm.adam.adam"), None);
        assert_eq!(kind("_chat.dm.a.b.c"), None);
    }

    #[test]
    fn non_conversations() {
        for t in [
            "_task.t1.progress",
            "_decision.d1.requested",
            "_system.registry.agent",
            "builds.main",
            "_project",
            "_project.*",
            "_project..x",
            "",
        ] {
            assert_eq!(kind(t), None, "{t}");
        }
    }

    #[test]
    fn case_insensitive_roots() {
        assert_eq!(
            classify_topic("_Project.Apollo.Chat").unwrap().id,
            "_project.apollo"
        );
    }

    #[test]
    fn conversation_id_round_trip_and_routing() {
        let c = parse_conversation_id("_agent.yannick.coder").unwrap();
        assert_eq!(send_topic(&c), "_agent.yannick.coder.inbox");
        assert_eq!(history_pattern(&c), "_agent.yannick.coder.*");
        assert!(is_chat_topic(&c, "_agent.yannick.coder.outbox"));
        assert!(!is_chat_topic(&c, "_agent.yannick.coder.heartbeat"));

        let p = parse_conversation_id("_project.apollo").unwrap();
        assert_eq!(send_topic(&p), "_project.apollo");
        assert_eq!(history_pattern(&p), "_project.apollo.**");

        assert!(parse_conversation_id("_project.apollo.chat").is_err());
        assert!(parse_conversation_id("_task.x").is_err());
        assert!(parse_conversation_id("_chat.dm.zoe.adam").is_err()); // not canonical
        assert!(parse_conversation_id("_chat.dm.adam.zoe").is_ok());
    }

    #[test]
    fn dm_ids() {
        assert_eq!(
            dm_conversation_id("human://Zoe".into(), "adam".into()).unwrap(),
            "_chat.dm.adam.zoe"
        );
        assert!(dm_conversation_id("a".into(), "a".into()).is_err());
        assert!(dm_conversation_id("a.b".into(), "c".into()).is_err());
    }
}
