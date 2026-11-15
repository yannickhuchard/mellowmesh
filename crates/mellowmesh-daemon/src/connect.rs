//! One-click connection of an AI platform to this hub.
//!
//! Every AI app learns about a tool (MCP) server in its own way: a command,
//! a JSON settings file, a TOML table, a settings screen. This module knows
//! each way, so "Connect" is one click instead of a ritual.
//!
//! Rules it keeps:
//! * it runs only on the owner's click (`POST /platforms/:id/connect`,
//!   owner only) — discovery never connects by itself;
//! * it writes only the paths in this file, and only MellowMesh's own
//!   entry: the hub's sibling `mellowmesh` binary with the `mcp` argument;
//! * the previous file is kept beside the new one (`.mellowmesh-backup`);
//! * a settings file it cannot read as expected is never touched.

use crate::platforms;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// The name MellowMesh registers itself under, everywhere.
pub const SERVER_NAME: &str = "mellowmesh";

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

/// The entry written for MellowMesh in a JSON servers object.
fn json_entry(command: &Path, typed: bool) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "command": command.to_string_lossy(),
        "args": ["mcp"],
    });
    if typed {
        entry["type"] = serde_json::json!("stdio");
    }
    entry
}

/// The snippet a person pastes in an app's own settings.
pub fn snippet() -> Option<String> {
    let command = own_command()?;
    serde_json::to_string_pretty(&serde_json::json!({
        SERVER_NAME: json_entry(&command, false)
    }))
    .ok()
}

/// Add MellowMesh to a JSON settings text. `Err` when the text is not the
/// object it should be — the caller then leaves the file alone.
pub fn merge_json(existing: &str, key: &str, command: &Path, typed: bool) -> Result<String, String> {
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
    servers.insert(SERVER_NAME.to_string(), json_entry(command, typed));
    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
}

/// A TOML literal string (`'…'`), which keeps Windows backslashes as they
/// are. A path holding a single quote cannot be written that way.
fn toml_literal(path: &Path) -> Result<String, String> {
    let s = path.to_string_lossy();
    if s.contains('\'') || s.contains('\n') {
        return Err("MellowMesh's own path cannot be written in that file".into());
    }
    Ok(format!("'{s}'"))
}

/// The block appended to Codex's `config.toml`.
pub fn codex_block(command: &Path) -> Result<String, String> {
    Ok(format!(
        "\n[mcp_servers.{SERVER_NAME}]\ncommand = {}\nargs = [\"mcp\"]\n",
        toml_literal(command)?
    ))
}

/// The block appended to Mistral Vibe's `config.toml`.
pub fn vibe_block(command: &Path) -> Result<String, String> {
    Ok(format!(
        "\n[[mcp_servers]]\nname = \"{SERVER_NAME}\"\ntransport = \"stdio\"\ncommand = {}\nargs = [\"mcp\"]\n",
        toml_literal(command)?
    ))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Whether MellowMesh is already among a platform's tool servers.
pub fn is_connected(id: &str) -> bool {
    match connector(id) {
        Connector::ClaudeCli => json_has(&read(&home().join(".claude.json")), "mcpServers"),
        Connector::Json { path, key, .. } => json_has(&read(&path), key),
        Connector::CodexToml(path) => read(&path).contains(&format!("[mcp_servers.{SERVER_NAME}]")),
        Connector::VibeToml(path) => read(&path).contains(&format!("name = \"{SERVER_NAME}\"")),
        _ => false,
    }
}

fn json_has(text: &str, key: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get(key).and_then(|s| s.get(SERVER_NAME)).cloned())
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

/// Keep the previous file beside the new one, then write.
fn write_keeping_previous(path: &Path, content: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("its settings folder could not be made ({e})"))?;
    }
    if path.is_file() {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".mellowmesh-backup");
        std::fs::copy(path, PathBuf::from(backup))
            .map_err(|e| format!("its previous settings could not be kept ({e})"))?;
    }
    std::fs::write(path, content).map_err(|e| format!("its settings could not be written ({e})"))
}

