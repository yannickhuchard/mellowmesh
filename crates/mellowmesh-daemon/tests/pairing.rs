//! One-step pairing end to end: `POST /pair` (url and LAN modes),
//! `GET /pair/:id`, device-token expiry, `GET/DELETE /devices`, and the
//! runtime LAN listener with its access rules.

mod common;

use common::{spawn_hub, spawn_hub_with, Hub, OWNER};
use mellowmesh_daemon::pairing::PairingState;
use reqwest::Method;
use serde_json::{json, Value};
use std::net::IpAddr;
use std::time::Duration;

/// The query parameters of a `mellowmesh-pair:` link.
fn link_params(link: &str) -> std::collections::HashMap<String, String> {
    let query = link
        .strip_prefix("mellowmesh-pair:?")
        .expect("a mellowmesh-pair link");
    url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

async fn pair(hub: &Hub, token: Option<&str>, body: Value) -> (u16, Value) {
    hub.send(Method::POST, "/pair", token, false, body).await
}

/// A request to `base` with a fresh client (no pooled connections).
async fn get_at(base: &str, path: &str, token: Option<&str>) -> Result<(u16, Value), String> {
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap();
    let mut req = client.get(format!("{base}{path}"));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

#[tokio::test]
async fn pair_with_url_opens_the_public_host_at_runtime() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let tunnel = "pairing-test.trycloudflare.com";

    // Before pairing, the hub does not answer to the tunnel's name.
    let r = hub
        .http
        .get(format!("{}/whoami", hub.base))
        .header("Host", tunnel)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403);

    let (s, p) = pair(
        &hub,
        Some(&owner),
        json!({ "name": "Pixel\u{7}  8", "url": format!("https://{tunnel}/") }),
    )
    .await;
    assert_eq!(s, 201, "{p}");
    assert_eq!(p["url"], format!("https://{tunnel}"));
    assert_eq!(p["name"], "Pixel 8");
    assert_eq!(p["network"], "My mesh");
    assert_eq!(p["principal"], OWNER);
    assert!(p["token_id"].as_str().unwrap().starts_with("tok_"));
    assert_eq!(
        p["lan"],
        json!({ "enabled": false, "address": null, "listening": false })
    );
    let expires: chrono::DateTime<chrono::Utc> =
        serde_json::from_value(p["expires_at"].clone()).unwrap();
    let left = expires - chrono::Utc::now();
    assert!(left > chrono::Duration::minutes(14) && left <= chrono::Duration::minutes(15));
    let params = link_params(p["link"].as_str().unwrap());
    assert_eq!(params["v"], "2");
    assert_eq!(params["url"], format!("https://{tunnel}"));
    assert_eq!(params["name"], "My mesh");
    assert_eq!(params["principal"], OWNER);
    assert!(params["token"].starts_with("mm_"));

    // Now the tunnel's name is a public host: requests naming it are
    // remote, and the device token works through it.
    let r = hub
        .http
        .get(format!("{}/whoami", hub.base))
        .header("Host", tunnel)
        .bearer_auth(&params["token"])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let who: Value = r.json().await.unwrap();
    assert_eq!(who["principal"], OWNER);
    assert_eq!(who["remote"], true);
    // ... and it is remembered for the next start.
    assert!(
        mellowmesh_daemon::pairing::persisted_public_hosts(&hub.store)
            .contains(&tunnel.to_string())
    );

    // A bad url is refused.
    for bad in ["ftp://x.example", "not a url", "https://u:p@x.example"] {
        let (s, v) = pair(&hub, Some(&owner), json!({ "url": bad })).await;
        assert_eq!(s, 400, "{bad}: {v}");
        assert!(v["error"].is_string());
    }
    let (s, _) = hub
        .req(Method::POST, "/pair", Some(&owner), false)
        .header("content-type", "application/json")
        .body("{not json")
        .send()
        .await
        .map(|r| (r.status().as_u16(), ()))
        .unwrap();
    assert_eq!(s, 400);
}

