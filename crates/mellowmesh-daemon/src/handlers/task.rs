//! Tasks: create, list, claim, complete, update (`PATCH`), and the task
//! graph behind task boards (`GET /tasks/graph`). Every lifecycle change is
//! announced on `_task.<id>.<event>` with the task as payload.

use crate::auth::{is_admin, AuthContext};
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::{SecondsFormat, Utc};
use mellowmesh_core::message::Message;
use mellowmesh_core::task::{status, Task, TaskUpdate, TransitionRule, DEFAULT_LEASE_SECONDS};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use ulid::Ulid;

type ApiError = (StatusCode, String);

#[derive(Deserialize)]
pub struct ClaimPayload {
    claimed_by: String,
    lease_seconds: Option<u64>,
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn internal(context: &str, e: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, format!("{context}: {e}"))
}

fn bad_request(msg: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, msg.into())
}

/// Client-supplied task ids become topic segments (`_task.<id>.created`),
/// so they are restricted to `[A-Za-z0-9_-]`.
fn valid_task_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

const PRIORITIES: &[&str] = &["low", "medium", "high", "critical"];

/// Announce a task lifecycle event on `_task.<id>.<event>` (payload: the
/// task), so boards subscribed to `_task.**` update live.
pub async fn publish_task_event(state: &AppState, task: &Task, event: &str) {
    let msg = Message {
        id: String::new(),
        topic: format!("_task.{}.{event}", task.id),
        from: state.node_id.clone(),
        owner: None,
        timestamp: Utc::now(),
        content_type: "application/json".to_string(),
        body: format!("Task '{}' {event}.", task.title),
        headers: None,
        payload: serde_json::to_value(task).ok(),
        parent_id: None,
    };
    if let Err(e) = crate::handlers::message::handle_publish(Arc::new(state.clone()), msg).await {
        tracing::warn!("Failed to announce task {} {}: {}", task.id, event, e);
    }
}

/// Dependencies must name existing tasks (other than `task_id`) and must
/// not create a cycle. Returns them de-duplicated.
fn check_dependencies(
    task_id: &str,
    deps: &[String],
    all: &[Task],
) -> Result<Vec<String>, ApiError> {
    let mut out: Vec<String> = Vec::new();
    for d in deps.iter().map(|d| d.trim()).filter(|d| !d.is_empty()) {
        if d == task_id {
            return Err(bad_request("A task cannot depend on itself"));
        }
        if !all.iter().any(|t| t.id == d) {
            return Err(bad_request(format!("Unknown dependency '{d}'")));
        }
        if !out.iter().any(|x| x == d) {
            out.push(d.to_string());
        }
    }
    let graph: HashMap<String, Vec<String>> = all
        .iter()
        .map(|t| (t.id.clone(), t.depends_on.clone()))
        .collect();
    if let Some(cycle) = mellowmesh_core::task::dependency_cycle(task_id, &out, &graph) {
        return Err(bad_request(format!(
            "Dependency cycle: {}",
            cycle.join(" -> ")
        )));
    }
    Ok(out)
}

/// A team id must be a slug naming an existing team; empty clears it.
fn check_team(state: &AppState, team: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(team) = team.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    mellowmesh_core::network::validate_slug(team).map_err(bad_request)?;
    if crate::access::snapshot(state).team(team).is_none() {
        return Err(bad_request(format!("Unknown team '{team}'")));
    }
    Ok(Some(team.to_string()))
}

async fn load_tasks(state: &AppState) -> Result<Vec<Task>, ApiError> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.list_tasks())
        .await
        .map_err(|e| internal("Task lookup failed", e))?
        .map_err(|e| internal("Task lookup failed", e))
}

