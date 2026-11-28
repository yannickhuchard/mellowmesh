//! Groups of Mellows kept by this hub.
//!
//! A person talks to several Mellows at once, on any of their machines. The
//! group lives on one hub the person owns, which keeps it going while the
//! phone sleeps:
//!
//! - it says each line to every member, in that Mellow's own conversation
//!   (tagged `payload.mm_group`, as the apps do), on its own machine;
//! - it keeps a line for a machine that is out of reach and delivers it
//!   when the machine comes back (for a day);
//! - it watches for each answer (`parent_id` = the line it answers);
//! - it hands over when an answer names another member with `@Name` (at most
//!   three per turn, never twice for the same answer and member);
//! - it writes everything into one thread, `_group.<id>`, that every app
//!   reads.
//!
//! A member on another machine is reached with a *pass* the person gave:
//! a token of that machine that may write only that Mellow's inbox and read
//! only its conversation. The pass is kept here, never shown again, and
//! revoked (by itself) when the member leaves or the group is deleted.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use mellowmesh_core::message::Message;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::AppState;

/// Payload key of an item of a group's thread (written by the hub only).
pub const THREAD_KEY: &str = "mm_group_thread";
/// The topics of the groups' threads (kept queryable by the hub).
pub const THREAD_PATTERN: &str = "_group.**";
/// Payload key of a line said in a group (the same tag the apps write).
pub const GROUP_KEY: &str = crate::mellows::GROUP_KEY;
const GROUPS_KEY: &str = "groups.v1";
const DELIVERIES_KEY: &str = "groups.deliveries.v1";
pub const MAX_MEMBERS: usize = 12;
pub const MAX_HANDOVERS: usize = 3;
const MAX_SAID: usize = 6;
const LINE_CHARS: usize = 400;
const NAME_CHARS: usize = 40;
const MAX_TEXT: usize = 8000;
/// How long a line waits for a machine that is out of reach.
const KEEP_FOR_SECS: i64 = 24 * 3600;
/// How long the hub watches for an answer.
const WATCH_FOR_SECS: i64 = 30 * 60;
/// Finished deliveries are remembered this long (hand-overs stay unique).
const FORGET_AFTER_SECS: i64 = 2 * 24 * 3600;
const TICK: Duration = Duration::from_millis(700);

/// Where a member on another machine is reached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pass {
    /// The machine's address (its relay URL, or a LAN address).
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    /// The Mellow's agent id (`agent://karim/movers`).
    pub id: String,
    pub name: String,
    /// Its conversation, `_agent.<owner>.<name>` (inbox and outbox below).
    pub conversation: String,
    /// The machine's name, for a member that is not on this hub.
    #[serde(default)]
    pub machine: Option<String>,
    #[serde(default)]
    pub pass: Option<Pass>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Stage {
    /// Not said yet: the machine is out of reach.
    Waiting,
    /// Said; the hub watches for the answer.
    Sent,
    Answered,
    /// Said, but no answer came in time.
    Quiet,
    /// Never said (refused, or kept too long).
    GaveUp,
}

/// One line on its way to one member.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Delivery {
    id: String,
    group: String,
    turn: String,
    member: String,
    body: String,
    payload: Value,
    created: DateTime<Utc>,
    stage: Stage,
    #[serde(default)]
    sent_id: Option<String>,
    #[serde(default)]
    sent_at: Option<DateTime<Utc>>,
    #[serde(default)]
    attempts: u32,
    /// Set on a hand-over: the answer it hands over.
    #[serde(default)]
    handover_of: Option<String>,
    #[serde(skip)]
    next_at: Option<Instant>,
}

#[derive(Default)]
struct Book {
    groups: Vec<Group>,
    deliveries: Vec<Delivery>,
    /// Per machine address: whether the last try reached it.
    reach: HashMap<String, bool>,
}

fn book() -> &'static Mutex<Book> {
    static CELL: OnceLock<Mutex<Book>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(Book::default()))
}

