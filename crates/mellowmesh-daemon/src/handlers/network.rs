//! Network access control endpoints: `/whoami`, `/network`, `/spaces`,
//! `/teams`, `/access/check` (contract: `docs/network.md`).

use crate::auth::{is_admin, AuthContext};
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use mellowmesh_core::network::{
    AccessCheck, Caller, NetworkPatch, NetworkView, Space, Team, TeamMember, TeamRole, WhoAmI,
    WriteAccess,
};
use mellowmesh_store::network_store::CreateOutcome;
use serde::Deserialize;

type ApiError = (StatusCode, String);

fn internal(e: anyhow::Error) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad_request(e: String) -> ApiError {
    (StatusCode::BAD_REQUEST, e)
}

fn require_admin(state: &AppState, ctx: &AuthContext) -> Result<(), ApiError> {
    if is_admin(state, ctx) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Only the owner may manage the network, spaces and teams".to_string(),
        ))
    }
}

/// Run a blocking store call on the blocking pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| internal(anyhow::anyhow!(e)))?
        .map_err(internal)
}

/// `GET /whoami`
pub async fn whoami(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
) -> impl IntoResponse {
    let principal = ctx.principal.as_ref();
    Json(WhoAmI {
        principal: principal.map(|p| p.id.clone()),
        kind: principal.map(|p| p.kind.clone()),
        is_owner: principal.is_some_and(|p| p.id == state.owner),
        remote: ctx.remote,
    })
}

/// `GET /network`: the network plus the spaces and teams the caller may see.
pub async fn get_network(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
) -> impl IntoResponse {
    let snap = crate::access::snapshot(&state);
    let admin = is_admin(&state, &ctx);
    let caller = ctx.caller();
    Json(NetworkView {
        network: snap.network.clone(),
        spaces: snap.visible_spaces(&caller, admin),
        teams: snap.visible_teams(&caller, admin),
    })
}

/// `PUT /network` (admin): partial update.
pub async fn put_network(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(patch): Json<NetworkPatch>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&state, &ctx)?;
    let mut network = state.store.get_network().map_err(internal)?;
    patch.apply(&mut network);
    network.validate().map_err(bad_request)?;
    let store = state.store.clone();
    let saved = network.clone();
    blocking(move || store.set_network(&saved)).await?;
    crate::access::refresh(&state);
    Ok(Json(network))
}

/// `GET /spaces`
pub async fn list_spaces(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
) -> impl IntoResponse {
    let snap = crate::access::snapshot(&state);
    Json(snap.visible_spaces(&ctx.caller(), is_admin(&state, &ctx)))
}

/// Validate a space body and check that the teams it names exist.
fn prepare_space(state: &AppState, space: &mut Space) -> Result<(), ApiError> {
    space.normalize().map_err(bad_request)?;
    let snap = crate::access::snapshot(state);
    if let Some(unknown) = space.teams.iter().find(|t| snap.team(t).is_none()) {
        return Err(bad_request(format!("Unknown team '{unknown}'")));
    }
    Ok(())
}

/// `POST /spaces` (admin): create.
pub async fn create_space(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(mut space): Json<Space>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&state, &ctx)?;
    prepare_space(&state, &mut space)?;
    let now = Utc::now();
    space.created_at = now;
    space.updated_at = now;
    let store = state.store.clone();
    let saved = space.clone();
    match blocking(move || store.create_space(&saved)).await? {
        CreateOutcome::Created => {
            crate::access::refresh(&state);
            Ok((StatusCode::CREATED, Json(space)))
        }
        CreateOutcome::Conflict(id) => {
            Err((StatusCode::CONFLICT, format!("Space '{id}' already exists")))
        }
    }
}

/// `PUT /spaces/:id` (admin): replace.
pub async fn replace_space(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(mut space): Json<Space>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&state, &ctx)?;
    if !space.id.is_empty() && space.id != id {
        return Err(bad_request(format!(
            "Body id '{}' does not match the path id '{id}'",
            space.id
        )));
    }
    space.id = id.clone();
    prepare_space(&state, &mut space)?;
    space.updated_at = Utc::now();
    let store = state.store.clone();
    let saved = space.clone();
    let replaced = blocking(move || {
        let ok = store.replace_space(&saved)?;
        Ok(if ok {
            store.get_space(&saved.id)?
        } else {
            None
        })
    })
    .await?;
    match replaced {
        Some(space) => {
            crate::access::refresh(&state);
            Ok(Json(space))
        }
        None => Err((StatusCode::NOT_FOUND, format!("Space '{id}' not found"))),
    }
}

/// `DELETE /spaces/:id` (admin).
pub async fn delete_space(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&state, &ctx)?;
    let store = state.store.clone();
    let key = id.clone();
    if blocking(move || store.delete_space(&key)).await? {
        crate::access::refresh(&state);
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("Space '{id}' not found")))
    }
}

/// `GET /teams`
pub async fn list_teams(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
) -> impl IntoResponse {
    let snap = crate::access::snapshot(&state);
    Json(snap.visible_teams(&ctx.caller(), is_admin(&state, &ctx)))
}

