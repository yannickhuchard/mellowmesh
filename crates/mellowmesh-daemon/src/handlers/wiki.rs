use crate::auth::AuthContext;
use crate::state::AppState;
use crate::wiki_sync;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use mellowmesh_core::message::Message;
use mellowmesh_core::okf::{self, OKFDocument};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct SearchWikiParams {
    pub query: Option<String>,
    pub doc_type: Option<String>,
    pub tag: Option<String>,
}

#[derive(Deserialize)]
pub struct WritePagePayload {
    pub doc_type: String,
    pub title: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub resource: Option<String>,
    pub body: String,
}

#[derive(Serialize)]
pub struct WikiGraphResult {
    pub nodes: Vec<WikiNodeInfo>,
    pub links: Vec<WikiLinkInfo>,
}

#[derive(Serialize)]
pub struct WikiNodeInfo {
    pub path: String,
    pub title: String,
    pub doc_type: String,
}

#[derive(Serialize)]
pub struct WikiLinkInfo {
    pub source: String,
    pub target: String,
}

type ApiError = (StatusCode, String);

/// The scope topic that governs access to a wiki's pages.
fn wiki_scope_topic(wiki: &str) -> String {
    format!("_wiki.{wiki}.page")
}

/// Wiki names become topic segments; restrict them to `[A-Za-z0-9_-]`.
fn check_wiki_name(wiki: &str) -> Result<(), ApiError> {
    let ok = !wiki.is_empty()
        && wiki.len() <= 128
        && wiki
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err((StatusCode::BAD_REQUEST, "Invalid wiki name".to_string()))
    }
}

fn require_read(state: &AppState, ctx: &AuthContext, wiki: &str) -> Result<(), ApiError> {
    check_wiki_name(wiki)?;
    if ctx.can_read(state, &wiki_scope_topic(wiki)) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            format!("Not authorized to read wiki '{wiki}'"),
        ))
    }
}

fn require_write(state: &AppState, ctx: &AuthContext, wiki: &str) -> Result<(), ApiError> {
    check_wiki_name(wiki)?;
    if ctx.can_write(state, &wiki_scope_topic(wiki)) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            format!("Not authorized to write wiki '{wiki}'"),
        ))
    }
}

/// Validate a wiki page path: a relative path made only of normal
/// components (no `..`, no root, no drive or UNC prefix, no backslashes or
/// colons, which are separators/prefixes on Windows). Page writes must also
/// name a `.md` file.
pub fn validate_page_path(path: &str, require_md: bool) -> Result<(), ApiError> {
    let bad = || (StatusCode::BAD_REQUEST, "Invalid file path".to_string());
    if path.is_empty() || path.contains('\\') || path.contains(':') || path.contains('\0') {
        return Err(bad());
    }
    let p = std::path::Path::new(path);
    if !p
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err(bad());
    }
    if require_md
        && !p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Wiki pages must be .md files".to_string(),
        ));
    }
    Ok(())
}

fn io_error(context: &str, e: std::io::Error) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, format!("{context}: {e}"))
}

/// Verify that `dir` (an existing directory) resolves inside `root`, after
/// following symlinks.
fn ensure_under_root(root: &std::path::Path, dir: &std::path::Path) -> Result<(), ApiError> {
    let root = root
        .canonicalize()
        .map_err(|e| io_error("Wiki root unavailable", e))?;
    let dir = dir
        .canonicalize()
        .map_err(|e| io_error("Path unavailable", e))?;
    if dir.starts_with(&root) {
        Ok(())
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            "Path escapes the wiki root".to_string(),
        ))
    }
}

