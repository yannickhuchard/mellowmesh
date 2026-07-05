use futures_util::StreamExt;
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::message::Message;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Connecting to MellowMesh daemon...");

    // Connects to the local MellowMesh daemon on port 40000 (spawns it if not already running)
    let client = MellowMeshClient::connect().await?;
    println!("Connected successfully!");

    // 1. Subscribe to a case-insensitive pattern with NATS-style multi-level '>' wildcard
    // Pattern "NEWS.>" matches any topic starting with "news." (case-insensitive) followed by one or more levels.
    // For example: "news.french.technology", "NEWS.German.Art", etc.
    let pattern = "NEWS.>";
    println!("Subscribing to pattern: '{pattern}' (case-insensitive = true)...");

    let mut stream = client.subscribe_with_options(pattern, true).await?;

    // Spawn a publisher task that sends test messages
    let client_clone = client.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Publish message 1: Lowercase with spaces and emojis (Unicode topic)
        println!("[Publisher] Sending message 1 on 'news.french.technology 🙂'...");
        let msg1 = Message {
            id: "msg_french_001".to_string(),
            topic: "news.french.technology 🙂".to_string(),
            from: "agent://news-crawler".to_string(),
            owner: None,
            timestamp: chrono::Utc::now(),
            content_type: "text/plain".to_string(),
            body: "MellowMesh now supports spaces and emojis in topics!".to_string(),
            headers: None,
            payload: None,
            parent_id: None,
        };
        if let Err(e) = client_clone.publish(&msg1).await {
            eprintln!("[Publisher] Failed to publish message 1: {e}");
        }

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Publish message 2: Uppercase/mixed case topic in German (Unicode topic)
        println!("[Publisher] Sending message 2 on 'NEWS.German.Art'...");
        let msg2 = Message {
            id: "msg_german_002".to_string(),
            topic: "NEWS.German.Art".to_string(),
            from: "agent://news-crawler".to_string(),
            owner: None,
            timestamp: chrono::Utc::now(),
            content_type: "text/plain".to_string(),
            body:
                "MellowMesh now supports full Unicode case folding and upper case topic validation!"
                    .to_string(),
            headers: None,
            payload: None,
            parent_id: None,
        };
        if let Err(e) = client_clone.publish(&msg2).await {
            eprintln!("[Publisher] Failed to publish message 2: {e}");
        }

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Publish message 3: Topic that DOES NOT match prefix (should not be received)
        println!("[Publisher] Sending message 3 on 'sports.football'...");
        let msg3 = Message {
            id: "msg_sports_003".to_string(),
            topic: "sports.football".to_string(),
            from: "agent://news-crawler".to_string(),
            owner: None,
            timestamp: chrono::Utc::now(),
            content_type: "text/plain".to_string(),
            body: "This message should not be received by the subscriber.".to_string(),
            headers: None,
            payload: None,
            parent_id: None,
        };
        if let Err(e) = client_clone.publish(&msg3).await {
            eprintln!("[Publisher] Failed to publish message 3: {e}");
        }
    });

    println!("Listening for messages (waiting up to 3 seconds)...");

    // We expect to receive exactly 2 messages
    let mut count = 0;
    while let Ok(msg_result) = tokio::time::timeout(Duration::from_secs(3), stream.next()).await {
        if let Some(Ok(msg)) = msg_result {
            println!("\n[Subscriber] Received message {}:", count + 1);
            println!("  ID:        {}", msg.id);
            println!("  Topic:     {}", msg.topic);
            println!("  Sender:    {}", msg.from);
            println!("  Body:      {}", msg.body);
            count += 1;
            if count == 2 {
                println!("\nAll matching messages received successfully!");
                break;
            }
        } else {
            break;
        }
    }

    if count < 2 {
        println!("\nSubscriber timed out. Received {count} messages.");
    }

    Ok(())
}
