use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::message::Message;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

#[async_trait::async_trait]
pub trait InterfaceConnector: Send + Sync {
    async fn run(&self) -> anyhow::Result<()>;
}

// Helper function to map external identity to MellowMesh identity
async fn resolve_identity(client: &MellowMeshClient, external_id: &str) -> String {
    if let Ok(mappings) = client.list_identity_mappings().await {
        for (ext, fm) in mappings {
            if ext == external_id {
                return fm;
            }
        }
    }
    external_id.to_string()
}

// ==========================================
// 1. Discord Connector
// ==========================================
pub struct DiscordConnector {
    client: MellowMeshClient,
    token: Option<String>,
    channel_id: Option<String>,
}

impl DiscordConnector {
    pub fn new(client: MellowMeshClient) -> Self {
        let token = std::env::var("DISCORD_TOKEN").ok();
        let channel_id = std::env::var("DISCORD_CHANNEL_ID").ok();
        Self {
            client,
            token,
            channel_id,
        }
    }

    async fn run_live(&self, token: &str, channel_id: &str) -> anyhow::Result<()> {
        info!(
            "Starting Live Discord Connector for channel: {}",
            channel_id
        );
        let http_client = reqwest::Client::new();
        let mut last_message_id: Option<String> = None;

        // Subscribe to MellowMesh messages to forward them to Discord
        let client_clone = self.client.clone();
        let channel_id_str = channel_id.to_string();
        let token_str = token.to_string();
        tokio::spawn(async move {
            if let Ok(mut sub) = client_clone.subscribe("_forum.**").await {
                while let Some(Ok(msg)) = sub.next().await {
                    // Avoid forwarding messages that came from Discord itself
                    if msg.from.starts_with("discord://") {
                        continue;
                    }
                    let discord_msg = format!("**{}** (via MellowMesh):\n{}", msg.from, msg.body);
                    let url =
                        format!("https://discord.com/api/v10/channels/{channel_id_str}/messages");
                    let _ = http_client
                        .post(&url)
                        .header("Authorization", format!("Bot {token_str}"))
                        .json(&serde_json::json!({ "content": discord_msg }))
                        .send()
                        .await;
                }
            }
        });

        // Polling loop for inbound Discord messages
        let poll_client = reqwest::Client::new();
        let default_interval = Duration::from_secs(5);
        let mut current_interval = default_interval;
        loop {
            let mut url =
                format!("https://discord.com/api/v10/channels/{channel_id}/messages?limit=5");
            if let Some(ref last_id) = last_message_id {
                url = format!("{url}&after={last_id}");
            }

            let resp = poll_client
                .get(&url)
                .header("Authorization", format!("Bot {token}"))
                .send()
                .await;

            let mut success = false;
            match resp {
                Ok(res) if res.status().is_success() => {
                    success = true;
                    if let Ok(msgs) = res.json::<Vec<serde_json::Value>>().await {
                        // Discord returns messages newest first if no "after", or oldest first if "after".
                        // Let's sort or process them appropriately.
                        let mut processed_any = false;
                        for discord_msg in msgs.iter().rev() {
                            if let (Some(id), Some(content), Some(author)) = (
                                discord_msg["id"].as_str(),
                                discord_msg["content"].as_str(),
                                discord_msg["author"].as_object(),
                            ) {
                                // Skip bot messages to avoid loops
                                if author.contains_key("bot")
                                    && author["bot"].as_bool().unwrap_or(false)
                                {
                                    last_message_id = Some(id.to_string());
                                    continue;
                                }

                                let author_id = author["id"].as_str().unwrap_or("unknown");
                                let author_name = author["username"].as_str().unwrap_or("unknown");
                                let ext_id = format!("discord://{author_id}");
                                let mellowmesh_id = resolve_identity(&self.client, &ext_id).await;

                                info!(
                                    "Discord message received from {} ({}): {}",
                                    author_name, ext_id, content
                                );

                                let fm_msg = Message {
                                    id: format!("msg_{id}"),
                                    topic: "_forum.general".to_string(),
                                    from: ext_id,
                                    owner: Some(mellowmesh_id),
                                    timestamp: chrono::Utc::now(),
                                    content_type: "text/markdown".to_string(),
                                    body: content.to_string(),
                                    headers: None,
                                    payload: None,
                                    parent_id: None,
                                };

                                let _ = self.client.publish(&fm_msg).await;
                                last_message_id = Some(id.to_string());
                                processed_any = true;
                            }
                        }
                        if !processed_any && last_message_id.is_none() && !msgs.is_empty() {
                            // If first poll, set the last_message_id to the newest message
                            if let Some(newest_id) = msgs[0]["id"].as_str() {
                                last_message_id = Some(newest_id.to_string());
                            }
                        }
                    }
                }
                Ok(res) => {
                    warn!("Discord API returned error status: {}", res.status());
                }
                Err(e) => {
                    warn!("Failed to poll Discord API: {}", e);
                }
            }

            if success {
                current_interval = default_interval;
            } else {
                current_interval = std::cmp::min(current_interval * 2, Duration::from_secs(60));
                info!(
                    "Discord poll failed. Backing off. Next poll in {:?}",
                    current_interval
                );
            }

            sleep(current_interval).await;
        }
    }

