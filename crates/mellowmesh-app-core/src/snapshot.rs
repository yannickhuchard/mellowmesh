//! Pure construction of the home snapshot from hub data (no I/O), so every
//! derivation rule is unit-testable without a server.

use std::collections::{BTreeMap, HashMap, HashSet};

use mellowmesh_client::sanitize::sanitize_line;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::decision::Decision;
use mellowmesh_core::message::Message;
use mellowmesh_core::task::Task;

use crate::conversation::{
    agent_conversation_id, classify_topic, ConversationKind, ConversationRef,
};
use crate::identity::{agent_id_from_path, agent_topic_path, Principal, PrincipalKind};
use crate::model::{
    chat_message, conversation_title, decision_card, is_pending_decision, is_terminal_task,
    preview, same_principal, task_card, ts_ms, visual, AgentCard, ChatMessage, ConversationSummary,
    Directory, HomeSnapshot,
};
use crate::presence::{derive_presence, presence_from_status, Presence};

/// Max participants listed per conversation summary.
const MAX_PARTICIPANTS: usize = 16;

pub(crate) struct HomeInputs<'a> {
    pub me: &'a Principal,
    pub agents: &'a [AgentRegistration],
    pub tasks: &'a [Task],
    pub decisions: &'a [Decision],
    /// Recent history, any order.
    pub history: &'a [Message],
    pub read_markers: &'a HashMap<String, i64>,
    pub now_ms: i64,
}

pub(crate) fn directory_of(agents: &[AgentRegistration]) -> Directory {
    let mut dir = Directory::default();
    for a in agents {
        dir.insert(&a.id, &a.name);
    }
    dir
}

/// Resolves `_agent.<path>.<leaf>` topics to registered agent ids.
pub(crate) struct AgentPaths {
    by_path: HashMap<String, String>,
    by_name: HashMap<String, Option<String>>,
}

impl AgentPaths {
    pub(crate) fn new(agents: &[AgentRegistration]) -> Self {
        let mut by_path = HashMap::new();
        let mut by_name: HashMap<String, Option<String>> = HashMap::new();
        for a in agents {
            if let Some(path) = agent_topic_path(&a.id) {
                let last = path.rsplit('.').next().unwrap_or(&path).to_string();
                by_path.insert(path, a.id.clone());
                by_name
                    .entry(last)
                    .and_modify(|v| *v = None) // ambiguous
                    .or_insert_with(|| Some(a.id.clone()));
            }
        }
        Self { by_path, by_name }
    }

    /// For `_agent.<path>.<leaf>`, the agent id and the leaf.
    pub(crate) fn resolve(&self, topic: &str) -> Option<(String, String)> {
        let lower = topic.to_lowercase();
        let segs: Vec<&str> = lower.split('.').collect();
        let ["_agent", path @ .., leaf] = segs.as_slice() else {
            return None;
        };
        if path.is_empty() {
            return None;
        }
        let joined = path.join(".");
        let id = self
            .by_path
            .get(&joined)
            .cloned()
            .or_else(|| {
                if path.len() == 1 {
                    self.by_name.get(path[0]).cloned().flatten()
                } else {
                    None
                }
            })
            .unwrap_or_else(|| agent_id_from_path(path));
        Some((id, leaf.to_string()))
    }
}

/// Explicit presence carried by a status/heartbeat message, if any.
pub(crate) fn explicit_presence(msg: &Message, leaf: &str) -> Option<Presence> {
    match leaf {
        "heartbeat" => Some(
            msg.payload
                .as_ref()
                .and_then(|p| p.get("status"))
                .and_then(|s| s.as_str())
                .and_then(presence_from_status)
                .unwrap_or(Presence::Online),
        ),
        "status" => msg
            .payload
            .as_ref()
            .and_then(|p| p.get("status"))
            .and_then(|s| s.as_str())
            .and_then(presence_from_status),
        _ => None,
    }
}

