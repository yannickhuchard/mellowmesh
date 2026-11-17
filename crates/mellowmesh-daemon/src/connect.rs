//! One-click connection of an AI platform to this hub.
//!
//! Every AI app learns about a tool (MCP) server in its own way: a command,
//! a JSON settings file, a TOML table, a settings screen. This module knows
//! each way, so "Connect" is one click instead of a ritual.
//!
//! Rules it keeps:
//! * it runs only on the owner's click (`POST /platforms/:id/connect`,
//!   owner only) — discovery never connects by itself;
//! * the app gets **its own scoped token** (`agent://<owner>/<platform>`),
//!   never the owner's powers: it can talk in rooms, work on tasks, ask for
//!   decisions and host its platform's Mellows — not answer decisions, pair
//!   devices, mint tokens or connect other apps;
//! * it writes only MellowMesh's own entry (the hub's sibling `mellowmesh`
//!   binary, `mcp`, and that token), replacing an earlier one;
//! * the previous file is kept beside the new one (`.mellowmesh-backup`);
//! * a settings file it cannot read as expected is never touched.

use crate::platforms::{self, PlatformSpec};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// The name MellowMesh registers itself under, everywhere.
pub const SERVER_NAME: &str = "mellowmesh";
/// The environment variable the MCP server reads its token from.
pub const TOKEN_ENV: &str = "MELLOWMESH_TOKEN";

/// How a platform is connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connector {
    /// Claude Code registers servers with its own command.
    ClaudeCli,
    /// A JSON settings file holding a servers object under `key`.
    Json {
        path: PathBuf,
        key: &'static str,
        /// VS Code's shape: entries carry `"type": "stdio"`.
        typed: bool,
    },
    /// Codex: a `[mcp_servers.<name>]` table in `config.toml`.
    CodexToml(PathBuf),
    /// Mistral Vibe: a `[[mcp_servers]]` entry in `config.toml`.
    VibeToml(PathBuf),
    /// The app has a door but keeps it in its own settings screen: the
    /// owner pastes a snippet there once.
    Snippet,
    /// A web assistant: no local door — invited by a pasted message.
    Message,
    /// No way in (yet).
    None,
}

impl Connector {
    /// The word clients switch on.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ClaudeCli | Self::Json { .. } | Self::CodexToml(_) | Self::VibeToml(_) => {
                "one_click"
            }
            Self::Snippet => "snippet",
            Self::Message => "message",
            Self::None => "none",
        }
    }

    /// The settings file this connector writes, if it is a file.
    pub fn file(&self) -> Option<&Path> {
        match self {
            Self::Json { path, .. } | Self::CodexToml(path) | Self::VibeToml(path) => Some(path),
            _ => None,
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Where apps keep per-user settings (`%APPDATA%`, or its Unix cousins).
fn app_data() -> PathBuf {
    if let Some(p) = std::env::var_os("APPDATA") {
        return PathBuf::from(p);
    }
    if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support")
    } else {
        home().join(".config")
    }
}

/// The way in for a catalog id.
pub fn connector(id: &str) -> Connector {
    let h = home();
    let json = |path: PathBuf| Connector::Json {
        path,
        key: "mcpServers",
        typed: false,
    };
    match id {
        "claude-code" => Connector::ClaudeCli,
        "claude-desktop" => json(app_data().join("Claude").join("claude_desktop_config.json")),
        "cursor" => json(h.join(".cursor").join("mcp.json")),
        "windsurf" => json(h.join(".codeium").join("windsurf").join("mcp_config.json")),
        "gemini" => json(h.join(".gemini").join("settings.json")),
        "antigravity" => json(h.join(".gemini").join("antigravity").join("mcp_config.json")),
        "kimi" => json(h.join(".kimi").join("mcp.json")),
        "github-copilot" => Connector::Json {
            path: app_data().join("Code").join("User").join("mcp.json"),
            key: "servers",
            typed: true,
        },
        "codex" => Connector::CodexToml(h.join(".codex").join("config.toml")),
        "mistral-vibe" => Connector::VibeToml(h.join(".vibe").join("config.toml")),
        "warp" => Connector::Snippet,
        "chatgpt" | "grok" | "mistral" | "deepseek" | "moonshot" => Connector::Message,
        _ => Connector::None,
    }
}

/// MellowMesh's own command: the `mellowmesh` binary beside this hub, else
/// the one on `PATH`.
pub fn own_command() -> Option<PathBuf> {
    let name = if cfg!(windows) { "mellowmesh.exe" } else { "mellowmesh" };
    if let Some(sibling) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(name)))
        .filter(|p| p.is_file())
    {
        return Some(sibling);
    }
    platforms::which("mellowmesh")
}

