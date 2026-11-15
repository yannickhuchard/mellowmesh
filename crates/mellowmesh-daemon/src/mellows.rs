//! Mellows made from an AI platform on this machine.
//!
//! The owner picks an installed platform the hub can pilot (Claude Code,
//! Codex, Gemini CLI), gives the Mellow a name and a purpose, and it is
//! registered on the network like any other agent. From then on a message
//! in its conversation (`_agent.<owner>.<name>.inbox`) is answered by one
//! headless turn of that platform, published back on `.outbox`.
//!
//! Safety shape: the prompt travels on standard input (never on a command
//! line), the platform runs with its own default permissions in a private
//! workspace folder, and turns for one Mellow are strictly one at a time.
//! A Mellow made this way answers in words; nothing here grants it tools.

use crate::platforms::{self, PlatformSpec};
use crate::state::AppState;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::message::Message;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

/// `app_config` key holding the bindings (JSON list).
const BINDINGS_KEY: &str = "mellow_bindings";
/// How long one headless turn may take.
const TURN_TIMEOUT: Duration = Duration::from_secs(180);
/// Exchanges kept as conversation memory (in process, not persisted).
const MEMORY_LINES: usize = 12;
/// Longest reply relayed, in characters.
const MAX_REPLY_CHARS: usize = 16_000;

/// A Mellow bound to the platform that speaks for it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Binding {
    pub agent_id: String,
    pub name: String,
    /// Catalog id of the platform (`claude-code`, `codex`, `gemini`).
    pub platform: String,
    /// The job it was made for, in the owner's words.
    pub purpose: String,
    pub inbox: String,
    pub outbox: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// One line of remembered conversation.
#[derive(Debug, Clone)]
pub struct Line {
    pub from_owner: bool,
    pub text: String,
}

struct Live {
    binding: Binding,
    /// Memory, and — being a mutex — the one-turn-at-a-time rule.
    memory: tokio::sync::Mutex<VecDeque<Line>>,
}

fn registry() -> &'static RwLock<HashMap<String, Arc<Live>>> {
    static CELL: OnceLock<RwLock<HashMap<String, Arc<Live>>>> = OnceLock::new();
    CELL.get_or_init(|| RwLock::new(HashMap::new()))
}

fn insert_live(binding: Binding) {
    registry().write().unwrap_or_else(|p| p.into_inner()).insert(
        binding.inbox.clone(),
        Arc::new(Live {
            binding,
            memory: tokio::sync::Mutex::new(VecDeque::new()),
        }),
    );
}

/// Every binding, oldest first.
pub fn list() -> Vec<Binding> {
    let mut all: Vec<Binding> = registry()
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .values()
        .map(|l| l.binding.clone())
        .collect();
    all.sort_by_key(|b| b.created_at);
    all
}

/// Load persisted bindings at startup.
pub fn load(store: &mellowmesh_store::Store) {
    let saved: Vec<Binding> = store
        .get_config(BINDINGS_KEY)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let count = saved.len();
    for b in saved {
        insert_live(b);
    }
    if count > 0 {
        tracing::info!("{count} Mellow(s) bound to platforms on this machine");
    }
}

fn persist(store: &mellowmesh_store::Store) -> anyhow::Result<()> {
    store.set_config(BINDINGS_KEY, &serde_json::to_string(&list())?)
}

/// A name as a topic-safe segment: lower case letters, digits, `_`.
pub fn slug(name: &str) -> Option<String> {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if (c == ' ' || c == '-' || c == '_') && !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').chars().take(32).collect::<String>();
    (!out.is_empty()).then_some(out)
}

/// `human://yannick` → `yannick` (topic-safe).
pub fn owner_handle(owner: &str) -> String {
    slug(owner.rsplit('/').next().unwrap_or(owner)).unwrap_or_else(|| "me".to_string())
}

