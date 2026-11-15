//! `MeshSession`: the object the shells hold.
//!
//! ## Threading model
//!
//! A session owns a small multi-threaded Tokio runtime (2 workers). Every
//! exported method is **blocking**: it runs its future to completion on that
//! runtime and returns. This is the most portable shape across UniFFI
//! targets (Swift, Kotlin and C# via uniffi-bindgen-cs all handle sync calls
//! identically). Shells call methods from a background context
//! (`Task.detached` / `Dispatchers.IO` / `Task.Run`), never from the UI thread.
//! Calling a method from inside a Tokio runtime thread returns
//! `MeshError::InvalidInput` instead of deadlocking.
//!
//! Live events are pushed by a runtime task into a channel drained by one
//! dedicated OS thread per subscription, which invokes the listener —
//! so listener code may call back into the session.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use futures_util::StreamExt;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::decision::Decision;
use mellowmesh_core::message::Message;
use mellowmesh_core::task::Task;
use reqwest::Method;

use crate::conversation::{history_pattern, is_chat_topic, parse_conversation_id, send_topic};
use crate::error::{clean_msg, MeshError};
use crate::events::{map_event, EventContext, MeshEvent, MeshEventListener};
use crate::graph::{graph_inputs, layout_graph, MeshGraph};
use crate::identity::{short_name, stable_hash, Principal, PrincipalKind};
use crate::meshui::is_valid_action_id;
use crate::model::{
    chat_message, ChatMessage, HomeSnapshot, UI_ACTION_CONTENT_TYPE, UI_ACTION_KEY,
};
use crate::snapshot::{build_home, directory_of, AgentPaths, HomeInputs};
use crate::transport::{enc, path_segment, Api};

/// How many recent messages `home()` scans for presence and conversations.
pub const HOME_HISTORY_WINDOW: usize = 500;
/// Largest message body `send_message` accepts.
pub const MAX_SEND_BYTES: usize = 16 * 1024;
const DEFAULT_PRINCIPAL: &str = "human://local";
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// How to reach a hub.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MeshConfig {
    /// `http://127.0.0.1:40000` for a local hub, or a relay hub URL
    /// (`https://relay.example.com/hub/<id>`, e.g. `PairingInfo.hub_url`).
    /// Plain http is only accepted for loopback.
    pub base_url: String,
    /// Bearer token. Must be a **human** token for answering decisions.
    pub token: Option<String>,
    /// Explicit end-to-end sealing; `None` = on for a remote hub with a token.
    pub e2e: Option<bool>,
    /// Shown to the hub as the sending device (`x-mellowmesh-device`).
    pub device_name: String,
    /// The user's principal URI (`human://<handle>`). The hub has no
    /// "who am I" endpoint yet, so the shell supplies it (from pairing or
    /// settings); under authentication the hub stamps the real principal on
    /// everything published regardless. Defaults to `human://local`.
    pub principal: Option<String>,
}

/// Handle to a live subscription. Cancelled on `cancel()` or when the last
/// reference is dropped — keep it alive as long as you want events.
#[derive(uniffi::Object)]
pub struct SubscriptionHandle {
    pattern: String,
    active: Arc<AtomicBool>,
    abort: tokio::task::AbortHandle,
}

#[uniffi::export]
impl SubscriptionHandle {
    /// Stop the subscription. No event is delivered after this returns
    /// (except one already being delivered concurrently). Idempotent.
    pub fn cancel(&self) {
        self.active.store(false, Ordering::SeqCst);
        self.abort.abort();
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn pattern(&self) -> String {
        self.pattern.clone()
    }
}

impl Drop for SubscriptionHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(uniffi::Object)]
pub struct MeshSession {
    rt: Option<tokio::runtime::Runtime>,
    api: Arc<Api>,
    me: Principal,
    device_name: String,
    read_markers: Mutex<HashMap<String, i64>>,
    /// Registry snapshot used to name senders in live events; refreshed by
    /// `home()` and `subscribe()`.
    event_ctx: Arc<RwLock<Arc<EventContext>>>,
}