/// What a platform's own token may read and write: the shared rooms, tasks,
/// projects, team rooms, decisions (read; creating one goes through its own
/// tool) and its own `_agent.<owner>.<platform>.**` corner. Not other
/// helpers' inboxes or outboxes, not the hub's reserved topics.
pub fn platform_scopes(owner: &str, platform: &str) -> (Vec<String>, Vec<String>) {
    let own = format!(
        "_agent.{}.{}.**",
        crate::mellows::owner_handle(owner),
        crate::mellows::slug(platform).unwrap_or_else(|| "platform".into())
    );
    let shared = ["_forum.**", "_task.**", "_artifact.**", "_project.**", "_team.**"];
    let mut read: Vec<String> = shared.iter().map(|s| s.to_string()).collect();
    read.push("_decision.**".into());
    read.push(own.clone());
    let mut write: Vec<String> = shared.iter().map(|s| s.to_string()).collect();
    write.push(own);
    (read, write)
}

/// Issue the platform's own token (revoking any earlier one, so there is
/// one key per app). Returns `(principal, plaintext token)`.
pub fn issue_platform_token(
    store: &mellowmesh_store::Store,
    owner: &str,
    spec: &PlatformSpec,
) -> Result<(String, String), String> {
    let principal = crate::mellows::platform_agent_id(owner, spec.id);
    let (read, write) = platform_scopes(owner, spec.id);
    crate::auth::revoke_tokens_of(store, &principal).map_err(|e| e.to_string())?;
    let label = format!("{} (connected app)", spec.name);
    let (_, token) = crate::auth::issue_token(
        store,
        &principal,
        Some(spec.name.to_string()),
        read,
        write,
        Some(&label),
    )
    .map_err(|e| e.to_string())?;
    Ok((principal, token))
}

/// The entry written for MellowMesh in a JSON servers object.
fn json_entry(command: &Path, typed: bool, token: &str) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "command": command.to_string_lossy(),
        "args": ["mcp"],
        "env": { TOKEN_ENV: token },
    });
    if typed {
        entry["type"] = serde_json::json!("stdio");
    }
    entry
}

/// Add (or replace) MellowMesh in a JSON settings text. `Err` when the text
/// is not the object it should be — the caller then leaves the file alone.
pub fn merge_json(
    existing: &str,
    key: &str,
    command: &Path,
    typed: bool,
    token: &str,
) -> Result<String, String> {
    let mut root: serde_json::Value = if existing.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(existing).map_err(|_| "its settings file is not plain JSON".to_string())?
    };
    let obj = root
        .as_object_mut()
        .ok_or("its settings file is not a JSON object")?;
    let servers = obj
        .entry(key.to_string())
        .or_insert_with(|| serde_json::json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or("its tool-server setting has an unexpected shape")?;
    servers.insert(SERVER_NAME.to_string(), json_entry(command, typed, token));
    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
}

/// A TOML literal string (`'…'`), which keeps Windows backslashes as they
/// are. A value holding a single quote cannot be written that way.
fn toml_literal(value: &str) -> Result<String, String> {
    if value.contains('\'') || value.contains('\n') {
        return Err("a value cannot be written in that file".into());
    }
    Ok(format!("'{value}'"))
}