/// Why a Mellow could not be made — each with words for the person.
#[derive(Debug, PartialEq, Eq)]
pub enum CreateError {
    UnknownPlatform,
    NotPilotable(&'static str),
    NotInstalled(&'static str),
    BadName,
    BadPurpose,
    Exists(String),
    Internal(String),
}

impl CreateError {
    pub fn message(&self) -> String {
        match self {
            Self::UnknownPlatform => "That AI app is not one MellowMesh knows.".into(),
            Self::NotPilotable(n) => format!(
                "{n} has no remote door, so it cannot be piloted from here. Invite it with a pasted message instead."
            ),
            Self::NotInstalled(n) => format!("{n} is not installed on this computer."),
            Self::BadName => "Give it a name with at least one letter or digit.".into(),
            Self::BadPurpose => "Say in one sentence what it is for (up to 500 characters).".into(),
            Self::Exists(n) => format!("A Mellow named {n} already exists. Pick another name."),
            Self::Internal(e) => format!("Something went wrong while making it: {e}"),
        }
    }
}

/// Make a Mellow from a platform: register it and bind its conversation.
pub async fn create(
    state: &AppState,
    platform_id: &str,
    name: &str,
    purpose: &str,
) -> Result<Binding, CreateError> {
    let spec = platforms::spec(platform_id).ok_or(CreateError::UnknownPlatform)?;
    if spec.drive.is_none() {
        return Err(CreateError::NotPilotable(spec.name));
    }
    if !platforms::is_installed(spec) {
        return Err(CreateError::NotInstalled(spec.name));
    }
    let seg = slug(name).ok_or(CreateError::BadName)?;
    let purpose = purpose.trim();
    if purpose.is_empty() || purpose.chars().count() > 500 {
        return Err(CreateError::BadPurpose);
    }
    let display = name.trim().chars().take(48).collect::<String>();
    let owner = owner_handle(&state.owner);
    let binding = Binding {
        agent_id: format!("agent://{owner}/{seg}"),
        name: display.clone(),
        platform: spec.id.to_string(),
        purpose: purpose.to_string(),
        inbox: format!("_agent.{owner}.{seg}.inbox"),
        outbox: format!("_agent.{owner}.{seg}.outbox"),
        created_at: chrono::Utc::now(),
    };
    if registry()
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .contains_key(&binding.inbox)
    {
        return Err(CreateError::Exists(display));
    }

    let agent = AgentRegistration {
        id: binding.agent_id.clone(),
        name: display.clone(),
        owner: state.owner.clone(),
        mode: "piloted".to_string(),
        // Its origin, readable by every client: "made from <platform>".
        capabilities: vec![format!("from:{}", spec.id), "mellow".to_string()],
    };
    state
        .store
        .register_agent(&agent)
        .map_err(|e| CreateError::Internal(e.to_string()))?;
    state.caches.agents.invalidate();
    insert_live(binding.clone());
    persist(&state.store).map_err(|e| CreateError::Internal(e.to_string()))?;
    tracing::info!(
        "Mellow '{}' ({}) made from {}",
        binding.name,
        binding.agent_id,
        spec.name
    );

    // Tell the fabric (clients refresh their directory), then say hello.
    let shared = Arc::new(state.clone());
    let mut headers = HashMap::new();
    headers.insert("x-registry-action".to_string(), "register".to_string());
    let _ = crate::handlers::message::handle_publish(
        shared.clone(),
        message(
            "_system.registry.agent",
            &format!("agent://{}/daemon", state.node_id),
            "application/json",
            serde_json::to_string(&agent).unwrap_or_default(),
            Some(headers),
        ),
    )
    .await;
    let hello = format!(
        "Hi! I'm **{}**, made from {} on this computer. My job: {} Write to me here whenever you like.",
        binding.name,
        spec.name,
        ensure_sentence(&binding.purpose)
    );
    let _ = crate::handlers::message::handle_publish(
        shared,
        message(&binding.outbox, &binding.agent_id, "text/markdown", hello, None),
    )
    .await;
    Ok(binding)
}

fn ensure_sentence(s: &str) -> String {
    let t = s.trim();
    if t.ends_with(['.', '!', '?']) {
        t.to_string()
    } else {
        format!("{t}.")
    }
}

fn message(
    topic: &str,
    from: &str,
    content_type: &str,
    body: String,
    headers: Option<HashMap<String, String>>,
) -> Message {
    Message {
        id: format!("msg_{}", ulid::Ulid::new().to_string().to_lowercase()),
        topic: topic.to_string(),
        from: from.to_string(),
        owner: None,
        timestamp: chrono::Utc::now(),
        content_type: content_type.to_string(),
        body,
        headers,
        payload: None,
        parent_id: None,
    }
}

/// Called for every routed message: a message in a bound Mellow's inbox
/// (from anyone but itself) starts one turn, off the publish path.
pub fn maybe_dispatch(state: &Arc<AppState>, msg: &Message) {
    let live = match registry()
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .get(&msg.topic)
    {
        Some(l) => l.clone(),
        None => return,
    };
    if msg.from == live.binding.agent_id || msg.body.trim().is_empty() {
        return;
    }
    let state = state.clone();
    let msg = msg.clone();
    tokio::spawn(async move {
        run_turn(state, live, msg).await;
    });
}

async fn run_turn(state: Arc<AppState>, live: Arc<Live>, msg: Message) {
    let mut memory = live.memory.lock().await;
    let b = &live.binding;
    let reply = match platforms::spec(&b.platform) {
        None => Err("its platform is no longer known to this hub".to_string()),
        Some(spec) => {
            let prompt = build_prompt(b, memory.make_contiguous(), &msg.from, &msg.body);
            run_platform(spec, &prompt, b).await
        }
    };
    memory.push_back(Line {
        from_owner: true,
        text: msg.body.clone(),
    });
    let body = match reply {
        Ok(text) => {
            memory.push_back(Line {
                from_owner: false,
                text: text.clone(),
            });
            text
        }
        Err(why) => {
            tracing::warn!("Mellow {} could not answer: {}", b.agent_id, why);
            format!("I could not answer just now — {why}. Try again in a moment.")
        }
    };
    while memory.len() > MEMORY_LINES {
        memory.pop_front();
    }
    drop(memory);
    let mut out = message(&b.outbox, &b.agent_id, "text/markdown", body, None);
    out.parent_id = Some(msg.id.clone());
    let _ = crate::handlers::message::handle_publish(state, out).await;
}

/// The whole prompt for one turn: who the Mellow is, what was said, the
/// new message. Pure, so it can be tested.
pub fn build_prompt(b: &Binding, memory: &[Line], from: &str, body: &str) -> String {
    let mut p = format!(
        "You are \"{}\", a helper on a personal MellowMesh network. Your job: {}\n\
         You are answering a chat message from your owner's phone or desktop.\n\
         Rules: answer in plain words or Markdown, be concise and concrete. You are \
         running without tool approvals: do not claim to have run commands, edited \
         files or sent anything. If the job needs an action on the computer, say \
         exactly what you would do and ask for the OK.\n",
        b.name,
        ensure_sentence(&b.purpose)
    );
    if !memory.is_empty() {
        p.push_str("\nConversation so far:\n");
        for line in memory {
            let who = if line.from_owner { "Owner" } else { "You" };
            p.push_str(&format!("{who}: {}\n", line.text.trim()));
        }
    }
    p.push_str(&format!("\nNew message from {from}:\n{}\n", body.trim()));
    p
}

/// A git-bash for Claude Code on Windows, when the environment names none.
#[cfg(windows)]
fn find_git_bash() -> Option<std::path::PathBuf> {
    if let Some(git) = platforms::which("git") {
        // <root>\cmd\git.exe or <root>\bin\git.exe → <root>\bin\bash.exe
        if let Some(root) = git.parent().and_then(|p| p.parent()) {
            let bash = root.join("bin").join("bash.exe");
            if bash.is_file() {
                return Some(bash);
            }
        }
    }
    platforms::which("bash")
}

/// One headless turn of a platform: prompt on stdin, answer on stdout.
async fn run_platform(spec: &PlatformSpec, prompt: &str, b: &Binding) -> Result<String, String> {
    let args = spec.drive.ok_or("this platform cannot be piloted")?;
    let exe = spec
        .bins
        .iter()
        .find_map(|bin| platforms::which(bin))
        .ok_or_else(|| format!("{} is not installed on this computer any more", spec.name))?;

    // Its own folder, so a platform that writes files does it away from yours.
    let seg = b.agent_id.rsplit('/').next().unwrap_or("mellow");
    let workspace = std::env::temp_dir().join("mellowmesh").join("mellows").join(seg);
    let _ = std::fs::create_dir_all(&workspace);
    run_command(&exe, args, prompt, &workspace, spec.name).await
}

/// Run one program for one turn: `prompt` on stdin, its answer on stdout.
async fn run_command(
    exe: &std::path::Path,
    args: &[&str],
    prompt: &str,
    workspace: &std::path::Path,
    name: &str,
) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;
    let is_script = exe
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    // A .cmd shim (npm-installed CLIs) must go through cmd; the arguments
    // are constants from the catalog, the prompt never touches this line.
    let mut cmd = if is_script {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(&exe);
        c
    } else {
        tokio::process::Command::new(&exe)
    };
    cmd.args(args)
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    if std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH").is_none() {
        if let Some(bash) = find_git_bash() {
            cmd.env("CLAUDE_CODE_GIT_BASH_PATH", bash);
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{} could not be started ({e})", name))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        // Dropping stdin closes it: the platform sees the end of the prompt.
    }
    let output = tokio::time::timeout(TURN_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| format!("{} took too long to answer", name))?
        .map_err(|e| format!("{} stopped unexpectedly ({e})", name))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() && !stdout.is_empty() {
        return Ok(stdout.chars().take(MAX_REPLY_CHARS).collect());
    }
    // The platform's own words are the most useful thing to relay
    // ("not logged in", "credit balance is too low"…), kept short.
    let said = if !stdout.is_empty() { stdout } else { stderr };
    let first = said.lines().find(|l| !l.trim().is_empty()).unwrap_or("no answer");
    Err(format!(
        "{} said: {}",
        name,
        first.chars().take(240).collect::<String>()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Persona simulation — Karim, on the tram, writes to the Mellow he
    /// made. A stand-in program plays the platform (`sort` reads stdin and
    /// prints it back on every OS): the turn must carry his words in on
    /// stdin and bring the platform's words back, untouched by any shell.
    #[tokio::test]
    async fn karim_writes_and_the_platform_answers_through_stdin() {
        let Some(stand_in) = platforms::which("sort") else {
            return; // no stand-in on this machine: nothing to simulate
        };
        let workspace = std::env::temp_dir().join("mellowmesh-test-karim");
        let _ = std::fs::create_dir_all(&workspace);
        let tricky = "Look at PR 12 & tell me; what about \"quotes\" | pipes > files?";
        let got = run_command(&stand_in, &[], tricky, &workspace, "Stand-in").await;
        assert_eq!(got.as_deref(), Ok(tricky));
    }

    /// The platform's own refusal reaches Karim in its words, kept short.
    #[tokio::test]
    async fn a_platform_that_fails_is_quoted_not_hidden() {
        let Some(stand_in) = platforms::which("sort") else {
            return;
        };
        let workspace = std::env::temp_dir().join("mellowmesh-test-karim");
        let _ = std::fs::create_dir_all(&workspace);
        // An option `sort` does not know: it exits non-zero with a message.
        let got = run_command(&stand_in, &["--mellowmesh-nope"], "hi", &workspace, "Stand-in").await;
        let err = got.expect_err("an unknown option must fail");
        assert!(err.starts_with("Stand-in said: "), "{err}");
    }

    fn binding() -> Binding {
        Binding {
            agent_id: "agent://karim/reviewer".into(),
            name: "Reviewer".into(),
            platform: "claude-code".into(),
            purpose: "Review my pull requests".into(),
            inbox: "_agent.karim.reviewer.inbox".into(),
            outbox: "_agent.karim.reviewer.outbox".into(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn names_become_topic_safe_segments() {
        assert_eq!(slug("Reviewer").as_deref(), Some("reviewer"));
        assert_eq!(slug("  PR  Review-Bot ").as_deref(), Some("pr_review_bot"));
        assert_eq!(slug("Été 2026 !").as_deref(), Some("t_2026"));
        assert_eq!(slug("…").as_deref(), None);
        assert_eq!(owner_handle("human://Yannick"), "yannick");
    }

    #[test]
    fn the_prompt_carries_identity_purpose_memory_and_the_new_message() {
        let memory = vec![
            Line { from_owner: true, text: "Hello".into() },
            Line { from_owner: false, text: "Hi Karim".into() },
        ];
        let p = build_prompt(&binding(), &memory, "human://karim", "Look at PR 12");
        assert!(p.contains("\"Reviewer\""));
        assert!(p.contains("Your job: Review my pull requests."));
        assert!(p.contains("Owner: Hello\nYou: Hi Karim"));
        assert!(p.trim_end().ends_with("Look at PR 12"));
        assert!(p.contains("ask for the OK"), "a piloted Mellow never acts on its own");
    }

    #[test]
    fn a_first_message_has_no_history_block() {
        let p = build_prompt(&binding(), &[], "human://karim", "Hi");
        assert!(!p.contains("Conversation so far"));
    }

    #[test]
    fn every_refusal_has_plain_words() {
        for e in [
            CreateError::UnknownPlatform,
            CreateError::NotPilotable("ChatGPT"),
            CreateError::NotInstalled("Codex"),
            CreateError::BadName,
            CreateError::BadPurpose,
            CreateError::Exists("Reviewer".into()),
        ] {
            let m = e.message();
            assert!(!m.is_empty() && !m.contains("Error") && !m.contains("null"));
        }
    }
}
