use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::message::Message;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Connecting to MellowMesh daemon...");
    let client = MellowMeshClient::connect().await?;
    println!("Connected successfully!");

    let topic_pattern = "_artifact.order.processing.**";
    let version = "v1";

    // 1. Register a JSON Schema contract for orders
    let schema_json = r#"{
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "Order",
        "type": "object",
        "properties": {
            "order_id": { "type": "string" },
            "total": { "type": "number", "minimum": 0 },
            "items": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["order_id", "total"]
    }"#;

    println!("Registering schema contract for '{topic_pattern}' (version {version})...");
    client
        .add_schema(topic_pattern, version, schema_json)
        .await?;
    println!("Schema registered successfully.");

    // 2. Publish a valid message
    let parent_msg_id = "msg_order_root_001";
    println!("\nPublishing valid order message...");
    let valid_msg = Message {
        id: parent_msg_id.to_string(),
        topic: "_artifact.order.processing".to_string(),
        from: "agent://order-service".to_string(),
        owner: None,
        timestamp: chrono::Utc::now(),
        content_type: "application/json".to_string(),
        body: r#"{"order_id": "order_1001", "total": 49.99, "items": ["book", "pen"]}"#.to_string(),
        headers: None,
        payload: None,
        parent_id: None,
    };
    client.publish(&valid_msg).await?;
    println!("Valid order message published successfully.");

    // 3. Publish an invalid message (missing 'total')
    println!("\nPublishing invalid order (missing required field 'total')...");
    let invalid_msg = Message {
        id: "msg_order_invalid_002".to_string(),
        topic: "_artifact.order.processing".to_string(),
        from: "agent://order-service".to_string(),
        owner: None,
        timestamp: chrono::Utc::now(),
        content_type: "application/json".to_string(),
        body: r#"{"order_id": "order_1002"}"#.to_string(), // Missing total
        headers: None,
        payload: None,
        parent_id: None,
    };

    match client.publish(&invalid_msg).await {
        Ok(_) => println!("WARNING: Invalid message was accepted (unexpected)!"),
        Err(e) => println!("Correctly rejected invalid message. Error: {e}"),
    }

    // 4. Publish a child message referencing the parent order to demonstrate lineage routing
    println!("\nPublishing a child message (billing invoice) linked to parent order (lineage-aware routing)...");
    let child_msg = Message {
        id: "msg_invoice_child_001".to_string(),
        topic: "_artifact.order.invoice".to_string(),
        from: "agent://billing-service".to_string(),
        owner: None,
        timestamp: chrono::Utc::now(),
        content_type: "application/json".to_string(),
        body: r#"{"invoice_id": "inv_1001", "status": "paid"}"#.to_string(),
        headers: None,
        payload: None,
        parent_id: Some(parent_msg_id.to_string()), // Links to parent
    };
    client.publish(&child_msg).await?;
    println!("Child invoice message published successfully.");

    // 5. Query context and trace lineage back to parent
    println!("\nQuerying context for '_artifact.order.invoice' to trace parent message lineage...");
    let context = client
        .get_context("_artifact.order.invoice", Some(5))
        .await?;

    println!("Relevant messages in topic:");
    for m in &context.relevant_messages {
        println!("  - ID: {}, Sender: {}, Body: {}", m.id, m.from, m.body);
    }

    if let Some(lineage) = &context.lineage {
        println!("\nLineage (transitive parents resolved recursively):");
        for m in lineage {
            println!(
                "  <- ID: {}, Sender: {}, Topic: {}, Body: {}",
                m.id, m.from, m.topic, m.body
            );
        }
    } else {
        println!("No lineage resolved.");
    }

    // Clean up schema
    client.remove_schema(topic_pattern, version).await?;
    println!("\nTest schema contract cleaned up.");

    Ok(())
}