fn wake() -> &'static tokio::sync::Notify {
    static CELL: OnceLock<tokio::sync::Notify> = OnceLock::new();
    CELL.get_or_init(tokio::sync::Notify::new)
}

fn lock() -> std::sync::MutexGuard<'static, Book> {
    book().lock().unwrap_or_else(|p| p.into_inner())
}

pub fn thread_topic(group: &str) -> String {
    format!("_group.{group}")
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", ulid::Ulid::new().to_string().to_lowercase())
}

fn clean(raw: &str, max: usize) -> String {
    raw.chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// `[a-z0-9_-]{1,48}`: an id that is a safe topic segment.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// `_agent.<owner>.<name>`: three topic-safe segments, no wildcard.
fn valid_conversation(c: &str) -> bool {
    let segs: Vec<&str> = c.split('.').collect();
    segs.len() >= 3
        && segs[0] == "_agent"
        && segs[1..].iter().all(|s| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

fn valid_url(u: &str) -> bool {
    url::Url::parse(u).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
}

// ---------------------------------------------------------------------------
// Loading and saving
// ---------------------------------------------------------------------------

pub fn load(store: &mellowmesh_store::Store) {
    let groups: Vec<Group> = store
        .get_config(GROUPS_KEY)
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let deliveries: Vec<Delivery> = store
        .get_config(DELIVERIES_KEY)
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let mut b = lock();
    if !groups.is_empty() {
        tracing::info!("{} group(s) of Mellows kept on this hub", groups.len());
    }
    b.groups = groups;
    b.deliveries = deliveries;
}

fn save(store: &mellowmesh_store::Store) {
    let (g, d) = {
        let b = lock();
        (
            serde_json::to_string(&b.groups),
            serde_json::to_string(&b.deliveries),
        )
    };
    if let Ok(g) = g {
        if let Err(e) = store.set_config(GROUPS_KEY, &g) {
            tracing::warn!("could not save groups: {e}");
        }
    }
    if let Ok(d) = d {
        let _ = store.set_config(DELIVERIES_KEY, &d);
    }
}

// ---------------------------------------------------------------------------
// What the apps see (passes never leave)
// ---------------------------------------------------------------------------

fn member_view(m: &Member, reach: &HashMap<String, bool>) -> Value {
    let reachable = match &m.pass {
        None => Some(true),
        Some(p) => reach.get(&p.url).copied(),
    };
    json!({
        "id": m.id,
        "name": m.name,
        "conversation": m.conversation,
        "machine": m.machine,
        "here": m.pass.is_none(),
        "reachable": reachable,
    })
}

fn group_view(g: &Group, reach: &HashMap<String, bool>) -> Value {
    json!({
        "id": g.id,
        "name": g.name,
        "created_at": g.created_at,
        "topic": thread_topic(&g.id),
        "members": g.members.iter().map(|m| member_view(m, reach)).collect::<Vec<_>>(),
    })
}

pub fn list() -> Vec<Value> {
    let b = lock();
    b.groups.iter().map(|g| group_view(g, &b.reach)).collect()
}

pub fn get(id: &str) -> Option<Value> {
    let b = lock();
    b.groups
        .iter()
        .find(|g| g.id == id)
        .map(|g| group_view(g, &b.reach))
}

fn group(id: &str) -> Option<Group> {
    lock().groups.iter().find(|g| g.id == id).cloned()
}

// ---------------------------------------------------------------------------
// Keeping a group
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MemberInput {
    pub id: String,
    pub name: String,
    pub conversation: String,
    #[serde(default)]
    pub machine: Option<String>,
    #[serde(default)]
    pub pass: Option<Pass>,
}

#[derive(Debug, Deserialize)]
pub struct GroupInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub members: Vec<MemberInput>,
}

/// Keep a group (new, or the same id again: its name and members are
/// replaced; a member given again without a pass keeps the one it had).
pub fn keep(state: &AppState, input: GroupInput) -> Result<Value, String> {
    let name = clean(&input.name, NAME_CHARS);
    if name.is_empty() {
        return Err("Give the group a name.".into());
    }
    let id = match input.id {
        Some(id) => {
            let id = id.trim().to_ascii_lowercase();
            if !valid_id(&id) {
                return Err(
                    "A group id is lower case letters, digits, - and _ (48 at most).".into(),
                );
            }
            id
        }
        None => new_id("grp"),
    };
    if input.members.is_empty() {
        return Err("A group needs at least one Mellow.".into());
    }
    if input.members.len() > MAX_MEMBERS {
        return Err(format!("A group has {MAX_MEMBERS} Mellows at most."));
    }
    let old = group(&id);
    let mut members: Vec<Member> = Vec::new();
    for m in input.members {
        let mname = clean(&m.name, NAME_CHARS);
        if mname.is_empty() || m.id.trim().is_empty() {
            return Err("Every member needs a name and an id.".into());
        }
        if !valid_conversation(&m.conversation) {
            return Err(format!(
                "{mname}: its conversation must look like _agent.<owner>.<name>."
            ));
        }
        if members.iter().any(|x| x.id.eq_ignore_ascii_case(&m.id)) {
            continue;
        }
        let pass = match m.pass {
            Some(p) => {
                if !valid_url(&p.url) || p.token.trim().is_empty() {
                    return Err(format!(
                        "{mname}: its pass needs the machine's address and a token."
                    ));
                }
                Some(Pass {
                    url: p.url.trim_end_matches('/').to_string(),
                    token: p.token.trim().to_string(),
                })
            }
            None => old
                .as_ref()
                .and_then(|g| g.members.iter().find(|x| x.id.eq_ignore_ascii_case(&m.id)))
                .and_then(|x| x.pass.clone()),
        };
        members.push(Member {
            id: m.id.trim().to_string(),
            name: mname,
            conversation: m.conversation,
            machine: m
                .machine
                .map(|s| clean(&s, NAME_CHARS))
                .filter(|s| !s.is_empty()),
            pass,
        });
    }
    // Members that left give their pass back.
    if let Some(old) = &old {
        for gone in old
            .members
            .iter()
            .filter(|o| !members.iter().any(|m| m.id.eq_ignore_ascii_case(&o.id)))
        {
            give_back(gone.pass.clone());
        }
    }
    let g = Group {
        id: id.clone(),
        name,
        created_at: old.as_ref().map(|o| o.created_at).unwrap_or_else(Utc::now),
        members,
    };
    let view = {
        let mut b = lock();
        b.groups.retain(|x| x.id != id);
        b.groups.push(g.clone());
        group_view(&g, &b.reach)
    };
    save(&state.store);
    Ok(view)
}

/// Forget a group: its passes are given back, its waiting lines dropped.
/// The thread stays in the hub's history.
pub fn forget(state: &AppState, id: &str) -> bool {
    let gone = {
        let mut b = lock();
        let before = b.groups.len();
        let gone: Vec<Group> = b.groups.iter().filter(|g| g.id == id).cloned().collect();
        b.groups.retain(|g| g.id != id);
        b.deliveries.retain(|d| d.group != id);
        (b.groups.len() != before).then_some(gone)
    };
    match gone {
        Some(gone) => {
            for g in gone {
                for m in g.members {
                    give_back(m.pass);
                }
            }
            save(&state.store);
            true
        }
        None => false,
    }
}

/// A pass revokes itself on its machine (best effort, in the background).
fn give_back(pass: Option<Pass>) {
    let Some(p) = pass else { return };
    tokio::spawn(async move {
        if let Err(e) = remote(&p).revoke_token("self").await {
            tracing::info!("a group pass could not be given back to {}: {e}", p.url);
        }
    });
}

fn remote(p: &Pass) -> mellowmesh_client::MellowMeshClient {
    mellowmesh_client::MellowMeshClient::new(0)
        .with_base_url(p.url.clone())
        .with_token(p.token.clone())
}

// ---------------------------------------------------------------------------
// Saying a line
// ---------------------------------------------------------------------------

/// Whether `text` names `name` with `@` (whole name, any case).
pub fn names(text: &str, name: &str) -> bool {
    let t = text.to_lowercase();
    let n = format!("@{}", name.to_lowercase());
    let ok = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
    let mut from = 0;
    while let Some(i) = t[from..].find(&n) {
        let at = from + i;
        if ok(t[..at].chars().next_back()) && ok(t[at + n.len()..].chars().next()) {
            return true;
        }
        from = at + n.len();
    }
    false
}

/// Recent lines of the thread, as the Mellows are told (`who`, `text`).
fn said_lately(state: &AppState, g: &Group) -> Vec<Value> {
    let items = thread_messages(state, &g.id, 60);
    let mut out: Vec<Value> = items
        .iter()
        .filter_map(|m| {
            let t = m.payload.as_ref()?.get(THREAD_KEY)?;
            let kind = t.get("kind")?.as_str()?;
            let who = match kind {
                "said" => "Owner".to_string(),
                "answer" => member_name(g, t.get("member")?.as_str()?),
                _ => return None,
            };
            Some(json!({"who": who, "text": clean(&m.body, LINE_CHARS)}))
        })
        .collect();
    let skip = out.len().saturating_sub(MAX_SAID);
    out.drain(..skip);
    out
}

fn member_name(g: &Group, id: &str) -> String {
    g.members
        .iter()
        .find(|m| m.id.eq_ignore_ascii_case(id))
        .map(|m| m.name.clone())
        .unwrap_or_else(|| id.rsplit('/').next().unwrap_or(id).to_string())
}

fn line_payload(
    g: &Group,
    turn: &str,
    to: &[&Member],
    said: &[Value],
    handover: Option<(&str, &Member)>,
) -> Value {
    let mut p = json!({
        "id": g.id,
        "name": g.name,
        "turn": turn,
        "members": g.members.iter().map(|m| json!({"id": m.id, "name": m.name})).collect::<Vec<_>>(),
        "to": if to.len() < g.members.len() { to.iter().map(|m| m.id.clone()).collect::<Vec<_>>() } else { Vec::new() },
        "said": said,
        "kept_by": "hub",
    });
    if let Some((of, from)) = handover {
        p["handover_of"] = json!(of);
        p["handover_from"] = json!(from.id);
        p["handover_from_name"] = json!(from.name);
    }
    json!({ GROUP_KEY: p })
}

/// The person says `text` in the group, to `to` (member ids; empty: the
/// members it names with `@`, else everyone). Returns the turn.
pub async fn say(state: &AppState, id: &str, text: &str, to: &[String]) -> Result<Value, String> {
    let g = group(id).ok_or("No such group on this hub.")?;
    let text: String = text.trim().chars().take(MAX_TEXT).collect();
    if text.is_empty() {
        return Err("Write something first.".into());
    }
    let mut chosen: Vec<&Member> = if to.is_empty() {
        g.members.iter().filter(|m| names(&text, &m.name)).collect()
    } else {
        g.members
            .iter()
            .filter(|m| to.iter().any(|t| t.eq_ignore_ascii_case(&m.id)))
            .collect()
    };
    if chosen.is_empty() {
        if !to.is_empty() {
            return Err("None of those Mellows is in this group.".into());
        }
        chosen = g.members.iter().collect();
    }
    let said = said_lately(state, &g);
    let turn = new_id("t");
    let to_ids: Vec<String> = if chosen.len() < g.members.len() {
        chosen.iter().map(|m| m.id.clone()).collect()
    } else {
        Vec::new()
    };
    let item = thread_item(
        &g.id,
        &state.owner,
        &text,
        json!({"kind": "said", "turn": turn, "to": to_ids}),
    );
    let item = crate::handlers::message::handle_publish(Arc::new(state.clone()), item).await?;
    let payload = line_payload(&g, &turn, &chosen, &said, None);
    {
        let mut b = lock();
        for m in &chosen {
            b.deliveries.push(Delivery {
                id: new_id("dlv"),
                group: g.id.clone(),
                turn: turn.clone(),
                member: m.id.clone(),
                body: text.clone(),
                payload: payload.clone(),
                created: Utc::now(),
                stage: Stage::Waiting,
                sent_id: None,
                sent_at: None,
                attempts: 0,
                handover_of: None,
                next_at: None,
            });
        }
    }
    save(&state.store);
    wake().notify_one();
    Ok(json!({
        "turn": turn,
        "message": item,
        "to": chosen.iter().map(|m| m.id.clone()).collect::<Vec<_>>(),
    }))
}

fn thread_item(group: &str, from: &str, body: &str, mut meta: Value) -> Message {
    meta["group"] = json!(group);
    Message {
        id: new_id("msg"),
        topic: thread_topic(group),
        from: from.to_string(),
        owner: None,
        timestamp: Utc::now(),
        content_type: "text/plain".into(),
        body: body.to_string(),
        headers: None,
        payload: Some(json!({ THREAD_KEY: meta })),
        parent_id: None,
    }
}

fn thread_messages(state: &AppState, group: &str, limit: usize) -> Vec<Message> {
    let mut msgs = state
        .store
        .get_history_matching(&thread_topic(group), limit)
        .unwrap_or_default();
    msgs.sort_by_key(|m| m.timestamp);
    msgs
}

/// The thread as the apps draw it: items oldest first, and who the group
/// still waits on.
pub fn thread(state: &AppState, id: &str, limit: usize) -> Option<Value> {
    let g = group(id)?;
    let items: Vec<Value> = thread_messages(state, id, limit)
        .into_iter()
        .filter_map(|m| {
            let t = m.payload.as_ref()?.get(THREAD_KEY)?.clone();
            let kind = t.get("kind")?.as_str()?.to_string();
            let who = |key: &str| {
                t.get(key)
                    .and_then(|v| v.as_str())
                    .map(|id| json!({"id": id, "name": member_name(&g, id)}))
            };
            let to: Vec<Value> = t
                .get("to")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .map(|id| json!({"id": id, "name": member_name(&g, id)}))
                        .collect()
                })
                .unwrap_or_default();
            Some(json!({
                "id": m.id,
                "kind": kind,
                "ts": m.timestamp,
                "from": m.from,
                "text": m.body,
                "turn": t.get("turn"),
                "to": to,
                "member": who("member"),
                "of": t.get("of"),
            }))
        })
        .collect();
    let now = Utc::now();
    let (pending, reach) = {
        let b = lock();
        let pending: Vec<Value> = b
            .deliveries
            .iter()
            .filter(|d| d.group == id)
            .filter_map(|d| {
                let state = match d.stage {
                    Stage::Waiting => "waiting",
                    Stage::Sent
                        if d.sent_at
                            .is_some_and(|t| (now - t).num_seconds() < WATCH_FOR_SECS) =>
                    {
                        "thinking"
                    }
                    _ => return None,
                };
                let m = g
                    .members
                    .iter()
                    .find(|m| m.id.eq_ignore_ascii_case(&d.member))?;
                Some(json!({
                    "turn": d.turn,
                    "member": {"id": m.id, "name": m.name},
                    "machine": m.machine,
                    "state": state,
                    "since": d.sent_at.unwrap_or(d.created),
                }))
            })
            .collect();
        (pending, b.reach.clone())
    };
    Some(json!({
        "group": group_view(&g, &reach),
        "items": items,
        "pending": pending,
    }))
}

