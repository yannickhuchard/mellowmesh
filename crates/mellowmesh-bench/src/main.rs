use chrono::Utc;
use futures_util::StreamExt;
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::message::Message;
use std::sync::Arc;
use tokio::sync::{Barrier, Mutex};
use tokio::time::Instant;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = 40001;
    println!("Starting MellowMesh Benchmarking Suite...");
    println!("Launching daemon on test port {port}...");

    // Auto-start on port 40001
    let _client = MellowMeshClient::connect_with_port(port).await?;

    let num_subscribers = 50;
    let num_publishers = 10;
    let msgs_per_publisher = 100;
    let total_messages = num_publishers * msgs_per_publisher;
    let expected_deliveries = num_subscribers * total_messages;

    println!("Registering subscribers ({num_subscribers} concurrent WebSocket connections)...");

    let start_barrier = Arc::new(Barrier::new(num_publishers + 1));
    let delivery_counter = Arc::new(Mutex::new(0));
    let latencies = Arc::new(Mutex::new(Vec::new()));

    let mut ws_handles = Vec::new();

    // Spawn subscribers
    for i in 0..num_subscribers {
        let client_clone = MellowMeshClient::connect_with_port(port).await?;
        let delivery_counter = delivery_counter.clone();
        let latencies = latencies.clone();

        let handle = tokio::spawn(async move {
            let mut stream = match client_clone.subscribe("_test.**").await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Subscriber {i} failed to connect: {e:?}");
                    return;
                }
            };

            while let Some(msg_res) = stream.next().await {
                if let Ok(m) = msg_res {
                    let now = Utc::now();
                    let latency = now
                        .signed_duration_since(m.timestamp)
                        .num_microseconds()
                        .unwrap_or(0);

                    let mut count = delivery_counter.lock().await;
                    *count += 1;

                    let mut lats = latencies.lock().await;
                    lats.push(latency);

                    if *count >= expected_deliveries {
                        break;
                    }
                }
            }
        });
        ws_handles.push(handle);
    }

    // Wait a brief moment to make sure WebSockets are fully connected
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    println!("Subscribers ready. Spawning {num_publishers} publisher tasks...");

    // Spawn publishers
    let mut pub_handles = Vec::new();
    for p in 0..num_publishers {
        let client_clone = MellowMeshClient::connect_with_port(port).await?;
        let start_barrier = start_barrier.clone();

        let handle = tokio::spawn(async move {
            start_barrier.wait().await; // Synchronize publisher starts
            for i in 0..msgs_per_publisher {
                let msg = Message {
                    id: String::new(),
                    topic: format!("_test.bench.item.{p}"),
                    from: format!("agent://publisher/{p}"),
                    owner: None,
                    timestamp: Utc::now(),
                    content_type: "text/plain".to_string(),
                    body: format!("Bench message {p} - {i}"),
                    headers: None,
                    payload: None,
                    parent_id: None,
                };
                if let Err(e) = client_clone.publish(&msg).await {
                    eprintln!("Publisher {p} failed to publish: {e:?}");
                }
            }
        });
        pub_handles.push(handle);
    }

    println!("Executing stress test...");
    let start_time = Instant::now();
    start_barrier.wait().await; // Trigger publishers

    // Wait for all publishers to finish publishing
    for h in pub_handles {
        h.await?;
    }
    let publish_duration = start_time.elapsed();
    println!("Publishing completed in {publish_duration:?}.");

    // Wait for subscribers to receive all deliveries (with a timeout of 15s)
    let timeout = tokio::time::Duration::from_secs(15);
    let start_delivery_wait = Instant::now();

    while start_delivery_wait.elapsed() < timeout {
        let count = *delivery_counter.lock().await;
        if count >= expected_deliveries {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }

    let total_duration = start_time.elapsed();
    let final_count = *delivery_counter.lock().await;

    println!("Benchmarking finished. Shutting down daemon...");

    // Abort active WS tasks
    for h in ws_handles {
        h.abort();
    }

    // Kill the daemon process by name
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/IM", "mellowmeshd.exe", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .arg("mellowmeshd")
            .status();
    }

    // Compute stats
    let publish_throughput = total_messages as f64 / publish_duration.as_secs_f64();
    let delivery_throughput = final_count as f64 / total_duration.as_secs_f64();

    let lats = latencies.lock().await;
    let avg_latency_us = if !lats.is_empty() {
        let sum: i64 = lats.iter().sum();
        sum as f64 / lats.len() as f64
    } else {
        0.0
    };

    println!("\n=== MellowMesh Stress Test Benchmark Results ===");
    println!("Total Messages Published:   {total_messages}");
    println!("Total Target Deliveries:   {expected_deliveries} (Fan-out: {num_subscribers})");
    println!("Actual Deliveries Received: {final_count}");
    println!("Publish Throughput:        {publish_throughput:.2} msgs/sec");
    println!(
        "Delivery Throughput:       {delivery_throughput:.2} msgs/sec (Fan-out delivery rate)"
    );
    println!(
        "Mean Fan-out Latency:      {:.2} ms ({:.2} µs)",
        avg_latency_us / 1000.0,
        avg_latency_us
    );
    println!("==============================================\n");

    Ok(())
}
