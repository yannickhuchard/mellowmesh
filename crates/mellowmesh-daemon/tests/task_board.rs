//! Task boards end to end: status transitions and who may make them,
//! dependency validation, live `_task.<id>.<event>` publications, and the
//! task graph with scope and space filtering.

mod common;

use common::{spawn_hub, Hub, OWNER};
use futures_util::StreamExt;
use mellowmesh_core::message::Message;
use reqwest::Method;
use serde_json::{json, Value};
use std::time::Duration;

const CREATOR: &str = "agent://owner/planner";
const CODER: &str = "agent://owner/coder";
const STRANGER: &str = "agent://other/bot";

async fn create(hub: &Hub, token: &str, body: Value) -> (u16, Value) {
    hub.send(Method::POST, "/tasks", Some(token), false, body)
        .await
}

async fn patch(hub: &Hub, token: &str, id: &str, body: Value) -> (u16, Value) {
    hub.send(
        Method::PATCH,
        &format!("/tasks/{id}"),
        Some(token),
        false,
        body,
    )
    .await
}

async fn claim(hub: &Hub, token: &str, id: &str, who: &str) -> u16 {
    hub.send(
        Method::POST,
        &format!("/tasks/{id}/claim"),
        Some(token),
        false,
        json!({ "claimed_by": who }),
    )
    .await
    .0
}

fn task_body(id: &str, topics: &[&str]) -> Value {
    json!({
        "id": id, "title": format!("Task {id}"), "created_by": "human://forged",
        "status": "in_progress", "priority": "high", "topics": topics,
        "required_capabilities": [], "artifacts": [], "decisions": [],
        "claimed_by": "agent://forged", "created_at": "2001-01-01T00:00:00Z",
        "completed_at": "2001-01-01T00:00:00Z"
    })
}

#[tokio::test]
async fn transitions_and_permissions() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let creator = hub.token(CREATOR, &["**"], &["**"]);
    let coder = hub.token(CODER, &["**"], &["**"]);
    let stranger = hub.token(STRANGER, &["**"], &["**"]);

    // Create: identity and lifecycle fields are the hub's.
    let (s, t) = create(&hub, &creator, task_body("t1", &[])).await;
    assert_eq!(s, 200, "{t}");
    assert_eq!(t["created_by"], CREATOR);
    assert_eq!(t["status"], "open");
    assert!(t.get("claimed_by").is_none());
    assert!(t.get("completed_at").is_none());
    assert_ne!(t["created_at"], "2001-01-01T00:00:00Z");
    // Ids become topic segments.
    let (s, _) = create(&hub, &creator, task_body("bad.id", &[])).await;
    assert_eq!(s, 400);

    // Strangers may not update; nobody claims through PATCH.
    assert_eq!(
        patch(&hub, &stranger, "t1", json!({ "priority": "low" }))
            .await
            .0,
        403
    );
    assert_eq!(
        patch(&hub, &creator, "t1", json!({ "status": "claimed" }))
            .await
            .0,
        400
    );
    assert_eq!(
        patch(&hub, &creator, "t1", json!({ "status": "in_progress" }))
            .await
            .0,
        400
    );
    assert_eq!(
        patch(&hub, &creator, "t1", json!({ "priority": "urgent" }))
            .await
            .0,
        400
    );
    let (s, t) = patch(
        &hub,
        &creator,
        "t1",
        json!({ "priority": "low", "assigned_to": CODER, "title": "Renamed" }),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(t["priority"], "low");
    assert_eq!(t["assigned_to"], CODER);
    assert_eq!(t["title"], "Renamed");

    // Claim, then only the claimant starts it.
    assert_eq!(claim(&hub, &coder, "t1", CODER).await, 200);
    let (_, t) = hub.get("/tasks", Some(&owner), false).await;
    assert!(t[0]["claimed_at"].is_string());
    assert_eq!(
        patch(&hub, &creator, "t1", json!({ "status": "in_progress" }))
            .await
            .0,
        403
    );
    let (s, t) = patch(&hub, &coder, "t1", json!({ "status": "in_progress" })).await;
    assert_eq!(s, 200);
    assert!(t["started_at"].is_string());

    // Blocked → only back to where it was.
    let (s, t) = patch(&hub, &creator, "t1", json!({ "status": "blocked" })).await;
    assert_eq!(s, 200);
    assert_eq!(t["status"], "blocked");
    assert!(
        t.get("blocked_from").is_none(),
        "internal field not exposed"
    );
    assert_eq!(
        patch(&hub, &creator, "t1", json!({ "status": "open" }))
            .await
            .0,
        400
    );
    assert_eq!(claim(&hub, &stranger, "t1", STRANGER).await, 400);
    let (s, t) = patch(&hub, &coder, "t1", json!({ "status": "in_progress" })).await;
    assert_eq!(s, 200);
    assert!(
        t["claim_expires_at"].is_string(),
        "lease renewed on unblock"
    );

    // Complete by the claimant; reopen only by creator or owner.
    let (s, t) = patch(&hub, &coder, "t1", json!({ "status": "completed" })).await;
    assert_eq!(s, 200);
    assert!(t["completed_at"].is_string());
    assert_eq!(
        patch(&hub, &coder, "t1", json!({ "status": "open" }))
            .await
            .0,
        403
    );
    assert_eq!(
        patch(&hub, &creator, "t1", json!({ "status": "blocked" }))
            .await
            .0,
        400
    );
    let (s, t) = patch(&hub, &creator, "t1", json!({ "status": "open" })).await;
    assert_eq!(s, 200);
    assert!(t.get("claimed_by").is_none() && t.get("completed_at").is_none());
    // The coder is still the assignee, so it may cancel.
    let (s, t) = patch(&hub, &coder, "t1", json!({ "status": "cancelled" })).await;
    assert_eq!(s, 200);
    assert_eq!(t["status"], "cancelled");
    let (s, _) = patch(&hub, &owner, "t1", json!({ "status": "open" })).await;
    assert_eq!(s, 200);
    assert_eq!(patch(&hub, &owner, "nope", json!({})).await.0, 404);
}