#[tokio::test]
async fn pairing_endpoints_are_owner_only() {
    for require_auth in [true, false] {
        let hub = spawn_hub(require_auth).await;
        let agent = hub.token("agent://owner/coder", &["**"], &["**"]);
        let body = json!({ "url": "https://x.example" });
        // Remote anonymous: never.
        let (s, _) = hub
            .send(Method::POST, "/pair", None, true, body.clone())
            .await;
        assert_eq!(s, 401);
        for path in ["/pair/tok_x", "/devices"] {
            assert_eq!(hub.get(path, None, true).await.0, 401, "{path}");
            assert_eq!(hub.get(path, Some(&agent), false).await.0, 403, "{path}");
        }
        let (s, _) = hub
            .send(Method::DELETE, "/devices/tok_x", None, true, json!({}))
            .await;
        assert_eq!(s, 401);
        // Another principal's token: forbidden, with a JSON error.
        let (s, v) = pair(&hub, Some(&agent), body.clone()).await;
        assert_eq!(s, 403);
        assert!(v["error"].as_str().unwrap().contains("owner"));
        let (s, _) = hub
            .send(
                Method::DELETE,
                "/devices/tok_x",
                Some(&agent),
                false,
                json!({}),
            )
            .await;
        assert_eq!(s, 403);
        // Local anonymous: the owner in open mode only.
        let (s, v) = pair(&hub, None, body.clone()).await;
        if require_auth {
            assert_eq!(s, 401);
        } else {
            assert_eq!(s, 201, "{v}");
            assert_eq!(v["principal"], OWNER);
        }
    }
}

#[tokio::test]
async fn pair_status_flips_connected_and_devices_can_be_removed() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let owner = Some(owner.as_str());
    let (s, p) = pair(&hub, owner, json!({ "url": "http://10.1.2.3:40000" })).await;
    assert_eq!(s, 201, "{p}");
    assert_eq!(p["name"], "Phone");
    let id = p["token_id"].as_str().unwrap().to_string();
    let device = link_params(p["link"].as_str().unwrap())["token"].clone();

    let (s, st) = hub.get(&format!("/pair/{id}"), owner, false).await;
    assert_eq!(s, 200);
    assert_eq!(
        st,
        json!({ "token_id": id, "name": "Phone", "connected": false,
                "last_seen": null, "expired": false })
    );
    let (_, devices) = hub.get("/devices", owner, false).await;
    assert_eq!(devices.as_array().unwrap().len(), 1);
    assert_eq!(devices[0]["connected"], false);
    assert!(devices[0]["expires_at"].is_string());

    // The phone uses its token: pairing sees it connected.
    let (s, who) = hub.get("/whoami", Some(&device), true).await;
    assert_eq!(s, 200);
    assert_eq!(who["principal"], OWNER);
    let (_, st) = hub.get(&format!("/pair/{id}"), owner, false).await;
    assert_eq!(st["connected"], true);
    assert_eq!(st["expired"], false);
    assert!(st["last_seen"].is_string());

    // A second, pending device; newest first.
    let (_, p2) = pair(
        &hub,
        owner,
        json!({ "name": "Tablet", "url": "http://10.1.2.3:40000" }),
    )
    .await;
    let (_, devices) = hub.get("/devices", owner, false).await;
    let devices = devices.as_array().unwrap().clone();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0]["token_id"], p2["token_id"]);
    assert_eq!(devices[1]["token_id"], id);
    assert_eq!(devices[1]["connected"], true);
    assert_eq!(devices[1]["expires_at"], Value::Null);
    assert!(devices[1]["created_at"].is_string());
    // Ordinary tokens are not devices.
    assert!(devices.iter().all(|d| d["name"] != "agent"));

    // Remove: the token stops working at once.
    let (s, _) = hub
        .send(
            Method::DELETE,
            &format!("/devices/{id}"),
            owner,
            false,
            json!({}),
        )
        .await;
    assert_eq!(s, 204);
    assert_eq!(hub.get("/whoami", Some(&device), true).await.0, 401);
    let (_, devices) = hub.get("/devices", owner, false).await;
    assert_eq!(devices.as_array().unwrap().len(), 1);
    let (s, v) = hub.get(&format!("/pair/{id}"), owner, false).await;
    assert_eq!(s, 404);
    assert!(v["error"].is_string());
    // A pending (never used) device can be removed too.
    let pending = p2["token_id"].as_str().unwrap();
    let (s, _) = hub
        .send(
            Method::DELETE,
            &format!("/devices/{pending}"),
            owner,
            false,
            json!({}),
        )
        .await;
    assert_eq!(s, 204);
    let (_, devices) = hub.get("/devices", owner, false).await;
    assert!(devices.as_array().unwrap().is_empty());
    assert_eq!(
        hub.get(&format!("/pair/{pending}"), owner, false).await.0,
        404
    );
    let (s, v) = hub
        .send(
            Method::DELETE,
            &format!("/devices/{id}"),
            owner,
            false,
            json!({}),
        )
        .await;
    assert_eq!(s, 404);
    assert!(v["error"].is_string());
    assert_eq!(hub.get("/pair/tok_unknown", owner, false).await.0, 404);
    // An ordinary token cannot be removed as a device.
    let tokens = hub.store.list_tokens().unwrap();
    let ordinary = tokens.iter().find(|t| t.principal == OWNER).unwrap();
    let (s, _) = hub
        .send(
            Method::DELETE,
            &format!("/devices/{}", ordinary.id),
            owner,
            false,
            json!({}),
        )
        .await;
    assert_eq!(s, 404);
}