impl Drop for MeshSession {
    fn drop(&mut self) {
        // Never block the (possibly UI) thread that drops the last reference.
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

fn validate_principal(p: &str) -> Result<String, MeshError> {
    let p = p.trim();
    let ok = p.len() <= 256
        && p.split_once("://").is_some_and(|(scheme, rest)| {
            !scheme.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !rest.is_empty()
                && !rest.chars().any(|c| c.is_whitespace() || c.is_control())
        });
    if ok {
        Ok(p.to_string())
    } else {
        Err(MeshError::invalid(format!("invalid principal URI '{p}'")))
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl MeshSession {
    fn rt(&self) -> &tokio::runtime::Runtime {
        self.rt.as_ref().expect("runtime present until drop")
    }

    /// Run a future to completion on the session runtime.
    fn run<T>(&self, fut: impl Future<Output = Result<T, MeshError>>) -> Result<T, MeshError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(MeshError::invalid(
                "MeshSession methods are blocking; call them from a background thread, \
                 not from an async runtime",
            ));
        }
        self.rt().block_on(fut)
    }

    fn refresh_event_ctx(&self, agents: &[AgentRegistration]) {
        let ctx = Arc::new(EventContext {
            me: self.me.id.clone(),
            dir: directory_of(agents),
            paths: AgentPaths::new(agents),
        });
        if let Ok(mut guard) = self.event_ctx.write() {
            *guard = ctx;
        }
    }

    async fn publish(&self, msg: Message) -> Result<ChatMessage, MeshError> {
        let body = serde_json::to_value(&msg).map_err(|e| MeshError::invalid(e.to_string()))?;
        let text = self.api.call(Method::POST, "/publish", Some(body)).await?;
        // The hub returns the stored message (with its assigned id/time).
        let stored: Message = serde_json::from_str(&text).unwrap_or(msg);
        let ctx = self.current_ctx();
        Ok(chat_message(&stored, &self.me.id, &ctx.dir))
    }

    fn current_ctx(&self) -> Arc<EventContext> {
        self.event_ctx
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }

    fn outgoing(&self, topic: String, body: String) -> Message {
        let mut headers = HashMap::new();
        headers.insert("x-mellowmesh-device".to_string(), self.device_name.clone());
        Message {
            id: String::new(),
            topic,
            from: self.me.id.clone(),
            owner: None,
            timestamp: chrono::Utc::now(),
            content_type: "text/plain".to_string(),
            body,
            headers: Some(headers),
            payload: None,
            parent_id: None,
        }
    }

    async fn fetch_home(&self) -> Result<HomeSnapshot, MeshError> {
        let history_path = format!("/history?limit={HOME_HISTORY_WINDOW}");
        let (agents, tasks, decisions, history) = tokio::try_join!(
            self.api.get_json::<Vec<AgentRegistration>>("/agents"),
            self.api.get_json::<Vec<Task>>("/tasks"),
            self.api.get_json::<Vec<Decision>>("/decisions"),
            self.api.get_json::<Vec<Message>>(&history_path),
        )?;
        self.refresh_event_ctx(&agents);
        let markers = self
            .read_markers
            .lock()
            .map(|m| m.clone())
            .unwrap_or_default();
        Ok(build_home(&HomeInputs {
            me: &self.me,
            agents: &agents,
            tasks: &tasks,
            decisions: &decisions,
            history: &history,
            read_markers: &markers,
            now_ms: now_ms(),
        }))
    }
}

#[uniffi::export]
impl MeshSession {
    /// Connect to a hub: validates the config, checks `/health`, then makes
    /// one authenticated read (`/agents`) so a bad token fails here with
    /// `Unauthorized` rather than on the first screen.
    #[uniffi::constructor]
    pub fn connect(config: MeshConfig) -> Result<Arc<Self>, MeshError> {
        let principal =
            validate_principal(config.principal.as_deref().unwrap_or(DEFAULT_PRINCIPAL))?;
        let device_name = clean_msg(&config.device_name);
        let device_name = if device_name.is_empty() {
            "device".to_string()
        } else {
            device_name.chars().take(64).collect()
        };
        let api = Arc::new(Api::new(&config.base_url, config.token, config.e2e)?);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("mellowmesh-core")
            .enable_all()
            .build()
            .map_err(|e| MeshError::server(format!("cannot start runtime: {e}")))?;
        let me = Principal {
            display_name: short_name(&principal),
            kind: PrincipalKind::of(&principal),
            id: principal.clone(),
            device_name: device_name.clone(),
        };
        let session = MeshSession {
            rt: Some(rt),
            api,
            me: me.clone(),
            device_name,
            read_markers: Mutex::new(HashMap::new()),
            event_ctx: Arc::new(RwLock::new(Arc::new(EventContext {
                me: principal,
                dir: Default::default(),
                paths: AgentPaths::new(&[]),
            }))),
        };
        session.run(async {
            let (status, body) = session.api.raw(Method::GET, "/health", None).await?;
            if !(200..300).contains(&status) {
                return Err(MeshError::from_status(status, &body));
            }
            let agents: Vec<AgentRegistration> = session.api.get_json("/agents").await?;
            session.refresh_event_ctx(&agents);
            Ok(())
        })?;
        Ok(Arc::new(session))
    }

    pub fn me(&self) -> Principal {
        self.me.clone()
    }

    pub fn base_url(&self) -> String {
        self.api.base_url().to_string()
    }

    /// Whether requests and subscriptions are sealed end-to-end.
    pub fn e2e_enabled(&self) -> bool {
        self.api.e2e()
    }

    /// Everything the home screen needs, in one round of parallel requests.
    pub fn home(&self) -> Result<HomeSnapshot, MeshError> {
        self.run(self.fetch_home())
    }

    /// The most recent `limit` messages of a conversation (1..=1000; 0 means
    /// 50), oldest first. Uses server-side pattern history.
    pub fn conversation(&self, id: String, limit: u32) -> Result<Vec<ChatMessage>, MeshError> {
        let conv = parse_conversation_id(&id)?;
        let limit = match limit {
            0 => 50,
            n => n.min(1000),
        };
        let path = format!(
            "/history?limit={limit}&pattern={}",
            enc(&history_pattern(&conv))
        );
        self.run(async {
            let history: Vec<Message> = self.api.get_json(&path).await?;
            let ctx = self.current_ctx();
            let mut seen = std::collections::HashSet::new();
            let mut out: Vec<ChatMessage> = history
                .iter()
                .filter(|m| is_chat_topic(&conv, &m.topic))
                .filter(|m| m.id.is_empty() || seen.insert(m.id.clone()))
                .map(|m| chat_message(m, &self.me.id, &ctx.dir))
                .collect();
            out.sort_by(|a, b| a.ts_ms.cmp(&b.ts_ms).then(a.id.cmp(&b.id)));
            Ok(out)
        })
    }

    /// Send a text message. For an `Agent` conversation it goes to the
    /// agent's inbox (`_agent.<owner>.<name>.inbox`); otherwise to the
    /// conversation root topic. Returns the message as stored by the hub.
    pub fn send_message(
        &self,
        conversation_id: String,
        text: String,
    ) -> Result<ChatMessage, MeshError> {
        let conv = parse_conversation_id(&conversation_id)?;
        if text.trim().is_empty() {
            return Err(MeshError::invalid("message is empty"));
        }
        if text.len() > MAX_SEND_BYTES {
            return Err(MeshError::invalid(format!(
                "message exceeds {MAX_SEND_BYTES} bytes"
            )));
        }
        let msg = self.outgoing(send_topic(&conv), text);
        self.run(self.publish(msg))
    }

    /// Reply to a MeshUI action tap. Publishes an ordinary message into the
    /// same conversation (`payload.ui_action = {card_message_id, action_id}`,
    /// `parent_id` = the card's message). It **never** answers a decision.
    pub fn send_ui_action(
        &self,
        conversation_id: String,
        card_message_id: String,
        action_id: String,
        label: String,
    ) -> Result<ChatMessage, MeshError> {
        let conv = parse_conversation_id(&conversation_id)?;
        if !is_valid_action_id(&action_id) {
            return Err(MeshError::invalid(format!(
                "invalid action id '{action_id}'"
            )));
        }
        if card_message_id.is_empty() || card_message_id.len() > 128 {
            return Err(MeshError::invalid("invalid card message id"));
        }
        let label: String = clean_msg(&label).chars().take(40).collect();
        let mut msg = self.outgoing(
            send_topic(&conv),
            if label.is_empty() {
                action_id.clone()
            } else {
                label
            },
        );
        msg.content_type = UI_ACTION_CONTENT_TYPE.to_string();
        msg.parent_id = Some(card_message_id.clone());
        msg.payload = Some(serde_json::json!({
            UI_ACTION_KEY: { "card_message_id": card_message_id, "action_id": action_id }
        }));
        self.run(self.publish(msg))
    }

    /// Answer a decision. Shells MUST gate this behind OS biometrics (Face
    /// ID / Touch ID / BiometricPrompt / Windows Hello) and the session's
    /// token must be a human token: the hub rejects agent principals and,
    /// under authentication, anyone but the `required_decider` or the hub
    /// owner (`Forbidden`). A second answer fails (`InvalidInput`).
    pub fn respond_decision(
        &self,
        decision_id: String,
        option_id: String,
    ) -> Result<(), MeshError> {
        if option_id.trim().is_empty() || option_id.len() > 128 {
            return Err(MeshError::invalid("invalid option id"));
        }
        let path = format!("/decisions/{}/respond", path_segment(&decision_id)?);
        self.run(async {
            self.api
                .call(
                    Method::POST,
                    &path,
                    Some(serde_json::json!({ "option_id": option_id })),
                )
                .await?;
            Ok(())
        })
    }

    /// Record that the user has read `conversation_id` up to `up_to_ms`
    /// (a message `ts_ms`). Read state is local to the session; shells
    /// persist it and restore it after `connect`.
    pub fn mark_read(&self, conversation_id: String, up_to_ms: i64) {
        if let Ok(mut m) = self.read_markers.lock() {
            let e = m.entry(conversation_id).or_insert(up_to_ms);
            *e = (*e).max(up_to_ms);
        }
    }

    pub fn read_marker(&self, conversation_id: String) -> Option<i64> {
        self.read_markers
            .lock()
            .ok()
            .and_then(|m| m.get(&conversation_id).copied())
    }

    /// Subscribe to live events for a topic pattern (`**` for everything;
    /// e.g. `_agent.yannick.**`, `_decision.**`). Reconnects with backoff
    /// (1 s → 30 s) and reports `StreamStatus` changes.
    pub fn subscribe(
        &self,
        pattern: String,
        listener: Box<dyn MeshEventListener>,
    ) -> Result<Arc<SubscriptionHandle>, MeshError> {
        mellowmesh_core::topic::validate_pattern(&pattern).map_err(MeshError::invalid)?;
        // Best effort: name senders with a fresh registry.
        if let Ok(agents) = self.run(self.api.get_json::<Vec<AgentRegistration>>("/agents")) {
            self.refresh_event_ctx(&agents);
        }

        let active = Arc::new(AtomicBool::new(true));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<MeshEvent>();
        let dispatch_active = active.clone();
        std::thread::Builder::new()
            .name("mellowmesh-events".to_string())
            .spawn(move || {
                while let Some(ev) = rx.blocking_recv() {
                    if !dispatch_active.load(Ordering::SeqCst) {
                        break;
                    }
                    // A panicking listener must not take the dispatcher down.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        listener.on_event(ev)
                    }));
                }
            })
            .map_err(|e| MeshError::server(format!("cannot start event thread: {e}")))?;

