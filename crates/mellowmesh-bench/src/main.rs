use chrono::Utc;
use futures_util::StreamExt;
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::message::Message;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Barrier;
use tokio::time::Instant;

const BENCH_PORT: u16 = 40001;

/// A daemon spawned exclusively for this benchmark run, backed by a throwaway
/// database in a private temporary directory. Dropping it terminates exactly
/// the process it started (never any other `mellowmeshd`) and removes the
/// temporary directory.
struct BenchDaemon {
    child: Option<Child>,
    data_dir: PathBuf,
}

impl BenchDaemon {
    fn spawn(port: u16) -> anyhow::Result<Self> {
        if is_port_open(port) {
            anyhow::bail!(
                "Port {port} is already in use; refusing to benchmark against a daemon \
                 this run did not start"
            );
        }

        let data_dir = std::env::temp_dir().join(format!(
            "mellowmesh-bench-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&data_dir)?;
        let db_path = data_dir.join("bench.db");

        let child = Command::new(daemon_binary())
            .arg("--port")
            .arg(port.to_string())
            .arg("--db")
            .arg(&db_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&data_dir);
                return Err(anyhow::anyhow!("Failed to launch mellowmeshd: {e}"));
            }
        };

        let mut daemon = BenchDaemon {
            child: Some(child),
            data_dir,
        };
        daemon.wait_ready(port)?;
        Ok(daemon)
    }

    fn wait_ready(&mut self, port: u16) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if let Some(child) = self.child.as_mut() {
                if let Some(status) = child.try_wait()? {
                    anyhow::bail!("mellowmeshd exited early with {status}");
                }
            }
            if is_port_open(port) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        anyhow::bail!("Timeout waiting for mellowmeshd to start on port {port}")
    }

    /// The owner token the daemon bootstraps next to its database on first run.
    fn owner_token(&self) -> Option<String> {
        read_token(&self.data_dir.join("owner.token"))
    }

    fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

impl Drop for BenchDaemon {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn read_token(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

fn is_port_open(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

/// Prefer the `mellowmeshd` built next to this binary; fall back to `PATH`.
fn daemon_binary() -> PathBuf {
    let name = if cfg!(windows) {
        "mellowmeshd.exe"
    } else {
        "mellowmeshd"
    };
    if let Ok(mut exe) = std::env::current_exe() {
        exe.pop();
        let local = exe.join(name);
        if local.exists() {
            return local;
        }
    }
    PathBuf::from(name)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = BENCH_PORT;
    println!("Starting MellowMesh Benchmarking Suite...");
    println!("Launching isolated daemon on test port {port}...");

    let mut daemon = BenchDaemon::spawn(port)?;

    // Talk to the benchmark daemon directly (ignoring any remote-hub env
    // configuration) and authenticate as its freshly bootstrapped owner.
    let mut client = MellowMeshClient::loopback(port);
    if let Some(token) = daemon.owner_token() {
        client = client.with_token(token);
    }

    let num_subscribers = 50;
    let num_publishers = 10;
    let msgs_per_publisher = 100;
    let total_messages = num_publishers * msgs_per_publisher;
    let expected_deliveries = num_subscribers * total_messages;

    println!("Registering subscribers ({num_subscribers} concurrent WebSocket connections)...");

    let start_barrier = Arc::new(Barrier::new(num_publishers + 1));
    let delivery_counter = Arc::new(AtomicUsize::new(0));
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);

    let mut ws_handles = Vec::new();

    // Spawn subscribers. Each keeps its own latency samples; they are merged
    // after the run so the hot path takes no shared lock.
    for i in 0..num_subscribers {
        let client = client.clone();
        let delivery_counter = delivery_counter.clone();
        let mut stop_rx = stop_rx.clone();

        let handle = tokio::spawn(async move {
            let mut latencies: Vec<i64> = Vec::with_capacity(total_messages);
            let mut stream = match client.subscribe("_test.**").await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Subscriber {i} failed to connect: {e:?}");
                    return latencies;
                }
            };

            while latencies.len() < total_messages {
                let msg_res = tokio::select! {
                    next = stream.next() => match next {
                        Some(r) => r,
                        None => break,
                    },
                    _ = stop_rx.changed() => break,
                };
                if let Ok(m) = msg_res {
                    let latency = Utc::now()
                        .signed_duration_since(m.timestamp)
                        .num_microseconds()
                        .unwrap_or(0);
                    latencies.push(latency);
                    delivery_counter.fetch_add(1, Ordering::Relaxed);
                }
            }
            latencies
        });
        ws_handles.push(handle);
    }

    // Wait a brief moment to make sure WebSockets are fully connected
    tokio::time::sleep(Duration::from_millis(1000)).await;
    println!("Subscribers ready. Spawning {num_publishers} publisher tasks...");

    // Spawn publishers
    let mut pub_handles = Vec::new();
    for p in 0..num_publishers {
        let client = client.clone();
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
                if let Err(e) = client.publish(&msg).await {
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
    let timeout = Duration::from_secs(15);
    let start_delivery_wait = Instant::now();

    while start_delivery_wait.elapsed() < timeout {
        if delivery_counter.load(Ordering::Relaxed) >= expected_deliveries {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let total_duration = start_time.elapsed();
    let final_count = delivery_counter.load(Ordering::Relaxed);

    println!("Benchmarking finished. Shutting down daemon...");

    // Stop subscribers that are still waiting and merge their samples.
    let _ = stop_tx.send(true);
    let mut lats: Vec<i64> = Vec::with_capacity(final_count);
    for h in ws_handles {
        if let Ok(samples) = h.await {
            lats.extend(samples);
        }
    }

    // Terminate only the daemon this run spawned and remove its database.
    daemon.shutdown();

    // Compute stats
    let publish_throughput = total_messages as f64 / publish_duration.as_secs_f64();
    let delivery_throughput = final_count as f64 / total_duration.as_secs_f64();

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
