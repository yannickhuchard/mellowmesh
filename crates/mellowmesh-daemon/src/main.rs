use clap::Parser;
use mellowmesh_daemon::server::create_router;
use mellowmesh_daemon::state::AppState;
use mellowmesh_daemon::subscription::SubscriptionRegistry;
use mellowmesh_store::{sqlite::default_db_path, Store};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(
    name = "mellowmeshd",
    version,
    about = "MellowMesh Intelligent Coordination Daemon"
)]
struct Args {
    #[arg(short, long, default_value_t = 40000)]
    port: u16,

    #[arg(short, long)]
    db: Option<PathBuf>,

    #[arg(long)]
    peer: Vec<String>,

    #[arg(long)]
    node_id: Option<String>,

    /// Require a valid bearer token on every request (except health and
    /// dashboard). Also enabled by MELLOWMESH_REQUIRE_AUTH=1.
    #[arg(long)]
    require_auth: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,mellowmeshd=debug,mellowmesh_daemon=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    let db_path = args.db.unwrap_or_else(default_db_path);
    tracing::info!("Initializing SQLite storage at: {:?}", db_path);
    let store = Store::new(&db_path)?;

    // First-run identity bootstrap: owner principal + full-access token.
    let owner = mellowmesh_daemon::auth::bootstrap_owner(&store, &db_path);
    let relay_config = mellowmesh_daemon::relay_link::load_config(&store);
    let mut require_auth = args.require_auth
        || matches!(
            std::env::var("MELLOWMESH_REQUIRE_AUTH")
                .unwrap_or_default()
                .as_str(),
            "1" | "true" | "yes"
        );
    // A relayed hub is reachable from outside this machine: token auth is
    // not optional in that configuration, so it is forced on.
    if relay_config.is_some() && !require_auth {
        tracing::warn!(
            "Relay link is configured — forcing --require-auth so remote requests must present tokens"
        );
        require_auth = true;
    }
    if require_auth {
        tracing::info!("Authentication REQUIRED: all requests must present a bearer token");
    } else {
        tracing::info!(
            "Running in open mode: localhost clients are trusted (enable --require-auth to enforce tokens)"
        );
    }

    let metrics = Arc::new(mellowmesh_daemon::metrics::DaemonMetrics::default());
    let pipeline = Arc::new(mellowmesh_daemon::pipeline::PersistencePipeline::new(
        store.clone(),
        metrics.clone(),
    ));
    pipeline.start();

    let trace_mgr = Arc::new(mellowmesh_daemon::trace_mgr::TraceSessionManager::new(
        store.clone(),
        metrics.clone(),
    ));
    let registry = SubscriptionRegistry::new(metrics.clone());

