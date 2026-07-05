use chrono::Utc;
use futures_util::StreamExt;
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::decision::{Decision, DecisionOption};
use mellowmesh_core::message::Message;
use mellowmesh_core::task::Task;
use std::collections::HashMap;

fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        println!("No data available.");
        return;
    }

    let mut widths = vec![0; headers.len()];
    for (i, h) in headers.iter().enumerate() {
        widths[i] = h.len();
    }
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() && val.len() > widths[i] {
                widths[i] = val.len();
            }
        }
    }

    let print_border = || {
        for w in &widths {
            print!("+-{}-", "-".repeat(*w));
        }
        println!("+");
    };

    print_border();
    for (i, h) in headers.iter().enumerate() {
        print!("| {:<width$} ", h, width = widths[i]);
    }
    println!("|");
    print_border();

    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() {
                print!("| {:<width$} ", val, width = widths[i]);
            }
        }
        println!("|");
    }
    print_border();
}

pub async fn run_daemon_start(port: u16) -> anyhow::Result<()> {
    println!("Starting MellowMesh daemon (mellowmeshd) on port {port}...");
    mellowmesh_client::autostart::spawn_daemon(port)?;
    println!("Daemon started successfully.");
    Ok(())
}

pub async fn run_daemon_stop(port: u16, force: bool) -> anyhow::Result<()> {
    if !mellowmesh_client::autostart::is_daemon_running(port) {
        println!("MellowMesh daemon is not running on port {port}.");
        return Ok(());
    }

    if force {
        println!("Force option specified. Immediately killing mellowmeshd process...");
        force_kill_daemon();
        return Ok(());
    }

    println!("Stopping MellowMesh daemon on port {port}...");

    let client = MellowMeshClient::new(port);
    match client.shutdown_daemon().await {
        Ok(_) => {
            println!("Shutdown request sent successfully.");
        }
        Err(e) => {
            eprintln!("Failed to send shutdown request to daemon: {e}. Attempting force kill...");
            force_kill_daemon();
            return Ok(());
        }
    }

    // Wait for the daemon to stop
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(3);
    let mut stopped = false;
    while start.elapsed() < timeout {
        if !mellowmesh_client::autostart::is_daemon_running(port) {
            stopped = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    if stopped {
        println!("Daemon stopped successfully.");
    } else {
        println!("Daemon did not stop in time. Attempting force kill...");
        force_kill_daemon();
    }

    Ok(())
}

fn force_kill_daemon() {
    #[cfg(windows)]
    {
        println!("Killing mellowmeshd.exe process...");
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/F", "/IM", "mellowmeshd.exe"]);
        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    println!("Daemon process killed successfully.");
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    if err.contains("not found") || err.contains("could not be found") {
                        println!("Daemon process is already stopped.");
                    } else {
                        eprintln!("Failed to kill daemon process: {}", err.trim());
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to execute taskkill command: {e}");
            }
        }
    }

    #[cfg(not(windows))]
    {
        println!("Killing mellowmeshd process...");
        let mut cmd = std::process::Command::new("pkill");
        cmd.args(&["-f", "mellowmeshd"]);
        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    println!("Daemon process killed successfully.");
                } else {
                    // Try killall as fallback
                    let mut cmd_fallback = std::process::Command::new("killall");
                    cmd_fallback.arg("mellowmeshd");
                    if let Ok(out) = cmd_fallback.output() {
                        if out.status.success() {
                            println!("Daemon process killed successfully.");
                            return;
                        }
                    }
                    eprintln!("Failed to kill daemon process. It may already be stopped.");
                }
            }
            Err(e) => {
                eprintln!("Failed to execute kill command: {}", e);
            }
        }
    }
}

pub async fn run_daemon_restart(port: u16) -> anyhow::Result<()> {
    println!("Restarting MellowMesh daemon on port {port}...");
    // Graceful stop
    run_daemon_stop(port, false).await?;
    // Short wait to ensure port is freed
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    // Start again
    run_daemon_start(port).await
}

