use crate::cache::Caches;
use crate::metrics::DaemonMetrics;
use crate::pipeline::PersistencePipeline;
use crate::subscription::SubscriptionRegistry;
use crate::trace_mgr::TraceSessionManager;
use mellowmesh_core::persistence::PersistenceConfig;
use mellowmesh_store::Store;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub registry: SubscriptionRegistry,
    pub metrics: Arc<DaemonMetrics>,
    pub pipeline: Arc<PersistencePipeline>,
    pub trace_mgr: Arc<TraceSessionManager>,
    pub policy_config: Arc<PersistenceConfig>,
    pub wikis: Arc<std::collections::HashMap<String, std::path::PathBuf>>,
    pub node_id: String,
    pub shutdown_trigger: Arc<tokio::sync::Notify>,
    /// When true, every request (except health/dashboard) must present a
    /// valid bearer token.
    pub require_auth: bool,
    /// Owner principal URI (`human://...`) — the only identity allowed to
    /// administer tokens.
    pub owner: String,
    /// Port this daemon is bound to (loopback base for internal dispatch).
    pub port: u16,
    /// Shared HTTP client for loopback dispatch (`/e2e/request`), so every
    /// request reuses one connection pool.
    pub http: reqwest::Client,
    /// Hot-path caches (tokens, agents, named topics, compiled schemas).
    pub caches: Arc<Caches>,
}

/// The HTTP client used for the daemon's loopback dispatch: never follows
/// redirects, so a dispatched request cannot be bounced to another host.
pub fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}
