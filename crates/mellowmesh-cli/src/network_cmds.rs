//! `network`, `space`, `team`, `access`, `pair` and the task-board commands.
//!
//! Admin commands run against the local daemon act as the hub owner with
//! the `owner.token` the daemon wrote next to its database (as `respond`
//! does), unless `MELLOWMESH_TOKEN` is set.

use crate::commands::as_local_human;
use mellowmesh_client::sanitize::sanitize_line;
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::network::{AccessLevel, NetworkPatch, Space, Team, TeamRole};
use mellowmesh_core::task::{status, TaskGraph, TaskUpdate};

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

pub async fn run_network_show(client: &MellowMeshClient) -> anyhow::Result<()> {
    let view = as_local_human(client).get_network().await?;
    let n = &view.network;
    println!("Network:        {}", sanitize_line(&n.name));
    if !n.description.is_empty() {
        println!("Description:    {}", sanitize_line(&n.description));
    }
    println!("Default access: {}", n.default_access.as_str());
    println!("Visitor posts:  {}", yes_no(n.visitor_write));
    println!();
    print_spaces(&view.spaces);
    println!();
    print_teams(&view.teams);
    Ok(())
}

pub async fn run_network_set(
    client: &MellowMeshClient,
    name: Option<String>,
    description: Option<String>,
    default_access: Option<String>,
    visitor_write: Option<bool>,
) -> anyhow::Result<()> {
    let default_access = default_access
        .map(|d| d.parse::<AccessLevel>())
        .transpose()
        .map_err(anyhow::Error::msg)?;
    let patch = NetworkPatch {
        name,
        description,
        default_access,
        visitor_write,
        lan_access: None,
    };
    let n = as_local_human(client).set_network(&patch).await?;
    println!(
        "Network '{}' updated: default access {}, visitor posts {}.",
        sanitize_line(&n.name),
        n.default_access.as_str(),
        yes_no(n.visitor_write)
    );
    Ok(())
}

fn print_spaces(spaces: &[Space]) {
    println!("Spaces:");
    let rows: Vec<Vec<String>> = spaces
        .iter()
        .map(|s| {
            vec![
                s.id.clone(),
                s.access.as_str().to_string(),
                s.patterns.join(" "),
                s.members.join(" "),
                s.teams.join(" "),
                format!(
                    "{}{}",
                    if s.share_with_peers { "peers " } else { "" },
                    if s.visitor_write { "visitor-posts" } else { "" }
                ),
            ]
        })
        .collect();
    crate::commands::print_table(
        &["Id", "Access", "Patterns", "Members", "Teams", "Flags"],
        &rows,
    );
}

fn print_teams(teams: &[Team]) {
    println!("Teams:");
    let rows: Vec<Vec<String>> = teams
        .iter()
        .map(|t| {
            vec![
                t.id.clone(),
                t.name.clone(),
                t.color.clone(),
                t.members
                    .iter()
                    .map(|m| format!("{} ({})", m.principal, m.role.as_str()))
                    .collect::<Vec<_>>()
                    .join(", "),
            ]
        })
        .collect();
    crate::commands::print_table(&["Id", "Name", "Color", "Members"], &rows);
}

pub struct SpaceArgs {
    pub name: Option<String>,
    pub description: Option<String>,
    pub patterns: Vec<String>,
    pub access: Option<String>,
    pub members: Vec<String>,
    pub teams: Vec<String>,
    pub peers: Option<bool>,
    pub visitor_write: Option<bool>,
}

fn apply_space_args(space: &mut Space, args: SpaceArgs) -> anyhow::Result<()> {
    if let Some(n) = args.name {
        space.name = n;
    }
    if let Some(d) = args.description {
        space.description = d;
    }
    if !args.patterns.is_empty() {
        space.patterns = args.patterns;
    }
    if let Some(a) = args.access {
        space.access = a.parse().map_err(anyhow::Error::msg)?;
    }
    if !args.members.is_empty() {
        space.members = args.members;
    }
    if !args.teams.is_empty() {
        space.teams = args.teams;
    }
    if let Some(p) = args.peers {
        space.share_with_peers = p;
    }
    if let Some(v) = args.visitor_write {
        space.visitor_write = v;
    }
    Ok(())
}

