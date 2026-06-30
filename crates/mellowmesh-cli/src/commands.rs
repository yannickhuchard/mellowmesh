use chrono::Utc;
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::decision::{Decision, DecisionOption};
use mellowmesh_core::message::Message;
use mellowmesh_core::task::Task;
use futures_util::StreamExt;
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
    println!("Starting MellowMesh daemon (mellowmeshd) on port {}...", port);
    mellowmesh_client::autostart::spawn_daemon(port)?;
    println!("Daemon started successfully.");
    Ok(())
}

pub async fn run_daemon_stop(port: u16, force: bool) -> anyhow::Result<()> {
    if !mellowmesh_client::autostart::is_daemon_running(port) {
        println!("MellowMesh daemon is not running on port {}.", port);
        return Ok(());
    }

    if force {
        println!("Force option specified. Immediately killing mellowmeshd process...");
        force_kill_daemon();
        return Ok(());
    }

    println!("Stopping MellowMesh daemon on port {}...", port);

    let client = MellowMeshClient::new(port);
    match client.shutdown_daemon().await {
        Ok(_) => {
            println!("Shutdown request sent successfully.");
        }
        Err(e) => {
            eprintln!(
                "Failed to send shutdown request to daemon: {}. Attempting force kill...",
                e
            );
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
        cmd.args(&["/F", "/IM", "mellowmeshd.exe"]);
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
                eprintln!("Failed to execute taskkill command: {}", e);
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
    println!("Restarting MellowMesh daemon on port {}...", port);
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
    println!("Deleting local database at: {:?}", db_path);

    if db_path.exists() {
        match std::fs::remove_file(&db_path) {
            Ok(_) => println!("Deleted database file: {:?}", db_path),
            Err(e) => eprintln!("Failed to delete database file {:?}: {}", db_path, e),
        }
    } else {
        println!("Database file does not exist.");
    }

    // Also delete SQLite sidecar files (WAL/SHM) if they exist
    let wal_path = db_path.with_extension("db-wal");
    if wal_path.exists() {
        if let Err(e) = std::fs::remove_file(&wal_path) {
            eprintln!("Failed to delete WAL file {:?}: {}", wal_path, e);
        } else {
            println!("Deleted WAL file: {:?}", wal_path);
        }
    }

    let shm_path = db_path.with_extension("db-shm");
    if shm_path.exists() {
        if let Err(e) = std::fs::remove_file(&shm_path) {
            eprintln!("Failed to delete SHM file {:?}: {}", shm_path, e);
        } else {
            println!("Deleted SHM file: {:?}", shm_path);
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
    println!("  Default Port:  {}", port);
    println!("  Database Path: {:?}", db_path);
    Ok(())
}

pub async fn run_topics(client: &MellowMeshClient) -> anyhow::Result<()> {
    let topics = client.list_topics().await?;
    println!("Topics:");
    for t in topics {
        println!("  - {}", t);
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

pub async fn run_read(client: &MellowMeshClient, topic: String, limit: usize) -> anyhow::Result<()> {
    let history = client.get_history(limit).await?;
    let filtered: Vec<Message> = history
        .into_iter()
        .filter(|m| mellowmesh_core::topic::match_topic(&topic, &m.topic))
        .collect();

    if filtered.is_empty() {
        println!("No messages found matching topic pattern '{}'.", topic);
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
    println!(
        "Tailing topic pattern '{}'. Press Ctrl+C to stop...",
        pattern
    );
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
                eprintln!("Error receiving message: {:?}", e);
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
    let name = id.split('/').last().unwrap_or(&id).to_string();
    let reg = AgentRegistration {
        id: if id.starts_with("agent://") {
            id
        } else {
            format!("agent://{}", id)
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
        ]);
    }
    print_table(
        &["Task ID", "Title", "Status", "Priority", "Claimed By"],
        &rows,
    );
    Ok(())
}

pub async fn run_claim(
    client: &MellowMeshClient,
    task_id: &str,
    agent_id: &str,
) -> anyhow::Result<()> {
    let full_agent_id = if agent_id.starts_with("agent://") {
        agent_id.to_string()
    } else {
        format!("agent://{}", agent_id)
    };
    client.claim_task(task_id, &full_agent_id).await?;
    println!("Task {} claimed by {}.", task_id, full_agent_id);
    Ok(())
}

pub async fn run_complete(client: &MellowMeshClient, task_id: &str) -> anyhow::Result<()> {
    client.complete_task(task_id).await?;
    println!("Task {} marked as completed.", task_id);
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
    println!(
        "Decision {} answered with option {}.",
        decision_id, option_id
    );
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
        println!("No forum messages found matching pattern '{}'.", pat);
        return Ok(());
    }

    // Sort topics
    let mut topics: Vec<String> = grouped.keys().cloned().collect();
    topics.sort();

    for topic in topics {
        println!("============================================================");
        println!("TOPIC: {}", topic);
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
        println!("No messages found matching search query '{}'.", query);
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
    println!("Trace session {} disabled successfully.", id);
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
            println!("  {:<40} : {}", k, v);
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
        println!("No pages found in wiki '{}'.", wiki);
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

pub async fn run_wiki_view(client: &MellowMeshClient, wiki: &str, path: &str) -> anyhow::Result<()> {
    let doc = client.get_wiki_page(wiki, path).await?;
    println!("============================================================");
    println!("WIKI: {} | PATH: {}", doc.wiki, doc.path);
    println!("TITLE: {}", doc.title);
    println!("TYPE: {}", doc.doc_type);
    if let Some(desc) = &doc.description {
        println!("DESCRIPTION: {}", desc);
    }
    println!("TAGS: {}", doc.tags.join(", "));
    if let Some(res) = &doc.resource {
        println!("RESOURCE: {}", res);
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
        println!("No pages matching query '{}' in wiki '{}'.", query, wiki);
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
    println!("Triggering sync for wiki '{}'...", wiki);
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
        .map_err(|e| anyhow::anyhow!("Failed to read schema file '{}': {}", file_path, e))?;
    client.add_schema(topic, version, &schema_content).await?;
    println!(
        "Schema for topic '{}' (version: {}) registered successfully.",
        topic, version
    );
    Ok(())
}

pub async fn run_schema_status(
    client: &MellowMeshClient,
    topic: &str,
    version: &str,
    status: &str,
) -> anyhow::Result<()> {
    client.set_schema_status(topic, version, status).await?;
    println!(
        "Schema status for '{}' (version: {}) updated to '{}'.",
        topic, version, status
    );
    Ok(())
}

pub async fn run_schema_remove(
    client: &MellowMeshClient,
    topic: &str,
    version: &str,
) -> anyhow::Result<()> {
    client.remove_schema(topic, version).await?;
    println!(
        "Schema version '{}' for topic '{}' deleted successfully.",
        version, topic
    );
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