fn has_active_claim(t: &Task, agent_id: &str, now_ms: i64) -> bool {
    let claimed = matches!(
        t.status.to_ascii_lowercase().as_str(),
        "claimed" | "in_progress"
    );
    let holder = t
        .claimed_by
        .as_deref()
        .is_some_and(|c| same_principal(c, agent_id));
    let live = t
        .claim_expires_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .is_none_or(|exp| exp.timestamp_millis() > now_ms);
    claimed && holder && live
}

fn presence_rank(p: Presence) -> u8 {
    match p {
        Presence::Online => 0,
        Presence::Busy => 1,
        Presence::Idle => 2,
        Presence::Offline => 3,
    }
}

fn priority_rank(p: &str) -> u8 {
    match p.to_ascii_lowercase().as_str() {
        "critical" | "urgent" => 0,
        "high" => 1,
        "medium" | "normal" => 2,
        "low" => 3,
        _ => 4,
    }
}

pub(crate) fn agent_cards(inp: &HomeInputs<'_>, dir: &Directory) -> Vec<AgentCard> {
    let paths = AgentPaths::new(inp.agents);
    let mut last_seen: HashMap<String, i64> = HashMap::new();
    let mut explicit: HashMap<String, (Presence, i64)> = HashMap::new();
    for m in inp.history {
        let t = ts_ms(m);
        let mut bump = |id: &str| {
            let e = last_seen.entry(id.to_lowercase()).or_insert(t);
            *e = (*e).max(t);
        };
        if PrincipalKind::of(&m.from) == PrincipalKind::Agent {
            bump(&m.from);
        }
        if let Some((id, leaf)) = paths.resolve(&m.topic) {
            // Writing into someone's inbox says nothing about the agent.
            if leaf != "inbox" {
                bump(&id);
            }
            if let Some(p) = explicit_presence(m, &leaf) {
                let e = explicit.entry(id.to_lowercase()).or_insert((p, t));
                if t >= e.1 {
                    *e = (p, t);
                }
            }
        }
    }

    let mut cards: Vec<AgentCard> = inp
        .agents
        .iter()
        .map(|a| {
            let key = a.id.to_lowercase();
            let claimed: Vec<&Task> = inp
                .tasks
                .iter()
                .filter(|t| has_active_claim(t, &a.id, inp.now_ms))
                .collect();
            let seen = last_seen.get(&key).copied();
            let presence = derive_presence(
                inp.now_ms,
                seen,
                !claimed.is_empty(),
                explicit.get(&key).copied(),
            );
            let (avatar_seed, accent) = visual(&a.id);
            AgentCard {
                id: a.id.clone(),
                display_name: dir.name_of(&a.id),
                owner: a.owner.clone(),
                is_mine: same_principal(&a.owner, &inp.me.id),
                mode: sanitize_line(&a.mode),
                capabilities: a.capabilities.iter().map(|c| sanitize_line(c)).collect(),
                presence,
                last_seen_ms: seen,
                current_task_title: claimed.first().map(|t| sanitize_line(&t.title)),
                avatar_seed,
                accent,
                conversation_id: agent_conversation_id(&a.id),
            }
        })
        .collect();
    cards.sort_by(|x, y| {
        y.is_mine
            .cmp(&x.is_mine)
            .then(presence_rank(x.presence).cmp(&presence_rank(y.presence)))
            .then(
                x.display_name
                    .to_lowercase()
                    .cmp(&y.display_name.to_lowercase()),
            )
            .then(x.id.cmp(&y.id))
    });
    cards
}

/// Group chat messages per conversation (oldest first within a group).
pub(crate) fn group_conversations(
    history: &[Message],
    me: &str,
    dir: &Directory,
) -> BTreeMap<String, (ConversationRef, Vec<ChatMessage>)> {
    let mut groups: BTreeMap<String, (ConversationRef, Vec<ChatMessage>)> = BTreeMap::new();
    let mut seen_ids = HashSet::new();
    for m in history {
        let Some(conv) = classify_topic(&m.topic) else {
            continue;
        };
        if !m.id.is_empty() && !seen_ids.insert(m.id.clone()) {
            continue;
        }
        let entry = groups
            .entry(conv.id.clone())
            .or_insert_with(|| (conv, Vec::new()));
        entry.1.push(chat_message(m, me, dir));
    }
    for (_, msgs) in groups.values_mut() {
        msgs.sort_by(|a, b| a.ts_ms.cmp(&b.ts_ms).then(a.id.cmp(&b.id)));
    }
    groups
}