pub async fn run_space_add(
    client: &MellowMeshClient,
    id: String,
    args: SpaceArgs,
) -> anyhow::Result<()> {
    let mut space = Space {
        id,
        ..Default::default()
    };
    apply_space_args(&mut space, args)?;
    let s = as_local_human(client).create_space(&space).await?;
    println!(
        "Space '{}' created ({}) over {}.",
        sanitize_line(&s.id),
        s.access.as_str(),
        sanitize_line(&s.patterns.join(", "))
    );
    Ok(())
}

pub async fn run_space_list(client: &MellowMeshClient) -> anyhow::Result<()> {
    let spaces = as_local_human(client).list_spaces().await?;
    print_spaces(&spaces);
    Ok(())
}

/// `space set`: change only the given fields (lists replace the old ones).
pub async fn run_space_set(
    client: &MellowMeshClient,
    id: String,
    args: SpaceArgs,
) -> anyhow::Result<()> {
    let admin = as_local_human(client);
    let mut space = admin
        .list_spaces()
        .await?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| anyhow::anyhow!("Space '{id}' not found"))?;
    apply_space_args(&mut space, args)?;
    let s = admin.replace_space(&space).await?;
    println!(
        "Space '{}' updated: {} over {}.",
        sanitize_line(&s.id),
        s.access.as_str(),
        sanitize_line(&s.patterns.join(", "))
    );
    Ok(())
}

pub async fn run_space_remove(client: &MellowMeshClient, id: &str) -> anyhow::Result<()> {
    as_local_human(client).delete_space(id).await?;
    println!("Space '{}' removed.", sanitize_line(id));
    Ok(())
}

pub async fn run_team_add(
    client: &MellowMeshClient,
    id: String,
    name: Option<String>,
    color: Option<String>,
    description: Option<String>,
) -> anyhow::Result<()> {
    let mut team = Team {
        id,
        name: name.unwrap_or_default(),
        description: description.unwrap_or_default(),
        ..Default::default()
    };
    if let Some(c) = color {
        team.color = c;
    }
    let t = as_local_human(client).create_team(&team).await?;
    println!(
        "Team '{}' created with its closed team room space 'team-{}' (topic _team.{}.chat).",
        sanitize_line(&t.id),
        sanitize_line(&t.id),
        sanitize_line(&t.id)
    );
    Ok(())
}

pub async fn run_team_list(client: &MellowMeshClient) -> anyhow::Result<()> {
    let teams = as_local_human(client).list_teams().await?;
    print_teams(&teams);
    Ok(())
}

pub async fn run_team_remove(client: &MellowMeshClient, id: &str) -> anyhow::Result<()> {
    as_local_human(client).delete_team(id).await?;
    println!("Team '{}' and its team space removed.", sanitize_line(id));
    Ok(())
}

pub async fn run_team_member_add(
    client: &MellowMeshClient,
    team: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<()> {
    let role: TeamRole = role.parse().map_err(anyhow::Error::msg)?;
    as_local_human(client)
        .add_team_member(team, principal, role)
        .await?;
    println!(
        "{} is now a {} of team '{}'.",
        sanitize_line(principal),
        role.as_str(),
        sanitize_line(team)
    );
    Ok(())
}

pub async fn run_team_member_remove(
    client: &MellowMeshClient,
    team: &str,
    principal: &str,
) -> anyhow::Result<()> {
    as_local_human(client)
        .remove_team_member(team, principal)
        .await?;
    println!(
        "{} removed from team '{}'.",
        sanitize_line(principal),
        sanitize_line(team)
    );
    Ok(())
}

pub async fn run_access_check(
    client: &MellowMeshClient,
    topic: &str,
    principal: Option<&str>,
) -> anyhow::Result<()> {
    let c = as_local_human(client)
        .access_check(topic, principal)
        .await?;
    println!(
        "{} on {}:",
        sanitize_line(principal.unwrap_or("A visitor")),
        sanitize_line(topic)
    );
    println!("  read:   {}", yes_no(c.read));
    println!("  write:  {}", yes_no(c.write));
    println!(
        "  space:  {}",
        sanitize_line(c.space.as_deref().unwrap_or("(none: network default)"))
    );
    println!("  level:  {}", c.level.as_str());
    println!("  reason: {}", sanitize_line(&c.reason));
    Ok(())
}

use mellowmesh_core::pairing::{PairRequest, PairResponse};

/// A QR code for `text`, drawn with Unicode half blocks (one module wide,
/// two modules per line, so modules come out square) with the standard
/// four-module quiet zone.
///
/// With `ansi`, the code sets its own colors (black modules on a white
/// background), so it reads correctly whatever the terminal theme. Without,
/// light modules are drawn in the terminal's text color and dark ones are
/// left blank: dark-on-light on the usual dark terminal background.
pub fn qr_text(text: &str, ansi: bool) -> anyhow::Result<String> {
    use qrcode::render::unicode::Dense1x2;
    // Low error correction keeps the code small; a screen does not smudge.
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::L)?;
    let mut render = code.render::<Dense1x2>();
    render.quiet_zone(true);
    if ansi {
        let body = render
            .dark_color(Dense1x2::Dark)
            .light_color(Dense1x2::Light)
            .build();
        Ok(body
            .lines()
            .map(|line| format!("\x1b[30;107m{line}\x1b[0m"))
            .collect::<Vec<_>>()
            .join("\n"))
    } else {
        Ok(render
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build())
    }
}