/// Connect a platform. `Ok` carries one sentence saying what changed;
/// `Err` one sentence saying why nothing did. Blocking: call from the
/// blocking pool.
pub fn connect(id: &str) -> Result<String, String> {
    let spec = platforms::spec(id).ok_or("That AI app is not one MellowMesh knows.")?;
    let name = spec.name;
    if is_connected(id) {
        return Ok(format!("{name} is already connected."));
    }
    let command = own_command()
        .ok_or("MellowMesh's own command (mellowmesh) was not found beside the hub or on PATH.")?;
    let refuse = |why: String| format!("{name} was left untouched: {why}.");
    match connector(id) {
        Connector::ClaudeCli => {
            let claude = platforms::which("claude")
                .ok_or_else(|| refuse("its command is not installed".into()))?;
            let out = std::process::Command::new(claude)
                .args(["mcp", "add", "--scope", "user", SERVER_NAME, "--"])
                .arg(&command)
                .arg("mcp")
                .stdin(Stdio::null())
                .output()
                .map_err(|e| refuse(format!("its command could not be run ({e})")))?;
            if !out.status.success() {
                let said = String::from_utf8_lossy(&out.stderr);
                let first = said.lines().find(|l| !l.trim().is_empty()).unwrap_or("no reason given");
                return Err(refuse(format!("it answered \"{}\"", first.chars().take(160).collect::<String>())));
            }
            Ok(format!("Added MellowMesh to {name}. Open a new {name} session to see it."))
        }
        Connector::Json { path, key, typed } => {
            let merged = merge_json(&read(&path), key, &command, typed).map_err(refuse)?;
            write_keeping_previous(&path, &merged).map_err(refuse)?;
            Ok(format!(
                "Added MellowMesh to {name}'s tool servers ({}). Restart {name} to see it.",
                path.display()
            ))
        }
        Connector::CodexToml(path) => {
            let block = codex_block(&command).map_err(refuse)?;
            write_keeping_previous(&path, &(read(&path) + &block)).map_err(refuse)?;
            Ok(format!("Added MellowMesh to {name} ({}). Open a new {name} session to see it.", path.display()))
        }
        Connector::VibeToml(path) => {
            let block = vibe_block(&command).map_err(refuse)?;
            write_keeping_previous(&path, &(read(&path) + &block)).map_err(refuse)?;
            Ok(format!("Added MellowMesh to {name} ({}). Open a new {name} session to see it.", path.display()))
        }
        Connector::Snippet => Err(format!(
            "{name} keeps its tool servers in its own settings: copy the setup and paste it there once."
        )),
        Connector::Message => Err(format!(
            "{name} lives on the web: invite it with a pasted message instead."
        )),
        Connector::None => Err(format!("{name} has no way to connect to MellowMesh yet.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\MellowMesh\mellowmesh.exe")
    }

    #[test]
    fn an_empty_settings_file_gets_exactly_our_entry() {
        let out = merge_json("", "mcpServers", &exe(), false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["mellowmesh"]["args"][0], "mcp");
        assert_eq!(
            v["mcpServers"]["mellowmesh"]["command"],
            r"C:\Program Files\MellowMesh\mellowmesh.exe"
        );
    }

    #[test]
    fn other_servers_and_other_settings_survive() {
        let before = r#"{"theme":"dark","mcpServers":{"github":{"command":"gh-mcp"}}}"#;
        let out = merge_json(before, "mcpServers", &exe(), false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["github"]["command"], "gh-mcp");
        assert!(v["mcpServers"]["mellowmesh"].is_object());
    }

    #[test]
    fn vs_code_entries_say_they_are_stdio() {
        let out = merge_json("{}", "servers", &exe(), true).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["servers"]["mellowmesh"]["type"], "stdio");
    }

    #[test]
    fn a_file_we_cannot_read_as_expected_is_refused_not_rewritten() {
        // Comments (JSONC), a list at the root, a servers value of the wrong shape.
        assert!(merge_json("// my notes\n{}", "mcpServers", &exe(), false).is_err());
        assert!(merge_json("[1,2]", "mcpServers", &exe(), false).is_err());
        assert!(merge_json(r#"{"mcpServers":"oops"}"#, "mcpServers", &exe(), false).is_err());
    }

    #[test]
    fn toml_blocks_keep_windows_paths_literal() {
        let c = codex_block(&exe()).unwrap();
        assert!(c.contains("[mcp_servers.mellowmesh]"));
        assert!(c.contains(r"command = 'C:\Program Files\MellowMesh\mellowmesh.exe'"));
        let v = vibe_block(&exe()).unwrap();
        assert!(v.contains("[[mcp_servers]]") && v.contains("name = \"mellowmesh\""));
        assert!(codex_block(Path::new("C:\\it's\\here")).is_err());
    }

    #[test]
    fn every_catalog_platform_has_a_stated_way_in() {
        for spec in platforms::CATALOG {
            let kind = connector(spec.id).kind();
            assert!(
                matches!(kind, "one_click" | "snippet" | "message" | "none"),
                "{}",
                spec.id
            );
        }
        assert_eq!(connector("cursor").kind(), "one_click");
        assert_eq!(connector("warp").kind(), "snippet");
        assert_eq!(connector("deepseek").kind(), "message");
        assert_eq!(connector("pi").kind(), "none");
    }

    /// Persona simulation — Karim taps Connect for Cursor: a settings file
    /// with his own server gains MellowMesh, his previous file is kept, and
    /// a second look says "connected".
    #[test]
    fn karim_connects_an_app_and_his_previous_settings_are_kept() {
        let dir = std::env::temp_dir().join(format!("mm-connect-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mcp.json");
        std::fs::write(&file, r#"{"mcpServers":{"mine":{"command":"x"}}}"#).unwrap();
        let merged = merge_json(&read(&file), "mcpServers", &exe(), false).unwrap();
        write_keeping_previous(&file, &merged).unwrap();
        assert!(json_has(&read(&file), "mcpServers"));
        let backup = dir.join("mcp.json.mellowmesh-backup");
        assert!(read(&backup).contains("\"mine\""));
        assert!(!read(&backup).contains("mellowmesh"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
