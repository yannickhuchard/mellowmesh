use futures_util::StreamExt;
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::message::Message;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("============================================================");
    println!("     MellowMesh Multi-Agent Core Insurance System Simulation");
    println!("============================================================");
    println!("Connecting to MellowMesh daemon...");
    let client = MellowMeshClient::connect().await?;
    println!("Connected successfully!");

    // 1. Register four agents of different kinds
    let architect_id = "agent://yannick/Architect".to_string();
    let configurator_id = "agent://yannick/ProductConfigurator".to_string();
    let developer_id = "agent://yannick/Developer".to_string();
    let qa_id = "agent://yannick/QA".to_string();

    println!("\n[System] Registering agents on the fabric...");
    let agents = vec![
        (architect_id.clone(), "Architect", vec!["architecture", "schema"]),
        (configurator_id.clone(), "ProductConfigurator", vec!["product-rules", "premium-calc"]),
        (developer_id.clone(), "Developer", vec!["rust-development", "api-generation"]),
        (qa_id.clone(), "QA", vec!["integration-tests", "premium-verification"]),
    ];

    for (id, name, caps) in &agents {
        println!("  -> Registering {} ({})", name, id);
        client
            .register_agent(&AgentRegistration {
                id: id.clone(),
                name: name.to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: caps.iter().map(|s| s.to_string()).collect(),
            })
            .await?;
    }

    let forum_topic = "_forum.insurance";

    // 2. Spawn Simulated Agent Listeners in the background
    
    // Architect Agent
    let client_arch = client.clone();
    let arch_id_clone = architect_id.clone();
    tokio::spawn(async move {
        let inbox_topic = "_agent.yannick.Architect.inbox";
        let mut stream = client_arch.subscribe(inbox_topic).await.unwrap();
        while let Some(Ok(msg)) = stream.next().await {
            // Ignore messages sent by itself
            if msg.from == arch_id_clone {
                continue;
            }
            println!("\n[Architect 🧠] Received mention message from {}", msg.from);
            tokio::time::sleep(Duration::from_secs(3)).await;
            
            let reply_body = "I have drafted the Core Insurance DB Schema. We will use a PostgreSQL database with partitioned tables.\n\nCore tables:\n1. policies (id, policy_number, status, coverage_type, premium_cents, created_at)\n2. claims (id, policy_id, status, amount_cents, loss_date, reported_at)\n3. policyholders (id, name, date_of_birth, risk_score)\n\nWe will expose REST APIs `/api/v1/policies` and `/api/v1/claims`. @ProductConfigurator, please define the premium rules and product configurations.";
            println!("[Architect 🧠] Publishing system architecture & database schema...");
            let reply = Message {
                id: String::new(),
                topic: forum_topic.to_string(),
                from: "agent://yannick/Architect".to_string(),
                owner: Some("human://yannick".to_string()),
                timestamp: chrono::Utc::now(),
                content_type: "text/plain".to_string(),
                body: reply_body.to_string(),
                headers: None,
                payload: None,
                parent_id: Some(msg.id.clone()),
            };
            let _ = client_arch.publish(&reply).await;
        }
    });

    // ProductConfigurator Agent
    let client_conf = client.clone();
    let conf_id_clone = configurator_id.clone();
    tokio::spawn(async move {
        let inbox_topic = "_agent.yannick.ProductConfigurator.inbox";
        let mut stream = client_conf.subscribe(inbox_topic).await.unwrap();
        while let Some(Ok(msg)) = stream.next().await {
            if msg.from == conf_id_clone {
                continue;
            }
            println!("\n[ProductConfigurator ⚙️] Received mention message from {}", msg.from);
            tokio::time::sleep(Duration::from_secs(3)).await;

            let reply_body = "Got the schema. I've defined the product configuration rules for auto & home insurance.\n\nRules:\n- Base Annual Premium: Auto ($500), Home ($1000)\n- Premium Multipliers:\n  * Auto: risk_score >= 8 (1.5x), age < 25 (1.3x)\n  * Home: wildfire_risk = High (2.0x), age_of_roof > 15 years (1.25x)\n\n@Developer, please implement the database structures, premium calculation engine, and policy creation API endpoints in Rust.";
            println!("[ProductConfigurator ⚙️] Publishing product rules & premium multipliers...");
            let reply = Message {
                id: String::new(),
                topic: forum_topic.to_string(),
                from: "agent://yannick/ProductConfigurator".to_string(),
                owner: Some("human://yannick".to_string()),
                timestamp: chrono::Utc::now(),
                content_type: "text/plain".to_string(),
                body: reply_body.to_string(),
                headers: None,
                payload: None,
                parent_id: Some(msg.id.clone()),
            };
            let _ = client_conf.publish(&reply).await;
        }
    });

    // Developer Agent
    let client_dev = client.clone();
    let dev_id_clone = developer_id.clone();
    tokio::spawn(async move {
        let inbox_topic = "_agent.yannick.Developer.inbox";
        let mut stream = client_dev.subscribe(inbox_topic).await.unwrap();
        while let Some(Ok(msg)) = stream.next().await {
            if msg.from == dev_id_clone {
                continue;
            }
            println!("\n[Developer 💻] Received mention message from {}", msg.from);
            tokio::time::sleep(Duration::from_secs(3)).await;

            let reply_body = "Backend implementation is complete! I created the Rust models using SQLx, implemented the premium calculation engine adhering to the ProductConfigurator multipliers, and added `/api/v1/policies/create` endpoint.\n\nHere is the premium calculation core code:\n```rust\nfn calculate_premium(base: u64, risk_score: u8) -> u64 {\n    let mut factor = 1.0;\n    if risk_score >= 8 { factor *= 1.5; }\n    (base as f64 * factor) as u64\n}\n```\n@QA, please run sanity checks, test the policy creation endpoint, and verify premium calculations.";
            println!("[Developer 💻] Publishing Rust backend implementation and premium code...");
            let reply = Message {
                id: String::new(),
                topic: forum_topic.to_string(),
                from: "agent://yannick/Developer".to_string(),
                owner: Some("human://yannick".to_string()),
                timestamp: chrono::Utc::now(),
                content_type: "text/plain".to_string(),
                body: reply_body.to_string(),
                headers: None,
                payload: None,
                parent_id: Some(msg.id.clone()),
            };
            let _ = client_dev.publish(&reply).await;
        }
    });

    // QA Agent
    let client_qa = client.clone();
    let qa_id_clone = qa_id.clone();
    tokio::spawn(async move {
        let inbox_topic = "_agent.yannick.QA.inbox";
        let mut stream = client_qa.subscribe(inbox_topic).await.unwrap();
        while let Some(Ok(msg)) = stream.next().await {
            if msg.from == qa_id_clone {
                continue;
            }
            println!("\n[QA 🧪] Received mention message from {}", msg.from);
            tokio::time::sleep(Duration::from_secs(3)).await;

            let reply_body = "All integration test suites passed! Verified policy creation API: Base Auto premium $500 with risk score 8 correctly computed as $750. Verified claims table constraint validations. Code coverage is at 94%.\n\n@yannick, the core insurance system backend is fully verified and ready for deployment!";
            println!("[QA 🧪] Publishing test report & verification confirmation...");
            let reply = Message {
                id: String::new(),
                topic: forum_topic.to_string(),
                from: "agent://yannick/QA".to_string(),
                owner: Some("human://yannick".to_string()),
                timestamp: chrono::Utc::now(),
                content_type: "text/plain".to_string(),
                body: reply_body.to_string(),
                headers: None,
                payload: None,
                parent_id: Some(msg.id.clone()),
            };
            let _ = client_qa.publish(&reply).await;
        }
    });

    // 3. Subscribe to the public forum to see the replies in real-time
    println!("\n[Human] Subscribing to public forum: '{}'...", forum_topic);
    let mut forum_stream = client.subscribe(forum_topic).await?;

    // 4. Publish the initial trigger message from the human
    tokio::time::sleep(Duration::from_millis(500)).await;
    let trigger_body = "Hello team! Let's design and build the backend for our new Core Insurance System (Policy & Claims modules). @Architect, please start by proposing the architecture and database schema.";
    println!("\n[Human 👤] Publishing trigger message to '{}':\n  \"{}\"", forum_topic, trigger_body);

    let init_msg = Message {
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
    client.publish(&init_msg).await?;

    // 5. Read replies from the forum (expecting 4 replies: Architect -> ProductConfigurator -> Developer -> QA)
    println!("\n[Human 👤] Listening for the exchange (waiting up to 45 seconds)...");
    let mut replies_received = 0;
    while let Ok(msg_result) = tokio::time::timeout(Duration::from_secs(45), forum_stream.next()).await {
        if let Some(Ok(recv_msg)) = msg_result {
            // Skip the initial trigger message sent by the human
            if recv_msg.from == "human://yannick" {
                continue;
            }

            println!("\n------------------------------------------------------------");
            println!("📢 MESSAGE ON FORUM: {}", recv_msg.topic);
            println!("👤 FROM:             {}", recv_msg.from);
            println!("💬 BODY:\n{}", recv_msg.body);
            println!("------------------------------------------------------------");

            replies_received += 1;
            if replies_received == 4 {
                println!("\nSUCCESS! The multi-agent exchange simulation has completed successfully.");
                println!("All 4 agents successfully coordinated to design, specify, implement, and verify the backend.");
                break;
            }
        } else {
            break;
        }
    }

    if replies_received < 4 {
        eprintln!("\nTimed out waiting for the full exchange. Received only {} replies.", replies_received);
    }

    Ok(())
}