/// Turn on escape-sequence processing for the console (Windows only;
/// other terminals always process them). `false` when it cannot be.
#[cfg(windows)]
fn enable_vt_processing() -> bool {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        STD_OUTPUT_HANDLE,
    };
    // SAFETY: plain console API calls on this process's own stdout handle,
    // with a valid pointer to a local for the mode.
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle == 0 || handle == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut mode = 0;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return false;
        }
        mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0
            || SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

#[cfg(not(windows))]
fn enable_vt_processing() -> bool {
    true
}

/// Whether the QR code may set its own colors: stdout is a terminal that
/// processes escape sequences, and `NO_COLOR` is not set.
fn qr_colors_supported() -> bool {
    use std::io::IsTerminal;
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return false;
    }
    if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
        return false;
    }
    std::io::stdout().is_terminal() && enable_vt_processing()
}

/// Local `HH:MM` of a timestamp.
fn local_time(ts: chrono::DateTime<chrono::Utc>) -> String {
    ts.with_timezone(&chrono::Local).format("%H:%M").to_string()
}

/// How often `mellowmesh pair` asks the hub whether the device connected.
const PAIR_POLL_EVERY: std::time::Duration = std::time::Duration::from_millis(1500);

/// `mellowmesh pair`: ask the hub for a pairing (it opens its LAN listener
/// when no url is given), print the QR code and link, then wait for the
/// device to connect.
pub async fn run_pair(
    client: &MellowMeshClient,
    name: Option<String>,
    url: Option<String>,
    no_wait: bool,
    port: u16,
) -> anyhow::Result<()> {
    let url = url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
    if let Some(u) = &url {
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            anyhow::bail!(
                "--url must start with http:// or https:// (the address the device will use)"
            );
        }
    }
    let lan_mode = url.is_none();
    let admin = as_local_human(client);
    let request = PairRequest { name, url };
    let mut first = admin.pair(&request).await;
    if let Err(e) = &first {
        let msg = e.to_string();
        // A hub started by an older install does not know /pair yet: on this
        // machine, restart it with the daemon next to this CLI and try again,
        // so pairing stays a single command.
        if (msg.contains("(404)") || msg.contains("(405)")) && admin.targets_local() {
            println!("The running hub is an older version: restarting it...");
            crate::commands::run_daemon_restart(port).await?;
            first = as_local_human(client).pair(&request).await;
        }
    }
    let pairing = first.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("(401)") || msg.contains("(403)") {
            anyhow::anyhow!(
                "{msg}\nPairing mints a token for the hub owner: run it on the hub's machine \
                     (it reads owner.token) or set MELLOWMESH_TOKEN to the owner token"
            )
        } else if msg.contains("(404)") || msg.contains("(405)") {
            anyhow::anyhow!(
                "{msg}\nThe running hub is older than this CLI and cannot pair in one step: \
                 restart it with `mellowmesh daemon restart`, then run `mellowmesh pair` again"
            )
        } else {
            e
        }
    })?;
    print_pairing(&pairing, lan_mode)?;
    if no_wait {
        println!(
            "Not waiting (--no-wait). The code works until {} if it is not used; \
             check with: mellowmesh devices",
            local_time(pairing.expires_at)
        );
        return Ok(());
    }
    wait_for_device(&admin, &pairing).await
}

