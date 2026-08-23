//! Multi-agent collaboration integration tests.
//!
//! These spin up a REAL daemon (Axum router + persistence pipeline + lease
//! sweeper) on a TCP port and drive it with the real Rust SDK over HTTP/WS,
//! with 2, 5, 10, and 20 agents working concurrently. They validate the core
//! coordination assertions the product rests on:
//!
//!   * a task is claimed by exactly ONE agent at a time (atomic leased claim),
//!   * N agents divide a task set with NO double-execution and NO lost work,
//!   * an expired lease returns work to the board and it can be re-claimed,
//!   * concurrent human decisions are each answered exactly once, with the
//!     approve/reject outcome recorded faithfully.

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
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

/// Build and serve a real daemon on `port`, returning the shared `Store` for
/// post-hoc assertions. Runs in open mode: these tests exercise the
/// coordination primitives, which must be correct regardless of auth.
async fn spawn_daemon(port: u16) -> Store {
    // Short sweep interval + no desktop toasts, so lease reclaim happens fast
    // and headlessly. edition 2021: set_var is safe.
    std::env::set_var("MELLOWMESH_SWEEP_INTERVAL_SECS", "1");
    std::env::set_var("MELLOWMESH_NOTIFICATIONS", "off");

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
        owner: "human://test".to_string(),
        port,
    };

    mellowmesh_daemon::sweeper::start(state.clone());

    let app = create_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    store
}

fn client(port: u16) -> MellowMeshClient {
    MellowMeshClient::loopback(port)
}

fn open_task(id: &str, title: &str) -> Task {
    Task {
        id: id.to_string(),
        title: title.to_string(),
        created_by: "human://test".to_string(),
        status: "open".to_string(),
        priority: "medium".to_string(),
        ..Default::default()
    }
}

fn progress_msg(agent: &str, task_id: &str, note: &str) -> Message {
    Message {
        id: String::new(),
        topic: format!("_task.{task_id}.progress"),
        from: agent.to_string(),
        owner: None,
        timestamp: chrono::Utc::now(),
        content_type: "text/plain".to_string(),
        body: note.to_string(),
        headers: None,
        payload: None,
        parent_id: None,
    }
}

/// Core scenario: `n_agents` race to claim, work, and complete `n_agents * 4`
/// tasks. Asserts exclusive ownership, full completion, and no double-work.
async fn run_competitive_claim(n_agents: usize, port: u16) {
    let store = spawn_daemon(port).await;
    let coordinator = client(port);

    // Register the fleet and post the work.
    let total_tasks = n_agents * 4;
    for i in 0..n_agents {
        coordinator
            .register_agent(&AgentRegistration {
                id: format!("agent://test/worker{i}"),
                name: format!("worker{i}"),
                owner: "human://test".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            })
            .await
            .unwrap();
    }
    for t in 0..total_tasks {
        coordinator
            .create_task(&open_task(&format!("task_{t}"), &format!("Task {t}")))
            .await
            .unwrap();
    }

    // All agents start together to maximize genuine contention.
    let barrier = Arc::new(tokio::sync::Barrier::new(n_agents));
    let mut handles = Vec::new();
    for i in 0..n_agents {
        let barrier = barrier.clone();
        let agent_id = format!("agent://test/worker{i}");
        let c = client(port);
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut won: Vec<String> = Vec::new();
            // One pass over every task id, offset so agents fan out across the
            // set rather than all fighting over task_0 first.
            for k in 0..total_tasks {
                let idx = (i * 4 + k) % total_tasks;
                let task_id = format!("task_{idx}");
                // Claim is exclusive: Ok means we hold the lease, Err (409)
                // means another agent already holds it.
                if c.claim_task_with_lease(&task_id, &agent_id, Some(120))
                    .await
                    .is_ok()
                {
                    // Heartbeat, then finish the work we hold.
                    let _ = c
                        .publish(&progress_msg(&agent_id, &task_id, "working"))
                        .await;
                    if c.complete_task(&task_id).await.is_ok() {
                        won.push(task_id);
                    }
                }
            }
            won
        }));
    }

    let mut all_won: Vec<String> = Vec::new();
    let mut distinct_winners = 0usize;
    for h in handles {
        let won = h.await.unwrap();
        if !won.is_empty() {
            distinct_winners += 1;
        }
        all_won.extend(won);
    }

    // No task was completed by two agents (the atomic-claim guarantee).
    let unique: HashSet<&String> = all_won.iter().collect();
    assert_eq!(
        unique.len(),
        all_won.len(),
        "a task was completed by more than one agent ({n_agents} agents)"
    );
    // Every task was completed exactly once — no lost work.
    assert_eq!(
        all_won.len(),
        total_tasks,
        "not all tasks completed exactly once ({n_agents} agents)"
    );
    // The work was genuinely shared, not done by a single agent.
    assert!(
        distinct_winners >= 2,
        "expected the work to be divided across agents, only {distinct_winners} did any"
    );

    // The persisted state agrees: every task is completed, held by one agent.
    let tasks = store.list_tasks().unwrap();
    assert_eq!(tasks.len(), total_tasks);
    for t in tasks {
        assert_eq!(t.status, "completed", "task {} not completed", t.id);
        assert!(t.claimed_by.is_some(), "task {} has no claimant", t.id);
    }
}

