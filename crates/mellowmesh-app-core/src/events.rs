//! Live events: mapping fabric messages to typed UI events.
//!
//! What the reference hub actually emits (checked against
//! `mellowmesh-daemon`): `_decision.<id>.requested` (payload: the decision),
//! `_decision.<id>.responded` (payload: `decision_id`, `status`, `option_id`,
//! `responded_by`) and `_task.<id>.reclaimed` (payload: `task_id`,
//! `previous_claimant`). The hub does **not** announce claims or completions;
//! `_task.<id>.claimed` / `.completed` are a client convention (used by the
//! standalone WASM engine and agents that announce their work), and
//! `_task.<id>.progress` is the lease heartbeat. Presence comes from
//! `_agent.<path>.heartbeat` / `.status`.

use mellowmesh_client::sanitize::sanitize_line;
use mellowmesh_core::decision::Decision;
use mellowmesh_core::message::Message;

use crate::conversation::{classify_topic, ConversationKind};
use crate::identity::PrincipalKind;
use crate::model::{chat_message, decision_card, ts_ms, ChatMessage, DecisionCard, Directory};
use crate::presence::Presence;
use crate::snapshot::{explicit_presence, AgentPaths};

/// Implemented by the shell (Swift/Kotlin/C#) to receive live events.
///
/// Events are delivered on a dedicated background thread owned by the
/// subscription (never on the UI thread and never on a runtime worker), one
/// at a time and in order. Hop to the main thread before touching UI. It is
/// safe to call `MeshSession` methods from inside `on_event`.
#[uniffi::export(callback_interface)]
pub trait MeshEventListener: Send + Sync {
    fn on_event(&self, event: MeshEvent);
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum MeshEvent {
    /// A chat message in a conversation (see `conversation_for_topic`).
    MessageReceived {
        conversation_id: String,
        kind: ConversationKind,
        message: ChatMessage,
    },
    DecisionCreated {
        decision: DecisionCard,
    },
    DecisionResolved {
        decision_id: String,
        status: String,
        option_id: Option<String>,
        responded_by: Option<String>,
    },
    TaskClaimed {
        task_id: String,
        agent: String,
    },
    /// The hub's lease sweeper returned the task to `open`; `agent` is the
    /// previous claimant.
    TaskReclaimed {
        task_id: String,
        agent: String,
    },
    TaskCompleted {
        task_id: String,
        agent: String,
    },
    /// A lease heartbeat / progress note on `_task.<id>.progress`.
    TaskProgress {
        task_id: String,
        agent: String,
        note: String,
    },
    PresenceChanged {
        agent_id: String,
        presence: Presence,
    },
    /// The live stream (re)connected or dropped; the subscription retries
    /// with backoff until cancelled.
    StreamStatus {
        connected: bool,
        detail: String,
    },
    /// Any other message matching the subscription pattern.
    Other {
        topic: String,
        from: String,
        preview: String,
        ts_ms: i64,
    },
}

pub(crate) struct EventContext {
    pub me: String,
    pub dir: Directory,
    pub paths: AgentPaths,
}

fn payload_str<'a>(msg: &'a Message, key: &str) -> Option<&'a str> {
    msg.payload.as_ref()?.get(key)?.as_str()
}

