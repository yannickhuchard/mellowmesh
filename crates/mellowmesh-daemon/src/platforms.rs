//! AI platform discovery: which assistants this machine has, and which run.
//!
//! The hub looks, locally and on its own machine only, at two things:
//! the NAMES of running processes (never their windows, arguments, files or
//! text) and whether a known program is present (on `PATH`, or at its usual
//! install location). Both are matched against a small catalog (Claude Code,
//! Codex, Gemini CLI, Ollama, …). Nothing is sent anywhere; the result is
//! served only to the owner (`GET /platforms`).
//!
//! Three plain states come out of it: *working right now*, *installed,
//! asleep*, *not on this computer* — plus whether the hub can pilot the
//! platform headless (`can_chat`), which is what lets a phone make a Mellow
//! out of it (see `mellows.rs`).

use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

/// How a platform joins the network once spotted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteKind {
    /// A one-line command typed where the platform runs (MCP stdio).
    Command,
    /// A message pasted into the platform's chat: it carries a one-time
    /// claim code and the hub address (remote MCP). For assistants with
    /// no terminal (ChatGPT, GrokBot, Claude Mobile…).
    Paste,
    /// Runs models locally; joins through a connector, not MCP.
    None,
}

/// One catalog entry: a platform the hub knows how to recognize.
pub struct PlatformSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// Process names that count as this platform, compared without case
    /// and without a trailing `.exe`.
    pub processes: &'static [&'static str],
    /// Program names looked up on `PATH` to tell "installed".
    pub bins: &'static [&'static str],
    /// Usual install locations (`%VAR%` expanded) for apps not on `PATH`.
    pub paths: &'static [&'static str],
    pub invite: InviteKind,
    /// The connect command, for `InviteKind::Command`.
    pub command: &'static str,
    /// Arguments for one headless turn; the prompt goes on standard input,
    /// never on the command line. `None`: the hub cannot pilot it.
    pub drive: Option<&'static [&'static str]>,
}

/// The platforms the hub recognizes.
pub const CATALOG: &[PlatformSpec] = &[
    PlatformSpec {
        id: "claude-code",
        name: "Claude Code",
        processes: &["claude"],
        bins: &["claude"],
        paths: &[],
        invite: InviteKind::Command,
        command: "claude mcp add mellowmesh -- mellowmesh mcp",
        drive: Some(&["-p"]),
    },
    PlatformSpec {
        id: "codex",
        name: "Codex",
        processes: &["codex"],
        bins: &["codex"],
        paths: &[],
        invite: InviteKind::Command,
        command: "codex mcp add mellowmesh -- mellowmesh mcp",
        drive: Some(&["exec", "--skip-git-repo-check", "-"]),
    },
    PlatformSpec {
        id: "gemini",
        name: "Gemini CLI",
        processes: &["gemini"],
        bins: &["gemini"],
        paths: &[],
        invite: InviteKind::Command,
        command: "gemini mcp add mellowmesh mellowmesh mcp",
        drive: Some(&["-p", "Answer the conversation given on standard input."]),
    },
    PlatformSpec {
        id: "antigravity",
        name: "Antigravity",
        processes: &["antigravity"],
        bins: &["antigravity"],
        paths: &["%LOCALAPPDATA%\\Programs\\Antigravity\\Antigravity.exe"],
        invite: InviteKind::Paste,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "chatgpt",
        name: "ChatGPT",
        processes: &["chatgpt"],
        bins: &["chatgpt"],
        paths: &[],
        invite: InviteKind::Paste,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "claude-desktop",
        name: "Claude Desktop",
        // Its processes are also named "claude": they cannot be told from
        // Claude Code by name, so only its presence is reported.
        processes: &[],
        bins: &[],
        paths: &["%LOCALAPPDATA%\\AnthropicClaude\\claude.exe"],
        invite: InviteKind::Paste,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "grok",
        name: "Grok / GrokBot",
        processes: &["grok", "grokbot"],
        bins: &["grok"],
        paths: &[],
        invite: InviteKind::Paste,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "mistral",
        name: "Mistral — Le Chat",
        processes: &["lechat", "le chat", "mistral"],
        bins: &["mistral"],
        paths: &[],
        invite: InviteKind::Paste,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "cursor",
        name: "Cursor",
        processes: &["cursor"],
        bins: &["cursor"],
        paths: &["%LOCALAPPDATA%\\Programs\\cursor\\Cursor.exe"],
        invite: InviteKind::Paste,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "windsurf",
        name: "Windsurf",
        processes: &["windsurf"],
        bins: &["windsurf"],
        paths: &["%LOCALAPPDATA%\\Programs\\Windsurf\\Windsurf.exe"],
        invite: InviteKind::Paste,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "github-copilot",
        name: "GitHub Copilot (VS Code)",
        processes: &["code"],
        bins: &["code"],
        paths: &["%LOCALAPPDATA%\\Programs\\Microsoft VS Code\\Code.exe"],
        invite: InviteKind::None,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "warp",
        name: "Warp",
        processes: &["warp"],
        bins: &["warp"],
        paths: &["%LOCALAPPDATA%\\Programs\\Warp\\warp.exe"],
        invite: InviteKind::None,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "mistral-vibe",
        name: "Mistral Vibe",
        processes: &["vibe"],
        bins: &["vibe"],
        paths: &[],
        invite: InviteKind::None,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "kimi",
        name: "Kimi Code",
        processes: &["kimi"],
        bins: &["kimi"],
        paths: &[],
        invite: InviteKind::None,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "pi",
        name: "Pi",
        processes: &["pi"],
        bins: &["pi"],
        paths: &[],
        invite: InviteKind::None,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "deepseek",
        name: "DeepSeek",
        processes: &[],
        bins: &[],
        paths: &[],
        invite: InviteKind::Paste,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "moonshot",
        name: "Moonshot Platform (Kimi)",
        processes: &[],
        bins: &[],
        paths: &[],
        invite: InviteKind::Paste,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "ollama",
        name: "Ollama (local models)",
        processes: &["ollama", "ollama app"],
        bins: &["ollama"],
        paths: &["%LOCALAPPDATA%\\Programs\\Ollama\\ollama.exe"],
        invite: InviteKind::None,
        command: "",
        drive: None,
    },
    PlatformSpec {
        id: "lmstudio",
        name: "LM Studio (local models)",
        processes: &["lm studio", "lmstudio"],
        bins: &["lms"],
        paths: &["%LOCALAPPDATA%\\Programs\\LM Studio\\LM Studio.exe"],
        invite: InviteKind::None,
        command: "",
        drive: None,
    },
];