pub async fn list_or_search_pages(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(wiki): Path<String>,
    Query(params): Query<SearchWikiParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_read(&state, &ctx, &wiki)?;
    let query_str = params.query.unwrap_or_default();
    let doc_type = params.doc_type.as_deref();
    let tag = params.tag.as_deref();

    let store = state.store.clone();
    let wiki_clone = wiki.clone();
    let query_clone = query_str.clone();
    let dt_clone = doc_type.map(|s| s.to_string());
    let tag_clone = tag.map(|s| s.to_string());

    let docs = tokio::task::spawn_blocking(move || {
        store.search_wiki(
            &wiki_clone,
            &query_clone,
            dt_clone.as_deref(),
            tag_clone.as_deref(),
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(docs))
}

pub async fn get_page(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path((wiki, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_read(&state, &ctx, &wiki)?;
    validate_page_path(&path, false)?;
    let store = state.store.clone();
    let wiki_clone = wiki.clone();
    let path_clone = path.clone();

    let doc_opt =
        tokio::task::spawn_blocking(move || store.get_wiki_page(&wiki_clone, &path_clone))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match doc_opt {
        Some(doc) => Ok(Json(doc)),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("Page '{path}' not found in wiki '{wiki}'"),
        )),
    }
}

pub async fn write_page(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path((wiki, path)): Path<(String, String)>,
    Json(payload): Json<WritePagePayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_write(&state, &ctx, &wiki)?;
    // 1. Resolve path on disk
    let wiki_root = state.wikis.get(&wiki).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("Wiki namespace '{wiki}' is not configured"),
        )
    })?;

    // Validate path (prevent directory traversal and non-page writes)
    validate_page_path(&path, true)?;

    let file_path = wiki_root.join(&path);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_error("Failed to create directories", e))?;
        // Symlinks inside the wiki must not lead the write outside it.
        ensure_under_root(wiki_root, parent)?;
    }
    if file_path
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Refusing to write through a symlink".to_string(),
        ));
    }

    // 2. Parse/Serialize and Write to Disk
    let temp_content = okf::serialize_okf(&OKFDocument {
        wiki: wiki.clone(),
        path: path.clone(),
        doc_type: payload.doc_type.clone(),
        title: payload.title.clone(),
        description: payload.description.clone(),
        tags: payload.tags.clone(),
        timestamp: Utc::now(),
        resource: payload.resource.clone(),
        body: payload.body.clone(),
        links: Vec::new(),
    });

    let doc = okf::parse_okf_string(&wiki, &path, &temp_content)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid OKF format: {e}")))?;

    let doc_serialized = okf::serialize_okf(&doc);

    std::fs::write(&file_path, &doc_serialized).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write file: {e}"),
        )
    })?;

    // Check if page already exists to decide event type
    let store_clone = state.store.clone();
    let wiki_clone = wiki.clone();
    let path_clone = path.clone();
    let exists = tokio::task::spawn_blocking(move || {
        store_clone
            .get_wiki_page(&wiki_clone, &path_clone)
            .map(|opt| opt.is_some())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let event_type = if exists { "updated" } else { "created" };

    // 3. Save to database
    let store_clone = state.store.clone();
    let doc_clone = doc.clone();
    tokio::task::spawn_blocking(move || store_clone.save_wiki_page(&doc_clone))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 4. Publish Event message to Mellowmesh Topic
    let event_topic = format!("_wiki.{wiki}.page.{event_type}");
    let mut payload_map = HashMap::new();
    payload_map.insert("wiki", serde_json::json!(wiki));
    payload_map.insert("path", serde_json::json!(path));
    payload_map.insert("title", serde_json::json!(doc.title));
    payload_map.insert("doc_type", serde_json::json!(doc.doc_type));

    let msg = Message {
        id: String::new(),
        topic: event_topic.clone(),
        from: "system://daemon".to_string(),
        owner: None,
        timestamp: Utc::now(),
        content_type: "application/json".to_string(),
        body: format!("Wiki page '{path}' was {event_type} in wiki '{wiki}'."),
        headers: None,
        payload: Some(serde_json::to_value(payload_map).unwrap()),
        parent_id: None,
    };

    let _ = crate::handlers::message::handle_publish(std::sync::Arc::new(state.clone()), msg)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to publish event: {e}"),
            )
        })?;

    Ok(Json(doc))
}

