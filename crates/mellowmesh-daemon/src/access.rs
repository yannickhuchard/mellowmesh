//! Network access control at runtime: the cached network snapshot, public
//! hosts, the remote-request marker, and per-request / per-message checks.
//!
//! The rules themselves live in [`mellowmesh_core::network`]; this module
//! wires them to requests. Every per-message check reads an in-memory
//! [`NetworkSnapshot`] that is reloaded after each admin change, so no
//! check touches SQLite.

use crate::auth::AuthContext;
use crate::state::AppState;
use mellowmesh_core::message::Message;
use mellowmesh_core::network::{
    AccessDecision, AccessLevel, Caller, NetworkSnapshot, Space, WriteAccess,
};
use std::sync::{Arc, RwLock};

/// Header carrying the per-process marker that flags a loopback-dispatched
/// request (relay link, sealed E2E request) as remote. The guard strips it
/// from every incoming request, so handlers never see it.
pub const REMOTE_MARKER_HEADER: &str = "x-mellowmesh-remote";

/// Hub-set header on a message posted as a visitor.
pub const VISITOR_HEADER: &str = "x-mellowmesh-visitor";

/// Whether a request is remote (public host, non-loopback peer, relay).
/// Inserted by the Host/Origin guard on every request.
#[derive(Debug, Clone, Copy, Default)]
pub struct RequestOrigin {
    pub remote: bool,
}

/// Runtime access state shared by every request.
pub struct AccessState {
    snapshot: RwLock<Option<Arc<NetworkSnapshot>>>,
    /// Extra `Host` names (`host` or `host:port`, lowercase) the hub answers
    /// to; requests naming one are remote.
    pub public_hosts: Vec<String>,
    /// Random per-process marker for [`REMOTE_MARKER_HEADER`].
    pub remote_marker: String,
}

impl Default for AccessState {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl AccessState {
    pub fn new(public_hosts: Vec<String>) -> Self {
        Self {
            snapshot: RwLock::new(None),
            public_hosts: public_hosts
                .into_iter()
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty())
                .collect(),
            remote_marker: mellowmesh_core::auth::generate_token(),
        }
    }

