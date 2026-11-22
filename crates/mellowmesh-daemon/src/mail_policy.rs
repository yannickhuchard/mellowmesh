//! Notes between AIs: which helpers' mailboxes an invited AI (a connected
//! app, `agent://<owner>/<platform>`) may write to.
//!
//! The owner picks one rule for the network — any helper's mailbox (the
//! default: the network organizes itself), only some, or none — and may
//! give one AI a rule of its own. The hub checks the rule on every write to
//! `_agent.<owner>.<helper>.inbox`, mention routing included, so a change
//! applies at once to keys already given.
//!
//! What never changes: a helper's outbox is its own voice (nobody writes
//! there), and an AI may always use its own corner. A note is only a note:
//! the helper that receives it decides whether to act.

use crate::auth::AuthContext;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Where the policy is kept (`app_config`).
const CONFIG_KEY: &str = "agent_mail_policy";

/// Which mailboxes a rule opens.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Any helper's mailbox.
    #[default]
    Any,
    /// Only the listed mailboxes.
    Only,
    /// No mailbox: shared rooms and tasks only.
    None,
}

/// One rule: a mode and, for [`Mode::Only`], the helpers (agent ids) whose
/// mailboxes accept notes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Rule {
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boxes: Vec<String>,
}

impl Rule {
    fn allows(&self, target: &str) -> bool {
        match self.mode {
            Mode::Any => true,
            Mode::Only => self.boxes.iter().any(|b| b == target),
            Mode::None => false,
        }
    }
}

/// The network rule and the AIs that have a rule of their own.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MailPolicy {
    #[serde(default)]
    pub network: Rule,
    /// Keyed by the AI's principal (`agent://<owner>/<platform>`).
    #[serde(default)]
    pub per_ai: BTreeMap<String, Rule>,
}

impl MailPolicy {
    /// The rule an AI follows: its own, else the network's.
    pub fn rule_for(&self, ai: &str) -> &Rule {
        self.per_ai.get(ai).unwrap_or(&self.network)
    }

    /// Whether `ai` may leave a note in `target`'s mailbox. Its own is
    /// always allowed.
    pub fn allows(&self, ai: &str, target: &str) -> bool {
        ai == target || self.rule_for(ai).allows(target)
    }
}

/// A policy that cannot be read fails closed: no mailbox.
fn closed() -> MailPolicy {
    MailPolicy {
        network: Rule {
            mode: Mode::None,
            boxes: Vec::new(),
        },
        per_ai: BTreeMap::new(),
    }
}

fn load(state: &AppState) -> MailPolicy {
    match state.store.get_config(CONFIG_KEY) {
        Ok(None) => MailPolicy::default(),
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            tracing::error!("Unreadable agent mail policy, closing mailboxes: {e}");
            closed()
        }),
        Err(e) => {
            tracing::error!("Cannot read the agent mail policy, closing mailboxes: {e}");
            closed()
        }
    }
}

/// The current policy (cached; reloaded after a change).
pub fn current(state: &AppState) -> Arc<MailPolicy> {
    let cache = &state.access.mail_policy;
    if let Some(p) = cache.read().unwrap_or_else(|p| p.into_inner()).as_ref() {
        return p.clone();
    }
    let p = Arc::new(load(state));
    *cache.write().unwrap_or_else(|p| p.into_inner()) = Some(p.clone());
    p
}

fn save(state: &AppState, policy: &MailPolicy) -> anyhow::Result<()> {
    state
        .store
        .set_config(CONFIG_KEY, &serde_json::to_string(policy)?)?;
    *state
        .access
        .mail_policy
        .write()
        .unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(policy.clone()));
    Ok(())
}

/// `agent://<owner>/<helper>` when `topic` is the mailbox
/// `_agent.<owner>.<helper>.inbox` of one of the owner's helpers.
pub fn mailbox_owner(state: &AppState, topic: &str) -> Option<String> {
    let handle = crate::mellows::owner_handle(&state.owner);
    let helper = topic
        .strip_prefix("_agent.")?
        .strip_prefix(handle.as_str())?
        .strip_prefix('.')?
        .strip_suffix(".inbox")?;
    (!helper.is_empty() && !helper.contains('.') && !helper.contains('*'))
        .then(|| format!("agent://{handle}/{helper}"))
}