/// Map one fabric message to zero or more events (pure).
pub(crate) fn map_event(msg: &Message, ctx: &EventContext) -> Vec<MeshEvent> {
    let lower = msg.topic.to_lowercase();
    let segs: Vec<&str> = lower.split('.').collect();
    let id_of = |i: usize| -> String {
        // Keep the original casing of the id segment.
        msg.topic.split('.').nth(i).unwrap_or_default().to_string()
    };
    match segs.as_slice() {
        ["_decision", _, "requested"] => {
            let decision = msg
                .payload
                .clone()
                .and_then(|p| serde_json::from_value::<Decision>(p).ok());
            match decision {
                Some(d) => vec![MeshEvent::DecisionCreated {
                    decision: decision_card(&d, &ctx.me, ts_ms(msg)),
                }],
                None => vec![other(msg)],
            }
        }
        ["_decision", _, "responded"] => vec![MeshEvent::DecisionResolved {
            decision_id: payload_str(msg, "decision_id")
                .map(String::from)
                .unwrap_or_else(|| id_of(1)),
            status: payload_str(msg, "status")
                .map(sanitize_line)
                .unwrap_or_else(|| "answered".to_string()),
            option_id: payload_str(msg, "option_id").map(String::from),
            responded_by: payload_str(msg, "responded_by").map(String::from),
        }],
        ["_task", _, event] => {
            let task_id = payload_str(msg, "task_id")
                .map(String::from)
                .unwrap_or_else(|| id_of(1));
            let sender = || msg.from.clone();
            match *event {
                "claimed" => vec![MeshEvent::TaskClaimed {
                    task_id,
                    agent: payload_str(msg, "claimed_by")
                        .map(String::from)
                        .unwrap_or_else(sender),
                }],
                "reclaimed" => vec![MeshEvent::TaskReclaimed {
                    task_id,
                    agent: payload_str(msg, "previous_claimant")
                        .map(String::from)
                        .unwrap_or_default(),
                }],
                "completed" => vec![MeshEvent::TaskCompleted {
                    task_id,
                    agent: payload_str(msg, "claimed_by")
                        .map(String::from)
                        .unwrap_or_else(sender),
                }],
                "progress" => vec![MeshEvent::TaskProgress {
                    task_id,
                    agent: sender(),
                    note: crate::model::preview(&chat_message(msg, &ctx.me, &ctx.dir)),
                }],
                _ => vec![other(msg)],
            }
        }
        _ => {
            if let Some(conv) = classify_topic(&msg.topic) {
                let message = chat_message(msg, &ctx.me, &ctx.dir);
                // A human posting in a squad makes it a group.
                let kind = if conv.kind == ConversationKind::Squad
                    && matches!(
                        message.from_kind,
                        PrincipalKind::Human | PrincipalKind::Interface
                    )
                    && !message.is_mine
                {
                    ConversationKind::Group
                } else {
                    conv.kind
                };
                return vec![MeshEvent::MessageReceived {
                    conversation_id: conv.id,
                    kind,
                    message,
                }];
            }
            if let Some((agent_id, leaf)) = ctx.paths.resolve(&msg.topic) {
                if let Some(presence) = explicit_presence(msg, &leaf) {
                    return vec![MeshEvent::PresenceChanged { agent_id, presence }];
                }
            }
            if lower.starts_with("_system.presence") {
                if let (Some(agent), Some(p)) = (
                    payload_str(msg, "agent_id"),
                    payload_str(msg, "status").and_then(crate::presence::presence_from_status),
                ) {
                    return vec![MeshEvent::PresenceChanged {
                        agent_id: agent.to_string(),
                        presence: p,
                    }];
                }
            }
            vec![other(msg)]
        }
    }
}

