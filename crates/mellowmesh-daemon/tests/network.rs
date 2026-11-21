//! Network access control end to end: public hosts and remote callers,
//! spaces and teams over HTTP, history / WebSocket filtering, `/whoami`,
//! `/access/check`, and the startup refusal to listen off loopback without
//! `--require-auth`.

mod common;

use common::{spawn_hub, topics_of, OWNER, PUBLIC_HOST};
use futures_util::StreamExt;
use mellowmesh_core::message::Message;
use serde_json::{json, Value};
use std::time::Duration;

#[tokio::test]
async fn remote_visitors_read_open_spaces_only() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let owner = Some(owner.as_str());
    use reqwest::Method;

    // Spaces: an open one (no visitor posts), an open guestbook (visitor
    // posts), a closed one.
    for (id, pattern, access, vw) in [
        ("public", "_forum.public.**", "open", false),
        ("guestbook", "_forum.guestbook.**", "open", true),
        ("family", "_chat.family.**", "closed", false),
    ] {
        let (s, v) = hub
            .send(
                Method::POST,
                "/spaces",
                owner,
                false,
                json!({ "id": id, "patterns": [pattern], "access": access, "visitor_write": vw }),
            )
            .await;
        assert_eq!(s, 201, "{v}");
        assert_eq!(v["id"], id);
    }
    for topic in ["_forum.public.news", "_chat.family.chat", "_forum.general"] {
        assert_eq!(hub.publish(topic, owner, false).await.0, 200);
    }

    // Remote anonymous: identity.
    let (s, who) = hub.get("/whoami", None, true).await;
    assert_eq!(s, 200);
    assert_eq!(
        who,
        json!({ "principal": null, "kind": null, "is_owner": false, "remote": true })
    );
    // Local anonymous under --require-auth: not remote, still a visitor.
    let (_, who) = hub.get("/whoami", None, false).await;
    assert_eq!(who["remote"], false);
    let (_, who) = hub.get("/whoami", owner, true).await;
    assert_eq!(who["principal"], OWNER);
    assert_eq!(who["kind"], "human");
    assert_eq!(who["is_owner"], true);
    assert_eq!(who["remote"], true);

    // Reads: open content only.
    let (s, h) = hub.get("/history?limit=50", None, true).await;
    assert_eq!(s, 200);
    assert_eq!(topics_of(&h), vec!["_forum.public.news"]);
    let (_, h) = hub.get("/history?limit=50", None, false).await;
    assert_eq!(topics_of(&h), vec!["_forum.public.news"]);
    let (_, t) = hub.get("/topics", None, true).await;
    assert_eq!(t, json!(["_forum.public.news"]));
    let (_, f) = hub.get("/forum", None, true).await;
    assert_eq!(topics_of(&f), vec!["_forum.public.news"]);
    let (_, sr) = hub.get("/search?query=hello", None, true).await;
    assert_eq!(topics_of(&sr), vec!["_forum.public.news"]);
    // The owner sees everything, remote or not.
    let (_, h) = hub.get("/history?limit=50", owner, true).await;
    assert_eq!(topics_of(&h).len(), 3);

    // Network view: open spaces only, member lists hidden.
    let (s, n) = hub.get("/network", None, true).await;
    assert_eq!(s, 200);
    let ids: Vec<&str> = n["spaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["guestbook", "public"]);
    assert_eq!(n["teams"], json!([]));
    assert_eq!(n["network"]["default_access"], "closed");
    let (_, spaces) = hub.get("/spaces", owner, false).await;
    assert_eq!(spaces.as_array().unwrap().len(), 3);

    // Admin and other endpoints: 401 for remote anonymous callers.
    for (method, path) in [
        (Method::POST, "/spaces"),
        (Method::PUT, "/network"),
        (Method::PUT, "/spaces/public"),
        (Method::DELETE, "/spaces/public"),
        (Method::POST, "/teams"),
        (Method::POST, "/auth/tokens"),
        (Method::POST, "/shutdown"),
    ] {
        let (s, _) = hub.send(method.clone(), path, None, true, json!({})).await;
        assert_eq!(s, 401, "{method} {path}");
    }
    for path in [
        "/access/check?topic=_forum.x",
        "/tasks",
        "/auth/tokens",
        "/agents",
    ] {
        assert_eq!(hub.get(path, None, true).await.0, 401, "{path}");
    }

    // Visitor posts: only where the space allows them; `from` is rewritten.
    assert_eq!(hub.publish("_forum.public.news", None, true).await.0, 403);
    assert_eq!(hub.publish("_chat.family.chat", None, true).await.0, 403);
    assert_eq!(hub.publish("_forum.general", None, true).await.0, 403);
    assert_eq!(
        hub.publish("_decision.d1.responded", None, true).await.0,
        403
    );
    let (s, m) = hub.publish("_forum.guestbook.hi", None, true).await;
    assert_eq!(s, 200, "{m}");
    assert_eq!(m["from"], "visitor://anonymous");
    assert_eq!(m["owner"], Value::Null);
    assert_eq!(m["headers"]["x-mellowmesh-visitor"], "true");
    // An authenticated non-member posts as itself, flagged as a visitor.
    let bob = hub.token("agent://bob/helper", &["_project.**"], &["_project.**"]);
    let (s, m) = hub.publish("_forum.guestbook.hi", Some(&bob), true).await;
    assert_eq!(s, 200);
    assert_eq!(m["from"], "agent://bob/helper");
    assert_eq!(m["headers"]["x-mellowmesh-visitor"], "true");
    // The owner (a member) posts normally.
    let (_, m) = hub.publish("_forum.guestbook.hi", owner, true).await;
    assert_eq!(m["from"], OWNER);
    assert!(m["headers"].get("x-mellowmesh-visitor").is_none());

    // Opening the network: visitors read uncovered topics too.
    let (s, n) = hub
        .send(
            Method::PUT,
            "/network",
            owner,
            false,
            json!({ "name": "Test mesh", "default_access": "open" }),
        )
        .await;
    assert_eq!(s, 200);
    assert_eq!(n["name"], "Test mesh");
    let (_, h) = hub.get("/history?limit=50", None, true).await;
    let t = topics_of(&h);
    assert!(t.contains(&"_forum.general".to_string()));
    assert!(!t.contains(&"_chat.family.chat".to_string()));
    // `private` is not a valid default.
    let (s, _) = hub
        .send(
            Method::PUT,
            "/network",
            owner,
            false,
            json!({ "default_access": "private" }),
        )
        .await;
    assert_eq!(s, 400);
}