#[tokio::test]
async fn unused_device_tokens_expire() {
    let hub = spawn_hub_with(
        true,
        PairingState::default().with_device_ttl(Duration::from_secs(1)),
    )
    .await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let owner = Some(owner.as_str());
    let body = json!({ "url": "http://10.1.2.3:40000" });
    let (_, unused) = pair(&hub, owner, body.clone()).await;
    let (_, used) = pair(&hub, owner, body).await;
    let unused_token = link_params(unused["link"].as_str().unwrap())["token"].clone();
    let used_token = link_params(used["link"].as_str().unwrap())["token"].clone();
    assert_eq!(hub.get("/whoami", Some(&used_token), true).await.0, 200);

    tokio::time::sleep(Duration::from_millis(2500)).await;

    // The unused one expired: it no longer authenticates.
    assert_eq!(hub.get("/whoami", Some(&unused_token), true).await.0, 401);
    let (_, st) = hub
        .get(
            &format!("/pair/{}", unused["token_id"].as_str().unwrap()),
            owner,
            false,
        )
        .await;
    assert_eq!(st["expired"], true);
    assert_eq!(st["connected"], false);
    // The used one stays valid.
    assert_eq!(hub.get("/whoami", Some(&used_token), true).await.0, 200);
    let (_, devices) = hub.get("/devices", owner, false).await;
    let ids: Vec<&Value> = devices
        .as_array()
        .unwrap()
        .iter()
        .map(|d| &d["token_id"])
        .collect();
    assert_eq!(ids, vec![&used["token_id"]]);
    // The sweep revokes the expired one for good.
    assert_eq!(
        hub.store
            .revoke_expired_device_tokens(chrono::Utc::now())
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn lan_mode_opens_a_listener_at_runtime() {
    let lan_ip: IpAddr = "127.0.0.2".parse().unwrap();
    let hub = spawn_hub_with(
        true,
        PairingState::default().with_lan_ip_source(move || Some(lan_ip)),
    )
    .await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let owner = Some(owner.as_str());
    let address = format!("127.0.0.2:{}", hub.port);
    let lan_base = format!("http://{address}");
    assert!(get_at(&lan_base, "/health", None).await.is_err());

    let (s, p) = pair(&hub, owner, json!({})).await;
    assert_eq!(s, 201, "{p}");
    assert_eq!(p["url"], lan_base);
    assert_eq!(
        p["lan"],
        json!({ "enabled": true, "address": address, "listening": true })
    );
    assert_eq!(link_params(p["link"].as_str().unwrap())["url"], lan_base);
    let device = link_params(p["link"].as_str().unwrap())["token"].clone();

    // The LAN listener serves the hub, and callers there are remote.
    let (s, who) = get_at(&lan_base, "/whoami", None).await.unwrap();
    assert_eq!(s, 200);
    assert_eq!(who["remote"], true);
    assert_eq!(who["principal"], Value::Null);
    let (s, who) = get_at(&lan_base, "/whoami", Some(&device)).await.unwrap();
    assert_eq!(s, 200);
    assert_eq!(who["principal"], OWNER);
    assert_eq!(who["remote"], true);
    // Token-less callers there cannot pair or list devices.
    assert_eq!(get_at(&lan_base, "/devices", None).await.unwrap().0, 401);
    let r = reqwest::Client::new()
        .post(format!("{lan_base}/pair"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // The setting is on and persisted; the owner sees the address.
    assert!(hub.store.get_network().unwrap().lan_access);
    let (_, n) = hub.get("/network", owner, false).await;
    assert_eq!(n["network"]["lan_access"], true);
    assert_eq!(n["network"]["lan_address"], address);
    let (_, n) = hub.get("/network", None, false).await;
    assert_eq!(n["network"]["lan_address"], Value::Null);

    // Pairing again reuses the listener.
    let (s, p2) = pair(&hub, owner, json!({ "name": "Tablet" })).await;
    assert_eq!(s, 201, "{p2}");
    assert_eq!(p2["url"], lan_base);

    // Turning LAN access off closes the listener and forgets its host.
    let (s, n) = hub
        .send(
            Method::PUT,
            "/network",
            owner,
            false,
            json!({ "lan_access": false }),
        )
        .await;
    assert_eq!(s, 200, "{n}");
    assert_eq!(n["lan_access"], false);
    assert_eq!(n["lan_address"], Value::Null);
    assert!(!hub.store.get_network().unwrap().lan_access);
    assert!(get_at(&lan_base, "/health", None).await.is_err());
    let r = hub
        .http
        .get(format!("{}/health", hub.base))
        .header("Host", &address)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403);

    // ... and back on.
    let (s, n) = hub
        .send(
            Method::PUT,
            "/network",
            owner,
            false,
            json!({ "lan_access": true }),
        )
        .await;
    assert_eq!(s, 200, "{n}");
    assert_eq!(n["lan_address"], address);
    assert_eq!(get_at(&lan_base, "/health", None).await.unwrap().0, 200);

    // At startup a persisted setting re-opens the listener.
    mellowmesh_daemon::pairing::close_lan(&hub.state).await;
    assert!(get_at(&lan_base, "/health", None).await.is_err());
    mellowmesh_daemon::pairing::restore_lan(&hub.state).await;
    assert_eq!(get_at(&lan_base, "/health", None).await.unwrap().0, 200);
    mellowmesh_daemon::pairing::close_lan(&hub.state).await;
}

#[tokio::test]
async fn lan_mode_without_a_usable_address_is_503() {
    // No LAN address at all.
    let hub = spawn_hub_with(true, PairingState::default().with_lan_ip_source(|| None)).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let (s, v) = pair(&hub, Some(&owner), json!({ "name": "Phone" })).await;
    assert_eq!(s, 503);
    assert!(
        v["error"].as_str().unwrap().contains("No LAN address"),
        "{v}"
    );
    assert!(!hub.store.get_network().unwrap().lan_access);
    assert!(hub
        .store
        .list_device_tokens(chrono::Utc::now())
        .unwrap()
        .is_empty());
    let (s, v) = hub
        .send(
            Method::PUT,
            "/network",
            Some(&owner),
            false,
            json!({ "lan_access": true }),
        )
        .await;
    assert_eq!(s, 503);
    assert!(v["error"].as_str().unwrap().contains("No LAN address"));
    assert!(!hub.store.get_network().unwrap().lan_access);

    // An address this machine does not have: the listener cannot bind.
    let elsewhere: IpAddr = "192.0.2.55".parse().unwrap();
    let hub = spawn_hub_with(
        true,
        PairingState::default().with_lan_ip_source(move || Some(elsewhere)),
    )
    .await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let (s, v) = pair(&hub, Some(&owner), json!({})).await;
    assert_eq!(s, 503);
    assert!(
        v["error"].as_str().unwrap().contains("Could not listen"),
        "{v}"
    );
    // Restoring at startup only logs.
    hub.store
        .set_network(&mellowmesh_core::network::Network {
            lan_access: true,
            ..Default::default()
        })
        .unwrap();
    mellowmesh_daemon::pairing::restore_lan(&hub.state).await;
    assert!(hub.state.access.pairing.lan_address().is_none());
}

/// LAN mode on this machine's real LAN address: it either works or says
/// why not (a sandbox may have no LAN).
#[tokio::test]
async fn lan_mode_on_the_real_network() {
    let hub = spawn_hub(true).await;
    let owner = hub.token(OWNER, &["**"], &["**"]);
    let (s, p) = pair(&hub, Some(&owner), json!({})).await;
    match s {
        201 => {
            let url = p["url"].as_str().unwrap();
            assert!(url.starts_with("http://"));
            assert_eq!(p["lan"]["listening"], true);
            let token = link_params(p["link"].as_str().unwrap())["token"].clone();
            let (s, who) = get_at(url, "/whoami", Some(&token)).await.unwrap();
            assert_eq!(s, 200);
            assert_eq!(who["remote"], true);
            mellowmesh_daemon::pairing::close_lan(&hub.state).await;
        }
        503 => assert!(p["error"].is_string(), "{p}"),
        other => panic!("unexpected status {other}: {p}"),
    }
}
