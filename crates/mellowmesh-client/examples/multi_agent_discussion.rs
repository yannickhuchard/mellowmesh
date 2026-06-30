use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::message::Message;
use futures_util::StreamExt;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Connecting to MellowMesh daemon...");
    let client = MellowMeshClient::connect().await?;
    println!("Connected successfully!");

    // 1. Register two agents
    let hermes_id = "agent://yannick/Hermes".to_string();
    let claude_id = "agent://yannick/Claude Cowork".to_string();

    println!("Registering agent: {} (Name: 'Hermes')", hermes_id);
    client
        .register_agent(&AgentRegistration {
            id: hermes_id.clone(),
            name: "Hermes".to_string(),
            owner: "human://yannick".to_string(),
            mode: "autonomous".to_string(),
            capabilities: vec!["research".to_string()],
        })
        .await?;

    println!("Registering agent: {} (Name: 'Claude Cowork')", claude_id);
    client
        .register_agent(&AgentRegistration {
            id: claude_id.clone(),
            name: "Claude Cowork".to_string(),
            owner: "human://yannick".to_string(),
            mode: "autonomous".to_string(),
            capabilities: vec!["drafting".to_string()],
        })
        .await?;

    // 2. Spawn Simulated Agent Tasks in the background
    // Simulated Agent 1: Hermes
    let client_hermes = client.clone();
    tokio::spawn(async move {
        let inbox_topic = "_agent.yannick.Hermes.inbox";
        println!("[System] Starting simulated Hermes agent listener...");
        let mut stream = match client_hermes.subscribe(inbox_topic).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[Hermes] Failed to subscribe to inbox: {}", e);
                return;
            }
        };

        while let Some(Ok(msg)) = stream.next().await {
            println!("\n[Hermes Inbox] Received directed mention message!");
            println!("  Content: \"{}\"", msg.body);
            
            // Wait and respond
            tokio::time::sleep(Duration::from_millis(1000)).await;
            println!("[Hermes] Replying back to the forum...");
            let reply = Message {
                id: String::new(),
                topic: "_forum.general".to_string(),
                from: "agent://yannick/Hermes".to_string(),
                owner: Some("human://yannick".to_string()),
                timestamp: chrono::Utc::now(),
                content_type: "text/plain".to_string(),
                body: "Hi @yannick, I have completed the EV stats research report.".to_string(),
                headers: None,
                payload: None,
                parent_id: Some(msg.id.clone()),
            };
            let _ = client_hermes.publish(&reply).await;
        }
    });

    // Simulated Agent 2: Claude Cowork
    let client_claude = client.clone();
    tokio::spawn(async move {
        let inbox_topic = "_agent.yannick.Claude Cowork.inbox";
        println!("[System] Starting simulated Claude Cowork agent listener...");
        let mut stream = match client_claude.subscribe(inbox_topic).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[Claude Cowork] Failed to subscribe to inbox: {}", e);
                return;
            }
        };

        while let Some(Ok(msg)) = stream.next().await {
            println!("\n[Claude Cowork Inbox] Received directed mention message!");
            println!("  Content: \"{}\"", msg.body);
            
            // Wait and respond
            tokio::time::sleep(Duration::from_millis(1500)).await;
            println!("[Claude Cowork] Replying back to the forum...");
            let reply = Message {
                id: String::new(),
                topic: "_forum.general".to_string(),
                from: "agent://yannick/Claude Cowork".to_string(),
                owner: Some("human://yannick".to_string()),
                timestamp: chrono::Utc::now(),
                content_type: "text/plain".to_string(),
                body: "Hi @yannick, I am working on the blog draft template now.".to_string(),
                headers: None,
                payload: None,
                parent_id: Some(msg.id.clone()),
            };
            let _ = client_claude.publish(&reply).await;
        }
    });

    // 3. Subscribe to the public forum to see the replies
    let forum_topic = "_forum.general";
    println!("\n[Human] Subscribing to public forum topic: '{}'...", forum_topic);
    let mut forum_stream = client.subscribe(forum_topic).await?;

    // 4. Publish a message triggering both agents
    tokio::time::sleep(Duration::from_millis(500)).await;
    let trigger_body = "Hello @Claude Cowork and @Hermes, can you start on the EV blog post?";
    println!(
        "\n[Human] Publishing trigger message to '{}':\n  \"{}\"",
        forum_topic, trigger_body
    );

    let msg = Message {
        id: String::new(),
        topic: forum_topic.to_string(),
        from: "human://yannick".to_string(),
        owner: Some("human://yannick".to_string()),
        timestamp: chrono::Utc::now(),
        content_type: "text/plain".to_string(),
        body: trigger_body.to_string(),
        headers: None,
        payload: None,
        parent_id: None,
    };
    client.publish(&msg).await?;

    // 5. Read replies from the forum (expecting 2 replies from our agents)
    println!("\n[Human] Listening for replies on the forum (waiting up to 10 seconds)...");
    let mut replies_received = 0;
    while let Some(msg_result) = tokio::time::timeout(Duration::from_secs(10), forum_stream.next()).await.ok() {
        if let Some(Ok(recv_msg)) = msg_result {
            // Skip the initial trigger message we sent
            if recv_msg.from == "human://yannick" {
                continue;
            }

            println!("\n[Human] Received reply on forum:");
            println!("  From:    {}", recv_msg.from);
            println!("  Body:    {}", recv_msg.body);
            if let Some(ref pid) = recv_msg.parent_id {
                println!("  In Reply To (parent_id): {}", pid);
            }
            
            replies_received += 1;
            if replies_received == 2 {
                println!("\nSUCCESS! Received replies from both Hermes and Claude Cowork.");
                break;
            }
        } else {
            break;
        }
    }

    if replies_received < 2 {
        eprintln!("Timed out waiting for replies. Received {} replies.", replies_received);
        eprintln!("Ensure the mellowmesh daemon is running locally on port 40000.");
    }

    Ok(())
}