/// The block for Codex's `config.toml`.
pub fn codex_block(command: &Path, token: &str) -> Result<String, String> {
    Ok(format!(
        "\n[mcp_servers.{SERVER_NAME}]\ncommand = {}\nargs = [\"mcp\"]\nenv = {{ {TOKEN_ENV} = {} }}\n",
        toml_literal(&command.to_string_lossy())?,
        toml_literal(token)?
    ))
}

/// The block for Mistral Vibe's `config.toml`.
pub fn vibe_block(command: &Path, token: &str) -> Result<String, String> {
    Ok(format!(
        "\n[[mcp_servers]]\nname = \"{SERVER_NAME}\"\ntransport = \"stdio\"\ncommand = {}\nargs = [\"mcp\"]\nenv = {{ {TOKEN_ENV} = {} }}\n",
        toml_literal(&command.to_string_lossy())?,
        toml_literal(token)?
    ))
}

/// Split a TOML text into its preamble and its `[table]` sections.
fn toml_sections(text: &str) -> (String, Vec<(String, String)>) {
    let mut pre = String::new();
    let mut sections: Vec<(String, String)> = Vec::new();
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with('[') {
            sections.push((line.trim().to_string(), line.to_string()));
        } else if let Some(last) = sections.last_mut() {
            last.1.push_str(line);
        } else {
            pre.push_str(line);
        }
    }
    (pre, sections)
}

/// Whether a TOML section is MellowMesh's own entry.
fn is_ours(header: &str, body: &str) -> bool {
    let codex = format!("[mcp_servers.{SERVER_NAME}");
    header.starts_with(&codex)
        || (header == "[[mcp_servers]]" && body.contains(&format!("name = \"{SERVER_NAME}\"")))
}

/// The TOML text without MellowMesh's earlier entries.
pub fn strip_toml(text: &str) -> String {
    let (pre, sections) = toml_sections(text);
    let kept: String = sections
        .into_iter()
        .filter(|(h, b)| !is_ours(h, b))
        .map(|(_, b)| b)
        .collect();
    let mut out = pre + &kept;
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

fn toml_has_our_token(text: &str) -> bool {
    let (_, sections) = toml_sections(text);
    sections
        .iter()
        .any(|(h, b)| is_ours(h, b) && b.contains(TOKEN_ENV))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Whether MellowMesh is among a platform's tool servers **with its own
/// token** — an older entry without one gave the app the owner's powers on
/// an open hub, and counts as not connected (Connect replaces it).
pub fn is_connected(id: &str) -> bool {
    match connector(id) {
        Connector::ClaudeCli => json_has_token(&read(&home().join(".claude.json")), "mcpServers"),
        Connector::Json { path, key, .. } => json_has_token(&read(&path), key),
        Connector::CodexToml(path) | Connector::VibeToml(path) => toml_has_our_token(&read(&path)),
        _ => false,
    }
}

fn json_has_token(text: &str, key: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| {
            v.get(key)
                .and_then(|s| s.get(SERVER_NAME))
                .and_then(|e| e.get("env"))
                .and_then(|e| e.get(TOKEN_ENV))
                .cloned()
        })
        .is_some()
}

/// Whether the platform left a trace on this machine even when its program
/// is not on `PATH`: its settings folder exists.
pub fn has_settings_folder(id: &str) -> bool {
    connector(id)
        .file()
        .and_then(Path::parent)
        .is_some_and(Path::is_dir)
}

/// Keep the previous file beside the new one, then write. Both may hold a
/// key (this one, or an earlier MellowMesh key), so both are written the
/// way `owner.token` is: atomically, readable by this user only on Unix.
fn write_keeping_previous(path: &Path, content: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("its settings folder could not be made ({e})"))?;
    }
    if path.is_file() {
        let previous = std::fs::read(path).map_err(|e| format!("its previous settings could not be read ({e})"))?;
        let mut backup = path.as_os_str().to_owned();
        backup.push(".mellowmesh-backup");
        crate::auth::write_secret_file(&PathBuf::from(backup), &previous)
            .map_err(|e| format!("its previous settings could not be kept ({e})"))?;
    }
    crate::auth::write_secret_file(path, content.as_bytes())
        .map_err(|e| format!("its settings could not be written ({e})"))
}