#[tokio::test]
async fn collaborate_2_agents() {
    run_competitive_claim(2, 41002).await;
}

#[tokio::test]
async fn collaborate_5_agents() {
    run_competitive_claim(5, 41005).await;
}

#[tokio::test]
async fn collaborate_10_agents() {
    run_competitive_claim(10, 41010).await;
}

#[tokio::test]
async fn collaborate_20_agents() {
    run_competitive_claim(20, 41020).await;
}

/// A crashed agent's lease expires and the daemon's sweeper returns the task
/// to the board, where another agent can pick it up — no work is stranded.
#[tokio::test]
async fn lease_reclaim_hands_work_to_another_agent() {
    let port = 41030;
    let store = spawn_daemon(port).await;
    let c = client(port);

    c.create_task(&open_task("task_crash", "Survive a crash"))
        .await
        .unwrap();

    // Agent A claims with a 1s lease, then "crashes" (never heartbeats).
    c.claim_task_with_lease("task_crash", "agent://test/a", Some(1))
        .await
        .unwrap();

    // Wait past the lease + a sweep tick; the sweeper should reclaim it.
    let mut reclaimed = false;
    for _ in 0..30 {
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        let t = store.get_task("task_crash").unwrap().unwrap();
        if t.status == "open" && t.claimed_by.is_none() {
            reclaimed = true;
            break;
        }
    }
    assert!(reclaimed, "expired lease was not reclaimed by the sweeper");

    // Agent B can now claim the reclaimed task and finish it.
    c.claim_task_with_lease("task_crash", "agent://test/b", Some(120))
        .await
        .expect("reclaimed task should be claimable by another agent");
    c.complete_task("task_crash").await.unwrap();
    let t = store.get_task("task_crash").unwrap().unwrap();
    assert_eq!(t.status, "completed");
    assert_eq!(t.claimed_by.as_deref(), Some("agent://test/b"));
}

/// Many agents concurrently propose decisions; a human resolves them all at
/// once. Each decision is answered exactly once, and a "reject" option is
/// recorded as rejected — never silently as approved.
#[tokio::test]
async fn concurrent_decisions_answered_once_with_correct_outcome() {
    let port = 41040;
    let n = 10;
    let store = spawn_daemon(port).await;
    let coordinator = client(port);

    // Each agent proposes one decision with explicit approve/reject options.
    let mut proposers = Vec::new();
    for i in 0..n {
        let c = client(port);
        proposers.push(tokio::spawn(async move {
            let dec = Decision {
                id: format!("decision_{i}"),
                title: format!("Deploy {i}?"),
                question: "Ship it?".to_string(),
                created_by: format!("agent://test/proposer{i}"),
                required_decider: "human://test".to_string(),
                status: "requested".to_string(),
                options: vec![
                    DecisionOption {
                        id: "approve".to_string(),
                        label: "Approve".to_string(),
                        pros: vec![],
                        cons: vec![],
                        outcome: Some(DecisionOutcome::Approve),
                    },
                    DecisionOption {
                        id: "reject".to_string(),
                        label: "Reject".to_string(),
                        pros: vec![],
                        cons: vec![],
                        outcome: Some(DecisionOutcome::Reject),
                    },
                ],
                response_option_id: None,
                response_timestamp: None,
                responded_by: None,
            };
            c.create_decision(&dec).await.unwrap();
        }));
    }
    for p in proposers {
        p.await.unwrap();
    }
    assert_eq!(coordinator.list_decisions().await.unwrap().len(), n);

    // The human answers all decisions concurrently: even ids approved, odd
    // rejected. Each is answered once; a duplicate answer is refused (409).
    let mut answerers = Vec::new();
    for i in 0..n {
        let c = client(port);
        answerers.push(tokio::spawn(async move {
            let option = if i % 2 == 0 { "approve" } else { "reject" };
            c.respond_decision(&format!("decision_{i}"), option)
                .await
                .unwrap();
            // A second answer to the same decision must be refused.
            c.respond_decision(&format!("decision_{i}"), "approve")
                .await
                .is_err()
        }));
    }
    for a in answerers {
        assert!(a.await.unwrap(), "a decision was answered more than once");
    }

    // Outcomes are recorded faithfully: approves are approved, rejects rejected.
    for i in 0..n {
        let d = store
            .get_decision(&format!("decision_{i}"))
            .unwrap()
            .unwrap();
        let expected = if i % 2 == 0 { "approved" } else { "rejected" };
        assert_eq!(d.status, expected, "decision_{i} recorded wrong outcome");
        assert_eq!(
            d.responded_by.as_deref(),
            Some("human://local-unauthenticated")
        );
    }
}