/// The catalog entry for an id.
pub fn spec(id: &str) -> Option<&'static PlatformSpec> {
    CATALOG.iter().find(|s| s.id == id)
}

/// One platform's observed state.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlatformStatus {
    pub id: &'static str,
    pub name: &'static str,
    /// Running process count. 0 = not running.
    pub sessions: u32,
    pub running: bool,
    /// Present on this machine (running counts as installed).
    pub installed: bool,
    /// The hub can pilot it headless: a Mellow can be made from it.
    pub can_chat: bool,
    pub invite: InviteKind,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub command: &'static str,
}

/// Normalize one process name for matching: lower case, no `.exe`.
fn norm(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    n.strip_suffix(".exe").unwrap_or(&n).to_string()
}

/// Match process names and install facts against the catalog. Pure: tests
/// feed any process list and any "is this spec installed?" answer.
pub fn detect_with(
    process_names: &[String],
    installed: impl Fn(&PlatformSpec) -> bool,
) -> Vec<PlatformStatus> {
    let normed: Vec<String> = process_names.iter().map(|n| norm(n)).collect();
    CATALOG
        .iter()
        .map(|spec| {
            let sessions = normed
                .iter()
                .filter(|p| spec.processes.contains(&p.as_str()))
                .count() as u32;
            let running = sessions > 0;
            let present = running || installed(spec);
            PlatformStatus {
                id: spec.id,
                name: spec.name,
                sessions,
                running,
                installed: present,
                // Piloting needs the program itself, not just a process.
                can_chat: spec.drive.is_some() && installed(spec),
                invite: spec.invite,
                command: spec.command,
            }
        })
        .collect()
}

/// Match a list of process names against the catalog, nothing installed.
pub fn detect(process_names: &[String]) -> Vec<PlatformStatus> {
    detect_with(process_names, |_| false)
}