#[tokio::test]
async fn relay_marker_and_non_owner_admin() {
    let hub = spawn_hub(false).await;
    // Open mode: a local caller without a token reads and publishes, but is
    // not the owner — administering needs the owner's token...
    let (s, _) = hub
        .send(
            reqwest::Method::POST,
            "/spaces",
            None,
            false,
            json!({ "id": "family", "patterns": ["_chat.family.**"] }),
        )
        .await;
    assert_eq!(s, 401);
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let (s, _) = hub
        .send(
            reqwest::Method::POST,
            "/spaces",
            Some(&owner),
            false,
            json!({ "id": "family", "patterns": ["_chat.family.**"] }),
        )
        .await;
    assert_eq!(s, 201);
    // ...but not through a public host (remote), even in open mode.
    let (s, _) = hub
        .send(
            reqwest::Method::DELETE,
            "/spaces/family",
            None,
            true,
            json!({}),
        )
        .await;
    assert_eq!(s, 401);
    assert_eq!(hub.get("/tasks", None, true).await.0, 401);

    // A loopback dispatch carrying the hub's marker counts as remote; a
    // wrong marker is ignored (and stripped).
    let r = hub
        .http
        .get(format!("{}/whoami", hub.base))
        .header("x-mellowmesh-remote", &hub.marker)
        .send()
        .await
        .unwrap();
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["remote"], true);
    let r = hub
        .http
        .get(format!("{}/tasks", hub.base))
        .header("x-mellowmesh-remote", &hub.marker)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    let r = hub
        .http
        .get(format!("{}/whoami", hub.base))
        .header("x-mellowmesh-remote", "mm_wrong")
        .send()
        .await
        .unwrap();
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["remote"], false);

    // A non-owner token cannot administer.
    let alice = hub.token("human://alice", &["**"], &["**"]);
    let (s, _) = hub
        .send(
            reqwest::Method::PUT,
            "/network",
            Some(&alice),
            false,
            json!({ "name": "mine" }),
        )
        .await;
    assert_eq!(s, 403);
    assert_eq!(
        hub.get("/access/check?topic=_x.y", Some(&alice), false)
            .await
            .0,
        403
    );

    // Public-host origins are accepted for the dashboard; others are not.
    let r = hub
        .http
        .get(format!("{}/whoami", hub.base))
        .header("Host", PUBLIC_HOST)
        .header("Origin", format!("https://{PUBLIC_HOST}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = hub
        .http
        .get(format!("{}/whoami", hub.base))
        .header("Host", PUBLIC_HOST)
        .header("Origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
}

#[tokio::test]
async fn closed_spaces_are_closed_to_local_open_mode_callers() {
    let hub = spawn_hub(false).await;
    let alice = hub.token("human://alice", &["**"], &["**"]);
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let (s, _) = hub
        .send(
            reqwest::Method::POST,
            "/spaces",
            Some(&owner),
            false,
            json!({ "id": "family", "patterns": ["_chat.family.**"], "access": "closed",
                    "members": ["human://alice"] }),
        )
        .await;
    assert_eq!(s, 201);

    // WebSocket subscriptions are filtered per message.
    let ws_url = |token: Option<&str>| {
        let mut u = format!("ws://127.0.0.1:{}/ws?pattern=**", hub.port);
        if let Some(t) = token {
            u.push_str(&format!("&token={t}"));
        }
        u
    };
    let (mut anon_ws, _) = tokio_tungstenite::connect_async(ws_url(None))
        .await
        .unwrap();
    let (mut alice_ws, _) = tokio_tungstenite::connect_async(ws_url(Some(&alice)))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Anonymous local callers cannot write a closed space, even in open mode.
    assert_eq!(hub.publish("_chat.family.chat", None, false).await.0, 403);
    assert_eq!(
        hub.publish("_chat.family.chat", Some(&alice), false)
            .await
            .0,
        200
    );
    // Uncovered topics keep today's open-mode trust.
    assert_eq!(hub.publish("_forum.general", None, false).await.0, 200);

    async fn next_topic(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> String {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("a delivery")
            .unwrap()
            .unwrap();
        let m: Message = serde_json::from_str(msg.to_text().unwrap()).unwrap();
        m.topic
    }
    // Alice receives both, in order; the anonymous subscriber only the
    // uncovered one.
    assert_eq!(next_topic(&mut alice_ws).await, "_chat.family.chat");
    assert_eq!(next_topic(&mut alice_ws).await, "_forum.general");
    assert_eq!(next_topic(&mut anon_ws).await, "_forum.general");

    // History: the same filtering.
    let (_, h) = hub.get("/history?limit=50", None, false).await;
    assert_eq!(topics_of(&h), vec!["_forum.general"]);
    let (_, h) = hub.get("/history?limit=50", Some(&alice), false).await;
    assert_eq!(topics_of(&h), vec!["_chat.family.chat", "_forum.general"]);

    // Removing Alice from the space takes effect immediately.
    let (s, _) = hub
        .send(
            reqwest::Method::PUT,
            "/spaces/family",
            Some(&owner),
            false,
            json!({ "patterns": ["_chat.family.**"], "access": "closed" }),
        )
        .await;
    assert_eq!(s, 200);
    let (_, h) = hub.get("/history?limit=50", Some(&alice), false).await;
    assert_eq!(topics_of(&h), vec!["_forum.general"]);
}

#[tokio::test]
async fn teams_get_a_closed_team_space() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let planner = hub.token("agent://owner/planner", &["**"], &["**"]);
    let bob = hub.token("human://bob", &["**"], &["**"]);
    use reqwest::Method;

    let (s, team) = hub
        .send(
            Method::POST,
            "/teams",
            Some(&owner),
            false,
            json!({ "id": "home", "name": "Home", "color": "#10B981",
                    "members": [{ "principal": "agent://owner/planner", "role": "member" }] }),
        )
        .await;
    assert_eq!(s, 201, "{team}");
    assert_eq!(team["members"][0]["role"], "member");
    let (_, spaces) = hub.get("/spaces", Some(&owner), false).await;
    let space = spaces
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "team-home")
        .expect("team space created")
        .clone();
    assert_eq!(space["access"], "closed");
    assert_eq!(space["patterns"], json!(["_team.home.**"]));
    assert_eq!(space["teams"], json!(["home"]));
    // A second team with the same id conflicts.
    let (s, _) = hub
        .send(
            Method::POST,
            "/teams",
            Some(&owner),
            false,
            json!({ "id": "home" }),
        )
        .await;
    assert_eq!(s, 409);
    // Bad ids are refused.
    let (s, _) = hub
        .send(
            Method::POST,
            "/teams",
            Some(&owner),
            false,
            json!({ "id": "Home!" }),
        )
        .await;
    assert_eq!(s, 400);

    // Team members use the team room; others do not.
    assert_eq!(
        hub.publish("_team.home.chat", Some(&planner), false)
            .await
            .0,
        200
    );
    assert_eq!(
        hub.publish("_team.home.chat", Some(&bob), false).await.0,
        403
    );
    let (_, teams) = hub.get("/teams", Some(&planner), false).await;
    assert_eq!(teams.as_array().unwrap().len(), 1);
    let (_, teams) = hub.get("/teams", Some(&bob), false).await;
    assert_eq!(teams, json!([]));
    let (_, n) = hub.get("/network", Some(&planner), false).await;
    assert!(n["spaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == "team-home"));

    // Access check (admin).
    let (s, c) = hub
        .get(
            "/access/check?topic=_team.home.chat&principal=human%3A%2F%2Fbob",
            Some(&owner),
            false,
        )
        .await;
    assert_eq!(s, 200);
    assert_eq!(c["read"], false);
    assert_eq!(c["write"], false);
    assert_eq!(c["space"], "team-home");
    assert_eq!(c["level"], "closed");
    let (_, c) = hub
        .get(
            "/access/check?topic=_team.home.chat&principal=agent%3A%2F%2Fowner%2Fplanner",
            Some(&owner),
            false,
        )
        .await;
    assert_eq!(
        (c["read"].clone(), c["write"].clone()),
        (json!(true), json!(true))
    );
    let (_, c) = hub
        .get("/access/check?topic=_forum.general", Some(&owner), false)
        .await;
    assert_eq!(c["read"], false);
    assert_eq!(c["space"], Value::Null);
    assert_eq!(c["level"], "closed");
    assert!(c["reason"].as_str().unwrap().contains("visitors"));

    // Add Bob, then remove the planner.
    let (s, t) = hub
        .send(
            Method::POST,
            "/teams/home/members",
            Some(&owner),
            false,
            json!({ "principal": "human://bob", "role": "admin" }),
        )
        .await;
    assert_eq!(s, 200);
    assert_eq!(t["members"].as_array().unwrap().len(), 2);
    assert_eq!(
        hub.publish("_team.home.chat", Some(&bob), false).await.0,
        200
    );
    let r = hub
        .req(
            Method::DELETE,
            "/teams/home/members/agent%3A%2F%2Fowner%2Fplanner",
            Some(&owner),
            false,
        )
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    assert_eq!(
        hub.publish("_team.home.chat", Some(&planner), false)
            .await
            .0,
        403
    );

    // Deleting the team removes its space.
    let r = hub
        .req(Method::DELETE, "/teams/home", Some(&owner), false)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    let (_, spaces) = hub.get("/spaces", Some(&owner), false).await;
    assert!(spaces.as_array().unwrap().is_empty());
}

#[test]
fn refuses_to_listen_off_loopback_without_require_auth() {
    let dir = std::env::temp_dir().join(format!(
        "mm-listen-{}",
        ulid::Ulid::new().to_string().to_lowercase()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mellowmeshd"))
        .args(["--listen", "0.0.0.0", "--port", "1", "--db"])
        .arg(dir.join("mm.db"))
        .env_remove("MELLOWMESH_REQUIRE_AUTH")
        .env_remove("MELLOWMESH_RELAY_URL")
        .env("RUST_LOG", "error")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("--require-auth"), "{text}");
    // Refused before touching the database.
    assert!(!dir.join("mm.db").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Rust SDK covers the whole network and task-board API.
#[tokio::test]
async fn sdk_round_trip() {
    use mellowmesh_client::MellowMeshClient;
    use mellowmesh_core::network::{AccessLevel, NetworkPatch, Space, Team, TeamRole};
    use mellowmesh_core::task::{Task, TaskUpdate};

    let hub = spawn_hub(true).await;
    let owner = MellowMeshClient::loopback(hub.port).with_token(hub.token(OWNER, &["**"], &["**"]));
    let me = owner.whoami().await.unwrap();
    assert_eq!(me.principal.as_deref(), Some(OWNER));
    assert!(me.is_owner && !me.remote);

    let n = owner
        .set_network(&NetworkPatch {
            name: Some("SDK mesh".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(n.name, "SDK mesh");

    let team = owner
        .create_team(&Team {
            id: "ops".into(),
            name: "Ops".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(team.color, "#10B981");
    let team = owner
        .add_team_member("ops", "human://alice", TeamRole::Admin)
        .await
        .unwrap();
    assert_eq!(team.members.len(), 1);
    let mut renamed = team.clone();
    renamed.name = "Operations".into();
    assert_eq!(
        owner.replace_team(&renamed).await.unwrap().name,
        "Operations"
    );

    let space = owner
        .create_space(&Space {
            id: "news".into(),
            patterns: vec!["_forum.news.**".into()],
            access: AccessLevel::Open,
            teams: vec!["ops".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(space.name, "news");
    let mut closed = space.clone();
    closed.access = AccessLevel::Closed;
    assert_eq!(
        owner.replace_space(&closed).await.unwrap().access,
        AccessLevel::Closed
    );
    let view = owner.get_network().await.unwrap();
    assert_eq!(view.network.name, "SDK mesh");
    assert_eq!(view.spaces.len(), 2, "news + team-ops");
    assert_eq!(owner.list_spaces().await.unwrap().len(), 2);
    assert_eq!(owner.list_teams().await.unwrap().len(), 1);

    let check = owner
        .access_check("_forum.news.today", Some("human://alice"))
        .await
        .unwrap();
    assert!(check.read && check.write);
    assert_eq!(check.space.as_deref(), Some("news"));
    let check = owner.access_check("_forum.news.today", None).await.unwrap();
    assert!(!check.read);

    // Tasks.
    owner
        .create_task(&Task {
            id: "sdk1".into(),
            title: "First".into(),
            team: Some("ops".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let t = owner
        .update_task(
            "sdk1",
            &TaskUpdate {
                status: Some("blocked".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(t.status, "blocked");
    let g = owner.task_graph(Some("team:ops")).await.unwrap();
    assert_eq!(g.tasks.len(), 1);
    assert_eq!(g.lanes[0].label, "Operations");
    assert!(owner.task_graph(None).await.is_ok());

    // A labelled token, as `mellowmesh pair` mints it.
    let minted = owner
        .create_labeled_token(OWNER, "Pixel", vec!["**".into()], vec!["**".into()])
        .await
        .unwrap();
    let tokens = owner.list_tokens().await.unwrap();
    assert!(tokens
        .iter()
        .any(|t| t["id"] == minted["id"] && t["label"] == "Pixel"));

    owner
        .remove_team_member("ops", "human://alice")
        .await
        .unwrap();
    owner.delete_space("news").await.unwrap();
    owner.delete_team("ops").await.unwrap();
    assert!(owner.list_spaces().await.unwrap().is_empty());
}
