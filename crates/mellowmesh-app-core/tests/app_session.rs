//! End-to-end test of the app core against a REAL in-process daemon (Axum
//! router + persistence pipeline + sweeper), driven exactly the way a shell
//! drives it: blocking `MeshSession` calls from a plain thread, live events
//! through a foreign-style listener.

use std::net::SocketAddr;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mellowmesh_app_core::{
    ConversationKind, MeshConfig, MeshError, MeshEvent, MeshEventListener, MeshSession, Presence,
};
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::decision::{Decision, DecisionOption, DecisionOutcome};
use mellowmesh_core::message::Message;
use mellowmesh_core::persistence::{PersistenceConfig, PersistenceMode, PersistencePolicy};
use mellowmesh_core::task::Task;
use mellowmesh_daemon::metrics::DaemonMetrics;
use mellowmesh_daemon::pipeline::PersistencePipeline;
use mellowmesh_daemon::server::create_router;
use mellowmesh_daemon::state::AppState;
use mellowmesh_daemon::subscription::SubscriptionRegistry;
use mellowmesh_daemon::trace_mgr::TraceSessionManager;
use mellowmesh_store::Store;

const ME: &str = "human://test";

/// Serve a real daemon (open mode) on `port` from its own runtime thread,
/// so the test thread stays free of any Tokio context — like a shell.
fn spawn_daemon(port: u16) {
    std::env::set_var("MELLOWMESH_SWEEP_INTERVAL_SECS", "1");
    std::env::set_var("MELLOWMESH_NOTIFICATIONS", "off");
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let store = Store::new_in_memory().unwrap();
            let metrics = Arc::new(DaemonMetrics::default());
            let pipeline = Arc::new(PersistencePipeline::new(store.clone(), metrics.clone()));
            pipeline.start();
            let trace_mgr = Arc::new(TraceSessionManager::new(store.clone(), metrics.clone()));
            let registry = SubscriptionRegistry::new(metrics.clone());
            let policy_config = Arc::new(PersistenceConfig {
                default: PersistencePolicy {
                    mode: PersistenceMode::Queryable,
                    retention: "7d".to_string(),
                    max_message_size: None,
                    sync: false,
                },
                rules: vec![],
            });
            let state = AppState {
                store: store.clone(),
                registry,
                metrics,
                pipeline,
                trace_mgr,
                policy_config,
                wikis: Arc::new(std::collections::HashMap::new()),
                node_id: format!("test-node-{port}"),
                shutdown_trigger: Arc::new(tokio::sync::Notify::new()),
                require_auth: false,
                owner: ME.to_string(),
                port,
                http: mellowmesh_daemon::state::loopback_http_client(),
                caches: Arc::new(mellowmesh_daemon::cache::Caches::default()),
            };
            mellowmesh_daemon::sweeper::start(state.clone());
            let app = create_router(state);
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            ready_tx.send(()).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
}

/// Seed the hub through the regular Rust SDK, as agents would.
fn seed(port: u16) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let c = MellowMeshClient::loopback(port);
        for (id, name, owner) in [
            ("agent://test/alpha", "Alpha", ME),
            ("agent://other/beta", "Beta", "human://other"),
        ] {
            c.register_agent(&AgentRegistration {
                id: id.into(),
                name: name.into(),
                owner: owner.into(),
                mode: "autonomous".into(),
                capabilities: vec!["code".into()],
            })
            .await
            .unwrap();
        }
        c.create_task(&Task {
            id: "task_app_1".into(),
            title: "Write release notes".into(),
            created_by: ME.into(),
            status: "open".into(),
            priority: "high".into(),
            ..Default::default()
        })
        .await
        .unwrap();
        c.claim_task("task_app_1", "agent://test/alpha").await.unwrap();
        c.create_decision(&Decision {
            id: "decision_app_1".into(),
            title: "Publish release?".into(),
            question: "Publish v0.2 to crates.io?".into(),
            created_by: "agent://test/alpha".into(),
            required_decider: ME.into(),
            status: "requested".into(),
            options: vec![
                DecisionOption {
                    id: "approve".into(),
                    label: "Publish".into(),
                    pros: vec![],
                    cons: vec![],
                    outcome: Some(DecisionOutcome::Approve),
                },
                DecisionOption {
                    id: "reject".into(),
                    label: "Hold".into(),
                    pros: vec![],
                    cons: vec![],
                    outcome: Some(DecisionOutcome::Reject),
                },
            ],
            response_option_id: None,
            response_timestamp: None,
            responded_by: None,
        })
        .await
        .unwrap();
        // The agent answers in its outbox with a MeshUI card.
        c.publish(&Message {
            id: String::new(),
            topic: "_agent.test.alpha.outbox".into(),
            from: "agent://test/alpha".into(),
            owner: None,
            timestamp: chrono::Utc::now(),
            content_type: "text/plain".into(),
            body: "Release notes drafted".into(),
            headers: None,
            payload: Some(serde_json::json!({
                "mesh_ui": {
                    "version": 1,
                    "title": "Draft",
                    "blocks": [
                        {"type": "progress", "label": "Done", "value": 0.5},
                        {"type": "actions", "actions": [{"id": "ship", "label": "Ship it", "style": "primary"}]}
                    ]
                }
            })),
            parent_id: None,
        })
        .await
        .unwrap();
    });
}