/// Expand `%VAR%` segments from the environment (unknown ones vanish).
fn expand_env(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        match rest[start + 1..].find('%') {
            Some(len) => {
                let var = &rest[start + 1..start + 1 + len];
                out.push_str(&std::env::var(var).unwrap_or_default());
                rest = &rest[start + 2 + len..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Find a program on `PATH` (with `PATHEXT` on Windows). The full path,
/// extension included — a `.cmd` shim must be run through `cmd`.
pub fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_string())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{bin}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Whether a catalog entry's program is present on this machine: on
/// `PATH`, at its usual place, or having left its settings folder behind.
pub fn is_installed(spec: &PlatformSpec) -> bool {
    spec.bins.iter().any(|b| which(b).is_some())
        || crate::connect::has_settings_folder(spec.id)
        || spec
            .paths
            .iter()
            .any(|p| std::path::Path::new(&expand_env(p)).exists())
}

/// The machine's running process names, by the lightest native means.
/// Only names leave this function; arguments and windows are never read.
/// Standard input is detached: a hub started without a console (a service,
/// `mellowmesh daemon start`) has no valid stdin to hand down, and the
/// listing would silently fail.
pub fn running_process_names() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        // CSV, no header: "name","pid",...
        let out = std::process::Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.split('"').nth(1).map(str::to_string))
                .collect(),
            Err(e) => {
                tracing::warn!("Could not list running programs: {e}");
                Vec::new()
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let out = std::process::Command::new("ps")
            .args(["-axo", "comm="])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| {
                    // Keep the program name only, no path.
                    l.trim().rsplit('/').next().map(str::to_string)
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Could not list running programs: {e}");
                Vec::new()
            }
        }
    }
}

/// Latest snapshot: the statuses and when the hub last looked.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Snapshot {
    pub platforms: Vec<PlatformStatus>,
    pub looked_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn snapshot_cell() -> &'static RwLock<Snapshot> {
    static CELL: OnceLock<RwLock<Snapshot>> = OnceLock::new();
    CELL.get_or_init(|| RwLock::new(Snapshot::default()))
}

/// The latest snapshot (empty until the first scan).
pub fn snapshot() -> Snapshot {
    snapshot_cell()
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

fn store_snapshot(platforms: Vec<PlatformStatus>) {
    *snapshot_cell().write().unwrap_or_else(|p| p.into_inner()) = Snapshot {
        platforms,
        looked_at: Some(chrono::Utc::now()),
    };
}

/// Scan once, now (on the blocking pool), and remember the result.
pub async fn scan_now() -> Snapshot {
    let statuses =
        tokio::task::spawn_blocking(|| detect_with(&running_process_names(), is_installed))
            .await
            .unwrap_or_default();
    store_snapshot(statuses);
    snapshot()
}

/// Background rhythm: look every 20 seconds.
pub fn start() {
    tokio::spawn(async {
        loop {
            let _ = scan_now().await;
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn spots_claude_code_sessions_case_and_exe_insensitive() {
        let got = detect(&s(&["Claude.EXE", "claude", "notepad.exe"]));
        let cc = got.iter().find(|p| p.id == "claude-code").unwrap();
        assert!(cc.running);
        assert_eq!(cc.sessions, 2);
        assert!(cc.installed, "a running platform is on this machine");
    }

    #[test]
    fn unseen_platforms_report_zero_but_stay_listed() {
        let got = detect(&s(&["explorer.exe"]));
        assert_eq!(got.len(), CATALOG.len());
        assert!(got.iter().all(|p| !p.running && p.sessions == 0));
    }

    #[test]
    fn similar_names_do_not_false_positive() {
        // "cursor" the editor, not "precursor.exe"; "grok" not "grokking_notes".
        let got = detect(&s(&[
            "precursor.exe",
            "grokking_notes",
            "Codecs.exe",
            "pipeline",
        ]));
        assert!(got.iter().all(|p| !p.running));
    }

    #[test]
    fn command_platforms_carry_their_connect_command() {
        let got = detect(&s(&["codex"]));
        let cx = got.iter().find(|p| p.id == "codex").unwrap();
        assert_eq!(cx.invite, InviteKind::Command);
        assert!(cx.command.contains("mcp add mellowmesh"));
    }

    #[test]
    fn installed_but_asleep_is_its_own_state() {
        let got = detect_with(&s(&["explorer.exe"]), |spec| spec.id == "gemini");
        let g = got.iter().find(|p| p.id == "gemini").unwrap();
        assert!(g.installed && !g.running);
        assert!(
            g.can_chat,
            "an installed CLI with a headless mode can be piloted"
        );
        let c = got.iter().find(|p| p.id == "claude-code").unwrap();
        assert!(!c.installed && !c.can_chat);
    }

    #[test]
    fn a_desktop_app_is_never_pilotable() {
        let got = detect_with(&s(&["ChatGPT.exe"]), |_| true);
        let c = got.iter().find(|p| p.id == "chatgpt").unwrap();
        assert!(c.running && c.installed && !c.can_chat);
    }

    #[test]
    fn env_segments_expand_and_unknown_ones_vanish() {
        std::env::set_var("MM_TEST_HOME", "C:\\Users\\k");
        assert_eq!(expand_env("%MM_TEST_HOME%\\x.exe"), "C:\\Users\\k\\x.exe");
        assert_eq!(expand_env("%MM_NOPE_NOPE%\\x"), "\\x");
        assert_eq!(expand_env("plain"), "plain");
    }
}