    /// Drop the cached snapshot; the next check reloads it.
    pub fn invalidate(&self) {
        *self.snapshot.write().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

/// Load the network snapshot from the store.
pub fn load_snapshot(state: &AppState) -> anyhow::Result<NetworkSnapshot> {
    Ok(NetworkSnapshot {
        owner: state.owner.clone(),
        network: state.store.get_network()?,
        spaces: state.store.list_spaces()?,
        teams: state.store.list_teams()?,
    })
}

/// The current network snapshot, loaded on a miss. If it cannot be loaded
/// the hub fails closed: a private space over everything (owner only).
pub fn snapshot(state: &AppState) -> Arc<NetworkSnapshot> {
    if let Some(s) = state
        .access
        .snapshot
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
    {
        return s.clone();
    }
    match load_snapshot(state) {
        Ok(s) => {
            let s = Arc::new(s);
            *state
                .access
                .snapshot
                .write()
                .unwrap_or_else(|p| p.into_inner()) = Some(s.clone());
            s
        }
        Err(e) => {
            tracing::error!("Failed to load network settings, denying access: {}", e);
            Arc::new(NetworkSnapshot {
                owner: state.owner.clone(),
                spaces: vec![Space {
                    id: "unavailable".into(),
                    patterns: vec!["**".into()],
                    access: AccessLevel::Private,
                    ..Default::default()
                }],
                ..Default::default()
            })
        }
    }
}

/// Reload the snapshot after an admin change.
pub fn refresh(state: &AppState) {
    state.access.invalidate();
    let _ = snapshot(state);
}

impl AuthContext {
    /// The caller as the access rules see it.
    pub fn caller(&self) -> Caller<'_> {
        match &self.principal {
            Some(p) => Caller::token(&p.id, &p.kind, &p.read_scopes, &p.write_scopes),
            None if self.trusted_local => Caller::trusted_local(),
            None => Caller::visitor(),
        }
    }

    /// Evaluate the rules for this caller on `topic`.
    pub fn access(&self, state: &AppState, topic: &str) -> AccessDecision {
        snapshot(state).evaluate(&self.caller(), topic)
    }

    /// Whether the caller may read `topic`.
    pub fn can_read(&self, state: &AppState, topic: &str) -> bool {
        self.access(state, topic).read
    }

    /// Whether the caller may write `topic` as itself (visitor posts, which
    /// only `POST /publish` accepts, do not count).
    pub fn can_write(&self, state: &AppState, topic: &str) -> bool {
        self.access(state, topic).write == WriteAccess::Allowed
    }
}

/// The topics deciding who sees a task: its topics, or `_task.<id>` when it
/// has none.
pub fn task_topics(task_id: &str, topics: &[String]) -> Vec<String> {
    let valid: Vec<String> = topics
        .iter()
        .filter(|t| mellowmesh_core::topic::Topic::new(t.as_str()).is_ok())
        .cloned()
        .collect();
    if valid.is_empty() {
        vec![format!("_task.{task_id}")]
    } else {
        valid
    }
}

/// Whether a task is visible to a caller: it may read at least one of the
/// task's topics.
pub fn task_visible(
    snap: &NetworkSnapshot,
    caller: &Caller,
    task_id: &str,
    topics: &[String],
) -> bool {
    task_topics(task_id, topics)
        .iter()
        .any(|t| snap.can_read(caller, t))
}

/// Whether a task is hidden from a caller in `GET /tasks`: every one of its
/// topics is covered by a space that denies the caller. Tasks on uncovered
/// topics stay listed as before.
pub fn task_hidden(
    snap: &NetworkSnapshot,
    caller: &Caller,
    task_id: &str,
    topics: &[String],
) -> bool {
    task_topics(task_id, topics).iter().all(|t| {
        let d = snap.evaluate(caller, t);
        d.space.is_some() && !d.read
    })
}

/// Hub-published task events whose payload is the task.
const TASK_EVENTS: &[&str] = &[
    "created",
    "claimed",
    "started",
    "completed",
    "updated",
    "cancelled",
    "reclaimed",
];

/// `(task_id, event)` when `topic` is a hub task event `_task.<id>.<event>`.
pub fn task_event(topic: &str) -> Option<(&str, &str)> {
    let rest = topic.strip_prefix("_task.")?;
    let (id, event) = rest.split_once('.')?;
    (!id.is_empty() && TASK_EVENTS.contains(&event)).then_some((id, event))
}

/// Whether a caller may receive `msg`. Hub task events follow the task's
/// visibility (its topics); everything else its own topic.
pub fn can_read_message(snap: &NetworkSnapshot, caller: &Caller, msg: &Message) -> bool {
    if let Some((task_id, _)) = task_event(&msg.topic) {
        if let Some(topics) = msg
            .payload
            .as_ref()
            .and_then(|p| p.get("topics"))
            .and_then(|t| t.as_array())
        {
            let topics: Vec<String> = topics
                .iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect();
            return task_visible(snap, caller, task_id, &topics);
        }
    }
    snap.can_read(caller, &msg.topic)
}

/// Keep only the messages a caller may read.
pub fn filter_messages(state: &AppState, ctx: &AuthContext, msgs: Vec<Message>) -> Vec<Message> {
    let snap = snapshot(state);
    let caller = ctx.caller();
    msgs.into_iter()
        .filter(|m| can_read_message(&snap, &caller, m))
        .collect()
}

/// Split a `host[:port]` value into a lowercase host and optional port.
fn host_and_port(value: &str) -> (String, Option<&str>) {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let host = v[..end + 2].to_ascii_lowercase();
            let port = rest[end + 1..].strip_prefix(':');
            return (host, port);
        }
    }
    match v.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (h.to_ascii_lowercase(), Some(p)),
        _ => (v.to_ascii_lowercase(), None),
    }
}