pub async fn delete_page(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path((wiki, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_write(&state, &ctx, &wiki)?;
    let wiki_root = state.wikis.get(&wiki).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("Wiki namespace '{wiki}' is not configured"),
        )
    })?;

    // Validate path
    validate_page_path(&path, true)?;

    let file_path = wiki_root.join(&path);
    if let Some(parent) = file_path.parent() {
        if parent.exists() {
            ensure_under_root(wiki_root, parent)?;
        }
    }

    // Fetch page details first for event payload
    let store_clone = state.store.clone();
    let wiki_clone = wiki.clone();
    let path_clone = path.clone();
    let doc_opt =
        tokio::task::spawn_blocking(move || store_clone.get_wiki_page(&wiki_clone, &path_clone))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let doc = match doc_opt {
        Some(d) => d,
        None => return Err((StatusCode::NOT_FOUND, format!("Page '{path}' not found"))),
    };

    // Delete file if exists
    if file_path.exists() {
        std::fs::remove_file(file_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete file: {e}"),
            )
        })?;
    }

    // Delete from DB
    let store_clone = state.store.clone();
    let wiki_clone = wiki.clone();
    let path_clone = path.clone();
    tokio::task::spawn_blocking(move || store_clone.delete_wiki_page(&wiki_clone, &path_clone))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Publish event
    let event_topic = format!("_wiki.{wiki}.page.deleted");
    let mut payload_map = HashMap::new();
    payload_map.insert("wiki", serde_json::json!(wiki));
    payload_map.insert("path", serde_json::json!(path));
    payload_map.insert("title", serde_json::json!(doc.title));
    payload_map.insert("doc_type", serde_json::json!(doc.doc_type));

    let msg = Message {
        id: String::new(),
        topic: event_topic,
        from: "system://daemon".to_string(),
        owner: None,
        timestamp: Utc::now(),
        content_type: "application/json".to_string(),
        body: format!("Wiki page '{path}' was deleted from wiki '{wiki}'."),
        headers: None,
        payload: Some(serde_json::to_value(payload_map).unwrap()),
        parent_id: None,
    };

    let _ = crate::handlers::message::handle_publish(std::sync::Arc::new(state.clone()), msg)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to publish event: {e}"),
            )
        })?;

    Ok(StatusCode::OK)
}

pub async fn sync_wiki_endpoint(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(wiki): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_write(&state, &ctx, &wiki)?;
    let wiki_root = state.wikis.get(&wiki).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("Wiki namespace '{wiki}' is not configured"),
        )
    })?;

    wiki_sync::sync_wiki(&state.store, &wiki, wiki_root)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Sync failed: {e}"),
            )
        })?;

    Ok(StatusCode::OK)
}

pub async fn get_wiki_graph(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(wiki): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_read(&state, &ctx, &wiki)?;
    let store = state.store.clone();
    let wiki_clone = wiki.clone();

    let docs = tokio::task::spawn_blocking(move || store.list_wiki_pages(&wiki_clone))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut nodes = Vec::new();
    let mut links = Vec::new();

    for doc in docs {
        nodes.push(WikiNodeInfo {
            path: doc.path.clone(),
            title: doc.title,
            doc_type: doc.doc_type,
        });

        for target in doc.links {
            links.push(WikiLinkInfo {
                source: doc.path.clone(),
                target,
            });
        }
    }

    Ok(Json(WikiGraphResult { nodes, links }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_paths() {
        for ok in ["page.md", "notes/page.md", "a/b/c.MD"] {
            assert!(validate_page_path(ok, true).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "../x.md",
            "a/../../x.md",
            "/etc/x.md",
            "C:/x.md",
            "C:\\x.md",
            "\\\\server\\share\\x.md",
            "a\\..\\x.md",
            "./x.md",
            "x.txt",
            "x",
            "dir/",
        ] {
            assert!(validate_page_path(bad, true).is_err(), "{bad:?}");
        }
        // Reads do not require the .md extension but still reject traversal.
        assert!(validate_page_path("x.txt", false).is_ok());
        assert!(validate_page_path("../x", false).is_err());
    }

    #[test]
    fn wiki_names() {
        assert!(check_wiki_name("team-docs_1").is_ok());
        assert!(check_wiki_name("a.b").is_err());
        assert!(check_wiki_name("**").is_err());
        assert!(check_wiki_name("").is_err());
    }
}
