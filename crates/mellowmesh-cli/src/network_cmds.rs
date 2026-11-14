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

/// Percent-encode a query value: only RFC 3986 unreserved characters pass.
fn pct(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The pairing link the apps scan or paste.
pub fn pairing_link(url: &str, token: &str, network_name: &str, principal: &str) -> String {
    format!(
        "mellowmesh-pair:?v=2&url={}&token={}&name={}&principal={}",
        pct(url.trim_end_matches('/')),
        pct(token),
        pct(network_name),
        pct(principal)
    )
}

/// A QR code for `text`, drawn with Unicode half blocks (light modules on a
/// dark terminal background, with a quiet zone).
pub fn qr_text(text: &str) -> anyhow::Result<String> {
    use qrcode::render::unicode::Dense1x2;
    let code = qrcode::QrCode::new(text.as_bytes())?;
    Ok(code
        .render::<Dense1x2>()
        .dark_color(Dense1x2::Light)
        .light_color(Dense1x2::Dark)
        .quiet_zone(true)
        .build())
}

/// `mellowmesh pair`: mint a full-access token for the owner, labelled
/// with the device name, and print a QR code plus the pairing link.
pub async fn run_pair(client: &MellowMeshClient, name: &str, url: &str) -> anyhow::Result<()> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        anyhow::bail!(
            "--url must start with http:// or https:// (the address the device will use)"
        );
    }
    let admin = as_local_human(client);
    let me = admin.whoami().await?;
    let owner = match (me.principal, me.is_owner) {
        (Some(p), true) => p,
        _ => anyhow::bail!(
            "Pairing mints a token for the hub owner: run it on the hub's machine \
             (it reads owner.token) or set MELLOWMESH_TOKEN to the owner token"
        ),
    };
    let network = admin.get_network().await?.network;
    let minted = admin
        .create_labeled_token(&owner, name, vec!["**".into()], vec!["**".into()])
        .await?;
    let token = minted
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("The hub did not return a token"))?;
    let id = minted.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let link = pairing_link(url, token, &network.name, &owner);

    println!(
        "Scan this with the MellowMesh app on \"{}\":",
        sanitize_line(name)
    );
    println!();
    println!("{}", qr_text(&link)?);
    println!();
    println!("Or paste this pairing link:");
    println!("{link}");
    println!();
    println!(
        "Token id: {} (for {})",
        sanitize_line(id),
        sanitize_line(&owner)
    );
    println!("This link contains a secret: show it only to the device you are pairing.");
    println!(
        "Revoke it any time with: mellowmesh token revoke {}",
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
        let qr = qr_text(&link).unwrap();
        assert!(qr.lines().count() > 10);
        assert!(qr.contains('█') || qr.contains('▀') || qr.contains('▄'));
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