    use mellowmesh_core::persistence::{PersistenceConfig, PersistenceMode, PersistencePolicy};
    let default_policy = PersistencePolicy {
        mode: PersistenceMode::Metadata,
        retention: "7d".to_string(),
        max_message_size: None,
        sync: false,
    };
    let rules = vec![
        (
            "_system.presence.**".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::Ephemeral,
                retention: "5m".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
        (
            "_agent.*.heartbeat".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::Ephemeral,
                retention: "1m".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
        (
            "_agent.*.status".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::Metadata,
                retention: "24h".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
        (
            "_agent.**.inbox".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::Queryable,
                retention: "30d".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
        (
            "_agent.*.stream.**".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::Ephemeral,
                retention: "30s".to_string(),
                max_message_size: Some("4KB".to_string()),
                sync: false,
            },
        ),
        (
            "_agent.*.scratch.**".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::Ephemeral,
                retention: "10m".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
        (
            "_task.**".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::EventLog,
                retention: "90d".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
        (
            "_flow.**".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::EventLog,
                retention: "180d".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
        (
            "_decision.**".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::Queryable,
                retention: "forever".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
        (
            "_artifact.**".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::Metadata,
                retention: "policy".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
        (
            "_project.*.architecture.**".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::Queryable,
                retention: "365d".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
        (
            "_forum.**".to_string(),
            PersistencePolicy {
                mode: PersistenceMode::Queryable,
                retention: "180d".to_string(),
                max_message_size: None,
                sync: false,
            },
        ),
    ];
    let policy_config = Arc::new(PersistenceConfig {
        default: default_policy,
        rules,
    });

    let node_id = args.node_id.unwrap_or_else(get_default_node_id);
    tracing::info!("Starting MellowMesh node with ID: {}", node_id);

    let peers_config: Vec<mellowmesh_daemon::peer::PeerConfig> = args
        .peer
        .into_iter()
        .map(|s| {
            if let Some((addr, pattern)) = s.split_once('=') {
                mellowmesh_daemon::peer::PeerConfig {
                    addr: addr.to_string(),
                    pattern: pattern.to_string(),
                }
            } else {
                mellowmesh_daemon::peer::PeerConfig {
                    addr: s,
                    pattern: "**".to_string(),
                }
            }
        })
        .collect();

    let wikis = Arc::new(mellowmesh_daemon::wiki_sync::get_configured_wikis());

    // Run initial sync on startup asynchronously
    let store_clone = store.clone();
    let wikis_clone = wikis.clone();
    tokio::spawn(async move {
        if let Err(e) =
            mellowmesh_daemon::wiki_sync::sync_all_wikis(&store_clone, &wikis_clone).await
        {
            tracing::error!("Initial wiki sync failed: {}", e);
        }
    });

    let shutdown_trigger = Arc::new(tokio::sync::Notify::new());
    let state = AppState {
        store,
        registry,
        metrics,
        pipeline,
        trace_mgr,
        policy_config,
        wikis: wikis.clone(),
        node_id,
        shutdown_trigger: shutdown_trigger.clone(),
        require_auth,
        owner,
        port: args.port,
    };

    // Background maintenance: lease reclaim + retention purge
    mellowmesh_daemon::sweeper::start(state.clone());

    // Outbound relay link (Phase 2 reach layer)
    if let Some(config) = relay_config {
        mellowmesh_daemon::relay_link::start(state.clone(), config, args.port);
    }

    let peer_manager = Arc::new(mellowmesh_daemon::peer::PeerManager::new(
        state.node_id.clone(),
        peers_config,
    ));
    peer_manager.set_state(state.clone()).await;
    peer_manager.start();

    let app = create_router(state.clone());

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    tracing::info!("MellowMesh daemon listening on: {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Initialize and start connectors. Under required auth they get their
    // own interface token (rotated each boot) so they can publish, subscribe,
    // and relay human decision responses.
    let mut client = mellowmesh_client::MellowMeshClient::new(args.port);
    if require_auth {
        use mellowmesh_core::auth::{generate_token, hash_token, Principal, TokenRecord};
        let principal_id = "interface://local/connectors".to_string();
        if let Ok(Some(old_id)) = state.store.get_config("connectors_token_id") {
            let _ = state.store.revoke_token(&old_id);
        }
        let plaintext = generate_token();
        let record = TokenRecord {
            id: format!("tok_{}", ulid::Ulid::new().to_string().to_lowercase()),
            principal: principal_id.clone(),
            token_hash: hash_token(&plaintext),
            read_scopes: vec!["**".to_string()],
            write_scopes: vec!["**".to_string()],
            created_at: chrono::Utc::now(),
            revoked: false,
        };
        let minted = state
            .store
            .upsert_principal(&Principal {
                id: principal_id,
                kind: "interface".to_string(),
                display_name: Some("Local interface connectors".to_string()),
                created_at: chrono::Utc::now(),
            })
            .and_then(|_| state.store.insert_token(&record))
            .and_then(|_| state.store.set_config("connectors_token_id", &record.id));
        match minted {
            Ok(_) => client = client.with_token(plaintext),
            Err(e) => tracing::error!("Failed to mint connectors token: {}", e),
        }
    }
    let connectors_mgr = mellowmesh_connectors::ConnectorsManager::new(client);
    connectors_mgr.start();

    let shutdown_trigger_clone = shutdown_trigger.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_trigger_clone.notified().await;
            tracing::info!("Received shutdown signal. Stopping daemon...");
        })
        .await?;

    // Allow a tiny window for any final writes or logs to finish
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(())
}

fn get_default_node_id() -> String {
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string());
    format!("node://{}", hostname.to_lowercase())
}