fn other(msg: &Message) -> MeshEvent {
    let body = sanitize_line(&msg.body);
    let preview: String = body.chars().take(140).collect();
    MeshEvent::Other {
        topic: sanitize_line(&msg.topic),
        from: msg.from.clone(),
        preview,
        ts_ms: ts_ms(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::tests::{agent, msg_at, NOW};

    fn ctx() -> EventContext {
        let agents = vec![agent("agent://yannick/coder", "Coder", "human://yannick")];
        EventContext {
            me: "human://yannick".into(),
            dir: crate::snapshot::directory_of(&agents),
            paths: AgentPaths::new(&agents),
        }
    }

    fn with_payload(mut m: Message, p: serde_json::Value) -> Message {
        m.payload = Some(p);
        m
    }

    #[test]
    fn decision_events() {
        let d = serde_json::json!({
            "id": "d1", "title": "Ship?", "question": "Ship it?",
            "created_by": "agent://yannick/coder", "required_decider": "human://yannick",
            "status": "requested",
            "options": [{"id": "yes", "label": "Yes", "outcome": "approve"},
                        {"id": "no", "label": "No", "outcome": "reject"}]
        });
        let ev = map_event(
            &with_payload(
                msg_at("_decision.d1.requested", "agent://yannick/coder", "", NOW),
                d,
            ),
            &ctx(),
        );
        let [MeshEvent::DecisionCreated { decision }] = ev.as_slice() else {
            panic!("{ev:?}")
        };
        assert_eq!(decision.id, "d1");
        assert!(decision.for_me);
        assert_eq!(
            decision.options[1].outcome,
            crate::model::OptionOutcome::Reject
        );
        assert_eq!(decision.created_ms, NOW);

        let ev = map_event(
            &with_payload(
                msg_at("_decision.d1.responded", "human://yannick", "", NOW),
                serde_json::json!({"decision_id": "d1", "status": "rejected", "option_id": "no", "responded_by": "human://yannick"}),
            ),
            &ctx(),
        );
        assert_eq!(
            ev,
            vec![MeshEvent::DecisionResolved {
                decision_id: "d1".into(),
                status: "rejected".into(),
                option_id: Some("no".into()),
                responded_by: Some("human://yannick".into()),
            }]
        );
    }

    #[test]
    fn task_events() {
        let c = ctx();
        let reclaimed = with_payload(
            msg_at("_task.T1.reclaimed", "test-node", "", NOW),
            serde_json::json!({"task_id": "T1", "previous_claimant": "agent://yannick/coder", "status": "open"}),
        );
        assert_eq!(
            map_event(&reclaimed, &c),
            vec![MeshEvent::TaskReclaimed {
                task_id: "T1".into(),
                agent: "agent://yannick/coder".into()
            }]
        );
        assert_eq!(
            map_event(
                &msg_at("_task.T1.claimed", "agent://yannick/coder", "", NOW),
                &c
            ),
            vec![MeshEvent::TaskClaimed {
                task_id: "T1".into(),
                agent: "agent://yannick/coder".into()
            }]
        );
        assert_eq!(
            map_event(
                &msg_at("_task.t2.completed", "agent://yannick/coder", "", NOW),
                &c
            ),
            vec![MeshEvent::TaskCompleted {
                task_id: "t2".into(),
                agent: "agent://yannick/coder".into()
            }]
        );
        assert_eq!(
            map_event(
                &msg_at(
                    "_task.t2.progress",
                    "agent://yannick/coder",
                    "50%  done",
                    NOW
                ),
                &c
            ),
            vec![MeshEvent::TaskProgress {
                task_id: "t2".into(),
                agent: "agent://yannick/coder".into(),
                note: "50% done".into()
            }]
        );
    }

    #[test]
    fn message_and_presence_events() {
        let c = ctx();
        let ev = map_event(
            &msg_at(
                "_agent.yannick.coder.outbox",
                "agent://yannick/coder",
                "hi",
                NOW,
            ),
            &c,
        );
        let [MeshEvent::MessageReceived {
            conversation_id,
            kind,
            message,
        }] = ev.as_slice()
        else {
            panic!("{ev:?}")
        };
        assert_eq!(conversation_id, "_agent.yannick.coder");
        assert_eq!(*kind, ConversationKind::Agent);
        assert_eq!(message.from_display, "Coder");

        let ev = map_event(&msg_at("_project.x", "human://zoe", "yo", NOW), &c);
        assert!(matches!(
            ev.as_slice(),
            [MeshEvent::MessageReceived {
                kind: ConversationKind::Group,
                ..
            }]
        ));

        let hb = msg_at(
            "_agent.yannick.coder.heartbeat",
            "agent://yannick/coder",
            "",
            NOW,
        );
        assert_eq!(
            map_event(&hb, &c),
            vec![MeshEvent::PresenceChanged {
                agent_id: "agent://yannick/coder".into(),
                presence: Presence::Online
            }]
        );
        let busy = with_payload(
            msg_at(
                "_agent.yannick.coder.status",
                "agent://yannick/coder",
                "",
                NOW,
            ),
            serde_json::json!({"status": "busy"}),
        );
        assert_eq!(
            map_event(&busy, &c),
            vec![MeshEvent::PresenceChanged {
                agent_id: "agent://yannick/coder".into(),
                presence: Presence::Busy
            }]
        );

        let ev = map_event(&msg_at("builds.main", "node://ci", "green", NOW), &c);
        assert!(matches!(ev.as_slice(), [MeshEvent::Other { .. }]));
    }
}