        let client = self.api.client().clone();
        let ctx_cell = self.event_ctx.clone();
        let task_pattern = pattern.clone();
        let task = self.rt().spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                match client.subscribe(&task_pattern).await {
                    Ok(stream) => {
                        backoff = Duration::from_secs(1);
                        let up = MeshEvent::StreamStatus {
                            connected: true,
                            detail: "subscribed".to_string(),
                        };
                        if tx.send(up).is_err() {
                            return;
                        }
                        futures_util::pin_mut!(stream);
                        let mut detail = "stream closed".to_string();
                        while let Some(item) = stream.next().await {
                            match item {
                                Ok(msg) => {
                                    let ctx = ctx_cell
                                        .read()
                                        .map(|g| g.clone())
                                        .unwrap_or_else(|p| p.into_inner().clone());
                                    for ev in map_event(&msg, &ctx) {
                                        if tx.send(ev).is_err() {
                                            return;
                                        }
                                    }
                                }
                                Err(e) => {
                                    detail = e.to_string();
                                    break;
                                }
                            }
                        }
                        let down = MeshEvent::StreamStatus {
                            connected: false,
                            detail: clean_msg(&detail),
                        };
                        if tx.send(down).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let down = MeshEvent::StreamStatus {
                            connected: false,
                            detail: clean_msg(&e.to_string()),
                        };
                        if tx.send(down).is_err() {
                            return;
                        }
                    }
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        });
        Ok(Arc::new(SubscriptionHandle {
            pattern,
            active,
            abort: task.abort_handle(),
        }))
    }

    /// The "Mesh" view: the home snapshot as a graph, laid out for a
    /// `width` × `height` canvas (deterministic for a given hub state).
    pub fn graph(&self, width: f64, height: f64) -> Result<MeshGraph, MeshError> {
        let home = self.home()?;
        let (nodes, edges) = graph_inputs(&home);
        layout_graph(nodes, edges, width, height, stable_hash(&self.me.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_validation() {
        assert_eq!(validate_principal(" human://y ").unwrap(), "human://y");
        assert!(validate_principal("human://").is_err());
        assert!(validate_principal("yannick").is_err());
        assert!(validate_principal("human://a b").is_err());
    }

    #[test]
    fn connect_fails_cleanly_without_a_hub() {
        // Port 9 (discard) on loopback: nothing listens there in CI.
        let err = MeshSession::connect(MeshConfig {
            base_url: "http://127.0.0.1:9".into(),
            token: None,
            e2e: None,
            device_name: "test".into(),
            principal: None,
        })
        .err()
        .unwrap();
        assert!(matches!(err, MeshError::Network { .. }), "{err:?}");
    }
}