// ---------------------------------------------------------------------------
// The work: deliver, watch, hand over
// ---------------------------------------------------------------------------

/// Start the hub's group work (once, at startup).
pub fn start(state: AppState) {
    tokio::spawn(async move {
        loop {
            step(&state).await;
            let _ = tokio::time::timeout(TICK, wake().notified()).await;
        }
    });
}

fn backoff(attempts: u32) -> Duration {
    Duration::from_secs(match attempts {
        0 | 1 => 2,
        2 => 5,
        3 => 15,
        4 => 30,
        _ => 60,
    })
}

enum Outcome {
    Sent(String, bool),
    Unreachable,
    Refused(String),
    GaveUp,
    Answer(Box<Message>),
    NoAnswerYet(Duration),
    Quiet,
}

/// One pass over the deliveries that are due.
pub async fn step(state: &AppState) {
    let now = Instant::now();
    let due: Vec<(Delivery, Option<Group>)> = {
        let b = lock();
        b.deliveries
            .iter()
            .filter(|d| matches!(d.stage, Stage::Waiting | Stage::Sent))
            .filter(|d| d.next_at.is_none_or(|t| t <= now))
            .map(|d| {
                (
                    d.clone(),
                    b.groups.iter().find(|g| g.id == d.group).cloned(),
                )
            })
            .collect()
    };
    if due.is_empty() {
        return;
    }
    let mut changed = false;
    for (d, g) in due {
        let Some(g) = g else { continue };
        let Some(m) = g
            .members
            .iter()
            .find(|m| m.id.eq_ignore_ascii_case(&d.member))
            .cloned()
        else {
            set_stage(&d.id, Stage::GaveUp);
            changed = true;
            continue;
        };
        let outcome = match d.stage {
            Stage::Waiting => deliver(state, &d, &m).await,
            _ => watch(state, &d, &m).await,
        };
        changed |= apply(state, &d, &g, &m, outcome).await;
    }
    if changed {
        prune();
        save(&state.store);
    }
}