/// What a click on Connect did.
#[derive(Debug)]
pub struct Connected {
    /// One sentence on what changed.
    pub message: String,
    /// For apps configured by pasting: the setup to paste (it holds the
    /// app's own token, so it is only ever handed out on this click).
    pub snippet: Option<String>,
}

/// Connect a platform. `Ok` says what changed; `Err` says why nothing did.
/// Blocking: call from the blocking pool.
pub fn connect(store: &mellowmesh_store::Store, owner: &str, id: &str) -> Result<Connected, String> {
    let spec = platforms::spec(id).ok_or("That AI app is not one MellowMesh knows.")?;
    let name = spec.name;
    let way = connector(id);
    match way {
        Connector::Message => {
            return Err(format!("{name} lives on the web: invite it with a pasted message instead."))
        }
        Connector::None => return Err(format!("{name} has no way to connect to MellowMesh yet.")),
        _ => {}
    }
    if way.kind() == "one_click" && is_connected(id) {
        return Ok(Connected {
            message: format!("{name} is already connected."),
            snippet: None,
        });
    }
    let command = own_command()
        .ok_or("MellowMesh's own command (mellowmesh) was not found beside the hub or on PATH.")?;
    let refuse = |why: String| format!("{name} was left untouched: {why}.");

    // Check what can be checked before a key exists.
    let plan: Option<String> = match &way {
        Connector::Json { path, key, typed } => {
            Some(merge_json(&read(path), key, &command, *typed, "check").map_err(refuse)?)
        }
        Connector::ClaudeCli if platforms::which("claude").is_none() => {
            return Err(refuse("its command is not installed".into()))
        }
        _ => None,
    };
    drop(plan);

    let (principal, token) = issue_platform_token(store, owner, spec)?;
    let result = write_entry(&way, name, &command, &token);
    if result.is_err() {
        let _ = crate::auth::revoke_tokens_of(store, &principal);
    }
    result
}