/// True when `value` (a `Host` header, or an Origin's authority) names one
/// of the configured public hosts. An entry without a port matches any
/// port; an entry with a port matches only that port.
pub fn is_public_host(public_hosts: &[String], value: &str) -> bool {
    let (host, port) = host_and_port(value);
    if host.is_empty() {
        return false;
    }
    public_hosts.iter().any(|entry| {
        let (eh, ep) = host_and_port(entry);
        eh == host && (ep.is_none() || ep == port)
    })
}

/// True when an `Origin` is `http(s)://<public-host>`.
pub fn is_public_origin(public_hosts: &[String], origin: &str) -> bool {
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"));
    match rest {
        Some(authority) if !authority.contains('/') && !authority.contains('@') => {
            is_public_host(public_hosts, authority)
        }
        _ => false,
    }
}

/// Parse `MELLOWMESH_PUBLIC_HOSTS` / repeated `--public-host` values.
pub fn parse_public_hosts(values: &[String]) -> Vec<String> {
    values
        .iter()
        .flat_map(|v| v.split(','))
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect()
}

/// Refuse to listen on a non-loopback address without `--require-auth`.
pub fn check_listen(listen: std::net::IpAddr, require_auth: bool) -> Result<(), String> {
    if !listen.is_loopback() && !require_auth {
        return Err(format!(
            "Refusing to listen on {listen} without --require-auth: a hub reachable from \
             other machines must require tokens"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_host_matching() {
        let hosts = parse_public_hosts(&[
            "192.168.1.20:40000, Mesh.Example.com".to_string(),
            "[fd00::1]:40000".to_string(),
        ]);
        assert_eq!(hosts.len(), 3);
        assert!(is_public_host(&hosts, "192.168.1.20:40000"));
        assert!(!is_public_host(&hosts, "192.168.1.20:40001"));
        assert!(!is_public_host(&hosts, "192.168.1.20"));
        assert!(is_public_host(&hosts, "mesh.example.com"));
        assert!(is_public_host(&hosts, "MESH.example.com:443"));
        assert!(!is_public_host(&hosts, "evil.mesh.example.com"));
        assert!(is_public_host(&hosts, "[fd00::1]:40000"));
        assert!(!is_public_host(&hosts, ""));
        assert!(is_public_origin(&hosts, "https://mesh.example.com"));
        assert!(is_public_origin(&hosts, "http://192.168.1.20:40000"));
        assert!(!is_public_origin(&hosts, "http://192.168.1.20:3000"));
        assert!(!is_public_origin(&hosts, "https://evil.example"));
        assert!(!is_public_origin(&hosts, "mesh.example.com"));
        assert!(!is_public_origin(&hosts, "https://x@mesh.example.com"));
    }

    #[test]
    fn listen_requires_auth_off_loopback() {
        use std::net::IpAddr;
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        let any: IpAddr = "0.0.0.0".parse().unwrap();
        let lan: IpAddr = "192.168.1.20".parse().unwrap();
        let v6lo: IpAddr = "::1".parse().unwrap();
        assert!(check_listen(lo, false).is_ok());
        assert!(check_listen(v6lo, false).is_ok());
        assert!(check_listen(any, false).is_err());
        assert!(check_listen(lan, false).is_err());
        assert!(check_listen(any, true).is_ok());
    }

    #[test]
    fn task_event_topics() {
        assert_eq!(task_event("_task.t1.created"), Some(("t1", "created")));
        assert_eq!(task_event("_task.t1.reclaimed"), Some(("t1", "reclaimed")));
        assert_eq!(task_event("_task.t1.progress"), None);
        assert_eq!(task_event("_task.t1"), None);
        assert_eq!(task_event("_forum.x.created"), None);
        assert_eq!(task_topics("t1", &[]), vec!["_task.t1"]);
        assert_eq!(
            task_topics("t1", &["_team.home.tasks".into()]),
            vec!["_team.home.tasks"]
        );
    }
}