    async fn run_mock(&self) -> anyhow::Result<()> {
        info!("Starting Discord Connector in Simulation/Mock Mode");
        let messages = [
            (
                "discord://foyer/yannick",
                "Can someone review the payment API before Friday?",
            ),
            (
                "discord://foyer/yannick",
                "Is there any security risk with the current token storage?",
            ),
            (
                "discord://foyer/yannick",
                "Great! The test coverage for the connector looks good.",
            ),
        ];
        let mut idx = 0;
        loop {
            sleep(Duration::from_secs(20)).await;
            let (ext_id, body) = messages[idx % messages.len()];
            let mellowmesh_id = resolve_identity(&self.client, ext_id).await;

            let fm_msg = Message {
                id: format!("msg_discord_{}", chrono::Utc::now().timestamp()),
                topic: "_forum.general".to_string(),
                from: ext_id.to_string(),
                owner: Some(mellowmesh_id),
                timestamp: chrono::Utc::now(),
                content_type: "text/markdown".to_string(),
                body: body.to_string(),
                headers: None,
                payload: None,
                parent_id: None,
            };

            info!("Simulating Discord message: {}", body);
            let _ = self.client.publish(&fm_msg).await;
            idx += 1;
        }
    }
}

#[async_trait::async_trait]
impl InterfaceConnector for DiscordConnector {
    async fn run(&self) -> anyhow::Result<()> {
        if let (Some(token), Some(channel_id)) = (&self.token, &self.channel_id) {
            self.run_live(token, channel_id).await
        } else {
            self.run_mock().await
        }
    }
}

// ==========================================
// 2. Telegram Connector
// ==========================================
pub struct TelegramConnector {
    client: MellowMeshClient,
    token: Option<String>,
    chat_id: Option<String>,
}

impl TelegramConnector {
    pub fn new(client: MellowMeshClient) -> Self {
        let token = std::env::var("TELEGRAM_TOKEN").ok();
        let chat_id = std::env::var("TELEGRAM_CHAT_ID").ok();
        Self {
            client,
            token,
            chat_id,
        }
    }

