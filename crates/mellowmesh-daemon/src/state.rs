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
}