#[tokio::test]
async fn a_task_joins_a_journey_and_carries_its_decisions() {
    let hub = spawn_hub(true).await;
    let coder = hub.token(CODER, &["**"], &["**"]);
    let stranger = hub.token(STRANGER, &["**"], &["**"]);
    let (s, _) = create(&hub, &coder, task_body("visit1", &["work"])).await;
    assert_eq!(s, 200);

    // A journey is a topic the hub writes from the name.
    let (s, t) = patch(
        &hub,
        &coder,
        "visit1",
        json!({ "journey": "Déménagement à Lyon" }),
    )
    .await;
    assert_eq!(s, 200, "{t}");
    assert_eq!(t["topics"], json!(["work", "journey.demenagement-a-lyon"]));
    let (s, t) = patch(&hub, &coder, "visit1", json!({ "journey": "Move house" })).await;
    assert_eq!(s, 200);
    assert_eq!(t["topics"], json!(["work", "journey.move-house"]));
    assert_eq!(
        patch(&hub, &coder, "visit1", json!({ "journey": "?!" }))
            .await
            .0,
        400
    );

    // A decision about the step is linked to it, once; unknown ones are refused.
    let (s, d) = hub
        .send(
            Method::POST,
            "/decisions",
            Some(&coder),
            false,
            json!({
                "id": "", "title": "Take the flat?", "question": "Rue Oberkampf, 3 rooms",
                "created_by": CODER, "required_decider": OWNER, "status": "requested",
                "options": [{"id": "option_0", "label": "Yes", "pros": [], "cons": []}],
                "response_option_id": null, "response_timestamp": null, "responded_by": null
            }),
        )
        .await;
    assert_eq!(s, 200, "{d}");
    let did = d["id"].as_str().unwrap().to_string();
    for _ in 0..2 {
        let (s, t) = patch(&hub, &coder, "visit1", json!({ "link_decision": did })).await;
        assert_eq!(s, 200, "{t}");
        assert_eq!(t["decisions"], json!([did]));
    }
    assert_eq!(
        patch(
            &hub,
            &coder,
            "visit1",
            json!({ "link_decision": "decision_nope" })
        )
        .await
        .0,
        404
    );
    assert_eq!(
        patch(&hub, &stranger, "visit1", json!({ "journey": "" }))
            .await
            .0,
        403
    );

    // The list says the same (the store kept topics and decisions).
    let (_, all) = hub.get("/tasks", Some(&coder), false).await;
    let t = all
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "visit1")
        .unwrap()
        .clone();
    assert_eq!(t["topics"], json!(["work", "journey.move-house"]));
    assert_eq!(t["decisions"], json!([did]));
}