pub async fn run_daemon_clean(port: u16) -> anyhow::Result<()> {
    // 1. Stop the daemon first (gracefully)
    run_daemon_stop(port, false).await?;

    // 2. Locate the database files
    let db_path = mellowmesh_store::sqlite::default_db_path();
    println!("Deleting local database at: {db_path:?}");

    if db_path.exists() {
        match std::fs::remove_file(&db_path) {
            Ok(_) => println!("Deleted database file: {db_path:?}"),
            Err(e) => eprintln!("Failed to delete database file {db_path:?}: {e}"),
        }
    } else {
        println!("Database file does not exist.");
    }

    // Also delete SQLite sidecar files (WAL/SHM) if they exist
    let wal_path = db_path.with_extension("db-wal");
    if wal_path.exists() {
        if let Err(e) = std::fs::remove_file(&wal_path) {
            eprintln!("Failed to delete WAL file {wal_path:?}: {e}");
        } else {
            println!("Deleted WAL file: {wal_path:?}");
        }
    }

    let shm_path = db_path.with_extension("db-shm");
    if shm_path.exists() {
        if let Err(e) = std::fs::remove_file(&shm_path) {
            eprintln!("Failed to delete SHM file {shm_path:?}: {e}");
        } else {
            println!("Deleted SHM file: {shm_path:?}");
        }
    }

    println!("Database cleanup completed.");
    Ok(())
}

pub async fn run_status(port: u16) -> anyhow::Result<()> {
    let running = mellowmesh_client::autostart::is_daemon_running(port);
    let db_path = mellowmesh_store::sqlite::default_db_path();

    println!("MellowMesh Daemon Status:");
    println!("  Running:       {}", if running { "YES" } else { "NO" });
    println!("  Default Port:  {port}");
    println!("  Database Path: {db_path:?}");
    Ok(())
}

pub async fn run_topics(client: &MellowMeshClient) -> anyhow::Result<()> {
    let topics = client.list_topics().await?;
    println!("Topics:");
    for t in topics {
        println!("  - {t}");
    }
    Ok(())
}

pub async fn run_publish(
    client: &MellowMeshClient,
    topic: String,
    body: String,
) -> anyhow::Result<()> {
    let msg = Message {
        id: String::new(), // Auto-generated by server
        topic,
        from: "human://cli".to_string(),
        owner: Some("human://cli".to_string()),
        timestamp: Utc::now(),
        content_type: "text/plain".to_string(),
        body,
        headers: None,
        payload: None,
        parent_id: None,
    };
    client.publish(&msg).await?;
    println!("Message published successfully.");
    Ok(())
}

pub async fn run_read(
    client: &MellowMeshClient,
    topic: String,
    limit: usize,
) -> anyhow::Result<()> {
    let history = client.get_history(limit).await?;
    let filtered: Vec<Message> = history
        .into_iter()
        .filter(|m| mellowmesh_core::topic::match_topic(&topic, &m.topic))
        .collect();

    if filtered.is_empty() {
        println!("No messages found matching topic pattern '{topic}'.");
        return Ok(());
    }

    for m in filtered {
        println!(
            "[{}] {} | from: {}",
            m.timestamp
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S"),
            m.topic,
            m.from
        );
        println!("  {}", m.body);
        println!("{}", "-".repeat(60));
    }
    Ok(())
}

pub async fn run_tail(client: &MellowMeshClient, pattern: String) -> anyhow::Result<()> {
    println!("Tailing topic pattern '{pattern}'. Press Ctrl+C to stop...");
    let mut stream = client.subscribe(&pattern).await?;
    while let Some(msg_res) = stream.next().await {
        match msg_res {
            Ok(m) => {
                println!(
                    "[{}] {} | from: {}",
                    m.timestamp
                        .with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M:%S"),
                    m.topic,
                    m.from
                );
                println!("  {}", m.body);
                println!("{}", "-".repeat(60));
            }
            Err(e) => {
                eprintln!("Error receiving message: {e:?}");
                break;
            }
        }
    }
    Ok(())
}