fn set_stage(id: &str, stage: Stage) {
    if let Some(x) = lock().deliveries.iter_mut().find(|x| x.id == id) {
        x.stage = stage;
    }
}

fn prune() {
    let now = Utc::now();
    lock().deliveries.retain(|d| {
        matches!(d.stage, Stage::Waiting | Stage::Sent)
            || (now - d.created).num_seconds() < FORGET_AFTER_SECS
    });
}

fn mark_reach(m: &Member, ok: bool) {
    if let Some(p) = &m.pass {
        lock().reach.insert(p.url.clone(), ok);
    }
}

async fn deliver(state: &AppState, d: &Delivery, m: &Member) -> Outcome {
    if (Utc::now() - d.created).num_seconds() > KEEP_FOR_SECS {
        return Outcome::GaveUp;
    }
    let msg = Message {
        id: new_id("msg"),
        topic: format!("{}.inbox", m.conversation),
        from: state.owner.clone(),
        owner: None,
        timestamp: Utc::now(),
        content_type: "text/plain".into(),
        body: d.body.clone(),
        headers: None,
        payload: Some(d.payload.clone()),
        parent_id: None,
    };
    match &m.pass {
        None => {
            let arc = Arc::new(state.clone());
            match crate::handlers::message::handle_publish(arc.clone(), msg).await {
                Ok(sent) => {
                    crate::mellows::maybe_dispatch(&arc, &sent);
                    Outcome::Sent(sent.id, d.attempts > 0)
                }
                Err(e) => Outcome::Refused(e),
            }
        }
        Some(p) => match remote(p).publish_returning(&msg).await {
            Ok(sent) => {
                mark_reach(m, true);
                Outcome::Sent(sent.id, d.attempts > 0)
            }
            Err(e) => {
                let e = e.to_string();
                if e.contains("failed (4") {
                    mark_reach(m, true);
                    Outcome::Refused(e)
                } else {
                    mark_reach(m, false);
                    Outcome::Unreachable
                }
            }
        },
    }
}

