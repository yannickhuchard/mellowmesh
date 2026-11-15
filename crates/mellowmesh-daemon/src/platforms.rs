//! AI platform discovery: which assistants run on the hub's machine.
//!
//! The hub looks, locally and on its own machine only, at the NAMES of
//! running processes — never their windows, arguments, files or text — and
//! matches them against a small catalog (Claude Code, Codex, Gemini CLI,
//! Ollama, …). Nothing is sent anywhere; the result is served only to the
//! owner (`GET /platforms`). This is what lets the dashboard say "Claude
//! Code is working right now — invite it?" instead of asking the user to
//! know and type everything themselves.

use serde::Serialize;
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
    pub invite: InviteKind,
    /// The connect command, for `InviteKind::Command`.
    pub command: &'static str,
}

/// The platforms the hub recognizes. Matching is by process NAME only.
pub const CATALOG: &[PlatformSpec] = &[
    PlatformSpec {
        id: "claude-code",
        name: "Claude Code",
        processes: &["claude"],
        invite: InviteKind::Command,
        command: "claude mcp add mellowmesh -- mellowmesh mcp",
    },
    PlatformSpec {
        id: "claude-desktop",
        name: "Claude Desktop",
        processes: &["claude desktop", "claude-desktop", "claude.app"],
        invite: InviteKind::Paste,
        command: "",
    },
    PlatformSpec {
        id: "codex",
        name: "Codex",
        processes: &["codex"],
        invite: InviteKind::Command,
        command: "codex mcp add mellowmesh -- mellowmesh mcp",
    },
    PlatformSpec {
        id: "gemini",
        name: "Gemini CLI",
        processes: &["gemini"],
        invite: InviteKind::Command,
        command: "gemini mcp add mellowmesh mellowmesh mcp",
    },
    PlatformSpec {
        id: "antigravity",
        name: "Antigravity",
        processes: &["antigravity"],
        invite: InviteKind::Paste,
        command: "",
    },
    PlatformSpec {
        id: "chatgpt",
        name: "ChatGPT",
        processes: &["chatgpt"],
        invite: InviteKind::Paste,
        command: "",
    },
    PlatformSpec {
        id: "grok",
        name: "Grok / GrokBot",
        processes: &["grok", "grokbot"],
        invite: InviteKind::Paste,
        command: "",
    },
    PlatformSpec {
        id: "mistral",
        name: "Mistral — Le Chat",
        processes: &["lechat", "le chat", "mistral"],
        invite: InviteKind::Paste,
        command: "",
    },
    PlatformSpec {
        id: "cursor",
        name: "Cursor",
        processes: &["cursor"],
        invite: InviteKind::Paste,
        command: "",
    },
    PlatformSpec {
        id: "windsurf",
        name: "Windsurf",
        processes: &["windsurf"],
        invite: InviteKind::Paste,
        command: "",
    },
    PlatformSpec {
        id: "ollama",
        name: "Ollama (local models)",
        processes: &["ollama", "ollama app"],
        invite: InviteKind::None,
        command: "",
    },
    PlatformSpec {
        id: "lmstudio",
        name: "LM Studio (local models)",
        processes: &["lm studio", "lmstudio"],
        invite: InviteKind::None,
        command: "",
    },
];

/// One platform's observed state.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlatformStatus {
    pub id: &'static str,
    pub name: &'static str,
    /// Running process count — "sessions" in plain words. 0 = not seen.
    pub sessions: u32,
    pub running: bool,
    pub invite: InviteKind,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub command: &'static str,
}

/// Normalize one process name for matching: lower case, no `.exe`.
fn norm(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    n.strip_suffix(".exe").unwrap_or(&n).to_string()
}

/// Match a list of process names against the catalog. Pure, so tests can
/// feed any list.
pub fn detect(process_names: &[String]) -> Vec<PlatformStatus> {
    let normed: Vec<String> = process_names.iter().map(|n| norm(n)).collect();
    CATALOG
        .iter()
        .map(|spec| {
            let sessions = normed
                .iter()
                .filter(|p| spec.processes.iter().any(|want| p.as_str() == *want))
                .count() as u32;
            PlatformStatus {
                id: spec.id,
                name: spec.name,
                sessions,
                running: sessions > 0,
                invite: spec.invite,
                command: spec.command,
            }
        })
        .collect()
}

/// The machine's running process names, by the lightest native means.
/// Only names leave this function; arguments and windows are never read.
pub fn running_process_names() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        // CSV, no header: "name","pid",...
        let out = std::process::Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.split('"').nth(1).map(str::to_string))
                .collect(),
            Err(_) => Vec::new(),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let out = std::process::Command::new("ps").args(["-axo", "comm="]).output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| {
                    // Keep the program name only, no path.
                    l.trim().rsplit('/').next().map(str::to_string)
                })
                .collect(),
            Err(_) => Vec::new(),
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
    snapshot_cell().read().unwrap_or_else(|p| p.into_inner()).clone()
}

fn store_snapshot(platforms: Vec<PlatformStatus>) {
    *snapshot_cell().write().unwrap_or_else(|p| p.into_inner()) = Snapshot {
        platforms,
        looked_at: Some(chrono::Utc::now()),
    };
}

/// Scan once, now (on the blocking pool), and remember the result.
pub async fn scan_now() -> Snapshot {
    let names = tokio::task::spawn_blocking(running_process_names)
        .await
        .unwrap_or_default();
    store_snapshot(detect(&names));
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
        let got = detect(&s(&["precursor.exe", "grokking_notes", "Code.exe"]));
        assert!(got.iter().all(|p| !p.running));
    }

    #[test]
    fn command_platforms_carry_their_connect_command() {
        let got = detect(&s(&["codex"]));
        let cx = got.iter().find(|p| p.id == "codex").unwrap();
        assert_eq!(cx.invite, InviteKind::Command);
        assert!(cx.command.contains("mcp add mellowmesh"));
    }
}