pub async fn run_agent_register(
    client: &MellowMeshClient,
    id: String,
    owner: String,
    mode: String,
    capabilities: Vec<String>,
) -> anyhow::Result<()> {
    // Determine name from ID
    let name = id.split('/').next_back().unwrap_or(&id).to_string();
    let reg = AgentRegistration {
        id: if id.starts_with("agent://") {
            id
        } else {
            format!("agent://{id}")
        },
        name,
        owner,
        mode,
        capabilities,
    };
    client.register_agent(&reg).await?;
    println!("Agent registered successfully.");
    Ok(())
}

pub async fn run_agents(client: &MellowMeshClient) -> anyhow::Result<()> {
    let agents = client.list_agents().await?;
    let mut rows = Vec::new();
    for a in agents {
        rows.push(vec![
            a.id,
            a.name,
            a.owner,
            a.mode,
            a.capabilities.join(", "),
        ]);
    }
    print_table(
        &["Agent ID", "Name", "Owner", "Mode", "Capabilities"],
        &rows,
    );
    Ok(())
}

pub async fn run_named_topic_register(
    client: &MellowMeshClient,
    name: String,
    topic: String,
) -> anyhow::Result<()> {
    client.register_named_topic(&name, &topic).await?;
    println!("Named topic registered successfully.");
    Ok(())
}

pub async fn run_named_topic_list(client: &MellowMeshClient) -> anyhow::Result<()> {
    let topics = client.list_named_topics().await?;
    let mut rows = Vec::new();
    for t in topics {
        rows.push(vec![t.name, t.topic]);
    }
    print_table(&["Short Name", "Target Topic Path"], &rows);
    Ok(())
}

pub async fn run_named_topic_remove(client: &MellowMeshClient, name: &str) -> anyhow::Result<()> {
    client.remove_named_topic(name).await?;
    println!("Named topic removed successfully.");
    Ok(())
}

pub async fn run_task_create(
    client: &MellowMeshClient,
    title: String,
    topics: Vec<String>,
    description: Option<String>,
    capabilities: Vec<String>,
    priority: String,
    created_by: Option<String>,
) -> anyhow::Result<()> {
    let creator = created_by.unwrap_or_else(|| "human://cli".to_string());
    let task = Task {
        id: String::new(), // server generated
        title,
        description,
        created_from: None,
        created_by: creator,
        status: "open".to_string(),
        priority,
        topics,
        required_capabilities: capabilities,
        assigned_to: None,
        claimed_by: None,
        deadline: None,
        artifacts: vec![],
        decisions: vec![],
        parent_id: None,
        lease_seconds: None,
        claim_expires_at: None,
    };
    client.create_task(&task).await?;
    println!("Task created successfully.");
    Ok(())
}

pub async fn run_tasks(client: &MellowMeshClient) -> anyhow::Result<()> {
    let tasks = client.list_tasks().await?;
    let mut rows = Vec::new();
    for t in tasks {
        rows.push(vec![
            t.id,
            t.title,
            t.status,
            t.priority,
            t.claimed_by.unwrap_or_else(|| "-".to_string()),
            t.claim_expires_at.unwrap_or_else(|| "-".to_string()),
        ]);
    }
    print_table(
        &[
            "Task ID",
            "Title",
            "Status",
            "Priority",
            "Claimed By",
            "Lease Expires",
        ],
        &rows,
    );
    Ok(())
}

pub async fn run_claim(
    client: &MellowMeshClient,
    task_id: &str,
    agent_id: &str,
    lease_seconds: Option<u64>,
) -> anyhow::Result<()> {
    let full_agent_id = if agent_id.starts_with("agent://") {
        agent_id.to_string()
    } else {
        format!("agent://{agent_id}")
    };
    client
        .claim_task_with_lease(task_id, &full_agent_id, lease_seconds)
        .await?;
    println!("Task {task_id} claimed by {full_agent_id}.");
    Ok(())
}

pub async fn run_complete(client: &MellowMeshClient, task_id: &str) -> anyhow::Result<()> {
    client.complete_task(task_id).await?;
    println!("Task {task_id} marked as completed.");
    Ok(())
}

