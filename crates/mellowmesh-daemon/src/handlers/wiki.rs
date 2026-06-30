use crate::state::AppState;
use crate::wiki_sync;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
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

pub async fn list_or_search_pages(
    State(state): State<AppState>,
    Path(wiki): Path<String>,
    Query(params): Query<SearchWikiParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
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
    Path((wiki, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
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
            format!("Page '{}' not found in wiki '{}'", path, wiki),
        )),
    }
}

pub async fn write_page(
    State(state): State<AppState>,
    Path((wiki, path)): Path<(String, String)>,
    Json(payload): Json<WritePagePayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // 1. Resolve path on disk
    let wiki_root = state.wikis.get(&wiki).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("Wiki namespace '{}' is not configured", wiki),
        )
    })?;

    // Validate path (prevent directory traversal)
    if path.contains("..") || path.starts_with('/') {
        return Err((StatusCode::BAD_REQUEST, "Invalid file path".to_string()));
    }

    let file_path = wiki_root.join(&path);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create directories: {}", e),
            )
        })?;
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

    let doc = okf::parse_okf_string(&wiki, &path, &temp_content).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid OKF format: {}", e),
        )
    })?;

    let doc_serialized = okf::serialize_okf(&doc);

    std::fs::write(&file_path, &doc_serialized).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write file: {}", e),
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
    let event_topic = format!("_wiki.{}.page.{}", wiki, event_type);
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
        body: format!(
            "Wiki page '{}' was {} in wiki '{}'.",
            path, event_type, wiki
        ),
        headers: None,
        payload: Some(serde_json::to_value(payload_map).unwrap()),
        parent_id: None,
    };

    let _ = crate::handlers::message::handle_publish(std::sync::Arc::new(state.clone()), msg)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to publish event: {}", e),
            )
        })?;

    Ok(Json(doc))
}

pub async fn delete_page(
    State(state): State<AppState>,
    Path((wiki, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let wiki_root = state.wikis.get(&wiki).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("Wiki namespace '{}' is not configured", wiki),
        )
    })?;

    // Validate path
    if path.contains("..") || path.starts_with('/') {
        return Err((StatusCode::BAD_REQUEST, "Invalid file path".to_string()));
    }

    let file_path = wiki_root.join(&path);

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
        None => return Err((StatusCode::NOT_FOUND, format!("Page '{}' not found", path))),
    };

    // Delete file if exists
    if file_path.exists() {
        std::fs::remove_file(file_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete file: {}", e),
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
    let event_topic = format!("_wiki.{}.page.deleted", wiki);
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
        body: format!("Wiki page '{}' was deleted from wiki '{}'.", path, wiki),
        headers: None,
        payload: Some(serde_json::to_value(payload_map).unwrap()),
        parent_id: None,
    };

    let _ = crate::handlers::message::handle_publish(std::sync::Arc::new(state.clone()), msg)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to publish event: {}", e),
            )
        })?;

    Ok(StatusCode::OK)
}

pub async fn sync_wiki_endpoint(
    State(state): State<AppState>,
    Path(wiki): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let wiki_root = state.wikis.get(&wiki).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("Wiki namespace '{}' is not configured", wiki),
        )
    })?;

    wiki_sync::sync_wiki(&state.store, &wiki, wiki_root)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Sync failed: {}", e),
            )
        })?;

    Ok(StatusCode::OK)
}

pub async fn get_wiki_graph(
    State(state): State<AppState>,
    Path(wiki): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
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