/// Squad becomes Group as soon as a human other than me takes part.
fn effective_kind(conv: &ConversationRef, msgs: &[ChatMessage], me: &str) -> ConversationKind {
    if conv.kind != ConversationKind::Squad {
        return conv.kind;
    }
    let other_human = msgs.iter().any(|m| {
        matches!(m.from_kind, PrincipalKind::Human | PrincipalKind::Interface)
            && !same_principal(&m.from, me)
    });
    if other_human {
        ConversationKind::Group
    } else {
        ConversationKind::Squad
    }
}

pub(crate) fn summarize(
    conv: &ConversationRef,
    msgs: &[ChatMessage],
    me: &str,
    dir: &Directory,
    read_markers: &HashMap<String, i64>,
) -> ConversationSummary {
    let mut participants = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |id: &str| {
        if participants.len() < MAX_PARTICIPANTS && seen.insert(id.to_lowercase()) {
            participants.push(dir.participant(id));
        }
    };
    if let Some(agent) = &conv.agent_id {
        add(agent);
    }
    for h in &conv.dm_handles {
        add(&format!("human://{h}"));
    }
    // Most recent speakers first.
    for m in msgs.iter().rev() {
        add(&m.from);
    }
    let marker = read_markers.get(&conv.id).copied().unwrap_or(0);
    let unread = msgs
        .iter()
        .filter(|m| !m.is_mine && m.ts_ms > marker)
        .count();
    let last = msgs.last();
    ConversationSummary {
        id: conv.id.clone(),
        kind: effective_kind(conv, msgs, me),
        title: conversation_title(conv, me, dir),
        participants,
        last_message_preview: last.map(preview),
        last_activity_ms: last.map(|m| m.ts_ms).unwrap_or(0),
        unread: u32::try_from(unread).unwrap_or(u32::MAX),
    }
}