pub async fn create_task(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(mut task): Json<Task>,
) -> Result<impl IntoResponse, ApiError> {
    if task.id.is_empty() {
        task.id = format!("task_{}", Ulid::new().to_string().to_lowercase());
    }
    if !valid_task_id(&task.id) {
        return Err(bad_request(
            "Task ids may only contain letters, digits, '_' and '-' (max 128)",
        ));
    }
    // A newly created task must start in a coordinatable state. Reject an
    // arbitrary status (e.g. a client-supplied "in_progress") that would leave
    // the task neither claimable nor sweepable.
    if !matches!(task.status.as_str(), "open" | "blocked") {
        task.status = "open".to_string();
    }
    if !PRIORITIES.contains(&task.priority.as_str()) {
        task.priority = "medium".to_string();
    }
    // Agents, humans and nodes create tasks as themselves.
    if let Some(p) = &ctx.principal {
        if matches!(p.kind.as_str(), "agent" | "human" | "node") {
            task.created_by = p.id.clone();
        }
    }
    // A new task is unclaimed; lifecycle timestamps are the hub's.
    task.claimed_by = None;
    task.claim_expires_at = None;
    task.created_at = Some(now_rfc3339());
    task.claimed_at = None;
    task.started_at = None;
    task.completed_at = None;
    task.blocked_from = None;
    task.team = check_team(&state, task.team.as_deref())?;
    if !task.depends_on.is_empty() {
        let all = load_tasks(&state).await?;
        task.depends_on = check_dependencies(&task.id, &task.depends_on, &all)?;
    }
    if let Err(e) = state.store.insert_task(&task) {
        return Err(internal("Failed to create task", e));
    }
    publish_task_event(&state, &task, "created").await;
    Ok((StatusCode::OK, Json(task)))
}

/// `GET /tasks`: every task, except those whose topics all lie in spaces
/// the caller may not read.
pub async fn list_tasks(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
) -> Result<impl IntoResponse, ApiError> {
    let tasks = load_tasks(&state).await?;
    let snap = crate::access::snapshot(&state);
    let caller = ctx.caller();
    let visible: Vec<Task> = tasks
        .into_iter()
        .filter(|t| !crate::access::task_hidden(&snap, &caller, &t.id, &t.topics))
        .collect();
    Ok(Json(visible))
}

pub async fn claim_task(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(task_id): Path<String>,
    Json(payload): Json<ClaimPayload>,
) -> Result<impl IntoResponse, ApiError> {
    use mellowmesh_store::task_store::ClaimOutcome;

    // An authenticated non-human principal (agent, node, interface) may only
    // claim as itself — no impersonation. Humans supervise the fleet and may
    // assign work.
    if let Some(p) = &ctx.principal {
        if p.kind != "human" && p.id != payload.claimed_by {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "{} {} cannot claim a task as {}",
                    p.kind, p.id, payload.claimed_by
                ),
            ));
        }
    }
    match state
        .store
        .claim_task(&task_id, &payload.claimed_by, payload.lease_seconds)
    {
        Ok(ClaimOutcome::Claimed { lease_expires_at }) => {
            if let Ok(Some(task)) = state.store.get_task(&task_id) {
                publish_task_event(&state, &task, "claimed").await;
            }
            Ok((
                StatusCode::OK,
                Json(serde_json::json!({ "lease_expires_at": lease_expires_at })),
            ))
        }
        Ok(ClaimOutcome::Conflict { held_by }) => Err((
            StatusCode::CONFLICT,
            format!("Task {task_id} is already claimed by {held_by} and its lease has not expired"),
        )),
        Ok(ClaimOutcome::NotFound) => {
            Err((StatusCode::NOT_FOUND, format!("Task {task_id} not found")))
        }
        Ok(ClaimOutcome::NotClaimable { status }) => Err((
            StatusCode::BAD_REQUEST,
            format!("Task {task_id} is not claimable (status: {status})"),
        )),
        Err(e) => Err(internal("Failed to claim task", e)),
    }
}