#[tokio::test]
async fn dependencies_are_validated() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let mut body = task_body("a", &[]);
    assert_eq!(create(&hub, &owner, body.clone()).await.0, 200);
    body = task_body("b", &[]);
    body["depends_on"] = json!(["a", "a"]);
    let (s, t) = create(&hub, &owner, body).await;
    assert_eq!(s, 200);
    assert_eq!(t["depends_on"], json!(["a"]));
    body = task_body("c", &[]);
    body["depends_on"] = json!(["missing"]);
    assert_eq!(create(&hub, &owner, body).await.0, 400);
    body = task_body("d", &[]);
    body["depends_on"] = json!(["d"]);
    assert_eq!(create(&hub, &owner, body).await.0, 400);
    // a → b → a would be a cycle.
    let r = hub
        .req(Method::PATCH, "/tasks/a", Some(&owner), false)
        .json(&json!({ "depends_on": ["b"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    assert!(r.text().await.unwrap().contains("cycle"));
    assert_eq!(
        patch(&hub, &owner, "a", json!({ "depends_on": ["zz"] }))
            .await
            .0,
        400
    );
    assert_eq!(
        patch(&hub, &owner, "b", json!({ "depends_on": [] }))
            .await
            .0,
        200
    );
    assert_eq!(
        patch(&hub, &owner, "a", json!({ "depends_on": ["b"] }))
            .await
            .0,
        200
    );
    // Teams must exist.
    assert_eq!(
        patch(&hub, &owner, "a", json!({ "team": "ghosts" }))
            .await
            .0,
        400
    );
}

#[tokio::test]
async fn lifecycle_events_are_published() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let coder = hub.token(CODER, &["**"], &["**"]);
    let url = format!(
        "ws://127.0.0.1:{}/ws?pattern=_task.**&token={owner}",
        hub.port
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(create(&hub, &owner, task_body("ev1", &[])).await.0, 200);
    assert_eq!(claim(&hub, &coder, "ev1", CODER).await, 200);
    assert_eq!(
        patch(&hub, &coder, "ev1", json!({ "status": "in_progress" }))
            .await
            .0,
        200
    );
    assert_eq!(
        patch(&hub, &owner, "ev1", json!({ "priority": "low" }))
            .await
            .0,
        200
    );
    let r = hub
        .req(Method::POST, "/tasks/ev1/complete", Some(&coder), false)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(create(&hub, &owner, task_body("ev2", &[])).await.0, 200);
    assert_eq!(
        patch(&hub, &owner, "ev2", json!({ "status": "cancelled" }))
            .await
            .0,
        200
    );

    let mut seen = Vec::new();
    while seen.len() < 7 {
        let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("event")
            .unwrap()
            .unwrap();
        let m: Message = serde_json::from_str(frame.to_text().unwrap()).unwrap();
        assert_eq!(m.from, "node://test");
        let payload = m.payload.unwrap();
        assert_eq!(payload["id"], m.topic.split('.').nth(1).unwrap());
        seen.push(m.topic);
    }
    assert_eq!(
        seen,
        vec![
            "_task.ev1.created",
            "_task.ev1.claimed",
            "_task.ev1.started",
            "_task.ev1.updated",
            "_task.ev1.completed",
            "_task.ev2.created",
            "_task.ev2.cancelled",
        ]
    );

    // Clients cannot forge lifecycle events; progress stays open.
    assert_eq!(
        hub.publish("_task.ev1.completed", Some(&owner), false)
            .await
            .0,
        403
    );
    assert_eq!(
        hub.publish("_task.ev1.progress", Some(&owner), false)
            .await
            .0,
        200
    );
}

#[tokio::test]
async fn graph_scopes_and_space_filtering() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let member = hub.token(CODER, &["**"], &["**"]);
    let outsider = hub.token(STRANGER, &["**"], &["**"]);
    let (s, _) = hub
        .send(
            Method::POST,
            "/teams",
            Some(&owner),
            false,
            json!({ "id": "home", "name": "Home", "members": [{ "principal": CODER }] }),
        )
        .await;
    assert_eq!(s, 201);

    let mut a = task_body("ta", &["_team.home.tasks"]);
    a["team"] = json!("home");
    assert_eq!(create(&hub, &owner, a).await.0, 200);
    let mut b = task_body("tb", &[]);
    b["depends_on"] = json!(["ta"]);
    assert_eq!(create(&hub, &owner, b).await.0, 200);
    assert_eq!(claim(&hub, &member, "ta", CODER).await, 200);

    let ids = |g: &Value| -> Vec<String> {
        let mut v: Vec<String> = g["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_string())
            .collect();
        v.sort();
        v
    };

    // The team member sees both, with lanes, edges and metrics.
    let (s, g) = hub.get("/tasks/graph", Some(&member), false).await;
    assert_eq!(s, 200);
    assert_eq!(ids(&g), vec!["ta", "tb"]);
    assert!(g["now"].is_string());
    assert_eq!(g["edges"], json!([{ "from": "ta", "to": "tb" }]));
    let tb = g["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "tb")
        .unwrap();
    assert_eq!(tb["blocked_by"], json!(["ta"]));
    assert_eq!(tb["lane"], "unassigned");
    assert_eq!(tb["critical"], true);
    let lanes: Vec<&str> = g["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["kind"].as_str().unwrap())
        .collect();
    assert_eq!(lanes, vec!["agent", "unassigned"]);

    // The outsider cannot see the team's task, in the graph or the list.
    let (_, g) = hub.get("/tasks/graph", Some(&outsider), false).await;
    assert_eq!(ids(&g), vec!["tb"]);
    assert_eq!(g["edges"], json!([]));
    let (_, list) = hub.get("/tasks", Some(&outsider), false).await;
    let listed: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, vec!["tb"]);
    // Nor its lifecycle events.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, h) = hub
        .get("/history?pattern=_task.**&limit=50", Some(&outsider), false)
        .await;
    assert!(h
        .as_array()
        .unwrap()
        .iter()
        .all(|m| !m["topic"].as_str().unwrap().starts_with("_task.ta.")));
    let (_, h) = hub
        .get("/history?pattern=_task.**&limit=50", Some(&member), false)
        .await;
    assert!(h
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["topic"] == "_task.ta.created"));

    // Scopes.
    let (_, g) = hub
        .get("/tasks/graph?scope=team:home", Some(&member), false)
        .await;
    assert_eq!(ids(&g), vec!["ta"]);
    let (_, g) = hub
        .get("/tasks/graph?scope=mine", Some(&member), false)
        .await;
    assert_eq!(ids(&g), vec!["ta"]);
    let (_, g) = hub
        .get("/tasks/graph?scope=mine", Some(&owner), false)
        .await;
    assert_eq!(ids(&g), vec!["ta", "tb"]);
    let (_, g) = hub
        .get(
            "/tasks/graph?scope=agent:agent%3A%2F%2Fowner%2Fcoder",
            Some(&owner),
            false,
        )
        .await;
    assert_eq!(ids(&g), vec!["ta"]);
    assert_eq!(
        hub.get("/tasks/graph?scope=weird", Some(&owner), false)
            .await
            .0,
        400
    );
    // Visitors get nothing.
    assert_eq!(hub.get("/tasks/graph", None, true).await.0, 401);
}