/// `POST /teams` (admin): create the team and its closed team space.
pub async fn create_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(mut team): Json<Team>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&state, &ctx)?;
    team.normalize().map_err(bad_request)?;
    let now = Utc::now();
    team.created_at = now;
    let space = team.default_space(now);
    let store = state.store.clone();
    let saved = team.clone();
    match blocking(move || store.create_team(&saved, &space)).await? {
        CreateOutcome::Created => {
            crate::access::refresh(&state);
            Ok((StatusCode::CREATED, Json(team)))
        }
        CreateOutcome::Conflict(id) => {
            Err((StatusCode::CONFLICT, format!("'{id}' already exists")))
        }
    }
}

/// `PUT /teams/:id` (admin): replace name, description, color, members.
pub async fn replace_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(mut team): Json<Team>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&state, &ctx)?;
    if !team.id.is_empty() && team.id != id {
        return Err(bad_request(format!(
            "Body id '{}' does not match the path id '{id}'",
            team.id
        )));
    }
    team.id = id.clone();
    team.normalize().map_err(bad_request)?;
    let store = state.store.clone();
    let saved = team.clone();
    let replaced = blocking(move || {
        let ok = store.replace_team(&saved)?;
        Ok(if ok { store.get_team(&saved.id)? } else { None })
    })
    .await?;
    match replaced {
        Some(team) => {
            crate::access::refresh(&state);
            Ok(Json(team))
        }
        None => Err((StatusCode::NOT_FOUND, format!("Team '{id}' not found"))),
    }
}

/// `DELETE /teams/:id` (admin): also removes the team space.
pub async fn delete_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&state, &ctx)?;
    let store = state.store.clone();
    let key = id.clone();
    let space_id = mellowmesh_core::network::team_space_id(&id);
    if blocking(move || store.delete_team(&key, &space_id)).await? {
        crate::access::refresh(&state);
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("Team '{id}' not found")))
    }
}

#[derive(Deserialize)]
pub struct MemberPayload {
    pub principal: String,
    #[serde(default)]
    pub role: TeamRole,
}

/// `POST /teams/:id/members` (admin): add or update a member; returns the
/// team.
pub async fn add_team_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(payload): Json<MemberPayload>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&state, &ctx)?;
    // Validate through the team rules (principal URI shape).
    let mut probe = Team {
        id: id.clone(),
        members: vec![TeamMember {
            principal: payload.principal,
            role: payload.role,
        }],
        ..Default::default()
    };
    probe.normalize().map_err(bad_request)?;
    let member = probe.members.remove(0);
    let store = state.store.clone();
    let key = id.clone();
    let team = blocking(move || {
        Ok(if store.upsert_team_member(&key, &member)? {
            store.get_team(&key)?
        } else {
            None
        })
    })
    .await?;
    match team {
        Some(team) => {
            crate::access::refresh(&state);
            Ok(Json(team))
        }
        None => Err((StatusCode::NOT_FOUND, format!("Team '{id}' not found"))),
    }
}

/// `DELETE /teams/:id/members/:principal` (admin).
pub async fn remove_team_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, principal)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&state, &ctx)?;
    let store = state.store.clone();
    let (key, who) = (id.clone(), principal.clone());
    if blocking(move || store.remove_team_member(&key, &who)).await? {
        crate::access::refresh(&state);
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            format!("'{principal}' is not a member of team '{id}'"),
        ))
    }
}

#[derive(Deserialize)]
pub struct AccessCheckParams {
    pub topic: String,
    /// Principal URI; omitted means a visitor.
    pub principal: Option<String>,
}

/// `GET /access/check?topic=T&principal=P` (admin): what `P` (or a visitor)
/// may do on `T`. For a principal the union of its active tokens' scopes is
/// used; a principal without tokens is assumed to have full scopes.
pub async fn access_check(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Query(params): Query<AccessCheckParams>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&state, &ctx)?;
    mellowmesh_core::topic::Topic::new(params.topic.as_str())
        .map_err(|e| bad_request(format!("Invalid topic: {e}")))?;
    let snap = crate::access::snapshot(&state);
    let principal = params
        .principal
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    let (decision, note) = match principal {
        None => (snap.evaluate(&Caller::visitor(), &params.topic), None),
        Some(p) => {
            let store = state.store.clone();
            let id = p.clone();
            let tokens = blocking(move || store.list_tokens()).await?;
            let mut read = Vec::new();
            let mut write = Vec::new();
            for t in tokens
                .into_iter()
                .filter(|t| t.principal == id && !t.revoked)
            {
                read.extend(t.read_scopes);
                write.extend(t.write_scopes);
            }
            let note = (read.is_empty() && write.is_empty())
                .then_some(" (no active token: full scopes assumed)");
            if note.is_some() {
                read = vec!["**".to_string()];
                write = vec!["**".to_string()];
            }
            let kind = mellowmesh_core::auth::kind_of_uri(&p);
            let caller = Caller::token(&p, kind, &read, &write);
            (snap.evaluate(&caller, &params.topic), note)
        }
    };
    let mut reason = decision.reason.clone();
    if decision.write == WriteAccess::Visitor {
        reason.push_str(" (writes are recorded as a visitor)");
    }
    if let Some(n) = note {
        reason.push_str(n);
    }
    Ok(Json(AccessCheck {
        read: decision.read,
        write: decision.write != WriteAccess::Denied,
        space: decision.space,
        level: decision.level,
        reason,
    }))
}