struct Collector(Mutex<mpsc::Sender<MeshEvent>>);

impl MeshEventListener for Collector {
    fn on_event(&self, event: MeshEvent) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

fn wait_for(
    rx: &mpsc::Receiver<MeshEvent>,
    what: &str,
    pred: impl Fn(&MeshEvent) -> bool,
) -> MeshEvent {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(ev) if pred(&ev) => return ev,
            Ok(_) => continue,
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

fn eventually<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn config(port: u16) -> MeshConfig {
    MeshConfig {
        base_url: format!("http://127.0.0.1:{port}"),
        token: None,
        e2e: None,
        device_name: "test-phone".into(),
        principal: Some(ME.into()),
    }
}

#[test]
fn app_core_against_real_daemon() {
    let port = 41201;
    spawn_daemon(port);
    seed(port);

    let session = MeshSession::connect(config(port)).unwrap();
    assert_eq!(session.me().id, ME);
    assert!(!session.e2e_enabled());

    // --- home snapshot -------------------------------------------------
    let home = eventually("home with seeded history", || {
        let h = session.home().unwrap();
        h.conversations
            .iter()
            .any(|c| c.id == "_agent.test.alpha" && c.last_activity_ms > 0)
            .then_some(h)
    });
    assert_eq!(home.agents.len(), 2);
    let alpha = &home.agents[0];
    assert_eq!(alpha.id, "agent://test/alpha");
    assert!(alpha.is_mine);
    assert_eq!(alpha.display_name, "Alpha");
    assert_eq!(alpha.presence, Presence::Busy);
    assert_eq!(
        alpha.current_task_title.as_deref(),
        Some("Write release notes")
    );
    assert!(!home.agents[1].is_mine);

    assert!(home
        .active_tasks
        .iter()
        .any(|t| t.id == "task_app_1" && t.claimed_by.as_deref() == Some("agent://test/alpha")));
    let decision = home
        .pending_decisions
        .iter()
        .find(|d| d.id == "decision_app_1")
        .expect("pending decision");
    assert!(decision.for_me);
    assert_eq!(decision.options.len(), 2);
    assert!(decision.created_ms > 0);

    let conv = home
        .conversations
        .iter()
        .find(|c| c.id == "_agent.test.alpha")
        .unwrap();
    assert_eq!(conv.kind, ConversationKind::Agent);
    assert_eq!(conv.title, "Alpha");
    assert_eq!(conv.unread, 1);
    assert_eq!(
        conv.last_message_preview.as_deref(),
        Some("Release notes drafted")
    );

    // --- live events + messaging ----------------------------------------
    let (tx, rx) = mpsc::channel();
    let sub = session
        .subscribe("**".into(), Box::new(Collector(Mutex::new(tx))))
        .unwrap();
    assert!(sub.is_active());
    wait_for(&rx, "stream connected", |e| {
        matches!(
            e,
            MeshEvent::StreamStatus {
                connected: true,
                ..
            }
        )
    });

    let sent = session
        .send_message("_agent.test.alpha".into(), "hello alpha".into())
        .unwrap();
    assert_eq!(sent.topic, "_agent.test.alpha.inbox");
    assert!(sent.is_mine);
    assert!(sent.id.starts_with("msg_"));

    let ev = wait_for(
        &rx,
        "MessageReceived",
        |e| matches!(e, MeshEvent::MessageReceived { message, .. } if message.body == "hello alpha"),
    );
    let MeshEvent::MessageReceived {
        conversation_id,
        kind,
        message,
    } = ev
    else {
        unreachable!()
    };
    assert_eq!(conversation_id, "_agent.test.alpha");
    assert_eq!(kind, ConversationKind::Agent);
    assert!(message.is_mine);

    let messages = eventually("conversation history", || {
        let m = session
            .conversation("_agent.test.alpha".into(), 50)
            .unwrap();
        (m.len() >= 2).then_some(m)
    });
    assert_eq!(messages[0].body, "Release notes drafted");
    assert_eq!(messages[0].from_display, "Alpha");
    let card = messages[0].ui.as_ref().expect("MeshUI card parsed");
    assert_eq!(card.title, "Draft");
    assert_eq!(messages[1].body, "hello alpha");
    assert!(messages.windows(2).all(|w| w[0].ts_ms <= w[1].ts_ms));

    // A MeshUI action is a plain reply, never a decision answer.
    let reply = session
        .send_ui_action(
            "_agent.test.alpha".into(),
            messages[0].id.clone(),
            "ship".into(),
            "Ship it".into(),
        )
        .unwrap();
    assert_eq!(reply.topic, "_agent.test.alpha.inbox");
    assert_eq!(reply.ui_action.as_ref().unwrap().action_id, "ship");

    // Read markers drive unread counts.
    session.mark_read("_agent.test.alpha".into(), messages[0].ts_ms);
    let home = session.home().unwrap();
    let conv = home
        .conversations
        .iter()
        .find(|c| c.id == "_agent.test.alpha")
        .unwrap();
    assert_eq!(conv.unread, 0);

    // --- decisions --------------------------------------------------------
    session
        .respond_decision("decision_app_1".into(), "approve".into())
        .unwrap();
    let ev = wait_for(&rx, "DecisionResolved", |e| {
        matches!(e, MeshEvent::DecisionResolved { .. })
    });
    assert_eq!(
        ev,
        MeshEvent::DecisionResolved {
            decision_id: "decision_app_1".into(),
            status: "approved".into(),
            option_id: Some("approve".into()),
            responded_by: match &ev {
                MeshEvent::DecisionResolved { responded_by, .. } => responded_by.clone(),
                _ => None,
            },
        }
    );
    let home = session.home().unwrap();
    assert!(home
        .pending_decisions
        .iter()
        .all(|d| d.id != "decision_app_1"));

    // Error mapping from real hub responses.
    assert!(matches!(
        session.respond_decision("decision_app_1".into(), "reject".into()),
        Err(MeshError::InvalidInput { .. })
    ));
    assert!(matches!(
        session.respond_decision("decision_missing".into(), "approve".into()),
        Err(MeshError::NotFound { .. })
    ));
    assert!(matches!(
        session.send_message("_task.x".into(), "nope".into()),
        Err(MeshError::InvalidInput { .. })
    ));

    // --- graph ------------------------------------------------------------
    let graph = session.graph(800.0, 600.0).unwrap();
    let me = graph.nodes.iter().find(|n| n.id == ME).unwrap();
    assert_eq!((me.x, me.y), (400.0, 300.0));
    assert!(graph.nodes.iter().any(|n| n.id == "agent://test/alpha"));
    assert!(graph.nodes.iter().any(|n| n.id == "agent://other/beta"));
    assert!(graph
        .nodes
        .iter()
        .all(|n| (0.0..=800.0).contains(&n.x) && (0.0..=600.0).contains(&n.y)));

    sub.cancel();
    assert!(!sub.is_active());
}

#[test]
fn connect_maps_auth_failures() {
    // A hub that requires authentication rejects an anonymous session at
    // connect time with Unauthorized (no token), not later.
    let port = 41202;
    std::env::set_var("MELLOWMESH_NOTIFICATIONS", "off");
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let store = Store::new_in_memory().unwrap();
            let metrics = Arc::new(DaemonMetrics::default());
            let pipeline = Arc::new(PersistencePipeline::new(store.clone(), metrics.clone()));
            pipeline.start();
            let trace_mgr = Arc::new(TraceSessionManager::new(store.clone(), metrics.clone()));
            let state = AppState {
                store: store.clone(),
                registry: SubscriptionRegistry::new(metrics.clone()),
                metrics,
                pipeline,
                trace_mgr,
                policy_config: Arc::new(PersistenceConfig {
                    default: PersistencePolicy {
                        mode: PersistenceMode::Queryable,
                        retention: "7d".to_string(),
                        max_message_size: None,
                        sync: false,
                    },
                    rules: vec![],
                }),
                wikis: Arc::new(std::collections::HashMap::new()),
                node_id: format!("test-node-{port}"),
                shutdown_trigger: Arc::new(tokio::sync::Notify::new()),
                require_auth: true,
                owner: ME.to_string(),
                port,
                http: mellowmesh_daemon::state::loopback_http_client(),
                caches: Arc::new(mellowmesh_daemon::cache::Caches::default()),
            };
            let app = create_router(state);
            let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
                .await
                .unwrap();
            ready_tx.send(()).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();

    let err = MeshSession::connect(config(port)).err().unwrap();
    assert!(matches!(err, MeshError::Unauthorized { .. }), "{err:?}");
    let mut bad = config(port);
    bad.token = Some("mm_not_a_real_token".into());
    let err = MeshSession::connect(bad).err().unwrap();
    assert!(matches!(err, MeshError::Unauthorized { .. }), "{err:?}");
}
