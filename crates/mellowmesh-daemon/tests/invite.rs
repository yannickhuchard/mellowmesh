//! Inviting an AI in one link: the owner gets a short-lived link per AI
//! (catalog apps, or any AI by name); the AI opens it — as text, or as JSON
//! for a bot — and finds its own limited key and the one step to connect.

mod common;

use common::*;
use reqwest::Method;
use serde_json::{json, Value};

/// The path part of a join link (`/join/<code>`).
fn join_path(inv: &Value) -> String {
    let url = inv["join_url"].as_str().unwrap();
    url[url.find("/join/").unwrap()..].to_string()
}

async fn open_text(hub: &Hub, path: &str, remote: bool) -> (u16, String) {
    let r = hub
        .req(Method::GET, path, None, remote)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    (status, r.text().await.unwrap())
}

#[tokio::test]
async fn a_link_gives_the_ai_its_own_key_in_one_step() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);

    let (s, inv) = hub
        .send(
            Method::POST,
            "/invite",
            Some(&owner),
            false,
            json!({"platform": "grok"}),
        )
        .await;
    assert_eq!(s, 200, "{inv}");
    assert_eq!(inv["principal"], "agent://owner/grok");
    assert_eq!(inv["opens"], 5);
    // The hub's public address (a tunnel's name) is the link's.
    assert_eq!(inv["reachable_from_web"], true);
    let link = inv["join_url"].as_str().unwrap();
    assert!(
        link.starts_with(&format!("https://{PUBLIC_HOST}/join/")),
        "{link}"
    );
    assert!(inv["message"].as_str().unwrap().contains(link));
    // The link itself carries no key.
    assert!(!link.contains("mm_") && !link.contains("token"));

    // The AI opens it without any key (from outside, too) and reads the
    // one step, with its own key in the connector address.
    let path = join_path(&inv);
    let (s, page) = open_text(&hub, &path, true).await;
    assert_eq!(s, 200, "{page}");
    assert!(page.contains("agent://owner/grok"));
    assert!(page.contains("/mcp?token=mm_"));
    assert!(page.contains("get_agent_mail_policy"));

    // A bot reads the same as JSON, and the key works as Grok's.
    let (s, j) = hub.get(&format!("{path}?format=json"), None, false).await;
    assert_eq!(s, 200);
    let key = j["token"].as_str().unwrap().to_string();
    assert_eq!(j["you"]["id"], "agent://owner/grok");
    let (s, who) = hub.get("/whoami", Some(&key), false).await;
    assert_eq!(s, 200);
    assert_eq!(who["principal"], "agent://owner/grok");
    // Grok's own limits, never the owner's.
    let (s, _) = hub.get("/email", Some(&key), false).await;
    assert_eq!(s, 200);
    let (s, _) = hub
        .send(
            Method::PUT,
            "/agent-mail",
            Some(&key),
            false,
            json!({"rule": {"mode": "none"}}),
        )
        .await;
    assert_eq!(s, 403);

    // Five opens, then the link is gone.
    for _ in 0..3 {
        assert_eq!(open_text(&hub, &path, false).await.0, 200);
    }
    let (s, page) = open_text(&hub, &path, false).await;
    assert_eq!(s, 410);
    assert!(page.contains("expired"));
    assert_eq!(
        open_text(&hub, "/join/00000-00000-00000-00000-00000", false)
            .await
            .0,
        410
    );

    // The short code still works for an AI that can only POST it.
    let (s, c) = hub
        .send(
            Method::POST,
            "/pair/claim",
            None,
            false,
            json!({"code": inv["code"]}),
        )
        .await;
    assert_eq!(s, 200, "{c}");
}

#[tokio::test]
async fn any_ai_can_be_invited_by_name() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    hub.store
        .register_agent(&mellowmesh_core::agent::AgentRegistration {
            id: "agent://owner/coder".into(),
            name: "Coder".into(),
            owner: OWNER.into(),
            mode: "autonomous".into(),
            capabilities: vec![],
        })
        .unwrap();

    let (s, inv) = hub
        .send(
            Method::POST,
            "/invite",
            Some(&owner),
            false,
            json!({"name": "Buzz"}),
        )
        .await;
    assert_eq!(s, 200, "{inv}");
    assert_eq!(inv["principal"], "agent://owner/buzz");
    assert_eq!(inv["name"], "Buzz");
    let (_, j) = hub
        .get(&format!("{}?format=json", join_path(&inv)), None, false)
        .await;
    let buzz = j["token"].as_str().unwrap().to_string();

    // Buzz counts as an invited AI: the owner's mail rules apply to it,
    // and it may leave a note in Coder's mailbox (open by default).
    let (_, policy) = hub.get("/agent-mail", Some(&owner), false).await;
    assert!(
        policy["invited"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "Buzz"),
        "{policy}"
    );
    let (s, _) = hub
        .publish("_agent.owner.coder.inbox", Some(&buzz), false)
        .await;
    assert_eq!(s, 200);
    let (s, _) = hub
        .publish("_agent.owner.coder.outbox", Some(&buzz), false)
        .await;
    assert_eq!(s, 403);

    // A name that is a catalog app is that app.
    let (_, grok) = hub
        .send(
            Method::POST,
            "/invite",
            Some(&owner),
            false,
            json!({"name": "Grok"}),
        )
        .await;
    assert_eq!(grok["principal"], "agent://owner/grok");
    // A helper's name is not free: its key is never replaced.
    let (s, _) = hub
        .send(
            Method::POST,
            "/invite",
            Some(&owner),
            false,
            json!({"name": "coder"}),
        )
        .await;
    assert_eq!(s, 409);
    // Bad requests.
    for body in [
        json!({}),
        json!({"name": "!!!"}),
        json!({"platform": "nope"}),
        json!({"name": "x".repeat(41)}),
    ] {
        let (s, _) = hub
            .send(Method::POST, "/invite", Some(&owner), false, body.clone())
            .await;
        assert!(s == 400 || s == 404, "{body} -> {s}");
    }
    // Owner only.
    let (s, _) = hub
        .send(
            Method::POST,
            "/invite",
            Some(&buzz),
            false,
            json!({"name": "Eve"}),
        )
        .await;
    assert_eq!(s, 403);
    let (s, _) = hub
        .send(Method::POST, "/invite", None, false, json!({"name": "Eve"}))
        .await;
    assert_eq!(s, 401);
    // Inviting again replaces the AI's key: the old one stops.
    let (_, again) = hub
        .send(
            Method::POST,
            "/invite",
            Some(&owner),
            false,
            json!({"name": "Buzz"}),
        )
        .await;
    assert_eq!(again["principal"], "agent://owner/buzz");
    let (s, _) = hub.get("/whoami", Some(&buzz), false).await;
    assert_eq!(s, 401);
}

#[tokio::test]
async fn the_platform_invite_also_gives_the_link() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let (s, inv) = hub
        .send(
            Method::POST,
            "/platforms/chatgpt/invite",
            Some(&owner),
            false,
            json!({}),
        )
        .await;
    assert_eq!(s, 200, "{inv}");
    assert!(inv["code"].as_str().is_some_and(|c| !c.is_empty()));
    assert!(inv["join_url"].as_str().unwrap().contains("/join/"));
    assert_eq!(inv["principal"], "agent://owner/chatgpt");
}