/// Whether `id` names one of the owner's helpers: `agent://<owner>/<helper>`.
fn is_helper_id(state: &AppState, id: &str) -> bool {
    let prefix = format!("agent://{}/", crate::mellows::owner_handle(&state.owner));
    id.strip_prefix(prefix.as_str()).is_some_and(|h| {
        !h.is_empty()
            && h.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    })
}

/// The name of an invited AI (a catalog platform's own principal).
fn invited_ai_name(state: &AppState, principal: &str) -> Option<&'static str> {
    crate::platforms::CATALOG
        .iter()
        .find(|spec| crate::mellows::platform_agent_id(&state.owner, spec.id) == principal)
        .map(|spec| spec.name)
}

/// Whether `principal` is an invited AI (a connected app's own key).
pub fn is_invited_ai(state: &AppState, principal: &str) -> bool {
    invited_ai_name(state, principal).is_some()
}

/// The extra write grant the policy gives this caller on `topic`: the
/// mailbox itself, when the caller is an invited AI and its rule opens that
/// mailbox. Spaces still apply on top (the grant only stands in for a
/// token scope).
pub fn grant(state: &AppState, ctx: &AuthContext, topic: &str) -> Option<String> {
    let p = ctx.principal.as_ref()?;
    if p.kind != "agent" || !is_invited_ai(state, &p.id) {
        return None;
    }
    let target = mailbox_owner(state, topic)?;
    current(state)
        .allows(&p.id, &target)
        .then(|| topic.to_string())
}

/// Whether an @mention from `from` may be routed into `inbox`: the same
/// rule as writing there directly, for invited AIs; unchanged for anyone
/// else.
pub fn may_route(state: &AppState, from: &str, inbox: &str) -> bool {
    if !is_invited_ai(state, from) {
        return true;
    }
    match mailbox_owner(state, inbox) {
        Some(target) => current(state).allows(from, &target),
        None => true,
    }
}

// --- HTTP ---------------------------------------------------------------

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

/// `PUT /agent-mail` body: set the network rule (`for` absent) or one AI's
/// rule (`for` = its principal; `rule` absent = follow the network rule).
#[derive(Debug, Deserialize)]
pub struct RuleChange {
    #[serde(rename = "for", default)]
    pub for_ai: Option<String>,
    #[serde(default)]
    pub rule: Option<Rule>,
}

fn helper_list(state: &AppState) -> Vec<serde_json::Value> {
    let prefix = format!("agent://{}/", crate::mellows::owner_handle(&state.owner));
    let mut out: Vec<serde_json::Value> = state
        .store
        .list_agents()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.id.starts_with(&prefix))
        .map(|a| serde_json::json!({ "id": a.id, "name": a.name }))
        .collect();
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    out
}

fn invited_list(state: &AppState) -> Vec<serde_json::Value> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for t in state.store.list_tokens().unwrap_or_default() {
        if t.revoked || !seen.insert(t.principal.clone()) {
            continue;
        }
        if let Some(name) = invited_ai_name(state, &t.principal) {
            out.push(serde_json::json!({ "id": t.principal, "name": name }));
        }
    }
    out
}

/// What the owner sees: the policy, the invited AIs, and the helpers whose
/// mailboxes a rule can open.
fn owner_view(state: &AppState) -> serde_json::Value {
    let policy = current(state);
    serde_json::json!({
        "network": policy.network,
        "per_ai": policy.per_ai,
        "invited": invited_list(state),
        "mailboxes": helper_list(state),
    })
}