    async fn run_live(&self, token: &str, chat_id: &str) -> anyhow::Result<()> {
        info!("Starting Live Telegram Connector");
        let http_client = reqwest::Client::new();
        let mut offset = 0i64;

        // Forward MellowMesh messages to Telegram
        let client_clone = self.client.clone();
        let chat_id_str = chat_id.to_string();
        let token_str = token.to_string();
        tokio::spawn(async move {
            if let Ok(mut sub) = client_clone.subscribe("_forum.**").await {
                while let Some(Ok(msg)) = sub.next().await {
                    if msg.from.starts_with("telegram://") {
                        continue;
                    }
                    let telegram_msg = format!("*{}* (via MellowMesh):\n{}", msg.from, msg.body);
                    let url = format!("https://api.telegram.org/bot{token_str}/sendMessage");
                    let _ = http_client
                        .post(&url)
                        .json(&serde_json::json!({
                            "chat_id": chat_id_str,
                            "text": telegram_msg,
                            "parse_mode": "Markdown"
                        }))
                        .send()
                        .await;
                }
            }
        });

        // Polling loop for updates
        let poll_client = reqwest::Client::new();
        let default_interval = Duration::from_secs(4);
        let mut current_interval = default_interval;
        loop {
            let url = format!(
                "https://api.telegram.org/bot{token}/getUpdates?offset={offset}&limit=5&timeout=5"
            );

            let resp = poll_client.get(&url).send().await;
            let mut success = false;
            match resp {
                Ok(res) if res.status().is_success() => {
                    success = true;
                    if let Ok(update_res) = res.json::<serde_json::Value>().await {
                        if let Some(updates) = update_res["result"].as_array() {
                            for update in updates {
                                if let Some(update_id) = update["update_id"].as_i64() {
                                    offset = update_id + 1;
                                }

                                if let Some(message) = update["message"].as_object() {
                                    if let (Some(_chat), Some(from_user), Some(text)) = (
                                        message.get("chat"),
                                        message.get("from"),
                                        message.get("text").and_then(|t| t.as_str()),
                                    ) {
                                        let from_id = from_user["id"].as_i64().unwrap_or(0);
                                        let username =
                                            from_user["username"].as_str().unwrap_or("unknown");
                                        let ext_id = format!("telegram://{from_id}");
                                        let mellowmesh_id =
                                            resolve_identity(&self.client, &ext_id).await;

                                        info!(
                                            "Telegram message received from {} ({}): {}",
                                            username, ext_id, text
                                        );

                                        let fm_msg = Message {
                                            id: format!(
                                                "msg_tg_{}",
                                                message["message_id"].as_i64().unwrap_or(0)
                                            ),
                                            topic: "_forum.general".to_string(),
                                            from: ext_id,
                                            owner: Some(mellowmesh_id),
                                            timestamp: chrono::Utc::now(),
                                            content_type: "text/markdown".to_string(),
                                            body: text.to_string(),
                                            headers: None,
                                            payload: None,
                                            parent_id: None,
                                        };

                                        let _ = self.client.publish(&fm_msg).await;
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(res) => {
                    warn!("Telegram API returned error status: {}", res.status());
                }
                Err(e) => {
                    warn!("Failed to poll Telegram API: {}", e);
                }
            }

            if success {
                current_interval = default_interval;
            } else {
                current_interval = std::cmp::min(current_interval * 2, Duration::from_secs(60));
                info!(
                    "Telegram poll failed. Backing off. Next poll in {:?}",
                    current_interval
                );
            }

            sleep(current_interval).await;
        }
    }

    async fn run_mock(&self) -> anyhow::Result<()> {
        info!("Starting Telegram Connector in Simulation/Mock Mode");
        let messages = [
            (
                "telegram://chat_456/alice",
                "I see a new task on the board. Let's make sure it is assigned.",
            ),
            (
                "telegram://chat_456/alice",
                "Has the security review been claimed yet?",
            ),
            (
                "telegram://chat_456/alice",
                "We should check the payment API decision. Who has approved it?",
            ),
        ];
        let mut idx = 0;
        loop {
            sleep(Duration::from_secs(25)).await;
            let (ext_id, body) = messages[idx % messages.len()];
            let mellowmesh_id = resolve_identity(&self.client, ext_id).await;

            let fm_msg = Message {
                id: format!("msg_telegram_{}", chrono::Utc::now().timestamp()),
                topic: "_forum.general".to_string(),
                from: ext_id.to_string(),
                owner: Some(mellowmesh_id),
                timestamp: chrono::Utc::now(),
                content_type: "text/markdown".to_string(),
                body: body.to_string(),
                headers: None,
                payload: None,
                parent_id: None,
            };

            info!("Simulating Telegram message: {}", body);
            let _ = self.client.publish(&fm_msg).await;
            idx += 1;
        }
    }
}

#[async_trait::async_trait]
impl InterfaceConnector for TelegramConnector {
    async fn run(&self) -> anyhow::Result<()> {
        if let (Some(token), Some(chat_id)) = (&self.token, &self.chat_id) {
            self.run_live(token, chat_id).await
        } else {
            self.run_mock().await
        }
    }
}

// ==========================================
// 3. Teams Connector
// ==========================================
pub struct TeamsConnector {
    client: MellowMeshClient,
    webhook_url: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct TeamsWebhookPayload {
    text: String,
    from: Option<String>,
}

impl TeamsConnector {
    pub fn new(client: MellowMeshClient) -> Self {
        let webhook_url = std::env::var("TEAMS_WEBHOOK_URL").ok();
        Self {
            client,
            webhook_url,
        }
    }

    fn make_receiver_router(&self) -> Router {
        let client = self.client.clone();
        Router::new().route("/webhook", post(move |headers: HeaderMap, body: Bytes| {
            let client = client.clone();
            async move {
                if let Ok(key_b64) = std::env::var("TEAMS_OUTGOING_WEBHOOK_KEY") {
                    let auth_header = match headers.get("Authorization").and_then(|h| h.to_str().ok()) {
                        Some(a) if a.starts_with("HMAC ") => &a[5..],
                        _ => {
                            warn!("Unauthorized Teams webhook request: missing or invalid Authorization header");
                            return Err((StatusCode::UNAUTHORIZED, "Missing or invalid Authorization header".to_string()));
                        }
                    };

                    let decoded_sig = match BASE64.decode(auth_header.trim()) {
                        Ok(sig) => sig,
                        Err(_) => {
                            warn!("Unauthorized Teams webhook request: failed to decode signature");
                            return Err((StatusCode::UNAUTHORIZED, "Invalid signature encoding".to_string()));
                        }
                    };

                    let decoded_key = match BASE64.decode(key_b64.trim()) {
                        Ok(k) => k,
                        Err(_) => {
                            error!("Invalid TEAMS_OUTGOING_WEBHOOK_KEY format: must be valid base64");
                            return Err((StatusCode::INTERNAL_SERVER_ERROR, "Invalid server configuration".to_string()));
                        }
                    };

                    type HmacSha256 = Hmac<Sha256>;
                    let mut mac = match HmacSha256::new_from_slice(&decoded_key) {
                        Ok(m) => m,
                        Err(_) => {
                            error!("Failed to initialize HMAC");
                            return Err((StatusCode::INTERNAL_SERVER_ERROR, "Server error".to_string()));
                        }
                    };

                    mac.update(&body);
                    if mac.verify_slice(&decoded_sig).is_err() {
                        warn!("Unauthorized Teams webhook request: HMAC signature verification failed");
                        return Err((StatusCode::UNAUTHORIZED, "Signature verification failed".to_string()));
                    }
                } else {
                    info!("TEAMS_OUTGOING_WEBHOOK_KEY not set. Skipping signature verification (unauthenticated mode).");
                }

                let payload: TeamsWebhookPayload = match serde_json::from_slice(&body) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("Invalid Teams webhook JSON payload: {}", e);
                        return Err((StatusCode::BAD_REQUEST, "Invalid JSON payload".to_string()));
                    }
                };

                let from_user = payload.from.unwrap_or_else(|| "bob".to_string());
                let ext_id = format!("teams://{from_user}");
                let mellowmesh_id = resolve_identity(&client, &ext_id).await;

                info!("Teams webhook message received from {}: {}", ext_id, payload.text);

                let fm_msg = Message {
                    id: format!("msg_teams_{}", chrono::Utc::now().timestamp()),
                    topic: "_forum.general".to_string(),
                    from: ext_id,
                    owner: Some(mellowmesh_id),
                    timestamp: chrono::Utc::now(),
                    content_type: "text/markdown".to_string(),
                    body: payload.text,
                    headers: None,
                    payload: None,
                    parent_id: None,
                };

                let _ = client.publish(&fm_msg).await;
                Ok((StatusCode::OK, "OK".to_string()))
            }
        }))
    }

    async fn run_live(&self, webhook_url: &str) -> anyhow::Result<()> {
        info!("Starting Live Teams Connector (Webhook Publisher + Webhook Receiver on port 40002)");
        let http_client = reqwest::Client::new();

        // Forward MellowMesh messages to Teams Incoming Webhook
        let client_clone = self.client.clone();
        let webhook_url_str = webhook_url.to_string();
        tokio::spawn(async move {
            if let Ok(mut sub) = client_clone.subscribe("_forum.**").await {
                while let Some(Ok(msg)) = sub.next().await {
                    if msg.from.starts_with("teams://") {
                        continue;
                    }
                    let teams_msg = format!("**{}** (via MellowMesh):\n{}", msg.from, msg.body);
                    let _ = http_client
                        .post(&webhook_url_str)
                        .json(&serde_json::json!({
                            "text": teams_msg
                        }))
                        .send()
                        .await;
                }
            }
        });

        // Start local listener for Teams Outgoing Webhooks (port 40002)
        let app = self.make_receiver_router();
        let addr = SocketAddr::from(([0, 0, 0, 0], 40002));
        info!(
            "Teams Connector Outgoing Webhook receiver listening on: {}",
            addr
        );
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    async fn run_mock(&self) -> anyhow::Result<()> {
        info!("Starting Microsoft Teams Connector in Simulation/Mock Mode");
        let messages = [
            (
                "teams://bob",
                "Please approve the decision for the payment API storage engine.",
            ),
            (
                "teams://bob",
                "We should ensure security compliance by reviewing standard procedures.",
            ),
            ("teams://bob", "I'll be online to approve any decisions."),
        ];
        let mut idx = 0;
        loop {
            sleep(Duration::from_secs(30)).await;
            let (ext_id, body) = messages[idx % messages.len()];
            let mellowmesh_id = resolve_identity(&self.client, ext_id).await;

            let fm_msg = Message {
                id: format!("msg_teams_{}", chrono::Utc::now().timestamp()),
                topic: "_forum.general".to_string(),
                from: ext_id.to_string(),
                owner: Some(mellowmesh_id),
                timestamp: chrono::Utc::now(),
                content_type: "text/markdown".to_string(),
                body: body.to_string(),
                headers: None,
                payload: None,
                parent_id: None,
            };

            info!("Simulating Teams message: {}", body);
            let _ = self.client.publish(&fm_msg).await;
            idx += 1;
        }
    }
}

#[async_trait::async_trait]
impl InterfaceConnector for TeamsConnector {
    async fn run(&self) -> anyhow::Result<()> {
        if let Some(ref webhook_url) = self.webhook_url {
            self.run_live(webhook_url).await
        } else {
            self.run_mock().await
        }
    }
}

// ==========================================
// Connectors Manager
// ==========================================
pub struct ConnectorsManager {
    discord: DiscordConnector,
    telegram: TelegramConnector,
    teams: TeamsConnector,
}

impl ConnectorsManager {
    pub fn new(client: MellowMeshClient) -> Self {
        Self {
            discord: DiscordConnector::new(client.clone()),
            telegram: TelegramConnector::new(client.clone()),
            teams: TeamsConnector::new(client),
        }
    }

    pub fn start(self) {
        info!("Starting Connectors Manager...");
        let discord = Arc::new(self.discord);
        let telegram = Arc::new(self.telegram);
        let teams = Arc::new(self.teams);

        tokio::spawn(async move {
            if let Err(e) = discord.run().await {
                error!("Discord connector failed: {}", e);
            }
        });

        tokio::spawn(async move {
            if let Err(e) = telegram.run().await {
                error!("Telegram connector failed: {}", e);
            }
        });

        tokio::spawn(async move {
            if let Err(e) = teams.run().await {
                error!("Teams connector failed: {}", e);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use hmac::{Hmac, Mac};
    use mellowmesh_client::MellowMeshClient;
    use sha2::Sha256;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_teams_webhook_signature_verification() {
        let key_str = "c2VjcmV0X2tleV9iYXNlNjRfZW5jb2RlZF92YWx1ZV9oZXJlXzEyMzQ1Njc4OTA="; // Valid base64
        let key_decoded = BASE64.decode(key_str).unwrap();

        let payload = r#"{"text":"hello","from":"alice"}"#;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(&key_decoded).unwrap();
        mac.update(payload.as_bytes());
        let correct_sig = mac.finalize().into_bytes();
        let correct_sig_b64 = BASE64.encode(correct_sig);

        // Scenario 1: Key is set in env, valid signature
        std::env::set_var("TEAMS_OUTGOING_WEBHOOK_KEY", key_str);

        let client = MellowMeshClient::new(40019);
        let conn = TeamsConnector::new(client);
        let router = conn.make_receiver_router();

        let req = Request::builder()
            .method("POST")
            .uri("/webhook")
            .header("Authorization", format!("HMAC {correct_sig_b64}"))
            .body(axum::body::Body::from(payload))
            .unwrap();

        let res = router.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Scenario 2: Key is set, invalid signature
        let req_invalid = Request::builder()
            .method("POST")
            .uri("/webhook")
            .header("Authorization", "HMAC invalid_sig")
            .body(axum::body::Body::from(payload))
            .unwrap();

        let res_invalid = router.clone().oneshot(req_invalid).await.unwrap();
        assert_eq!(res_invalid.status(), StatusCode::UNAUTHORIZED);

        // Scenario 3: Key is not set in env (unauthenticated bypass mode)
        std::env::remove_var("TEAMS_OUTGOING_WEBHOOK_KEY");
        let req_bypass = Request::builder()
            .method("POST")
            .uri("/webhook")
            .body(axum::body::Body::from(payload))
            .unwrap();

        let res_bypass = router.clone().oneshot(req_bypass).await.unwrap();
        assert_eq!(res_bypass.status(), StatusCode::OK);
    }
}
