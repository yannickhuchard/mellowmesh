use chrono::Utc;
use futures_util::StreamExt;
use mellowmesh_client::sanitize::{sanitize_line, sanitize_text};
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::decision::{Decision, DecisionOption, DecisionOutcome};
use mellowmesh_core::message::Message;
use mellowmesh_core::task::Task;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Print one message (history, tail, search) with every untrusted field
/// sanitized.
fn print_message(m: &Message) {
    println!(
        "[{}] {} | from: {}",
        m.timestamp
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S"),
        sanitize_line(&m.topic),
        sanitize_line(&m.from)
    );
    println!("  {}", sanitize_text(&m.body));
    println!("{}", "-".repeat(60));
}

/// Render a table. Every cell is sanitized (escape sequences and control
/// characters removed, newlines flattened) since most cells are untrusted.
fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        println!("No data available.");
        return;
    }
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|c| sanitize_line(c)).collect())
        .collect();

    let mut widths = vec![0; headers.len()];
    for (i, h) in headers.iter().enumerate() {
        widths[i] = h.chars().count();
    }
    for row in &rows {
        for (i, val) in row.iter().enumerate() {
            let len = val.chars().count();
            if i < widths.len() && len > widths[i] {
                widths[i] = len;
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

    for row in &rows {
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
    mellowmesh_client::autostart::spawn_daemon_async(port).await?;
    println!("Daemon started successfully.");
    Ok(())
}

/// How long to wait for the daemon's port to close after a stop request.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// The directory holding the daemon's database, `owner.token` and
/// `mellowmeshd.pid`.
fn daemon_data_dir() -> PathBuf {
    let db_path = mellowmesh_store::sqlite::default_db_path();
    match db_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// The owner token the daemon wrote next to its database, if readable.
fn read_owner_token(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("owner.token"))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Parse the contents of `mellowmeshd.pid`. Rejects anything that is not a
/// plain process id greater than 1.
fn parse_pid(contents: &str) -> Option<u32> {
    contents.trim().parse::<u32>().ok().filter(|pid| *pid > 1)
}

/// True when something accepts TCP connections on the daemon's port.
async fn port_in_use(port: u16) -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_secs(1),
            tokio::net::TcpStream::connect(("127.0.0.1", port)),
        )
        .await,
        Ok(Ok(_))
    )
}

/// Wait up to `timeout` for the daemon's port to close.
async fn wait_for_port_closed(port: u16, timeout: Duration) -> bool {
    let start = tokio::time::Instant::now();
    loop {
        if !port_in_use(port).await {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Terminate the process recorded in `mellowmeshd.pid` — and only that one.
fn kill_daemon_from_pid_file(dir: &Path, force: bool) -> anyhow::Result<()> {
    let pid_path = dir.join("mellowmeshd.pid");
    let contents = std::fs::read_to_string(&pid_path).map_err(|_| {
        anyhow::anyhow!(
            "No daemon PID file at {}; refusing to kill processes by name. Stop the daemon manually.",
            pid_path.display()
        )
    })?;
    let pid = parse_pid(&contents)
        .ok_or_else(|| anyhow::anyhow!("Invalid PID file {}", pid_path.display()))?;

    // On Linux, make sure the PID still belongs to a mellowmeshd process (a
    // stale PID file could otherwise point at an unrelated, reused PID).
    #[cfg(target_os = "linux")]
    {
        if let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) {
            if !comm.trim().starts_with("mellowmeshd") {
                anyhow::bail!(
                    "PID {pid} from {} is not a mellowmeshd process (stale PID file?); not killing it",
                    pid_path.display()
                );
            }
        }
    }

    println!("Terminating mellowmeshd (PID {pid})...");
    #[cfg(windows)]
    let status = {
        let _ = force; // taskkill needs /F for a windowless background process
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()
    };
    #[cfg(not(windows))]
    let status = {
        let mut cmd = std::process::Command::new("kill");
        if force {
            cmd.arg("-KILL");
        }
        cmd.arg(pid.to_string()).status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(anyhow::anyhow!("Failed to terminate PID {pid} ({s})")),
        Err(e) => Err(anyhow::anyhow!("Failed to terminate PID {pid}: {e}")),
    }
}

/// Stop the local daemon on `port`: a graceful `/shutdown` over loopback
/// (authenticated with the owner token), then — only if it is still running
/// — terminating the PID recorded in `mellowmeshd.pid`. Returns an error if
/// the daemon is still running afterwards.
pub async fn run_daemon_stop(port: u16, force: bool) -> anyhow::Result<()> {
    if !port_in_use(port).await {
        println!("MellowMesh daemon is not running on port {port}.");
        return Ok(());
    }
    let dir = daemon_data_dir();

    if !force {
        println!("Stopping MellowMesh daemon on port {port}...");
        if mellowmesh_client::autostart::is_daemon_running_async(port).await {
            // Always loopback: MELLOWMESH_URL must never redirect a stop to a
            // remote hub.
            let mut client = MellowMeshClient::loopback(port);
            if let Some(token) = read_owner_token(&dir) {
                client = client.with_token(token);
            }
            match client.shutdown_daemon().await {
                Ok(_) => println!("Shutdown request sent."),
                Err(e) => eprintln!("Shutdown request failed: {e}"),
            }
            if wait_for_port_closed(port, STOP_TIMEOUT).await {
                println!("Daemon stopped successfully.");
                return Ok(());
            }
            println!("Daemon did not stop in time.");
        } else {
            println!("Port {port} is in use but no MellowMesh daemon answers its health check.");
        }
    }

    kill_daemon_from_pid_file(&dir, force)?;
    if wait_for_port_closed(port, STOP_TIMEOUT).await {
        println!("Daemon stopped.");
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "The daemon is still running on port {port}; stop it manually."
        ))
    }
}

pub async fn run_daemon_restart(port: u16) -> anyhow::Result<()> {
    println!("Restarting MellowMesh daemon on port {port}...");
    // Graceful stop
    run_daemon_stop(port, false).await?;
    // Short wait to ensure port is freed
    tokio::time::sleep(Duration::from_millis(200)).await;
    // Start again
    run_daemon_start(port).await
}

pub async fn run_daemon_clean(port: u16) -> anyhow::Result<()> {
    // 1. Stop the daemon first (gracefully). Never delete files under a
    //    running daemon.
    run_daemon_stop(port, false)
        .await
        .map_err(|e| anyhow::anyhow!("{e} Nothing was deleted."))?;
    if port_in_use(port).await {
        anyhow::bail!("The daemon is still running on port {port}. Nothing was deleted.");
    }

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
    let running = mellowmesh_client::autostart::is_daemon_running_async(port).await;
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
        println!("  - {}", sanitize_line(&t));
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
    // Filtered server-side, so `limit` counts matching messages.
    let messages = client.get_history_matching(&topic, limit).await?;

    if messages.is_empty() {
        println!(
            "No messages found matching topic pattern '{}'.",
            sanitize_line(&topic)
        );
        return Ok(());
    }

    for m in &messages {
        print_message(m);
    }
    Ok(())
}

pub async fn run_tail(client: &MellowMeshClient, pattern: String) -> anyhow::Result<()> {
    println!("Tailing topic pattern '{pattern}'. Press Ctrl+C to stop...");
    let mut stream = client.subscribe(&pattern).await?;
    while let Some(msg_res) = stream.next().await {
        match msg_res {
            Ok(m) => print_message(&m),
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

/// Parse a `--option` value. An `approve:`, `reject:` or `neutral:` prefix
/// sets the option's outcome (so answering it resolves the decision to
/// approved / rejected / answered); a plain label has no outcome.
fn parse_decision_option(raw: &str) -> anyhow::Result<(String, Option<DecisionOutcome>)> {
    let prefixes = [
        ("approve:", DecisionOutcome::Approve),
        ("reject:", DecisionOutcome::Reject),
        ("neutral:", DecisionOutcome::Neutral),
    ];
    for (prefix, outcome) in prefixes {
        if let Some(rest) = raw.strip_prefix(prefix) {
            let label = rest.trim();
            if label.is_empty() {
                anyhow::bail!("Option '{raw}' has an empty label after '{prefix}'");
            }
            return Ok((label.to_string(), Some(outcome)));
        }
    }
    let label = raw.trim();
    if label.is_empty() {
        anyhow::bail!("Option labels must not be empty");
    }
    Ok((label.to_string(), None))
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
        .iter()
        .enumerate()
        .map(|(i, raw)| {
            let (label, outcome) = parse_decision_option(raw)?;
            Ok(DecisionOption {
                id: format!("option_{}", i + 1),
                label,
                pros: vec![],
                cons: vec![],
                outcome,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

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
        responded_by: None,
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
        println!("TOPIC: {}", sanitize_line(&topic));
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
                sanitize_line(&m.from)
            );
            println!("  {}", sanitize_text(&m.body));
            println!();
        }
    }
    Ok(())
}

pub async fn run_search(client: &MellowMeshClient, query: String) -> anyhow::Result<()> {
    let results = client.search_messages(&query).await?;
    if results.is_empty() {
        println!(
            "No messages found matching search query '{}'.",
            sanitize_line(&query)
        );
        return Ok(());
    }
    for m in &results {
        print_message(m);
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
    println!("  ID:          {}", sanitize_line(&ts.id));
    println!(
        "  Target:      {} ({})",
        sanitize_line(&ts.target),
        sanitize_line(&ts.target_type)
    );
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

    let mut metrics = metrics;
    mellowmesh_client::sanitize::sanitize_json(&mut metrics);
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
    println!(
        "WIKI: {} | PATH: {}",
        sanitize_line(&doc.wiki),
        sanitize_line(&doc.path)
    );
    println!("TITLE: {}", sanitize_line(&doc.title));
    println!("TYPE: {}", sanitize_line(&doc.doc_type));
    if let Some(desc) = &doc.description {
        println!("DESCRIPTION: {}", sanitize_line(desc));
    }
    println!("TAGS: {}", sanitize_line(&doc.tags.join(", ")));
    if let Some(res) = &doc.resource {
        println!("RESOURCE: {}", sanitize_line(res));
    }
    println!(
        "UPDATED: {}",
        doc.timestamp
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
    );
    println!("============================================================");
    println!("\n{}\n", sanitize_text(&doc.body));
    println!("============================================================");
    if !doc.links.is_empty() {
        println!("LINKS: {}", sanitize_line(&doc.links.join(", ")));
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

pub async fn run_e2e(
    client: &MellowMeshClient,
    method: &str,
    path: &str,
    body: Option<String>,
) -> anyhow::Result<()> {
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let (status, response) = client
        .e2e_request(&method.to_uppercase(), &path, body)
        .await?;
    println!("Status: {status}");
    if !response.is_empty() {
        // Pretty-print JSON when possible.
        match serde_json::from_str::<serde_json::Value>(&response) {
            Ok(mut v) => {
                mellowmesh_client::sanitize::sanitize_json(&mut v);
                println!("{}", serde_json::to_string_pretty(&v)?)
            }
            Err(_) => println!("{}", sanitize_text(&response)),
        }
    }
    Ok(())
}

pub async fn run_token_create(
    client: &MellowMeshClient,
    principal: &str,
    name: Option<String>,
    read_scopes: Vec<String>,
    write_scopes: Vec<String>,
) -> anyhow::Result<()> {
    let read = if read_scopes.is_empty() {
        None
    } else {
        Some(read_scopes)
    };
    let write = if write_scopes.is_empty() {
        None
    } else {
        Some(write_scopes)
    };
    let result = client
        .create_token(principal, name.as_deref(), read, write)
        .await?;
    println!("Token created for {principal}:");
    println!(
        "  id:     {}",
        sanitize_line(result.get("id").and_then(|v| v.as_str()).unwrap_or("?"))
    );
    println!(
        "  token:  {}",
        sanitize_line(result.get("token").and_then(|v| v.as_str()).unwrap_or("?"))
    );
    println!();
    println!("This token is shown ONCE and never stored in plaintext.");
    println!("Give it to the agent via the MELLOWMESH_TOKEN environment variable.");
    Ok(())
}

pub async fn run_token_list(client: &MellowMeshClient) -> anyhow::Result<()> {
    let tokens = client.list_tokens().await?;
    let mut rows = Vec::new();
    for t in tokens {
        rows.push(vec![
            t.get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string(),
            t.get("principal")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string(),
            t.get("read_scopes")
                .map(|v| v.to_string())
                .unwrap_or_default(),
            t.get("write_scopes")
                .map(|v| v.to_string())
                .unwrap_or_default(),
            t.get("revoked")
                .and_then(|v| v.as_bool())
                .map(|b| if b { "revoked" } else { "active" })
                .unwrap_or("?")
                .to_string(),
        ]);
    }
    print_table(
        &[
            "Token ID",
            "Principal",
            "Read Scopes",
            "Write Scopes",
            "Status",
        ],
        &rows,
    );
    Ok(())
}

pub async fn run_token_revoke(client: &MellowMeshClient, id: &str) -> anyhow::Result<()> {
    client.revoke_token(id).await?;
    println!("Token {id} revoked.");
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
                    outcome: Some(mellowmesh_core::decision::DecisionOutcome::Approve),
                },
                DecisionOption {
                    id: "option_keep".to_string(),
                    label: "No, keep it for now".to_string(),
                    pros: vec![],
                    cons: vec![],
                    outcome: Some(mellowmesh_core::decision::DecisionOutcome::Reject),
                },
            ],
            response_option_id: None,
            response_timestamp: None,
            responded_by: None,
        })
        .await?;
    println!("      The agent is now blocked, waiting for a human. That is the point.");
    println!();

    let answer = demo_prompt("      Approve the replacement? [y/n]: ").await?;
    let approved = answer.trim().to_lowercase().starts_with('y');
    let option = if approved {
        "option_replace"
    } else {
        "option_keep"
    };
    client.respond_decision(&decision_id, option).await?;
    println!(
        "      Decision recorded: {option} (status: {})",
        if approved { "approved" } else { "rejected" }
    );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_option_prefixes_set_outcomes() {
        assert_eq!(
            parse_decision_option("approve:Ship it").unwrap(),
            ("Ship it".to_string(), Some(DecisionOutcome::Approve))
        );
        assert_eq!(
            parse_decision_option("reject: Hold").unwrap(),
            ("Hold".to_string(), Some(DecisionOutcome::Reject))
        );
        assert_eq!(
            parse_decision_option("neutral:Ask again later").unwrap(),
            (
                "Ask again later".to_string(),
                Some(DecisionOutcome::Neutral)
            )
        );
        assert_eq!(
            parse_decision_option("Maybe").unwrap(),
            ("Maybe".to_string(), None)
        );
        // Only the exact lowercase prefixes are special.
        assert_eq!(
            parse_decision_option("Approve: later").unwrap(),
            ("Approve: later".to_string(), None)
        );
        assert!(parse_decision_option("approve:").is_err());
        assert!(parse_decision_option("reject:   ").is_err());
        assert!(parse_decision_option("").is_err());
    }

    #[test]
    fn pid_file_parsing() {
        assert_eq!(parse_pid("4242\n"), Some(4242));
        assert_eq!(parse_pid(" 17 "), Some(17));
        assert_eq!(parse_pid("1"), None);
        assert_eq!(parse_pid("0"), None);
        assert_eq!(parse_pid("-1"), None);
        assert_eq!(parse_pid("12 34"), None);
        assert_eq!(parse_pid("abc"), None);
        assert_eq!(parse_pid(""), None);
    }

    #[test]
    fn owner_token_is_read_and_trimmed() {
        let dir = std::env::temp_dir().join(format!(
            "mellowmesh-cli-test-{}",
            ulid::Ulid::new().to_string().to_lowercase()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_owner_token(&dir), None);
        std::fs::write(dir.join("owner.token"), "mm_secret\n").unwrap();
        assert_eq!(read_owner_token(&dir), Some("mm_secret".to_string()));
        std::fs::write(dir.join("owner.token"), "  \n").unwrap();
        assert_eq!(read_owner_token(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kill_requires_a_pid_file() {
        let dir = std::env::temp_dir().join(format!(
            "mellowmesh-cli-test-{}",
            ulid::Ulid::new().to_string().to_lowercase()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let err = kill_daemon_from_pid_file(&dir, false).unwrap_err();
        assert!(err.to_string().contains("No daemon PID file"));
        std::fs::write(dir.join("mellowmeshd.pid"), "not-a-pid").unwrap();
        assert!(kill_daemon_from_pid_file(&dir, false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stop_is_a_no_op_when_nothing_listens() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        run_daemon_stop(port, false).await.unwrap();
    }
}