pub async fn complete_task(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let task = match state.store.get_task(&task_id) {
        Ok(Some(t)) => t,
        Ok(None) => return Err((StatusCode::NOT_FOUND, format!("Task {task_id} not found"))),
        Err(e) => return Err(internal("Task lookup failed", e)),
    };

    // An authenticated agent may only complete a task it currently holds; it
    // cannot close out another agent's work. Humans/interfaces/owners are not
    // restricted (they supervise the fleet).
    if let Some(p) = &ctx.principal {
        if p.kind == "agent" {
            match &task.claimed_by {
                Some(holder) if holder == &p.id => {}
                _ => {
                    return Err((
                        StatusCode::FORBIDDEN,
                        format!("Agent {} does not hold task {task_id}", p.id),
                    ))
                }
            }
        }
    }

    match state.store.complete_task(&task_id) {
        Ok(true) => {
            if let Ok(Some(task)) = state.store.get_task(&task_id) {
                publish_task_event(&state, &task, "completed").await;
            }
            Ok(StatusCode::OK)
        }
        Ok(false) => Err((
            StatusCode::CONFLICT,
            format!("Task {task_id} is already in a terminal state"),
        )),
        Err(e) => Err(internal("Failed to complete task", e)),
    }
}

/// Lease expiry for a task returning to a claimed state.
fn fresh_lease(task: &Task) -> Option<String> {
    let secs = task
        .lease_seconds
        .unwrap_or(DEFAULT_LEASE_SECONDS)
        .min(mellowmesh_store::task_store::MAX_LEASE_SECONDS) as i64;
    chrono::Duration::try_seconds(secs)
        .and_then(|d| Utc::now().checked_add_signed(d))
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// `PATCH /tasks/:id`: change status (validated transitions), assignee,
/// priority, dependencies, team, title or description. Allowed for the
/// owner, the task's creator, assignee or claimant; token-less local
/// callers in open mode keep today's trust.
pub async fn update_task(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(task_id): Path<String>,
    Json(patch): Json<TaskUpdate>,
) -> Result<impl IntoResponse, ApiError> {
    let all = load_tasks(&state).await?;
    let Some(current) = all.iter().find(|t| t.id == task_id).cloned() else {
        return Err((StatusCode::NOT_FOUND, format!("Task {task_id} not found")));
    };

    let trusted = is_admin(&state, &ctx) || ctx.trusted_local;
    let me = ctx.principal.as_ref().map(|p| p.id.as_str());
    let is = |who: Option<&str>| me.is_some() && me == who;
    let involved = is(Some(current.created_by.as_str()))
        || is(current.assigned_to.as_deref())
        || is(current.claimed_by.as_deref());
    if !trusted && !involved {
        return Err((
            StatusCode::FORBIDDEN,
            format!(
                "Only the owner, or the task's creator, assignee or claimant may update task {task_id}"
            ),
        ));
    }

    let mut task = current.clone();
    let old_status = current.status.clone();
    let mut event = "updated";

    if let Some(to) = patch.status.as_deref().map(str::trim) {
        let rule = mellowmesh_core::task::check_transition(
            &old_status,
            to,
            current.blocked_from.as_deref(),
        )
        .map_err(bad_request)?;
        let allowed = match rule {
            TransitionRule::Anyone => true,
            TransitionRule::Claimant => trusted || is(current.claimed_by.as_deref()),
            TransitionRule::CreatorOrOwner => trusted || is(Some(current.created_by.as_str())),
        };
        if !allowed {
            return Err((
                StatusCode::FORBIDDEN,
                format!("Not allowed to move task {task_id} from '{old_status}' to '{to}'"),
            ));
        }
        if to != old_status {
            let now = now_rfc3339();
            task.status = to.to_string();
            match to {
                // Unblocked: back where it was, with a fresh lease.
                _ if old_status == status::BLOCKED && to != status::CANCELLED => {
                    task.blocked_from = None;
                    if matches!(to, status::CLAIMED | status::IN_PROGRESS) {
                        task.claim_expires_at = fresh_lease(&task);
                    }
                }
                status::IN_PROGRESS => {
                    task.started_at.get_or_insert(now);
                    event = "started";
                }
                status::COMPLETED => {
                    task.completed_at = Some(now);
                    task.claim_expires_at = None;
                    event = "completed";
                }
                status::CANCELLED => {
                    task.claim_expires_at = None;
                    task.blocked_from = None;
                    event = "cancelled";
                }
                status::BLOCKED => {
                    // A blocked task is paused: its lease is not swept.
                    task.blocked_from = Some(old_status.clone());
                    task.claim_expires_at = None;
                }
                _ => {
                    // Reopen (the only other allowed change): back to the
                    // board, unclaimed.
                    task.claimed_by = None;
                    task.claim_expires_at = None;
                    task.claimed_at = None;
                    task.started_at = None;
                    task.completed_at = None;
                }
            }
        }
    }
    if let Some(a) = patch.assigned_to {
        let a = a.trim();
        task.assigned_to = (!a.is_empty()).then(|| a.to_string());
    }
    if let Some(p) = patch.priority {
        let p = p.trim();
        if !PRIORITIES.contains(&p) {
            return Err(bad_request(format!(
                "Unknown priority '{p}' (expected {})",
                PRIORITIES.join(", ")
            )));
        }
        task.priority = p.to_string();
    }
    if let Some(deps) = patch.depends_on {
        task.depends_on = check_dependencies(&task.id, &deps, &all)?;
    }
    if let Some(team) = patch.team {
        task.team = check_team(&state, Some(&team))?;
    }
    if let Some(title) = patch.title {
        if title.trim().is_empty() {
            return Err(bad_request("Title must not be empty"));
        }
        task.title = title;
    }
    if let Some(d) = patch.description {
        task.description = (!d.is_empty()).then_some(d);
    }

    if task == current {
        return Ok(Json(task));
    }
    match state.store.update_task_if_status(&task, &old_status) {
        Ok(true) => {}
        Ok(false) => {
            return Err((
                StatusCode::CONFLICT,
                format!("Task {task_id} changed concurrently; retry"),
            ))
        }
        Err(e) => return Err(internal("Failed to update task", e)),
    }
    publish_task_event(&state, &task, event).await;
    Ok(Json(task))
}

#[derive(Deserialize)]
pub struct GraphParams {
    /// `all` (default), `mine`, `agent:<principal>` or `team:<id>`.
    pub scope: Option<String>,
}

/// `GET /tasks/graph?scope=…`: lanes, tasks with board metrics, and
/// dependency edges, limited to tasks whose topics the caller may read.
pub async fn task_graph(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Query(params): Query<GraphParams>,
) -> Result<impl IntoResponse, ApiError> {
    let scope = params.scope.unwrap_or_else(|| "all".to_string());
    let me = ctx.principal.as_ref().map(|p| p.id.clone());
    let filter: Box<dyn Fn(&Task) -> bool + Send> = match scope.as_str() {
        "all" | "" => Box::new(|_| true),
        "mine" => {
            let Some(me) = me else {
                return Err(bad_request("scope=mine needs a token"));
            };
            Box::new(move |t: &Task| {
                t.assigned_to.as_deref() == Some(&me)
                    || t.claimed_by.as_deref() == Some(&me)
                    || t.created_by == me
            })
        }
        s => {
            if let Some(p) = s.strip_prefix("agent:") {
                let p = p.to_string();
                Box::new(move |t: &Task| {
                    t.assigned_to.as_deref() == Some(&p) || t.claimed_by.as_deref() == Some(&p)
                })
            } else if let Some(team) = s.strip_prefix("team:") {
                let team = team.to_string();
                Box::new(move |t: &Task| t.team.as_deref() == Some(&team))
            } else {
                return Err(bad_request(format!(
                    "Unknown scope '{s}' (expected all, mine, agent:<principal> or team:<id>)"
                )));
            }
        }
    };
    let all = load_tasks(&state).await?;
    let snap = crate::access::snapshot(&state);
    let caller = ctx.caller();
    let visible: Vec<Task> = all
        .iter()
        .filter(|t| crate::access::task_visible(&snap, &caller, &t.id, &t.topics))
        .filter(|t| filter(t))
        .cloned()
        .collect();
    let graph = mellowmesh_core::task::build_graph(&visible, &all, Utc::now(), |id| {
        snap.team(id).map(|t| t.name.clone())
    });
    Ok(Json(graph))
}