pub async fn run_decision_create(
    client: &MellowMeshClient,
    title: String,
    question: String,
    created_by: String,
    decider: String,
    options: Vec<String>,
) -> anyhow::Result<()> {
    let dec_options = options
        .into_iter()
        .enumerate()
        .map(|(i, label)| DecisionOption {
            id: format!("option_{}", i + 1),
            label,
            pros: vec![],
            cons: vec![],
        })
        .collect();

    let decision = Decision {
        id: String::new(),
        title,
        question,
        created_by,
        required_decider: decider,
        status: "requested".to_string(),
        options: dec_options,
        response_option_id: None,
        response_timestamp: None,
    };
    client.create_decision(&decision).await?;
    println!("Decision request created successfully.");
    Ok(())
}

pub async fn run_decisions(client: &MellowMeshClient) -> anyhow::Result<()> {
    let decisions = client.list_decisions().await?;
    let mut rows = Vec::new();
    for d in decisions {
        rows.push(vec![
            d.id,
            d.title,
            d.required_decider,
            d.status,
            d.response_option_id.unwrap_or_else(|| "-".to_string()),
        ]);
    }
    print_table(
        &[
            "Decision ID",
            "Title",
            "Required Decider",
            "Status",
            "Response Option",
        ],
        &rows,
    );
    Ok(())
}

pub async fn run_respond(
    client: &MellowMeshClient,
    decision_id: &str,
    option_id: &str,
) -> anyhow::Result<()> {
    client.respond_decision(decision_id, option_id).await?;
    println!("Decision {decision_id} answered with option {option_id}.");
    Ok(())
}

pub async fn run_forum(client: &MellowMeshClient, pattern: Option<String>) -> anyhow::Result<()> {
    let history = client.get_forum(pattern.clone()).await?;
    let pat = pattern.unwrap_or_else(|| "**".to_string());

    // Group messages by topic
    let mut grouped: HashMap<String, Vec<Message>> = HashMap::new();
    for m in history {
        grouped.entry(m.topic.clone()).or_default().push(m);
    }

    if grouped.is_empty() {
        println!("No forum messages found matching pattern '{pat}'.");
        return Ok(());
    }

    // Sort topics
    let mut topics: Vec<String> = grouped.keys().cloned().collect();
    topics.sort();

    for topic in topics {
        println!("============================================================");
        println!("TOPIC: {topic}");
        println!("============================================================");
        let mut msgs = grouped.remove(&topic).unwrap();
        // Chronological order
        msgs.sort_by_key(|m| m.timestamp);
        for m in msgs {
            println!(
                "[{}] {}:",
                m.timestamp
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S"),
                m.from
            );
            println!("  {}", m.body);
            println!();
        }
    }
    Ok(())
}

pub async fn run_search(client: &MellowMeshClient, query: String) -> anyhow::Result<()> {
    let results = client.search_messages(&query).await?;
    if results.is_empty() {
        println!("No messages found matching search query '{query}'.");
        return Ok(());
    }
    for m in results {
        println!(
            "[{}] {} | from: {}",
            m.timestamp
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S"),
            m.topic,
            m.from
        );
        println!("  {}", m.body);
        println!("{}", "-".repeat(60));
    }
    Ok(())
}

pub async fn run_trace_enable(
    client: &MellowMeshClient,
    target_type: &str,
    target: &str,
    level: &str,
    duration: &str,
    reason: Option<String>,
    enabled_by: &str,
) -> anyhow::Result<()> {
    let ts = client
        .enable_trace(target_type, target, level, duration, reason, enabled_by)
        .await?;
    println!("Trace session enabled successfully:");
    println!("  ID:          {}", ts.id);
    println!("  Target:      {} ({})", ts.target, ts.target_type);
    println!("  Level:       {:?}", ts.level);
    println!(
        "  Expires At:  {}",
        ts.expires_at
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
    );
    Ok(())
}

pub async fn run_trace_disable(client: &MellowMeshClient, id: &str) -> anyhow::Result<()> {
    client.disable_trace(id).await?;
    println!("Trace session {id} disabled successfully.");
    Ok(())
}

pub async fn run_traces(client: &MellowMeshClient) -> anyhow::Result<()> {
    let sessions = client.list_traces().await?;
    if sessions.is_empty() {
        println!("No active trace sessions.");
        return Ok(());
    }
    let mut rows = Vec::new();
    for ts in sessions {
        rows.push(vec![
            ts.id,
            ts.target,
            format!("{:?}", ts.level),
            ts.status,
            ts.expires_at
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        ]);
    }
    print_table(
        &["Session ID", "Target", "Level", "Status", "Expires At"],
        &rows,
    );
    Ok(())
}

