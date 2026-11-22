//! Notes between AIs: the owner's mail policy decides which helpers'
//! mailboxes an invited AI may write to — any (default), only some, none,
//! or a rule of its own — at once, on keys already given.

mod common;

use common::*;
use reqwest::Method;
use serde_json::json;
use std::time::Duration;

const GROK: &str = "agent://owner/grok";
const CODEX: &str = "agent://owner/codex";
const CODER_INBOX: &str = "_agent.owner.coder.inbox";
const FINANCE_INBOX: &str = "_agent.owner.finance.inbox";

/// An invited AI's own key, with exactly the scopes the hub gives it.
fn invited(hub: &Hub, platform: &str, principal: &str) -> String {
    let (read, write) = mellowmesh_daemon::connect::platform_scopes(OWNER, platform);
    let read: Vec<&str> = read.iter().map(String::as_str).collect();
    let write: Vec<&str> = write.iter().map(String::as_str).collect();
    hub.token(principal, &read, &write)
}

fn helper(hub: &Hub, slug: &str) {
    hub.store
        .register_agent(&mellowmesh_core::agent::AgentRegistration {
            id: format!("agent://owner/{slug}"),
            name: slug.to_string(),
            owner: OWNER.to_string(),
            mode: "autonomous".into(),
            capabilities: vec![],
        })
        .unwrap();
}

async fn inbox_count(hub: &Hub, owner: &str) -> usize {
    let (_, h) = hub
        .get(
            &format!("/history?pattern={CODER_INBOX}&limit=50"),
            Some(owner),
            false,
        )
        .await;
    h.as_array().map_or(0, |a| a.len())
}

async fn set(hub: &Hub, owner: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
    hub.send(Method::PUT, "/agent-mail", Some(owner), false, body)
        .await
}

#[tokio::test]
async fn invited_ais_write_to_mailboxes_as_the_owner_decides() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let grok = invited(&hub, "grok", GROK);
    let codex = invited(&hub, "codex", CODEX);
    helper(&hub, "coder");
    helper(&hub, "finance");

    // Default: any helper's mailbox — the network organizes itself.
    assert_eq!(hub.publish(CODER_INBOX, Some(&grok), true).await.0, 200);
    assert_eq!(hub.publish(FINANCE_INBOX, Some(&grok), true).await.0, 200);
    // Never another helper's voice, nor the hub's reserved topics.
    assert_eq!(
        hub.publish("_agent.owner.coder.outbox", Some(&grok), true)
            .await
            .0,
        403
    );
    // Its own corner always works.
    assert_eq!(
        hub.publish("_agent.owner.grok.inbox", Some(&grok), true)
            .await
            .0,
        200
    );

    // The owner closes every mailbox: refused at once, same key.
    let (s, v) = set(&hub, &owner, json!({ "rule": { "mode": "none" } })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["network"]["mode"], "none");
    assert_eq!(hub.publish(CODER_INBOX, Some(&grok), true).await.0, 403);
    assert_eq!(hub.publish(CODER_INBOX, Some(&codex), true).await.0, 403);
    assert_eq!(
        hub.publish("_agent.owner.grok.inbox", Some(&grok), true)
            .await
            .0,
        200
    );

    // Grok gets a rule of its own: Coder only. Codex keeps the network's.
    let (s, v) = set(
        &hub,
        &owner,
        json!({ "for": GROK, "rule": { "mode": "only", "boxes": ["agent://owner/coder"] } }),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["per_ai"][GROK]["boxes"][0], "agent://owner/coder");
    assert_eq!(hub.publish(CODER_INBOX, Some(&grok), true).await.0, 200);
    assert_eq!(hub.publish(FINANCE_INBOX, Some(&grok), true).await.0, 403);
    assert_eq!(hub.publish(CODER_INBOX, Some(&codex), true).await.0, 403);

    // Grok can ask what it may do.
    let (s, mine) = hub.get("/agent-mail", Some(&grok), true).await;
    assert_eq!(s, 200);
    assert_eq!(mine["you"], GROK);
    assert_eq!(mine["own_rule"], true);
    assert_eq!(mine["may_write"][0], "agent://owner/coder");
    // ...but cannot change it.
    assert_eq!(
        set(&hub, &grok, json!({ "rule": { "mode": "any" } }))
            .await
            .0,
        403
    );

    // Back to the network rule.
    let (s, v) = set(&hub, &owner, json!({ "for": GROK })).await;
    assert_eq!(s, 200);
    assert!(v["per_ai"].get(GROK).is_none());
    assert_eq!(hub.publish(CODER_INBOX, Some(&grok), true).await.0, 403);

    // The owner's view lists invited AIs and the helpers' mailboxes.
    let (_, v) = hub.get("/agent-mail", Some(&owner), false).await;
    let invited: Vec<&str> = v["invited"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert!(invited.contains(&GROK) && invited.contains(&CODEX));
    assert_eq!(v["mailboxes"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn mail_policy_input_is_checked() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    invited(&hub, "grok", GROK);
    // Not one of the owner's helpers.
    let (s, _) = set(
        &hub,
        &owner,
        json!({ "rule": { "mode": "only", "boxes": ["agent://someone/coder"] } }),
    )
    .await;
    assert_eq!(s, 400);
    // Not an invited AI.
    let (s, _) = set(
        &hub,
        &owner,
        json!({ "for": "agent://owner/coder", "rule": { "mode": "none" } }),
    )
    .await;
    assert_eq!(s, 400);
    // A person without the owner's key.
    let other = hub.token("human://guest", &["**"], &["**"]);
    assert_eq!(hub.get("/agent-mail", Some(&other), false).await.0, 403);
    assert_eq!(hub.get("/agent-mail", None, true).await.0, 401);
}

#[tokio::test]
async fn an_invited_ais_mention_follows_the_same_rule() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let grok = invited(&hub, "grok", GROK);
    helper(&hub, "coder");
    let mention = |body: &str| {
        json!({
            "id": "", "topic": "_forum.general", "from": GROK,
            "timestamp": chrono::Utc::now(), "content_type": "text/plain",
            "body": body
        })
    };
    let (s, _) = hub
        .send(
            Method::POST,
            "/publish",
            Some(&grok),
            true,
            mention("@coder first"),
        )
        .await;
    assert_eq!(s, 200);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(inbox_count(&hub, &owner).await, 1);

    set(&hub, &owner, json!({ "rule": { "mode": "none" } })).await;
    let (s, _) = hub
        .send(
            Method::POST,
            "/publish",
            Some(&grok),
            true,
            mention("@coder second"),
        )
        .await;
    // The room post itself is fine; only the copy into Coder's mailbox is not made.
    assert_eq!(s, 200);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(inbox_count(&hub, &owner).await, 1);
}