/// `GET /agent-mail`: the owner gets the whole policy; an invited AI gets
/// the rule it follows and the mailboxes it may write to, so it can pick a
/// note or a task without trying and failing.
pub async fn get_policy(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<AuthContext>,
) -> impl IntoResponse {
    if crate::auth::is_admin(&state, &ctx) {
        return (StatusCode::OK, Json(owner_view(&state))).into_response();
    }
    match ctx.principal.as_ref() {
        Some(p) if is_invited_ai(&state, &p.id) => {
            let policy = current(&state);
            let rule = policy.rule_for(&p.id).clone();
            let own = policy.per_ai.contains_key(&p.id);
            let may_write: serde_json::Value = match rule.mode {
                Mode::Any => serde_json::json!("any"),
                _ => serde_json::json!(rule.boxes),
            };
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "you": p.id,
                    "rule": rule,
                    "own_rule": own,
                    "may_write": may_write,
                    "always": "your own mailbox and corner; never another helper's outbox",
                    "otherwise": "create a task and assign it to the helper",
                })),
            )
                .into_response()
        }
        _ => (
            crate::auth::admin_refusal(&ctx),
            Json(serde_json::json!({
                "error": "Only the owner, or an invited AI asking about itself, may read this"
            })),
        )
            .into_response(),
    }
}

/// `PUT /agent-mail` (owner only): change the network rule or one AI's.
pub async fn put_policy(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<AuthContext>,
    Json(change): Json<RuleChange>,
) -> impl IntoResponse {
    if !crate::auth::is_admin(&state, &ctx) {
        return (
            crate::auth::admin_refusal(&ctx),
            Json(serde_json::json!({ "error": "Only the owner may change who AIs can write to" })),
        )
            .into_response();
    }
    let bad = |msg: &str| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response()
    };
    if let Some(rule) = &change.rule {
        if rule.boxes.len() > 256 {
            return bad("Too many mailboxes (256 at most)");
        }
        if !rule.boxes.iter().all(|b| is_helper_id(&state, b)) {
            return bad("Each mailbox is one of your helpers: agent://<you>/<helper>");
        }
    }
    let mut policy = (*current(&state)).clone();
    match (change.for_ai, change.rule) {
        (None, Some(rule)) => policy.network = rule,
        (None, None) => return bad("Give a rule for the network"),
        (Some(ai), rule) => {
            if !is_invited_ai(&state, &ai) {
                return bad("Not an invited AI of this network (agent://<you>/<app>)");
            }
            match rule {
                Some(r) => {
                    policy.per_ai.insert(ai, r);
                }
                None => {
                    policy.per_ai.remove(&ai);
                }
            }
        }
    }
    for rule in std::iter::once(&mut policy.network).chain(policy.per_ai.values_mut()) {
        if rule.mode != Mode::Only {
            rule.boxes.clear();
        }
        rule.boxes.sort();
        rule.boxes.dedup();
    }
    if let Err(e) = save(&state, &policy) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    (StatusCode::OK, Json(owner_view(&state))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(mode: Mode, boxes: &[&str]) -> Rule {
        Rule {
            mode,
            boxes: boxes.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn open_by_default() {
        let p = MailPolicy::default();
        assert!(p.allows("agent://y/grok", "agent://y/coder"));
        assert_eq!(serde_json::to_value(&p).unwrap()["network"]["mode"], "any");
    }

    #[test]
    fn only_and_none_and_own_rule() {
        let mut p = MailPolicy {
            network: rule(Mode::None, &[]),
            per_ai: BTreeMap::new(),
        };
        assert!(!p.allows("agent://y/grok", "agent://y/coder"));
        // Its own mailbox stays open.
        assert!(p.allows("agent://y/grok", "agent://y/grok"));
        p.per_ai.insert(
            "agent://y/grok".into(),
            rule(Mode::Only, &["agent://y/coder"]),
        );
        assert!(p.allows("agent://y/grok", "agent://y/coder"));
        assert!(!p.allows("agent://y/grok", "agent://y/finance"));
        // Others keep the network rule.
        assert!(!p.allows("agent://y/codex", "agent://y/coder"));
    }

    #[test]
    fn rule_json_round_trips() {
        let raw = r#"{"network":{"mode":"only","boxes":["agent://y/coder"]},"per_ai":{"agent://y/grok":{"mode":"none"}}}"#;
        let p: MailPolicy = serde_json::from_str(raw).unwrap();
        assert_eq!(p.network.mode, Mode::Only);
        assert_eq!(p.rule_for("agent://y/grok").mode, Mode::None);
        assert_eq!(p.rule_for("agent://y/codex").mode, Mode::Only);
    }
}