fn print_pairing(p: &PairResponse, lan_mode: bool) -> anyhow::Result<()> {
    let name = sanitize_line(&p.name);
    let qr = qr_text(&p.link, qr_colors_supported())?;
    println!();
    println!(
        "Pair \"{name}\" with {}: scan this code with the MellowMesh app",
        sanitize_line(&p.network)
    );
    println!();
    println!("{qr}");
    println!();
    if !p.code.is_empty() {
        println!(
            "No camera handy? In the app, choose \"Enter a code\" and type: {}",
            sanitize_line(&p.code)
        );
        println!();
    }
    println!("Or paste this pairing link in the app:");
    println!("{}", p.link);
    println!();
    let via = if lan_mode { " (local network)" } else { "" };
    println!("Hub address: {}{via}", sanitize_line(&p.url));
    let minutes = (p.expires_at - chrono::Utc::now()).num_minutes().max(0) + 1;
    println!(
        "The code works for {minutes} min (until {}) and contains a secret: show it only \
         to the device you are pairing.",
        local_time(p.expires_at)
    );
    if cfg!(windows) && lan_mode {
        println!(
            "If Windows Firewall asks, click \"Allow access\" so the phone can reach this PC."
        );
    }
    println!();
    Ok(())
}

/// Poll `GET /pair/:id` until the device connects, the code expires, or
/// Ctrl+C.
async fn wait_for_device(admin: &MellowMeshClient, p: &PairResponse) -> anyhow::Result<()> {
    let name = sanitize_line(&p.name);
    println!("Waiting for {name} to connect... (Ctrl+C to stop waiting)");
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    let mut failures = 0u32;
    loop {
        let poll = async {
            tokio::time::sleep(PAIR_POLL_EVERY).await;
            admin.pair_status(&p.token_id).await
        };
        let status = tokio::select! {
            _ = &mut ctrl_c => {
                println!();
                println!(
                    "Stopped waiting. The code still works until {} if it is not used.",
                    local_time(p.expires_at)
                );
                return Ok(());
            }
            status = poll => status,
        };
        match status {
            Ok(s) if s.connected => {
                println!("✓ {} connected", sanitize_line(&s.name));
                println!(
                    "Remove it any time with: mellowmesh devices remove {}",
                    sanitize_line(&p.token_id)
                );
                return Ok(());
            }
            Ok(s) if s.expired => anyhow::bail!(
                "The code expired before {name} connected. Run `mellowmesh pair` again."
            ),
            Ok(_) => failures = 0,
            Err(e) => {
                failures += 1;
                if failures >= 5 {
                    return Err(e.context("Lost contact with the hub while waiting"));
                }
            }
        }
    }
}