pub(crate) fn build_home(inp: &HomeInputs<'_>) -> HomeSnapshot {
    let dir = directory_of(inp.agents);
    let me = &inp.me.id;
    let agents = agent_cards(inp, &dir);

    // Decision request times come from their `_decision.<id>.requested` event.
    let mut requested_at: HashMap<String, i64> = HashMap::new();
    for m in inp.history {
        let lower = m.topic.to_lowercase();
        if let Some(rest) = lower.strip_prefix("_decision.") {
            if let Some(id) = rest.strip_suffix(".requested") {
                requested_at.insert(id.to_string(), ts_ms(m));
            }
        }
    }
    let mut pending_decisions: Vec<_> = inp
        .decisions
        .iter()
        .filter(|d| is_pending_decision(&d.status))
        .map(|d| {
            let created = requested_at.get(&d.id.to_lowercase()).copied().unwrap_or(0);
            decision_card(d, me, created)
        })
        .collect();
    pending_decisions.sort_by(|a, b| {
        b.for_me
            .cmp(&a.for_me)
            .then(b.created_ms.cmp(&a.created_ms))
            .then(a.id.cmp(&b.id))
    });

    let mut active_tasks: Vec<_> = inp
        .tasks
        .iter()
        .filter(|t| !is_terminal_task(&t.status))
        .map(task_card)
        .collect();
    active_tasks.sort_by(|a, b| {
        priority_rank(&a.priority)
            .cmp(&priority_rank(&b.priority))
            .then(a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            .then(a.id.cmp(&b.id))
    });

    let mut groups = group_conversations(inp.history, me, &dir);
    // Every one of my agents gets a conversation, even before the first message.
    for a in agents.iter().filter(|a| a.is_mine) {
        if let Some(conv) = a
            .conversation_id
            .as_deref()
            .and_then(|id| classify_topic(&format!("{id}.inbox")))
        {
            groups.entry(conv.id.clone()).or_insert((conv, Vec::new()));
        }
    }
    let mut conversations: Vec<_> = groups
        .values()
        .map(|(conv, msgs)| summarize(conv, msgs, me, &dir, inp.read_markers))
        .collect();
    conversations.sort_by(|a, b| {
        b.last_activity_ms
            .cmp(&a.last_activity_ms)
            .then(a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            .then(a.id.cmp(&b.id))
    });

    HomeSnapshot {
        me: inp.me.clone(),
        agents,
        pending_decisions,
        active_tasks,
        conversations,
        generated_ms: inp.now_ms,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::TimeZone;
    use mellowmesh_core::decision::{DecisionOption, DecisionOutcome};

    pub(crate) const NOW: i64 = 1_800_000_000_000;

    pub(crate) fn me() -> Principal {
        Principal {
            id: "human://yannick".into(),
            kind: PrincipalKind::Human,
            display_name: "yannick".into(),
            device_name: "test".into(),
        }
    }

    pub(crate) fn agent(id: &str, name: &str, owner: &str) -> AgentRegistration {
        AgentRegistration {
            id: id.into(),
            name: name.into(),
            owner: owner.into(),
            mode: "autonomous".into(),
            capabilities: vec!["code".into()],
        }
    }

    pub(crate) fn msg_at(topic: &str, from: &str, body: &str, ts: i64) -> Message {
        Message {
            id: format!("msg_{topic}_{ts}"),
            topic: topic.into(),
            from: from.into(),
            owner: None,
            timestamp: chrono::Utc.timestamp_millis_opt(ts).unwrap(),
            content_type: "text/plain".into(),
            body: body.into(),
            headers: None,
            payload: None,
            parent_id: None,
        }
    }

    fn task(id: &str, status: &str, claimed_by: Option<&str>) -> Task {
        Task {
            id: id.into(),
            title: format!("Task {id}"),
            created_by: "human://yannick".into(),
            status: status.into(),
            priority: "medium".into(),
            claimed_by: claimed_by.map(String::from),
            ..Default::default()
        }
    }

    fn decision(id: &str, status: &str, decider: &str) -> Decision {
        Decision {
            id: id.into(),
            title: "Ship?".into(),
            question: "Ship it?".into(),
            created_by: "agent://yannick/coder".into(),
            required_decider: decider.into(),
            status: status.into(),
            options: vec![DecisionOption {
                id: "yes".into(),
                label: "Yes".into(),
                pros: vec![],
                cons: vec![],
                outcome: Some(DecisionOutcome::Approve),
            }],
            response_option_id: None,
            response_timestamp: None,
            responded_by: None,
        }
    }

    #[test]
    fn home_snapshot_derivation() {
        let agents = vec![
            agent("agent://zed/scout", "Scout", "human://zed"),
            agent("agent://yannick/coder", "Coder", "human://yannick"),
            agent("agent://yannick/writer", "Writer", "human://yannick"),
        ];
        let tasks = vec![
            task("t1", "claimed", Some("agent://yannick/coder")),
            task("t2", "completed", None),
            task("t3", "open", None),
        ];
        let decisions = vec![
            decision("d1", "requested", "human://zed"),
            decision("d2", "requested", "human://yannick"),
            decision("d3", "approved", "human://yannick"),
        ];
        let history = vec![
            msg_at(
                "_agent.yannick.writer.outbox",
                "agent://yannick/writer",
                "draft ready",
                NOW - 30_000,
            ),
            msg_at(
                "_agent.yannick.writer.inbox",
                "human://yannick",
                "write",
                NOW - 60_000,
            ),
            msg_at(
                "_project.apollo",
                "agent://zed/scout",
                "found it",
                NOW - 10_000,
            ),
            msg_at(
                "_project.apollo",
                "agent://yannick/coder",
                "on it",
                NOW - 5_000,
            ),
            msg_at("_forum.general", "human://zed", "hello", NOW - 1_000),
            msg_at(
                "_decision.d2.requested",
                "agent://yannick/coder",
                "Decision",
                NOW - 7_000,
            ),
            msg_at(
                "_task.t1.progress",
                "agent://yannick/coder",
                "50%",
                NOW - 2_000,
            ),
        ];
        let mut markers = HashMap::new();
        markers.insert("_agent.yannick.writer".to_string(), NOW - 40_000);
        let me = me();
        let home = build_home(&HomeInputs {
            me: &me,
            agents: &agents,
            tasks: &tasks,
            decisions: &decisions,
            history: &history,
            read_markers: &markers,
            now_ms: NOW,
        });

        // My agents first; coder busy (claim), writer online (recent outbox).
        let ids: Vec<_> = home.agents.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "agent://yannick/writer",
                "agent://yannick/coder",
                "agent://zed/scout"
            ]
        );
        assert_eq!(home.agents[0].presence, Presence::Online);
        assert_eq!(home.agents[1].presence, Presence::Busy);
        assert_eq!(
            home.agents[1].current_task_title.as_deref(),
            Some("Task t1")
        );
        assert_eq!(home.agents[2].presence, Presence::Online);
        assert!(!home.agents[2].is_mine);
        assert_eq!(
            home.agents[1].conversation_id.as_deref(),
            Some("_agent.yannick.coder")
        );

        // Pending only, mine first.
        let d: Vec<_> = home
            .pending_decisions
            .iter()
            .map(|d| d.id.as_str())
            .collect();
        assert_eq!(d, ["d2", "d1"]);
        assert!(home.pending_decisions[0].for_me);
        assert_eq!(home.pending_decisions[0].created_ms, NOW - 7_000);

        let t: Vec<_> = home.active_tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(t, ["t1", "t3"]);

        // Conversations: forum (other human → Group), project (agents only → Squad),
        // writer (agent), coder (empty, but mine).
        let c: Vec<_> = home
            .conversations
            .iter()
            .map(|c| (c.id.as_str(), c.kind))
            .collect();
        assert_eq!(
            c,
            [
                ("_forum.general", ConversationKind::Group),
                ("_project.apollo", ConversationKind::Squad),
                ("_agent.yannick.writer", ConversationKind::Agent),
                ("_agent.yannick.coder", ConversationKind::Agent),
            ]
        );
        let writer = &home.conversations[2];
        assert_eq!(writer.title, "Writer");
        assert_eq!(writer.unread, 1); // the outbox reply, after the marker
        assert_eq!(writer.last_message_preview.as_deref(), Some("draft ready"));
        assert_eq!(home.conversations[1].unread, 2);
        assert_eq!(home.conversations[3].last_activity_ms, 0);
    }

    #[test]
    fn heartbeats_and_explicit_status() {
        let agents = vec![
            agent("agent://y/a", "a", "human://yannick"),
            agent("agent://y/b", "b", "human://yannick"),
        ];
        let mut off = msg_at("_agent.y.b.status", "agent://y/b", "", NOW - 1_000);
        off.payload = Some(serde_json::json!({"status": "offline"}));
        let history = vec![
            msg_at("_agent.a.heartbeat", "node://n", "", NOW - 1_000),
            msg_at("_agent.y.b.outbox", "agent://y/b", "bye", NOW - 2_000),
            off,
        ];
        let me = me();
        let home = build_home(&HomeInputs {
            me: &me,
            agents: &agents,
            tasks: &[],
            decisions: &[],
            history: &history,
            read_markers: &HashMap::new(),
            now_ms: NOW,
        });
        let by_id: HashMap<_, _> = home
            .agents
            .iter()
            .map(|a| (a.id.as_str(), a.presence))
            .collect();
        assert_eq!(by_id["agent://y/a"], Presence::Online); // single-segment heartbeat
        assert_eq!(by_id["agent://y/b"], Presence::Offline);
    }
}