pub async fn run_metrics(client: &MellowMeshClient) -> anyhow::Result<()> {
    let metrics = client.get_metrics().await?;

    println!("============================================================");
    println!("MELLOWMESH SYSTEM METRICS");
    println!("============================================================");

    if let Some(obj) = metrics.as_object() {
        for (k, v) in obj {
            println!("  {k:<40} : {v}");
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&metrics)?);
    }
    println!("============================================================");
    Ok(())
}

pub async fn run_wiki_list(
    client: &MellowMeshClient,
    wiki: &str,
    doc_type: Option<&str>,
    tag: Option<&str>,
) -> anyhow::Result<()> {
    let pages = client.list_wiki_pages(wiki, None, doc_type, tag).await?;
    if pages.is_empty() {
        println!("No pages found in wiki '{wiki}'.");
        return Ok(());
    }

    let mut rows = Vec::new();
    for p in pages {
        rows.push(vec![
            p.path,
            p.title,
            p.doc_type,
            p.tags.join(", "),
            p.timestamp
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        ]);
    }
    print_table(&["Path", "Title", "Type", "Tags", "Last Updated"], &rows);
    Ok(())
}

pub async fn run_wiki_view(
    client: &MellowMeshClient,
    wiki: &str,
    path: &str,
) -> anyhow::Result<()> {
    let doc = client.get_wiki_page(wiki, path).await?;
    println!("============================================================");
    println!("WIKI: {} | PATH: {}", doc.wiki, doc.path);
    println!("TITLE: {}", doc.title);
    println!("TYPE: {}", doc.doc_type);
    if let Some(desc) = &doc.description {
        println!("DESCRIPTION: {desc}");
    }
    println!("TAGS: {}", doc.tags.join(", "));
    if let Some(res) = &doc.resource {
        println!("RESOURCE: {res}");
    }
    println!(
        "UPDATED: {}",
        doc.timestamp
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
    );
    println!("============================================================");
    println!("\n{}\n", doc.body);
    println!("============================================================");
    if !doc.links.is_empty() {
        println!("LINKS: {}", doc.links.join(", "));
    }
    Ok(())
}

pub async fn run_wiki_search(
    client: &MellowMeshClient,
    wiki: &str,
    query: &str,
    doc_type: Option<&str>,
    tag: Option<&str>,
) -> anyhow::Result<()> {
    let pages = client
        .list_wiki_pages(wiki, Some(query), doc_type, tag)
        .await?;
    if pages.is_empty() {
        println!("No pages matching query '{query}' in wiki '{wiki}'.");
        return Ok(());
    }

    let mut rows = Vec::new();
    for p in pages {
        rows.push(vec![
            p.path,
            p.title,
            p.doc_type,
            p.description.unwrap_or_default(),
        ]);
    }
    print_table(&["Path", "Title", "Type", "Description"], &rows);
    Ok(())
}

pub async fn run_wiki_sync(client: &MellowMeshClient, wiki: &str) -> anyhow::Result<()> {
    println!("Triggering sync for wiki '{wiki}'...");
    client.sync_wiki(wiki).await?;
    println!("Wiki sync completed successfully.");
    Ok(())
}