fn write_entry(way: &Connector, name: &str, command: &Path, token: &str) -> Result<Connected, String> {
    let refuse = |why: String| format!("{name} was left untouched: {why}.");
    let done = |message: String| Connected { message, snippet: None };
    match way {
        Connector::ClaudeCli => {
            let claude = platforms::which("claude").ok_or_else(|| refuse("its command is not installed".into()))?;
            // An earlier entry (possibly without a token) is replaced.
            let _ = std::process::Command::new(&claude)
                .args(["mcp", "remove", "--scope", "user", SERVER_NAME])
                .stdin(Stdio::null())
                .output();
            let out = std::process::Command::new(&claude)
                .args(["mcp", "add", "--env"])
                .arg(format!("{TOKEN_ENV}={token}"))
                .args(["--scope", "user", SERVER_NAME, "--"])
                .arg(command)
                .arg("mcp")
                .stdin(Stdio::null())
                .output()
                .map_err(|e| refuse(format!("its command could not be run ({e})")))?;
            if !out.status.success() {
                let said = String::from_utf8_lossy(&out.stderr);
                let first = said.lines().find(|l| !l.trim().is_empty()).unwrap_or("no reason given");
                return Err(refuse(format!("it answered \"{}\"", first.chars().take(160).collect::<String>())));
            }
            Ok(done(format!(
                "Added MellowMesh to {name} with its own limited key. Open a new {name} session to see it."
            )))
        }
        Connector::Json { path, key, typed } => {
            let merged = merge_json(&read(path), key, command, *typed, token).map_err(refuse)?;
            write_keeping_previous(path, &merged).map_err(refuse)?;
            Ok(done(format!(
                "Added MellowMesh to {name}'s tool servers ({}), with its own limited key. Restart {name} to see it.",
                path.display()
            )))
        }
        Connector::CodexToml(path) => {
            let text = strip_toml(&read(path)) + &codex_block(command, token).map_err(refuse)?;
            write_keeping_previous(path, &text).map_err(refuse)?;
            Ok(done(format!("Added MellowMesh to {name} ({}), with its own limited key.", path.display())))
        }
        Connector::VibeToml(path) => {
            let text = strip_toml(&read(path)) + &vibe_block(command, token).map_err(refuse)?;
            write_keeping_previous(path, &text).map_err(refuse)?;
            Ok(done(format!("Added MellowMesh to {name} ({}), with its own limited key.", path.display())))
        }
        Connector::Snippet => {
            let snippet = serde_json::to_string_pretty(&serde_json::json!({
                SERVER_NAME: json_entry(command, false, token)
            }))
            .map_err(|e| e.to_string())?;
            Ok(Connected {
                message: format!(
                    "Setup for {name} made, with its own limited key: paste it once in {name}'s tool-server settings."
                ),
                snippet: Some(snippet),
            })
        }
        Connector::Message | Connector::None => Err(refuse("it has no local door".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\MellowMesh\mellowmesh.exe")
    }

    #[test]
    fn an_empty_settings_file_gets_exactly_our_entry_with_its_key() {
        let out = merge_json("", "mcpServers", &exe(), false, "mm_k").unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["mellowmesh"]["args"][0], "mcp");
        assert_eq!(v["mcpServers"]["mellowmesh"]["env"]["MELLOWMESH_TOKEN"], "mm_k");
        assert!(json_has_token(&out, "mcpServers"));
    }

    #[test]
    fn other_servers_and_other_settings_survive() {
        let before = r#"{"theme":"dark","mcpServers":{"github":{"command":"gh-mcp"}}}"#;
        let out = merge_json(before, "mcpServers", &exe(), false, "mm_k").unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["github"]["command"], "gh-mcp");
    }

    #[test]
    fn an_old_entry_without_a_key_is_not_connected_and_gets_replaced() {
        let old = r#"{"mcpServers":{"mellowmesh":{"command":"x","args":["mcp"]}}}"#;
        assert!(!json_has_token(old, "mcpServers"), "an owner-power entry must be redone");
        let out = merge_json(old, "mcpServers", &exe(), false, "mm_new").unwrap();
        assert!(json_has_token(&out, "mcpServers"));
    }

    #[test]
    fn vs_code_entries_say_they_are_stdio() {
        let out = merge_json("{}", "servers", &exe(), true, "mm_k").unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["servers"]["mellowmesh"]["type"], "stdio");
    }

    #[test]
    fn a_file_we_cannot_read_as_expected_is_refused_not_rewritten() {
        assert!(merge_json("// my notes\n{}", "mcpServers", &exe(), false, "k").is_err());
        assert!(merge_json("[1,2]", "mcpServers", &exe(), false, "k").is_err());
        assert!(merge_json(r#"{"mcpServers":"oops"}"#, "mcpServers", &exe(), false, "k").is_err());
    }

    #[test]
    fn toml_blocks_carry_the_key_and_replace_the_previous_entry() {
        let c = codex_block(&exe(), "mm_k").unwrap();
        assert!(c.contains(r"command = 'C:\Program Files\MellowMesh\mellowmesh.exe'"));
        assert!(c.contains("env = { MELLOWMESH_TOKEN = 'mm_k' }"));
        let before = "model = \"o4\"\n\n[mcp_servers.mellowmesh]\ncommand = 'x'\nargs = [\"mcp\"]\n\n[mcp_servers.other]\ncommand = 'y'\n";
        assert!(!toml_has_our_token(before));
        let after = strip_toml(before) + &c;
        assert!(after.contains("model = \"o4\"") && after.contains("[mcp_servers.other]"));
        assert_eq!(after.matches("[mcp_servers.mellowmesh]").count(), 1);
        assert!(toml_has_our_token(&after));

        let v = vibe_block(&exe(), "mm_k").unwrap();
        let vibe_before = "[[mcp_servers]]\nname = \"mellowmesh\"\ncommand = 'x'\n\n[[mcp_servers]]\nname = \"fs\"\n";
        let vibe_after = strip_toml(vibe_before) + &v;
        assert_eq!(vibe_after.matches("name = \"mellowmesh\"").count(), 1);
        assert!(vibe_after.contains("name = \"fs\""));
        assert!(codex_block(Path::new("C:\\it's\\here"), "k").is_err());
    }

    #[test]
    fn a_platform_key_reaches_shared_rooms_and_its_own_corner_only() {
        let (read, write) = platform_scopes("human://yannick", "claude-code");
        assert!(write.contains(&"_agent.yannick.claude_code.**".to_string()));
        assert!(!write.iter().any(|s| s == "**" || s == "_agent.**"));
        assert!(!read.iter().any(|s| s == "**" || s == "_agent.**"));
        assert!(!write.iter().any(|s| s.starts_with("_decision") || s.starts_with("_system")));
    }

    #[test]
    fn every_catalog_platform_has_a_stated_way_in() {
        for spec in platforms::CATALOG {
            let kind = connector(spec.id).kind();
            assert!(matches!(kind, "one_click" | "snippet" | "message" | "none"), "{}", spec.id);
        }
        assert_eq!(connector("cursor").kind(), "one_click");
        assert_eq!(connector("warp").kind(), "snippet");
        assert_eq!(connector("deepseek").kind(), "message");
        assert_eq!(connector("pi").kind(), "none");
    }

    #[test]
    fn a_stale_owner_token_file_is_renewed_and_a_valid_one_kept() {
        let store = mellowmesh_store::Store::new_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("mm-owner-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("mellowmesh.db");
        let owner = crate::auth::bootstrap_owner(&store, &db);
        let file = crate::auth::owner_token_path(&db);
        let first = read(&file);
        assert!(!first.trim().is_empty());
        crate::auth::bootstrap_owner(&store, &db);
        assert_eq!(read(&file), first, "a valid owner.token is left alone");
        std::fs::write(&file, "mm_stale_from_another_database").unwrap();
        assert_eq!(crate::auth::bootstrap_owner(&store, &db), owner);
        let renewed = read(&file);
        assert_ne!(renewed.trim(), "mm_stale_from_another_database");
        let rec = store
            .find_token_by_hash(&mellowmesh_core::auth::hash_token(renewed.trim()))
            .unwrap()
            .unwrap();
        assert_eq!(rec.principal, owner);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Persona simulation — Karim taps Connect for Cursor: a settings file
    /// with his own server gains MellowMesh and its key, his previous file
    /// is kept, and a second look says "connected".
    #[test]
    fn karim_connects_an_app_and_his_previous_settings_are_kept() {
        let dir = std::env::temp_dir().join(format!("mm-connect-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mcp.json");
        std::fs::write(&file, r#"{"mcpServers":{"mine":{"command":"x"}}}"#).unwrap();
        let merged = merge_json(&read(&file), "mcpServers", &exe(), false, "mm_k").unwrap();
        write_keeping_previous(&file, &merged).unwrap();
        assert!(json_has_token(&read(&file), "mcpServers"));
        let backup = dir.join("mcp.json.mellowmesh-backup");
        assert!(read(&backup).contains("\"mine\""));
        assert!(!read(&backup).contains("mellowmesh"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for f in [&file, &backup] {
                let mode = std::fs::metadata(f).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "a file holding a key is the user's alone");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