/// `mellowmesh devices`: paired and pending devices.
pub async fn run_devices(client: &MellowMeshClient) -> anyhow::Result<()> {
    let devices = as_local_human(client).list_devices().await?;
    if devices.is_empty() {
        println!("No devices paired yet. Pair one with: mellowmesh pair");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = devices
        .iter()
        .map(|d| {
            let status = match (d.connected, d.expires_at) {
                (true, _) => "connected".to_string(),
                (false, Some(e)) => format!("waiting (until {})", local_time(e)),
                (false, None) => "waiting".to_string(),
            };
            vec![
                d.token_id.clone(),
                d.name.clone(),
                status,
                d.last_seen
                    .map(|t| {
                        t.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_else(|| "never".to_string()),
                d.created_at
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string(),
            ]
        })
        .collect();
    crate::commands::print_table(&["Id", "Name", "Status", "Last seen", "Paired"], &rows);
    Ok(())
}

/// `mellowmesh devices remove <id>`: revoke a device.
pub async fn run_device_remove(client: &MellowMeshClient, id: &str) -> anyhow::Result<()> {
    as_local_human(client).remove_device(id).await?;
    println!(
        "Device {} removed: its token no longer works.",
        sanitize_line(id)
    );
    Ok(())
}

pub struct TaskUpdateArgs {
    pub status: Option<String>,
    pub assign: Option<String>,
    pub priority: Option<String>,
    pub depends_on: Vec<String>,
    pub no_deps: bool,
    pub team: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
}

pub async fn run_task_update(
    client: &MellowMeshClient,
    id: &str,
    args: TaskUpdateArgs,
) -> anyhow::Result<()> {
    let depends_on = if args.no_deps {
        Some(Vec::new())
    } else if args.depends_on.is_empty() {
        None
    } else {
        Some(args.depends_on)
    };
    let update = TaskUpdate {
        status: args.status,
        assigned_to: args.assign,
        priority: args.priority,
        depends_on,
        team: args.team,
        title: args.title,
        description: args.description,
    };
    let t = as_local_human(client).update_task(id, &update).await?;
    println!(
        "Task {} updated: {} ({}).",
        sanitize_line(&t.id),
        sanitize_line(&t.title),
        sanitize_line(&t.status)
    );
    Ok(())
}

pub async fn run_task_board(
    client: &MellowMeshClient,
    scope: Option<String>,
) -> anyhow::Result<()> {
    let graph = as_local_human(client).task_graph(scope.as_deref()).await?;
    print!("{}", render_board(&graph, 24));
    Ok(())
}

const COLUMNS: [&str; 5] = ["To do", "Claimed", "Doing", "Waiting", "Done"];

/// Which board column a task belongs in.
fn column_of(status_: &str, unmet_deps: bool) -> usize {
    match status_ {
        status::CLAIMED => 1,
        status::IN_PROGRESS => 2,
        status::BLOCKED => 3,
        s if status::is_final(s) => 4,
        _ if unmet_deps => 3,
        _ => 0,
    }
}

fn fit(text: &str, width: usize) -> String {
    let clean = sanitize_line(text);
    let n = clean.chars().count();
    if n <= width {
        format!("{clean:<width$}")
    } else {
        let cut: String = clean.chars().take(width.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// Plain-text kanban: To do / Claimed / Doing / Waiting / Done. Each card
/// is the title (`!` marks the critical chain) and its lane on a second
/// line.
pub fn render_board(graph: &TaskGraph, width: usize) -> String {
    let mut cols: Vec<Vec<(String, String)>> = vec![Vec::new(); COLUMNS.len()];
    for n in &graph.tasks {
        let col = column_of(&n.task.status, !n.blocked_by.is_empty());
        let mark = if n.critical { "! " } else { "" };
        let suffix = if n.task.status == status::CANCELLED {
            " (cancelled)"
        } else {
            ""
        };
        let lane = if n.lane == "unassigned" {
            String::new()
        } else {
            format!(
                "  @{}",
                n.lane.split_once("://").map(|(_, r)| r).unwrap_or(&n.lane)
            )
        };
        cols[col].push((format!("{mark}{}{suffix}", n.task.title), lane));
    }
    let mut out = String::new();
    let sep = |out: &mut String| {
        for _ in 0..COLUMNS.len() {
            out.push_str(&format!("+{}", "-".repeat(width + 2)));
        }
        out.push_str("+\n");
    };
    sep(&mut out);
    for (i, c) in COLUMNS.iter().enumerate() {
        out.push_str(&format!(
            "| {} ",
            fit(&format!("{c} ({})", cols[i].len()), width)
        ));
    }
    out.push_str("|\n");
    sep(&mut out);
    let rows = cols.iter().map(Vec::len).max().unwrap_or(0);
    for r in 0..rows {
        for line in 0..2 {
            for col in &cols {
                let cell = col
                    .get(r)
                    .map(|(title, lane)| {
                        if line == 0 {
                            title.as_str()
                        } else {
                            lane.as_str()
                        }
                    })
                    .unwrap_or("");
                out.push_str(&format!("| {} ", fit(cell, width)));
            }
            out.push_str("|\n");
        }
    }
    sep(&mut out);
    if graph.tasks.iter().any(|n| n.critical) {
        out.push_str("! = on the critical chain\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mellowmesh_core::pairing::pairing_link;
    use mellowmesh_core::task::{build_graph, Task};

    #[test]
    fn pairing_link_is_percent_encoded() {
        let link = pairing_link(
            "http://192.168.1.20:40000/",
            "mm_abc",
            "Yannick's mesh",
            "human://yannick",
        );
        assert_eq!(
            link,
            "mellowmesh-pair:?v=2&url=http%3A%2F%2F192.168.1.20%3A40000&token=mm_abc\
             &name=Yannick%27s%20mesh&principal=human%3A%2F%2Fyannick"
        );
    }

    /// Modules (`true` = dark) from a rendering with explicit colors.
    fn modules(rendered: &str) -> Vec<Vec<bool>> {
        let mut rows = Vec::new();
        for line in rendered.lines() {
            let line = line
                .strip_prefix("\x1b[30;107m")
                .and_then(|l| l.strip_suffix("\x1b[0m"))
                .expect("every line sets and resets its colors");
            let (top, bottom): (Vec<bool>, Vec<bool>) = line
                .chars()
                .map(|c| match c {
                    '█' => (true, true),
                    '▀' => (true, false),
                    '▄' => (false, true),
                    ' ' => (false, false),
                    other => panic!("unexpected character {other:?}"),
                })
                .unzip();
            rows.push(top);
            rows.push(bottom);
        }
        rows
    }

    #[test]
    fn qr_code_decodes_to_the_link() {
        let link = pairing_link(
            "http://192.168.1.20:40000",
            &mellowmesh_core::auth::generate_token(),
            "Yannick's mesh",
            "human://yannick",
        );
        let rendered = qr_text(&link, true).unwrap();
        let grid = modules(&rendered);
        let width = grid[0].len();
        assert!(grid.iter().all(|r| r.len() == width));
        // Square (the last half line may be padding) and compact enough for
        // an 80-column terminal.
        assert!(
            grid.len() == width || grid.len() == width + 1,
            "{}x{}",
            width,
            grid.len()
        );
        assert!(width <= 61, "QR is {width} modules wide");
        // Quiet zone: four light modules on every side.
        for row in grid
            .iter()
            .take(4)
            .chain(grid.iter().skip(width - 4).take(4))
        {
            assert!(row.iter().all(|m| !m));
        }
        for row in &grid {
            assert!(row[..4].iter().chain(&row[width - 4..]).all(|m| !m));
        }

        // Decode it back: dark modules black, 4 px per module.
        let scale = 4;
        let size = width * scale;
        let mut img = rqrr::PreparedImage::prepare_from_greyscale(size, size, |x, y| {
            if grid[y / scale][x / scale] {
                0
            } else {
                255
            }
        });
        let grids = img.detect_grids();
        assert_eq!(grids.len(), 1);
        let (_, content) = grids[0].decode().unwrap();
        assert_eq!(content, link);

        // The plain rendering is the same code with dark modules left blank
        // (drawn on a dark terminal background).
        let plain = qr_text(&link, false).unwrap();
        let swap = |c: char| match c {
            '█' => ' ',
            ' ' => '█',
            '▀' => '▄',
            '▄' => '▀',
            c => c,
        };
        let inverted: Vec<String> = rendered
            .lines()
            .map(|l| {
                l.trim_start_matches("\x1b[30;107m")
                    .trim_end_matches("\x1b[0m")
                    .chars()
                    .map(swap)
                    .collect()
            })
            .collect();
        let plain: Vec<&str> = plain.lines().collect();
        assert_eq!(plain.len(), inverted.len());
        // The last half line is padding when the module count is odd.
        let full = plain.len() - (grid.len() - width);
        assert_eq!(plain[..full], inverted[..full]);
    }

    #[test]
    fn board_columns() {
        let t = |id: &str, st: &str, deps: &[&str]| Task {
            id: id.into(),
            title: format!("Task {id}"),
            status: st.into(),
            depends_on: deps.iter().map(|d| d.to_string()).collect(),
            ..Default::default()
        };
        let mut claimed = t("c", "claimed", &[]);
        claimed.claimed_by = Some("agent://o/coder".into());
        let tasks = vec![
            t("a", "open", &[]),
            t("b", "open", &["a"]),
            claimed,
            t("d", "in_progress", &[]),
            t("e", "blocked", &[]),
            t("f", "completed", &[]),
            t("g", "cancelled", &[]),
        ];
        let g = build_graph(&tasks, &tasks, chrono::Utc::now(), |_| None);
        let board = render_board(&g, 20);
        let header = board.lines().nth(1).unwrap();
        for (col, n) in [
            ("To do", 1),
            ("Claimed", 1),
            ("Doing", 1),
            ("Waiting", 2),
            ("Done", 2),
        ] {
            assert!(header.contains(&format!("{col} ({n})")), "{header}");
        }
        assert!(board.contains("@o/coder"));
        assert!(board.contains("Task g (cancelled)"));
    }
}