pub async fn run_schema_add(
    client: &MellowMeshClient,
    topic: &str,
    version: &str,
    file_path: &str,
) -> anyhow::Result<()> {
    let schema_content = std::fs::read_to_string(file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read schema file '{file_path}': {e}"))?;
    client.add_schema(topic, version, &schema_content).await?;
    println!("Schema for topic '{topic}' (version: {version}) registered successfully.");
    Ok(())
}

pub async fn run_schema_status(
    client: &MellowMeshClient,
    topic: &str,
    version: &str,
    status: &str,
) -> anyhow::Result<()> {
    client.set_schema_status(topic, version, status).await?;
    println!("Schema status for '{topic}' (version: {version}) updated to '{status}'.");
    Ok(())
}

pub async fn run_schema_remove(
    client: &MellowMeshClient,
    topic: &str,
    version: &str,
) -> anyhow::Result<()> {
    client.remove_schema(topic, version).await?;
    println!("Schema version '{version}' for topic '{topic}' deleted successfully.");
    Ok(())
}

pub async fn run_schema_list(client: &MellowMeshClient) -> anyhow::Result<()> {
    let schemas = client.list_schemas().await?;
    if schemas.is_empty() {
        println!("No topic schema contracts registered.");
        return Ok(());
    }
    let mut rows = Vec::new();
    for s in schemas {
        rows.push(vec![
            s.topic_pattern,
            s.version,
            s.status,
            s.created_at
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        ]);
    }
    print_table(&["Topic Pattern", "Version", "Status", "Created At"], &rows);
    Ok(())
}

// ---------------------------------------------------------------------------
// Guided demo: two simulated agents divide tasks, survive a crash via claim
// leases, and pause for a human decision — the core MellowMesh loop in ~2 min.
// ---------------------------------------------------------------------------

const DEMO_HUMAN: &str = "human://you";
const DEMO_BUILDER: &str = "agent://you/builder";
const DEMO_SCOUT: &str = "agent://you/scout";

fn demo_id(prefix: &str) -> String {
    format!(
        "{}_{}",
        prefix,
        ulid::Ulid::new().to_string().to_lowercase()
    )
}

fn demo_task(id: &str, title: &str, topic: &str) -> Task {
    Task {
        id: id.to_string(),
        title: title.to_string(),
        description: None,
        created_from: None,
        created_by: DEMO_HUMAN.to_string(),
        status: "open".to_string(),
        priority: "medium".to_string(),
        topics: vec![topic.to_string()],
        required_capabilities: vec!["demo".to_string()],
        assigned_to: None,
        claimed_by: None,
        deadline: None,
        artifacts: vec![],
        decisions: vec![],
        parent_id: None,
        lease_seconds: None,
        claim_expires_at: None,
    }
}

async fn demo_progress(
    client: &MellowMeshClient,
    task_id: &str,
    agent: &str,
    pct: i64,
    text: &str,
) -> anyhow::Result<()> {
    let msg = Message {
        id: String::new(),
        topic: format!("_task.{task_id}.progress"),
        from: agent.to_string(),
        owner: Some(DEMO_HUMAN.to_string()),
        timestamp: Utc::now(),
        content_type: "application/json".to_string(),
        body: text.to_string(),
        headers: None,
        payload: Some(serde_json::json!({ "task_id": task_id, "percentage": pct, "status": text })),
        parent_id: None,
    };
    client.publish(&msg).await?;
    println!("      [{agent}] {pct}% — {text}");
    Ok(())
}

async fn demo_prompt(question: &str) -> anyhow::Result<String> {
    print!("{question}");
    use std::io::Write;
    std::io::stdout().flush()?;
    let answer = tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).map(|_| line)
    })
    .await??;
    Ok(answer.trim().to_lowercase())
}