async fn watch(state: &AppState, d: &Delivery, m: &Member) -> Outcome {
    let Some(sent_id) = &d.sent_id else {
        return Outcome::Quiet;
    };
    let age = d
        .sent_at
        .map(|t| (Utc::now() - t).num_seconds())
        .unwrap_or(0);
    if age > WATCH_FOR_SECS {
        return Outcome::Quiet;
    }
    let pattern = format!("{}.*", m.conversation);
    let history = match &m.pass {
        None => state.store.get_history_matching(&pattern, 40).ok(),
        Some(p) => {
            let h = remote(p).get_history_matching(&pattern, 40).await.ok();
            mark_reach(m, h.is_some());
            h
        }
    };
    let found = history.unwrap_or_default().into_iter().find(|x| {
        x.parent_id.as_deref() == Some(sent_id.as_str()) && x.from.eq_ignore_ascii_case(&m.id)
    });
    match found {
        Some(a) => Outcome::Answer(Box::new(a)),
        None => Outcome::NoAnswerYet(match (&m.pass, age) {
            (None, _) => Duration::from_millis(600),
            (Some(_), a) if a < 120 => Duration::from_secs(2),
            _ => Duration::from_secs(10),
        }),
    }
}

async fn apply(state: &AppState, d: &Delivery, g: &Group, m: &Member, o: Outcome) -> bool {
    let arc = Arc::new(state.clone());
    match o {
        Outcome::Sent(sent_id, late) => {
            {
                let mut b = lock();
                if let Some(x) = b.deliveries.iter_mut().find(|x| x.id == d.id) {
                    x.stage = Stage::Sent;
                    x.sent_id = Some(sent_id);
                    x.sent_at = Some(Utc::now());
                    x.next_at = Some(Instant::now() + Duration::from_millis(500));
                }
            }
            if late {
                let machine = m.machine.clone().unwrap_or_else(|| "its machine".into());
                let item = thread_item(
                    &g.id,
                    &state.node_id,
                    &format!("Delivered to {} when {machine} came back.", m.name),
                    json!({"kind": "delivered", "turn": d.turn, "member": m.id}),
                );
                let _ = crate::handlers::message::handle_publish(arc, item).await;
            }
            true
        }
        Outcome::Unreachable => {
            let mut b = lock();
            if let Some(x) = b.deliveries.iter_mut().find(|x| x.id == d.id) {
                x.attempts += 1;
                x.next_at = Some(Instant::now() + backoff(x.attempts));
            }
            false
        }
        Outcome::Refused(why) => {
            set_stage(&d.id, Stage::GaveUp);
            tracing::info!("group {}: {} refused a line: {why}", g.id, m.name);
            let item = thread_item(
                &g.id,
                &state.node_id,
                &format!("{} could not be told: {}", m.name, clean(&why, 200)),
                json!({"kind": "gave_up", "turn": d.turn, "member": m.id}),
            );
            let _ = crate::handlers::message::handle_publish(arc, item).await;
            true
        }
        Outcome::GaveUp => {
            set_stage(&d.id, Stage::GaveUp);
            let machine = m.machine.clone().unwrap_or_else(|| "its machine".into());
            let item = thread_item(
                &g.id,
                &state.node_id,
                &format!(
                    "Not delivered to {}: {machine} stayed out of reach for a day.",
                    m.name
                ),
                json!({"kind": "gave_up", "turn": d.turn, "member": m.id}),
            );
            let _ = crate::handlers::message::handle_publish(arc, item).await;
            true
        }
        Outcome::Answer(a) => {
            set_stage(&d.id, Stage::Answered);
            let item = thread_item(
                &g.id,
                &m.id,
                &a.body,
                json!({"kind": "answer", "turn": d.turn, "member": m.id, "source": a.id}),
            );
            let mirrored = crate::handlers::message::handle_publish(arc, item).await;
            if let Ok(mirrored) = mirrored {
                hand_over(state, g, m, d, &mirrored).await;
            }
            true
        }
        Outcome::NoAnswerYet(after) => {
            if let Some(x) = lock().deliveries.iter_mut().find(|x| x.id == d.id) {
                x.next_at = Some(Instant::now() + after);
            }
            false
        }
        Outcome::Quiet => {
            set_stage(&d.id, Stage::Quiet);
            true
        }
    }
}