pub async fn run_demo(client: &MellowMeshClient) -> anyhow::Result<()> {
    println!();
    println!("=== MellowMesh Demo: agents dividing work under human command ===");
    println!();
    println!("Two simulated agents will register on your local fabric, split tasks,");
    println!("survive a crash thanks to claim leases, and stop to ask YOU for a decision.");
    println!("Everything they do is a real message you can inspect afterwards.");
    println!();

    // 1. Register the fleet
    println!("[1/5] Registering agents on the fabric...");
    for (id, name) in [(DEMO_BUILDER, "builder"), (DEMO_SCOUT, "scout")] {
        client
            .register_agent(&AgentRegistration {
                id: id.to_string(),
                name: name.to_string(),
                owner: DEMO_HUMAN.to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec!["demo".to_string()],
            })
            .await?;
        println!("      Registered {id}");
    }

    // 2. Create work
    println!();
    println!("[2/5] Creating two tasks...");
    let task_a = demo_id("task");
    let task_b = demo_id("task");
    client
        .create_task(&demo_task(
            &task_a,
            "Draft the release notes",
            "_task.demo.docs",
        ))
        .await?;
    client
        .create_task(&demo_task(
            &task_b,
            "Audit dependency licenses",
            "_task.demo.audit",
        ))
        .await?;
    println!("      {task_a} — Draft the release notes");
    println!("      {task_b} — Audit dependency licenses");

    // 3. Builder works task A end to end
    println!();
    println!("[3/5] Builder claims task A and reports progress...");
    client
        .claim_task_with_lease(&task_a, DEMO_BUILDER, None)
        .await?;
    for (pct, text) in [
        (25, "Collected merged pull requests"),
        (60, "Drafting highlights section"),
        (100, "Release notes ready"),
    ] {
        demo_progress(client, &task_a, DEMO_BUILDER, pct, text).await?;
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    }
    client.complete_task(&task_a).await?;
    println!("      Task A completed by builder.");

    // 4. Scout claims task B with a short lease, then "crashes"
    println!();
    println!("[4/5] Scout claims task B with a 5-second lease... and crashes.");
    client
        .claim_task_with_lease(&task_b, DEMO_SCOUT, Some(5))
        .await?;
    println!("      Scout stopped responding. No progress heartbeats are arriving.");
    println!("      Watch the daemon reclaim the task when the lease expires");
    println!("      (lease sweep runs every ~10s)...");

    let reclaim_deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let tasks = client.list_tasks().await?;
        let b = tasks.iter().find(|t| t.id == task_b);
        match b {
            Some(t) if t.status == "open" => {
                println!("      Lease expired: task B is OPEN again. No work was lost.");
                break;
            }
            _ if std::time::Instant::now() > reclaim_deadline => {
                anyhow::bail!(
                    "Task B was not reclaimed in time — is the daemon running an older version without the lease sweeper?"
                );
            }
            _ => {}
        }
    }

    println!("      Builder picks up the abandoned task...");
    client
        .claim_task_with_lease(&task_b, DEMO_BUILDER, None)
        .await?;
    demo_progress(client, &task_b, DEMO_BUILDER, 50, "License scan complete").await?;

    // 5. Human-in-the-loop decision
    println!();
    println!("[5/5] Builder found a GPL dependency and wants YOUR decision before acting.");
    let decision_id = demo_id("decision");
    client
        .create_decision(&Decision {
            id: decision_id.clone(),
            title: "Replace GPL-licensed dependency?".to_string(),
            question: "The audit found one GPL-3.0 dependency. Replace it with an MIT alternative?"
                .to_string(),
            created_by: DEMO_BUILDER.to_string(),
            required_decider: DEMO_HUMAN.to_string(),
            status: "requested".to_string(),
            options: vec![
                DecisionOption {
                    id: "option_replace".to_string(),
                    label: "Yes, replace it".to_string(),
                    pros: vec![],
                    cons: vec![],
                },
                DecisionOption {
                    id: "option_keep".to_string(),
                    label: "No, keep it for now".to_string(),
                    pros: vec![],
                    cons: vec![],
                },
            ],
            response_option_id: None,
            response_timestamp: None,
        })
        .await?;
    println!("      The agent is now blocked, waiting for a human. That is the point.");
    println!();

    let answer = demo_prompt("      Approve the replacement? [y/n]: ").await?;
    let option = if answer.starts_with('y') {
        "option_replace"
    } else {
        "option_keep"
    };
    client.respond_decision(&decision_id, option).await?;
    println!("      Decision recorded: {option}");

    demo_progress(
        client,
        &task_b,
        DEMO_BUILDER,
        100,
        "Audit finished per your decision",
    )
    .await?;
    client.complete_task(&task_b).await?;

    println!();
    println!("=== Demo complete ===");
    println!();
    println!("Everything you just saw is permanent, structured data on YOUR machine:");
    println!("  mellowmesh tasks                     # both tasks, completed");
    println!("  mellowmesh decisions                 # your decision, recorded for audit");
    println!("  mellowmesh read \"_task.**\" --limit 20  # every progress heartbeat");
    println!();
    println!("Next step — let your real agents join the fabric:");
    println!("  claude mcp add mellowmesh -- mellowmesh mcp");
    Ok(())
}