/// An answer that names other members with `@Name` is handed to them
/// (at most [`MAX_HANDOVERS`] per turn, never twice per answer and member).
async fn hand_over(state: &AppState, g: &Group, from: &Member, d: &Delivery, answer: &Message) {
    let targets: Vec<&Member> = g
        .members
        .iter()
        .filter(|t| !t.id.eq_ignore_ascii_case(&from.id) && names(&answer.body, &t.name))
        .collect();
    if targets.is_empty() {
        return;
    }
    let said = said_lately(state, g);
    let mut made = Vec::new();
    {
        let mut b = lock();
        let mut used = b
            .deliveries
            .iter()
            .filter(|x| x.group == g.id && x.turn == d.turn && x.handover_of.is_some())
            .count();
        for t in targets {
            if used >= MAX_HANDOVERS {
                break;
            }
            let dup = b.deliveries.iter().any(|x| {
                x.handover_of.as_deref() == Some(answer.id.as_str())
                    && x.member.eq_ignore_ascii_case(&t.id)
            });
            if dup {
                continue;
            }
            used += 1;
            let body = format!("{} handed this over to you:\n\n{}", from.name, answer.body);
            b.deliveries.push(Delivery {
                id: new_id("dlv"),
                group: g.id.clone(),
                turn: d.turn.clone(),
                member: t.id.clone(),
                body,
                payload: line_payload(g, &d.turn, &[t], &said, Some((&answer.id, from))),
                created: Utc::now(),
                stage: Stage::Waiting,
                sent_id: None,
                sent_at: None,
                attempts: 0,
                handover_of: Some(answer.id.clone()),
                next_at: None,
            });
            made.push(t.clone());
        }
    }
    for t in made {
        let item = thread_item(
            &g.id,
            &from.id,
            &format!("{} handed this over to {}.", from.name, t.name),
            json!({"kind": "handover", "turn": d.turn, "member": from.id, "to": [t.id], "of": answer.id}),
        );
        let _ = crate::handlers::message::handle_publish(Arc::new(state.clone()), item).await;
    }
    wake().notify_one();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_counts_only_as_a_whole_word_after_an_at() {
        assert!(names("Not mine: @Movers handles vans.", "Movers"));
        assert!(names("@movers, over to you", "Movers"));
        assert!(names("ask @Van Team", "Van Team"));
        assert!(!names("email karim@movers.example", "Movers"));
        assert!(!names("@Moverszz", "Movers"));
        assert!(!names("Movers will do it", "Movers"));
    }

    #[test]
    fn ids_and_conversations_are_safe_topic_segments() {
        assert!(valid_id("grp_01abc"));
        assert!(!valid_id("grp.01"));
        assert!(!valid_id(""));
        assert!(valid_conversation("_agent.karim.movers"));
        assert!(!valid_conversation("_agent.karim.*"));
        assert!(!valid_conversation("_team.x.y"));
        assert!(!valid_conversation("_agent.karim"));
    }

    #[test]
    fn a_line_carries_the_group_the_way_the_mellows_read_it() {
        let m = |n: &str| Member {
            id: format!("agent://karim/{}", n.to_lowercase()),
            name: n.into(),
            conversation: format!("_agent.karim.{}", n.to_lowercase()),
            machine: None,
            pass: None,
        };
        let g = Group {
            id: "grp_1".into(),
            name: "Move team".into(),
            created_at: Utc::now(),
            members: vec![m("Reviewer"), m("Movers")],
        };
        let p = line_payload(
            &g,
            "t_1",
            &[&g.members[1]],
            &[],
            Some(("msg_a", &g.members[0])),
        );
        let msg = Message {
            id: "x".into(),
            topic: "_agent.karim.movers.inbox".into(),
            from: "human://karim".into(),
            owner: None,
            timestamp: Utc::now(),
            content_type: "text/plain".into(),
            body: "hi".into(),
            headers: None,
            payload: Some(p.clone()),
            parent_id: None,
        };
        let ctx = crate::mellows::group_context(&msg, "Movers").unwrap();
        assert_eq!(ctx.name, "Move team");
        assert_eq!(ctx.members, vec!["Reviewer".to_string()]);
        assert_eq!(ctx.handed_over_by.as_deref(), Some("Reviewer"));
        assert_eq!(p[GROUP_KEY]["to"], json!(["agent://karim/movers"]));
    }
}
