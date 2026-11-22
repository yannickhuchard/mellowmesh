//! Email: the hub's own mailbox, read and written by the hub itself.
//!
//! The owner gives the hub an address and an app password. The hub then
//! reads its inbox (IMAP) every few seconds and sends from it (SMTP):
//!
//! * **Commands by email.** A person on the hub's list writes
//!   `@coder run the tests`, `task: …`, `status` or `help`. A note reaches
//!   the agent's mailbox; the agent's answer (any message published with
//!   the note as its `parent_id`) comes back in the same email thread.
//! * **Requests by email.** A decision addressed to a listed person is
//!   emailed with numbered options; replying `1`, a name, *yes* or *no*
//!   answers it (recorded as `human://… (via interface://local/email)`).
//! * **Agents email people.** An agent may email a person on the list —
//!   never any address — and the reply comes back to its mailbox.
//!
//! A sender's address is easy to forge, so a command runs only when the
//! email replies to one the hub sent to that address (its Message-ID is
//! unguessable), or when the person's provider vouched for it (DMARC or an
//! aligned DKIM pass in its `Authentication-Results`). Otherwise the hub
//! asks the real address to confirm. Email from anyone not on the list is
//! ignored without an answer.

pub mod command;
pub mod transport;

use crate::auth::{admin_refusal, is_admin, AuthContext, AuthPrincipal};
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Extension, Json};
use chrono::{DateTime, Utc};
use command::Command;
use mellowmesh_core::decision::Decision;
use mellowmesh_core::message::Message;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use transport::{Cursor, Outgoing, Transport};

const SETTINGS_KEY: &str = "email_settings";
const SECRET_KEY: &str = "email_secret";
const MEMORY_KEY: &str = "email_memory";
/// Set in the hub's environment, the app password never touches the disk.
pub const PASSWORD_ENV: &str = "MELLOWMESH_EMAIL_PASSWORD";
/// Identity mappings `email://<address>` → `human://…` are the hub's list
/// of people who may write to it.
const EMAIL_SCHEME: &str = "email://";
/// The interface that relays answers given by email.
pub const INTERFACE: &str = "interface://local/email";
/// Threads, notes and handled Message-IDs remembered.
const KEEP: usize = 500;
/// How long an unconfirmed command waits for its confirmation.
const CONFIRM_FOR: chrono::Duration = chrono::Duration::minutes(30);
/// Emails the hub sends per hour, and of those, emails agents ask for.
const SEND_PER_HOUR: usize = 60;
const AGENT_SEND_PER_HOUR: usize = 20;
const MAX_SUBJECT: usize = 200;
const MAX_BODY: usize = 20_000;

/// How a connection to a mail server is protected.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Security {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    #[default]
    Tls,
    /// Plain, then upgraded with STARTTLS (IMAP 143, SMTP 587).
    Starttls,
    /// No encryption: only for a server on this machine.
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Server {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub security: Security,
}

impl Server {
    fn new(host: &str, port: u16, security: Security) -> Self {
        Self {
            host: host.to_string(),
            port,
            security,
        }
    }
}

/// The hub's mailbox and what it does with it. The password is kept apart.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// The hub's own address.
    pub address: String,
    /// Login, when it differs from the address.
    pub username: String,
    pub imap: Server,
    pub smtp: Server,
    pub folder: String,
    pub poll_seconds: u64,
    /// Email decisions to the person who must answer them.
    pub notify_decisions: bool,
    /// Email a person when an agent @mentions them.
    pub notify_mentions: bool,
    /// Accept the hub's provider's word (DMARC/DKIM pass) as proof.
    pub trust_provider: bool,
    /// The authserv-id the hub's provider writes in `Authentication-Results`
    /// (`mx.google.com` for Gmail). Empty: no provider's word is taken, and
    /// every command not sent as a reply is confirmed by reply first.
    pub provider_id: String,
}

/// The authserv-id of the providers known to add (and police) their own
/// `Authentication-Results` header on delivery.
pub fn provider_id_for(domain: &str) -> &'static str {
    match domain.trim().to_lowercase().as_str() {
        "gmail.com" | "googlemail.com" => "mx.google.com",
        _ => "",
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            address: String::new(),
            username: String::new(),
            imap: Server::default(),
            smtp: Server::default(),
            folder: "INBOX".into(),
            poll_seconds: 30,
            notify_decisions: true,
            notify_mentions: false,
            trust_provider: true,
            provider_id: String::new(),
        }
    }
}

impl Settings {
    pub fn login(&self) -> &str {
        if self.username.trim().is_empty() {
            &self.address
        } else {
            &self.username
        }
    }
}

/// Servers of the common providers, from the address's domain.
pub fn preset(domain: &str) -> (Server, Server) {
    use Security::*;
    let d = domain.trim().to_lowercase();
    let (imap, smtp) = match d.as_str() {
        "gmail.com" | "googlemail.com" => (
            Server::new("imap.gmail.com", 993, Tls),
            Server::new("smtp.gmail.com", 465, Tls),
        ),
        "outlook.com" | "hotmail.com" | "hotmail.fr" | "live.com" | "live.fr" | "msn.com" => (
            Server::new("outlook.office365.com", 993, Tls),
            Server::new("smtp-mail.outlook.com", 587, Starttls),
        ),
        "icloud.com" | "me.com" | "mac.com" => (
            Server::new("imap.mail.me.com", 993, Tls),
            Server::new("smtp.mail.me.com", 587, Starttls),
        ),
        "yahoo.com" | "yahoo.fr" | "ymail.com" => (
            Server::new("imap.mail.yahoo.com", 993, Tls),
            Server::new("smtp.mail.yahoo.com", 465, Tls),
        ),
        "fastmail.com" | "fastmail.fm" => (
            Server::new("imap.fastmail.com", 993, Tls),
            Server::new("smtp.fastmail.com", 465, Tls),
        ),
        "gmx.com" | "gmx.fr" => (
            Server::new("imap.gmx.com", 993, Tls),
            Server::new("mail.gmx.com", 587, Starttls),
        ),
        "gmx.net" | "gmx.de" => (
            Server::new("imap.gmx.net", 993, Tls),
            Server::new("mail.gmx.net", 587, Starttls),
        ),
        "zoho.com" | "zohomail.com" => (
            Server::new("imap.zoho.com", 993, Tls),
            Server::new("smtp.zoho.com", 465, Tls),
        ),
        "orange.fr" | "wanadoo.fr" => (
            Server::new("imap.orange.fr", 993, Tls),
            Server::new("smtp.orange.fr", 465, Tls),
        ),
        "free.fr" => (
            Server::new("imap.free.fr", 993, Tls),
            Server::new("smtp.free.fr", 465, Tls),
        ),
        "laposte.net" => (
            Server::new("imap.laposte.net", 993, Tls),
            Server::new("smtp.laposte.net", 465, Tls),
        ),
        _ if !d.is_empty() => (
            Server::new(&format!("imap.{d}"), 993, Tls),
            Server::new(&format!("smtp.{d}"), 465, Tls),
        ),
        _ => (Server::default(), Server::default()),
    };
    (imap, smtp)
}

/// How the mailbox is doing.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    pub last_check: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    /// Emails from people on the list.
    pub received: u64,
    /// Emails from anyone else, and automatic replies.
    pub ignored: u64,
    pub sent: u64,
}

/// An email the hub sent, that a reply can answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Thread {
    message_id: String,
    to: String,
    person: String,
    subject: String,
    kind: ThreadKind,
    at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ThreadKind {
    /// A decision to answer.
    Decision { decision_id: String },
    /// A conversation with an agent: a reply is a new note to it.
    Agent { agent_id: String, inbox: String },
    /// A mention: a reply is posted in the topic.
    Topic { topic: String },
    /// A command waiting for the sender to confirm it.
    Confirm { command: Command },
    /// Nothing to answer (help, status, a test email).
    Info,
}

/// A note delivered by email, whose answer goes back by email.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NoteLink {
    note_id: String,
    agent_id: String,
    inbox: String,
    to: String,
    person: String,
    subject: String,
    /// The person's email the answer replies to.
    reply_to: String,
    references: Vec<String>,
    at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Memory {
    #[serde(default)]
    cursor: Option<Cursor>,
    #[serde(default)]
    threads: VecDeque<Thread>,
    #[serde(default)]
    notes: VecDeque<NoteLink>,
    /// Message-IDs of emails already handled.
    #[serde(default)]
    seen: VecDeque<String>,
}

/// Per-hub email state (lives in [`crate::access::AccessState`]).
#[derive(Default)]
pub struct Runtime {
    settings: RwLock<Option<Arc<Settings>>>,
    memory: Mutex<Option<Memory>>,
    status: Mutex<Status>,
    wake: tokio::sync::Notify,
    fake: RwLock<Option<Arc<transport::Fake>>>,
    sent_at: Mutex<VecDeque<Instant>>,
    agent_sent_at: Mutex<VecDeque<Instant>>,
    polling: tokio::sync::Mutex<()>,
}

fn rt(state: &AppState) -> &Runtime {
    &state.access.email
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Use an in-memory mailbox instead of the real servers (tests).
pub fn use_fake_transport(state: &AppState, fake: Arc<transport::Fake>) {
    *rt(state).fake.write().unwrap_or_else(|p| p.into_inner()) = Some(fake);
}

fn transport(state: &AppState) -> Transport {
    match rt(state)
        .fake
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
    {
        Some(f) => Transport::Fake(f),
        None => Transport::Real,
    }
}

/// The current settings (cached).
pub fn settings(state: &AppState) -> Arc<Settings> {
    if let Some(s) = rt(state)
        .settings
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
    {
        return s;
    }
    let s: Settings = state
        .store
        .get_config(SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let s = Arc::new(s);
    *rt(state)
        .settings
        .write()
        .unwrap_or_else(|p| p.into_inner()) = Some(s.clone());
    s
}

fn password_from_env() -> Option<String> {
    std::env::var(PASSWORD_ENV)
        .ok()
        .filter(|p| !p.trim().is_empty())
}

fn password(state: &AppState) -> Option<String> {
    password_from_env().or_else(|| {
        state
            .store
            .get_config(SECRET_KEY)
            .ok()
            .flatten()
            .filter(|p| !p.is_empty())
    })
}

/// Run `f` on the remembered threads and notes; `save` writes them back.
fn with_memory<R>(state: &AppState, save: bool, f: impl FnOnce(&mut Memory) -> R) -> R {
    let mut guard = lock(&rt(state).memory);
    let mem = guard.get_or_insert_with(|| {
        state
            .store
            .get_config(MEMORY_KEY)
            .ok()
            .flatten()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default()
    });
    let out = f(mem);
    if save {
        while mem.threads.len() > KEEP {
            mem.threads.pop_front();
        }
        while mem.notes.len() > KEEP {
            mem.notes.pop_front();
        }
        while mem.seen.len() > KEEP {
            mem.seen.pop_front();
        }
        if let Ok(json) = serde_json::to_string(&*mem) {
            if let Err(e) = state.store.set_config(MEMORY_KEY, &json) {
                tracing::warn!("Could not save the email memory: {}", e);
            }
        }
    }
    out
}

fn status_update(state: &AppState, f: impl FnOnce(&mut Status)) {
    f(&mut lock(&rt(state).status));
}

// ---------------------------------------------------------------------------
// People
// ---------------------------------------------------------------------------

fn normalize_address(a: &str) -> String {
    a.trim().trim_matches(['<', '>']).to_lowercase()
}

/// A plausible single address: `local@domain.tld`, no spaces or controls.
pub fn valid_address(a: &str) -> bool {
    let a = a.trim();
    if a.len() > 254 || a.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    match a.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && !domain.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !a.contains(['<', '>', ',', ';', '"', '(', ')', '[', ']', '\\'])
        }
        None => false,
    }
}

/// `(address, person)` for everyone who may write to the hub.
fn people(state: &AppState) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = state
        .store
        .list_identity_mappings()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, person)| person.starts_with("human://"))
        .filter_map(|(ext, person)| {
            ext.strip_prefix(EMAIL_SCHEME)
                .map(|a| (a.to_string(), person))
        })
        .collect();
    out.sort();
    out
}

fn person_for(state: &AppState, address: &str) -> Option<String> {
    state
        .store
        .get_mellowmesh_id(&format!("{EMAIL_SCHEME}{}", normalize_address(address)))
        .ok()
        .flatten()
        // Only people write by email, whatever a mapping says.
        .filter(|p| p.starts_with("human://"))
}

fn addresses_of(state: &AppState, person: &str) -> Vec<String> {
    people(state)
        .into_iter()
        .filter(|(_, p)| p == person)
        .map(|(a, _)| a)
        .collect()
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

fn inbox_of(agent_id: &str) -> String {
    let path = agent_id
        .strip_prefix("agent://")
        .unwrap_or(agent_id)
        .replace('/', ".");
    format!("_agent.{path}.inbox")
}

fn agents(state: &AppState) -> Vec<mellowmesh_core::agent::AgentRegistration> {
    let store = &state.store;
    state
        .caches
        .agents
        .get_or_load(|| store.list_agents())
        .map(|a| a.to_vec())
        .unwrap_or_default()
}

fn agent_name(state: &AppState, id: &str) -> String {
    agents(state)
        .into_iter()
        .find(|a| a.id == id)
        .map(|a| a.name)
        .unwrap_or_else(|| id.rsplit('/').next().unwrap_or(id).to_string())
}

/// The agent `@name` means: by name or by the last part of its id, the
/// owner's own agents first.
fn resolve_agent(state: &AppState, name: &str) -> Option<(String, String)> {
    let want = name.trim().to_lowercase();
    let squash = |s: &str| {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let mine = format!("agent://{}/", crate::mellows::owner_handle(&state.owner));
    let mut found: Vec<_> = agents(state)
        .into_iter()
        .filter(|a| a.id.starts_with("agent://"))
        .filter(|a| {
            let last = a.id.rsplit('/').next().unwrap_or("");
            last.to_lowercase() == want
                || a.name.to_lowercase() == want
                || squash(&a.name) == squash(&want)
                || squash(last) == squash(&want)
        })
        .collect();
    found.sort_by_key(|a| !a.id.starts_with(&mine));
    found.into_iter().next().map(|a| (a.id, a.name))
}

// ---------------------------------------------------------------------------
// Reading an email
// ---------------------------------------------------------------------------

struct Incoming {
    from: String,
    message_id: Option<String>,
    subject: String,
    text: String,
    /// In-Reply-To and References, without angle brackets.
    refs: Vec<String>,
    /// The topmost Authentication-Results (added by the provider).
    auth_results: Option<String>,
    /// An automatic reply or a mailing list: never answered.
    automatic: bool,
}

fn header_first<'a>(msg: &'a mail_parser::Message<'a>, name: &str) -> Option<&'a str> {
    msg.headers_raw()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

fn ids(value: &mail_parser::HeaderValue) -> Vec<String> {
    let list = value
        .as_text_list()
        .unwrap_or_else(|| value.as_text().into_iter().collect());
    list.into_iter()
        .flat_map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .map(|v| v.trim_matches(['<', '>']).to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

fn read_incoming(raw: &[u8]) -> Option<Incoming> {
    let msg = mail_parser::MessageParser::default().parse(raw)?;
    let from = msg
        .from()
        .and_then(|a| a.first())
        .and_then(|a| a.address())
        .map(normalize_address)?;
    let mut refs = ids(msg.in_reply_to());
    for r in ids(msg.references()) {
        if !refs.contains(&r) {
            refs.push(r);
        }
    }
    let lower = |name: &str| header_first(&msg, name).map(|v| v.trim().to_lowercase());
    let automatic = lower("Auto-Submitted").is_some_and(|v| v != "no")
        || lower("Precedence")
            .is_some_and(|v| matches!(v.as_str(), "bulk" | "junk" | "list" | "auto_reply"))
        || header_first(&msg, "X-Autoreply").is_some()
        || header_first(&msg, "X-Autorespond").is_some()
        || header_first(&msg, "List-Id").is_some();
    Some(Incoming {
        from,
        message_id: msg
            .message_id()
            .map(|m| m.trim_matches(['<', '>']).to_string()),
        subject: msg.subject().unwrap_or("").to_string(),
        text: msg.body_text(0).map(|t| t.to_string()).unwrap_or_default(),
        refs,
        auth_results: header_first(&msg, "Authentication-Results").map(str::to_string),
        automatic,
    })
}

fn new_message_id(s: &Settings) -> String {
    let token = mellowmesh_core::auth::generate_token();
    let random = token.trim_start_matches("mm_");
    let domain = command::domain_of(&s.address);
    let domain = if domain.is_empty() {
        "mellowmesh.local".to_string()
    } else {
        domain
    };
    format!("mm.{}@{}", &random[..random.len().min(32)], domain)
}

fn is_ours(message_id: &str) -> bool {
    message_id.starts_with("mm.")
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

fn within_rate(times: &Mutex<VecDeque<Instant>>, per_hour: usize) -> bool {
    let mut t = lock(times);
    let hour = Duration::from_secs(3600);
    while t.front().is_some_and(|at| at.elapsed() > hour) {
        t.pop_front();
    }
    if t.len() >= per_hour {
        return false;
    }
    t.push_back(Instant::now());
    true
}

struct Mail {
    to: String,
    person: String,
    subject: String,
    body: String,
    from_name: String,
    in_reply_to: Option<String>,
    references: Vec<String>,
    kind: ThreadKind,
}

/// Send one email and remember it, so a reply finds its way.
async fn send(state: &AppState, s: &Settings, mail: Mail) -> Result<String, String> {
    if !within_rate(&rt(state).sent_at, SEND_PER_HOUR) {
        return Err(format!(
            "The hub already sent {SEND_PER_HOUR} emails this hour; try again later."
        ));
    }
    let password = password(state).ok_or("The hub has no app password yet.")?;
    let out = Outgoing {
        from_name: mail.from_name,
        to: mail.to.clone(),
        subject: mail.subject.clone(),
        body: mail.body,
        message_id: new_message_id(s),
        in_reply_to: mail.in_reply_to,
        references: mail.references,
    };
    match transport(state).send(s, &password, &out).await {
        Ok(()) => {
            status_update(state, |st| st.sent += 1);
            with_memory(state, true, |m| {
                m.threads.push_back(Thread {
                    message_id: out.message_id.clone(),
                    to: mail.to,
                    person: mail.person,
                    subject: command::clean_subject(&mail.subject),
                    kind: mail.kind,
                    at: Utc::now(),
                })
            });
            Ok(out.message_id)
        }
        Err(e) => {
            tracing::warn!("Email to a listed person failed: {}", e);
            status_update(state, |st| st.last_error = Some(e.clone()));
            Err(e)
        }
    }
}

/// Answer an incoming email in its own thread.
async fn answer(
    state: &AppState,
    s: &Settings,
    to: &Incoming,
    person: &str,
    body: String,
    kind: ThreadKind,
) {
    let subject = command::clean_subject(&to.subject);
    let subject = if subject.is_empty() {
        "Your hub".to_string()
    } else {
        format!("Re: {subject}")
    };
    let mut references = to.refs.clone();
    if let Some(id) = &to.message_id {
        references.push(id.clone());
    }
    let _ = send(
        state,
        s,
        Mail {
            to: to.from.clone(),
            person: person.to_string(),
            subject,
            body,
            from_name: "MellowMesh".into(),
            in_reply_to: to.message_id.clone(),
            references,
            kind,
        },
    )
    .await;
}

fn help_text(state: &AppState) -> String {
    let names: Vec<String> = agents(state).into_iter().map(|a| a.name).collect();
    let agents = if names.is_empty() {
        "No agent is registered yet.".to_string()
    } else {
        format!("Agents: {}.", names.join(", "))
    };
    format!(
        "Write to this address:\n\n  @coder run the tests     a note to an agent; its answer comes back here\n  task: renew the domain   a task on the board (the rest of the email is its details)\n  status                   what waits for you\n  help                     this list\n\nReply to a request with 1, 2, its name, yes or no to answer it.\nReply to an agent's email to keep talking to it.\n\n{agents}"
    )
}

fn status_text(state: &AppState, person: &str) -> String {
    let owner = person == state.owner;
    let decisions: Vec<Decision> = state
        .store
        .list_decisions()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| matches!(d.status.as_str(), "requested" | "discussed"))
        .filter(|d| owner || d.required_decider == person)
        .collect();
    let tasks: Vec<_> = state
        .store
        .list_tasks()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !mellowmesh_core::task::status::is_final(&t.status))
        .collect();
    let mut out = String::from("What waits for you\n\n");
    out.push_str(&format!("Requests to answer: {}\n", decisions.len()));
    for d in decisions.iter().take(10) {
        out.push_str(&format!("  - {} ({})\n", d.title, d.id));
    }
    out.push_str(&format!("\nTasks not done: {}\n", tasks.len()));
    for t in tasks.iter().take(5) {
        out.push_str(&format!("  - {} [{}]\n", t.title, t.status));
    }
    let names: Vec<String> = agents(state).into_iter().map(|a| a.name).collect();
    out.push_str(&format!(
        "\nAgents: {}\n",
        if names.is_empty() {
            "none yet".to_string()
        } else {
            names.join(", ")
        }
    ));
    out
}

// ---------------------------------------------------------------------------
// Handling an incoming email
// ---------------------------------------------------------------------------

/// An interface principal, to relay a person's answer to a decision.
fn interface_ctx() -> AuthContext {
    AuthContext {
        principal: Some(AuthPrincipal {
            id: INTERFACE.to_string(),
            token_id: String::new(),
            kind: "interface".to_string(),
            read_scopes: Vec::new(),
            write_scopes: Vec::new(),
            token_hash: String::new(),
        }),
        remote: false,
        trusted_local: false,
    }
}

fn preview(cmd: &Command) -> String {
    match cmd {
        Command::Note { agent, text } => {
            let first: String = text
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect();
            format!("@{agent} {first}")
        }
        Command::Task { title, .. } => format!("task: {title}"),
        Command::Status => "status".into(),
        Command::Help => "help".into(),
        Command::Reply { text } => text.chars().take(120).collect(),
        Command::Unknown => String::new(),
    }
}

/// Read the hub's inbox once and handle what arrived. Returns how many
/// emails were read.
pub async fn poll_once(state: &AppState) -> Result<usize, String> {
    let _one_at_a_time = rt(state).polling.lock().await;
    let s = settings(state);
    if !s.enabled {
        return Ok(0);
    }
    let password = password(state).ok_or("The hub has no app password yet.")?;
    let cursor = with_memory(state, false, |m| m.cursor);
    let result = transport(state).fetch(&s, &password, cursor).await;
    status_update(state, |st| st.last_check = Some(Utc::now()));
    let (emails, cursor) = match result {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Reading the hub's mailbox failed: {}", e);
            status_update(state, |st| st.last_error = Some(e.clone()));
            return Err(e);
        }
    };
    status_update(state, |st| st.last_error = None);
    with_memory(state, true, |m| m.cursor = cursor);
    let n = emails.len();
    for raw in emails {
        handle_incoming(state, &s, &raw).await;
    }
    Ok(n)
}

fn ignore(state: &AppState, why: &str) {
    tracing::debug!("Email ignored: {}", why);
    status_update(state, |st| st.ignored += 1);
}

async fn handle_incoming(state: &AppState, s: &Settings, raw: &[u8]) {
    let Some(mail) = read_incoming(raw) else {
        return ignore(state, "unreadable");
    };
    if mail.message_id.as_deref().is_some_and(is_ours) || mail.from == normalize_address(&s.address)
    {
        return; // the hub's own email
    }
    if mail.automatic {
        return ignore(state, "automatic reply or list");
    }
    if let Some(id) = &mail.message_id {
        let again = with_memory(state, true, |m| {
            if m.seen.contains(id) {
                true
            } else {
                m.seen.push_back(id.clone());
                false
            }
        });
        if again {
            return;
        }
    }
    let Some(person) = person_for(state, &mail.from) else {
        return ignore(state, "sender not on the list");
    };
    status_update(state, |st| st.received += 1);

    // A reply to an email the hub sent to this very address.
    let thread = with_memory(state, false, |m| {
        m.threads
            .iter()
            .rev()
            .find(|t| t.to == mail.from && mail.refs.contains(&t.message_id))
            .cloned()
    });
    let vouched = s.trust_provider
        && command::provider_vouches(mail.auth_results.as_deref(), &mail.from, &s.provider_id);
    let proven = thread.is_some() || vouched;
    match thread {
        Some(t) if !matches!(t.kind, ThreadKind::Info) => {
            let cmd = command::parse(&mail.subject, &mail.text, true);
            in_thread(state, s, &person, &mail, t, cmd).await
        }
        _ => {
            let cmd = command::parse(&mail.subject, &mail.text, false);
            run(state, s, &person, &mail, cmd, proven).await
        }
    }
}

async fn in_thread(
    state: &AppState,
    s: &Settings,
    person: &str,
    mail: &Incoming,
    t: Thread,
    cmd: Command,
) {
    let text = match cmd {
        Command::Reply { text } => text,
        _ => {
            let body = match &t.kind {
                ThreadKind::Decision { .. } => {
                    "Reply with the number of an option, its name, yes or no.".to_string()
                }
                _ => "Write your answer above the quoted email.".to_string(),
            };
            return answer(state, s, mail, person, body, ThreadKind::Info).await;
        }
    };
    match t.kind {
        ThreadKind::Decision { decision_id } => {
            answer_decision(state, s, person, mail, &decision_id, &text).await
        }
        ThreadKind::Agent { agent_id, inbox } => {
            deliver_note(state, s, person, mail, &agent_id, &inbox, &text).await
        }
        ThreadKind::Topic { topic } => {
            let msg = Message {
                id: String::new(),
                topic: topic.clone(),
                from: person.to_string(),
                owner: Some(person.to_string()),
                timestamp: Utc::now(),
                content_type: "text/markdown".into(),
                body: text,
                headers: Some(HashMap::from([("x-via".to_string(), "email".to_string())])),
                payload: None,
                parent_id: None,
            };
            let st = Arc::new(state.clone());
            match crate::handlers::message::handle_publish(st.clone(), msg).await {
                Ok(m) => {
                    if person == state.owner {
                        crate::mellows::maybe_dispatch(&st, &m);
                    }
                }
                Err(e) => {
                    answer(
                        state,
                        s,
                        mail,
                        person,
                        format!("Not posted: {e}"),
                        ThreadKind::Info,
                    )
                    .await
                }
            }
        }
        ThreadKind::Confirm { command } => {
            let word = text
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches(['.', '!'])
                .to_lowercase();
            if matches!(word.as_str(), "no" | "non" | "cancel" | "annuler" | "stop") {
                return answer(
                    state,
                    s,
                    mail,
                    person,
                    "Dropped: nothing was done.".into(),
                    ThreadKind::Info,
                )
                .await;
            }
            if Utc::now() - t.at > CONFIRM_FOR {
                return answer(
                    state,
                    s,
                    mail,
                    person,
                    "This confirmation expired: nothing was done. Send the command again.".into(),
                    ThreadKind::Info,
                )
                .await;
            }
            Box::pin(run(state, s, person, mail, command, true)).await
        }
        ThreadKind::Info => {}
    }
}

/// Run a command from a fresh email (or a confirmed one).
async fn run(
    state: &AppState,
    s: &Settings,
    person: &str,
    mail: &Incoming,
    cmd: Command,
    proven: bool,
) {
    let acts = matches!(cmd, Command::Note { .. } | Command::Task { .. });
    if acts && !proven {
        let body = format!(
            "Your hub received this command by email:\n\n    {}\n\nYour mail provider did not vouch for the sender, so nothing was done yet.\nReply to this email (\"yes\" is enough) to do it, or \"no\" to drop it.\nThis request expires in 30 minutes.",
            preview(&cmd)
        );
        return answer(
            state,
            s,
            mail,
            person,
            body,
            ThreadKind::Confirm { command: cmd },
        )
        .await;
    }
    match cmd {
        Command::Note { agent, text } => match resolve_agent(state, &agent) {
            Some((agent_id, _)) => {
                let inbox = inbox_of(&agent_id);
                deliver_note(state, s, person, mail, &agent_id, &inbox, &text).await
            }
            None => {
                let body = format!(
                    "No agent called @{agent} on this hub.\n\n{}",
                    help_text(state)
                );
                answer(state, s, mail, person, body, ThreadKind::Info).await
            }
        },
        Command::Task { title, details } => {
            let body = match add_task(state, person, &title, &details).await {
                Ok(id) => format!("Task added: {title} ({id})."),
                Err(e) => format!("The task was not added: {e}"),
            };
            answer(state, s, mail, person, body, ThreadKind::Info).await
        }
        Command::Status => {
            let body = status_text(state, person);
            answer(state, s, mail, person, body, ThreadKind::Info).await
        }
        Command::Help => {
            let body = help_text(state);
            answer(state, s, mail, person, body, ThreadKind::Info).await
        }
        Command::Reply { .. } | Command::Unknown => {
            let body = format!("I did not understand that.\n\n{}", help_text(state));
            answer(state, s, mail, person, body, ThreadKind::Info).await
        }
    }
}

async fn deliver_note(
    state: &AppState,
    s: &Settings,
    person: &str,
    mail: &Incoming,
    agent_id: &str,
    inbox: &str,
    text: &str,
) {
    let note_id = format!("msg_{}", ulid::Ulid::new().to_string().to_lowercase());
    let body = format!(
        "{text}\n\n(Sent by email. Publish your answer with parent_id \"{note_id}\" and it is emailed back.)"
    );
    let note = Message {
        id: note_id.clone(),
        topic: inbox.to_string(),
        from: person.to_string(),
        owner: Some(person.to_string()),
        timestamp: Utc::now(),
        content_type: "text/markdown".into(),
        body,
        headers: Some(HashMap::from([("x-via".to_string(), "email".to_string())])),
        payload: None,
        parent_id: None,
    };
    let mut references = mail.refs.clone();
    if let Some(id) = &mail.message_id {
        references.push(id.clone());
    }
    with_memory(state, true, |m| {
        m.notes.push_back(NoteLink {
            note_id: note_id.clone(),
            agent_id: agent_id.to_string(),
            inbox: inbox.to_string(),
            to: mail.from.clone(),
            person: person.to_string(),
            subject: command::clean_subject(&mail.subject),
            reply_to: mail.message_id.clone().unwrap_or_default(),
            references,
            at: Utc::now(),
        })
    });
    let st = Arc::new(state.clone());
    match crate::handlers::message::handle_publish(st.clone(), note).await {
        // A Mellow takes a turn only when its owner speaks.
        Ok(m) => {
            if person == state.owner {
                crate::mellows::maybe_dispatch(&st, &m);
            }
        }
        Err(e) => {
            answer(
                state,
                s,
                mail,
                person,
                format!("The note was not delivered: {e}"),
                ThreadKind::Info,
            )
            .await
        }
    }
}

async fn answer_decision(
    state: &AppState,
    s: &Settings,
    person: &str,
    mail: &Incoming,
    decision_id: &str,
    text: &str,
) {
    let decision = match state.store.get_decision(decision_id) {
        Ok(Some(d)) => d,
        _ => {
            return answer(
                state,
                s,
                mail,
                person,
                "That request no longer exists.".into(),
                ThreadKind::Info,
            )
            .await
        }
    };
    let Some(option_id) = command::pick_option(&decision.options, text) else {
        let options: String = decision
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| format!("  {} · {}\n", i + 1, o.label))
            .collect();
        let body = format!(
            "I could not tell which option you chose. Reply with its number, its name, yes or no:\n\n{options}"
        );
        return answer(
            state,
            s,
            mail,
            person,
            body,
            ThreadKind::Decision {
                decision_id: decision_id.to_string(),
            },
        )
        .await;
    };
    let label = decision
        .options
        .iter()
        .find(|o| o.id == option_id)
        .map(|o| o.label.clone())
        .unwrap_or_else(|| option_id.clone());
    let result = crate::handlers::decision::respond_decision(
        State(state.clone()),
        Extension(interface_ctx()),
        axum::extract::Path(decision_id.to_string()),
        Json(crate::handlers::decision::ResponsePayload {
            option_id,
            responded_by: Some(format!("{EMAIL_SCHEME}{}", mail.from)),
        }),
    )
    .await
    .map(|_| ());
    let body = match result {
        Ok(()) => format!("Recorded: {label}. Answered by you, by email."),
        Err((_, why)) => format!("Not recorded: {why}"),
    };
    answer(state, s, mail, person, body, ThreadKind::Info).await
}

async fn add_task(
    state: &AppState,
    person: &str,
    title: &str,
    details: &str,
) -> Result<String, String> {
    let title: String = title.chars().take(MAX_SUBJECT).collect();
    let task: mellowmesh_core::task::Task = serde_json::from_value(serde_json::json!({
        "id": format!("task_{}", ulid::Ulid::new().to_string().to_lowercase()),
        "title": title,
        "description": if details.trim().is_empty() { None } else { Some(details.trim()) },
        "created_by": person,
        "status": "open",
        "priority": "medium",
        "topics": [],
        "required_capabilities": [],
        "artifacts": [],
        "decisions": [],
        "created_at": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    }))
    .map_err(|e| e.to_string())?;
    state.store.insert_task(&task).map_err(|e| e.to_string())?;
    crate::handlers::task::publish_task_event(state, &task, "created").await;
    Ok(task.id)
}

// ---------------------------------------------------------------------------
// Watching the hub: answers, decisions and mentions that go out by email
// ---------------------------------------------------------------------------

/// Called for every message the hub publishes.
pub fn observe(state: &Arc<AppState>, msg: &Message) {
    let s = settings(state);
    if !s.enabled {
        return;
    }
    let header = |name: &str| msg.headers.as_ref().and_then(|h| h.get(name)).cloned();
    let routed_copy = header("x-mellowmesh-routed").is_some_and(|v| v == "true");

    // An agent's answer to a note that came by email.
    if let Some(parent) = msg.parent_id.as_deref() {
        let link = with_memory(state, false, |m| {
            m.notes
                .iter()
                .rev()
                .find(|n| n.note_id == parent && n.agent_id == msg.from)
                .cloned()
        });
        if let Some(link) = link {
            let (state, s, msg) = (state.clone(), s.clone(), msg.clone());
            tokio::spawn(async move { email_answer(&state, &s, link, &msg).await });
        }
    }

    if s.notify_decisions
        && msg.topic.starts_with("_decision.")
        && msg.topic.ends_with(".requested")
    {
        if let Some(d) = msg
            .payload
            .as_ref()
            .and_then(|p| serde_json::from_value::<Decision>(p.clone()).ok())
        {
            let (state, s) = (state.clone(), s.clone());
            tokio::spawn(async move { email_decision(&state, &s, &d).await });
        }
    }

    if s.notify_mentions && !routed_copy && header("x-via").as_deref() != Some("email") {
        let mentioned: Vec<String> = header("x-mentions")
            .and_then(|m| serde_json::from_str(&m).ok())
            .unwrap_or_default();
        for human in mentioned
            .into_iter()
            .filter(|m| m.starts_with("human://") && *m != msg.from)
        {
            let (state, s, msg) = (state.clone(), s.clone(), msg.clone());
            tokio::spawn(async move { email_mention(&state, &s, &human, &msg).await });
        }
    }
}

async fn email_answer(state: &AppState, s: &Settings, link: NoteLink, msg: &Message) {
    let name = agent_name(state, &link.agent_id);
    let subject = if link.subject.is_empty() {
        format!("{name} answered")
    } else {
        format!("Re: {}", link.subject)
    };
    let mut references = link.references.clone();
    if !link.reply_to.is_empty() && !references.contains(&link.reply_to) {
        references.push(link.reply_to.clone());
    }
    let _ = send(
        state,
        s,
        Mail {
            to: link.to.clone(),
            person: link.person.clone(),
            subject,
            body: format!("{}\n\n--\nReply to keep talking to {name}.", msg.body),
            from_name: format!("{name} via MellowMesh"),
            in_reply_to: (!link.reply_to.is_empty()).then(|| link.reply_to.clone()),
            references,
            kind: ThreadKind::Agent {
                agent_id: link.agent_id.clone(),
                inbox: link.inbox.clone(),
            },
        },
    )
    .await;
}

async fn email_decision(state: &AppState, s: &Settings, d: &Decision) {
    let mut to = addresses_of(state, &d.required_decider);
    let mut person = d.required_decider.clone();
    if to.is_empty() {
        to = addresses_of(state, &state.owner);
        person = state.owner.clone();
    }
    let who = agent_name(state, &d.created_by);
    let options: String = d
        .options
        .iter()
        .enumerate()
        .map(|(i, o)| format!("  {} · {}\n", i + 1, o.label))
        .collect();
    for address in to {
        let _ = send(
            state,
            s,
            Mail {
                to: address,
                person: person.clone(),
                subject: format!("Decision needed: {}", d.title),
                body: format!(
                    "{who} needs you.\n\n{}\n\n{options}\nReply with a number, the option's name, yes or no.\n({})",
                    d.question, d.id
                ),
                from_name: format!("{who} via MellowMesh"),
                in_reply_to: None,
                references: Vec::new(),
                kind: ThreadKind::Decision {
                    decision_id: d.id.clone(),
                },
            },
        )
        .await;
    }
}

async fn email_mention(state: &AppState, s: &Settings, human: &str, msg: &Message) {
    let who = agent_name(state, &msg.from);
    for address in addresses_of(state, human) {
        let _ = send(
            state,
            s,
            Mail {
                to: address,
                person: human.to_string(),
                subject: format!("{who} mentioned you on {}", msg.topic),
                body: format!("{}\n\n--\nReply to answer in {}.", msg.body, msg.topic),
                from_name: format!("{who} via MellowMesh"),
                in_reply_to: None,
                references: Vec::new(),
                kind: ThreadKind::Topic {
                    topic: msg.topic.clone(),
                },
            },
        )
        .await;
    }
}

/// Read the mailbox every `poll_seconds` while email is on; a change of
/// settings wakes it at once.
pub fn start(state: AppState) {
    tokio::spawn(async move {
        loop {
            let s = settings(&state);
            let wait = if s.enabled && password(&state).is_some() {
                let _ = poll_once(&state).await;
                s.poll_seconds.clamp(10, 3600)
            } else {
                3600
            };
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
                _ = rt(&state).wake.notified() => {}
            }
        }
    });
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

type ApiError = (StatusCode, String);

fn bad(msg: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, msg.into())
}

#[derive(Serialize)]
struct PersonView {
    address: String,
    person: String,
}

fn owner_view(state: &AppState) -> serde_json::Value {
    let s = settings(state);
    serde_json::json!({
        "settings": &*s,
        "password_set": password(state).is_some(),
        "password_from_env": password_from_env().is_some(),
        "people": people(state)
            .into_iter()
            .map(|(address, person)| PersonView { address, person })
            .collect::<Vec<_>>(),
        "status": lock(&rt(state).status).clone(),
    })
}

/// `GET /email`: everything, for the owner; for anyone else signed in,
/// whether email is on and whom they may email (people, never addresses).
pub async fn get_email(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if is_admin(&state, &ctx) {
        return Ok(Json(owner_view(&state)));
    }
    if ctx.principal.is_none() {
        return Err((admin_refusal(&ctx), "Sign in to see the hub's email".into()));
    }
    let s = settings(&state);
    let mut can_email: Vec<String> = people(&state).into_iter().map(|(_, p)| p).collect();
    can_email.dedup();
    Ok(Json(serde_json::json!({
        "enabled": s.enabled,
        "can_email": can_email,
    })))
}

/// The fields `PUT /email` may change. `password` replaces the app
/// password; `forget_password` removes it; `owner_address` adds the
/// owner's own address to the list.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub enabled: Option<bool>,
    pub address: Option<String>,
    pub username: Option<String>,
    pub imap: Option<Server>,
    pub smtp: Option<Server>,
    pub folder: Option<String>,
    pub poll_seconds: Option<u64>,
    pub notify_decisions: Option<bool>,
    pub notify_mentions: Option<bool>,
    pub trust_provider: Option<bool>,
    /// The provider's authserv-id (`mx.google.com`); empty to take no
    /// provider's word.
    pub provider_id: Option<String>,
    pub password: Option<String>,
    pub forget_password: Option<bool>,
    pub owner_address: Option<String>,
}

fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_matches(['[', ']']).to_lowercase();
    h == "localhost"
        || h.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn check_server(what: &str, srv: &Server) -> Result<(), ApiError> {
    let host = srv.host.trim();
    if host.is_empty()
        || host.len() > 253
        || host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '@' | '?' | '#'))
    {
        return Err(bad(format!("{what}: the server name is not valid")));
    }
    if srv.port == 0 {
        return Err(bad(format!("{what}: the port is not valid")));
    }
    if srv.security == Security::None && !is_loopback_host(host) {
        return Err(bad(format!(
            "{what}: an unencrypted connection is only allowed to a server on this machine"
        )));
    }
    Ok(())
}

fn no_newline(what: &str, v: &str, max: usize) -> Result<(), ApiError> {
    if v.len() > max || v.contains(['\r', '\n', '\0']) {
        return Err(bad(format!("{what} is not valid")));
    }
    Ok(())
}

/// `PUT /email` (owner): change the settings.
pub async fn put_email(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(patch): Json<Patch>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !is_admin(&state, &ctx) {
        return Err((
            admin_refusal(&ctx),
            "Only the hub owner sets up email".into(),
        ));
    }
    let mut s = (*settings(&state)).clone();
    let old_address = s.address.clone();
    if let Some(a) = &patch.address {
        let a = normalize_address(a);
        if !a.is_empty() && !valid_address(&a) {
            return Err(bad("The hub's address is not a valid email address"));
        }
        if a != s.address {
            s.address = a;
            // A new mailbox: its provider's servers, unless given.
            let (imap, smtp) = preset(&command::domain_of(&s.address));
            s.imap = imap;
            s.smtp = smtp;
            s.provider_id = provider_id_for(&command::domain_of(&s.address)).to_string();
        }
    }
    if let Some(u) = &patch.username {
        no_newline("The login", u, 254)?;
        s.username = u.trim().to_string();
    }
    if let Some(i) = patch.imap.clone() {
        s.imap = Server {
            host: i.host.trim().to_lowercase(),
            ..i
        };
    }
    if let Some(o) = patch.smtp.clone() {
        s.smtp = Server {
            host: o.host.trim().to_lowercase(),
            ..o
        };
    }
    if let Some(f) = &patch.folder {
        no_newline("The folder", f, 100)?;
        if f.trim().is_empty() || f.contains('"') {
            return Err(bad("The folder is not valid"));
        }
        s.folder = f.trim().to_string();
    }
    if let Some(p) = patch.poll_seconds {
        if !(10..=3600).contains(&p) {
            return Err(bad("Read the mailbox every 10 to 3600 seconds"));
        }
        s.poll_seconds = p;
    }
    if let Some(v) = patch.notify_decisions {
        s.notify_decisions = v;
    }
    if let Some(v) = patch.notify_mentions {
        s.notify_mentions = v;
    }
    if let Some(v) = patch.trust_provider {
        s.trust_provider = v;
    }
    if let Some(id) = &patch.provider_id {
        let id = id.trim().to_lowercase();
        if id.len() > 253
            || id
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == ';')
        {
            return Err(bad("The provider's server name is not valid"));
        }
        s.provider_id = id;
    }
    if let Some(v) = patch.enabled {
        s.enabled = v;
    }
    if let Some(p) = &patch.password {
        if p.is_empty() {
            return Err(bad("The app password is empty"));
        }
        no_newline("The app password", p, 1024)?;
    }
    let owner_address = match &patch.owner_address {
        Some(a) => {
            let a = normalize_address(a);
            if !valid_address(&a) {
                return Err(bad("Your address is not a valid email address"));
            }
            Some(a)
        }
        None => None,
    };
    if !s.address.is_empty() {
        check_server("Reading (IMAP)", &s.imap)?;
        check_server("Sending (SMTP)", &s.smtp)?;
    }
    let will_have_password = patch.password.is_some()
        || (patch.forget_password != Some(true) && password(&state).is_some());
    if s.enabled && (s.address.is_empty() || !will_have_password) {
        return Err(bad(
            "Give the hub its address and app password before turning email on",
        ));
    }
    if owner_address.as_deref() == Some(s.address.as_str()) {
        return Err(bad(
            "Your own address must differ from the hub's: the hub ignores its own email",
        ));
    }

    let store = &state.store;
    if let Some(p) = &patch.password {
        store
            .set_config(SECRET_KEY, p)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    } else if patch.forget_password == Some(true) {
        store
            .set_config(SECRET_KEY, "")
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    if let Some(a) = owner_address {
        store
            .insert_identity_mapping(&format!("{EMAIL_SCHEME}{a}"), &state.owner)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    let json = serde_json::to_string(&s)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    store
        .set_config(SETTINGS_KEY, &json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if s.address != old_address {
        // Another mailbox: start reading it from now.
        with_memory(&state, true, |m| m.cursor = None);
    }
    *rt(&state)
        .settings
        .write()
        .unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(s));
    status_update(&state, |st| st.last_error = None);
    rt(&state).wake.notify_waiters();
    Ok(Json(owner_view(&state)))
}

/// `POST /email/check` (owner): open the inbox, then send a test email to
/// the owner's own address.
pub async fn check(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !is_admin(&state, &ctx) {
        return Err((
            admin_refusal(&ctx),
            "Only the hub owner checks email".into(),
        ));
    }
    let s = settings(&state);
    let Some(password) = password(&state) else {
        return Err(bad("Give the hub its app password first"));
    };
    if s.address.is_empty() {
        return Err(bad("Give the hub its address first"));
    }
    let inbox = transport(&state).check_inbox(&s, &password).await;
    let to = addresses_of(&state, &state.owner).into_iter().next();
    let sent = match &to {
        None => Err("Add your own address to the list first.".to_string()),
        Some(address) => send(
            &state,
            &s,
            Mail {
                to: address.clone(),
                person: state.owner.clone(),
                subject: "Your hub's mailbox works".into(),
                body: format!(
                    "This is a test from your MellowMesh hub.\n\n{}",
                    help_text(&state)
                ),
                from_name: "MellowMesh".into(),
                in_reply_to: None,
                references: Vec::new(),
                kind: ThreadKind::Info,
            },
        )
        .await
        .map(|_| format!("A test email was sent to {address}.")),
    };
    let step = |r: Result<String, String>| match r {
        Ok(detail) => serde_json::json!({"ok": true, "detail": detail}),
        Err(detail) => serde_json::json!({"ok": false, "detail": detail}),
    };
    Ok(Json(serde_json::json!({
        "inbox": step(inbox),
        "send": step(sent),
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonChange {
    pub address: String,
    /// `human://…`; the owner when absent.
    pub person: Option<String>,
    #[serde(default)]
    pub remove: bool,
}

/// `PUT /email/people` (owner): add a person who may write to the hub, or
/// remove one.
pub async fn put_person(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(change): Json<PersonChange>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !is_admin(&state, &ctx) {
        return Err((
            admin_refusal(&ctx),
            "Only the hub owner chooses who may write".into(),
        ));
    }
    let address = normalize_address(&change.address);
    if !valid_address(&address) {
        return Err(bad("Not a valid email address"));
    }
    let key = format!("{EMAIL_SCHEME}{address}");
    if change.remove {
        state
            .store
            .delete_identity_mapping(&key)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    } else {
        if address == normalize_address(&settings(&state).address) {
            return Err(bad("That is the hub's own address"));
        }
        let person = change
            .person
            .map(|p| p.trim().to_string())
            .unwrap_or_else(|| state.owner.clone());
        let known = person == state.owner
            || state
                .store
                .get_principal(&person)
                .ok()
                .flatten()
                .is_some_and(|p| p.kind == "human");
        if !person.starts_with("human://") || !known {
            return Err(bad(format!("{person} is not a person on this hub")));
        }
        state
            .store
            .insert_identity_mapping(&key, &person)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok(Json(owner_view(&state)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendRequest {
    /// A person (`human://…`) or one of their listed addresses.
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// `POST /email/send`: email a person on the hub's list. An agent's email
/// is answered in its own mailbox.
pub async fn post_send(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(req): Json<SendRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(caller) = ctx.principal.clone() else {
        return Err((StatusCode::UNAUTHORIZED, "Sign in to send email".into()));
    };
    let owner = is_admin(&state, &ctx);
    if !owner && !matches!(caller.kind.as_str(), "agent" | "human") {
        return Err((
            StatusCode::FORBIDDEN,
            "Only agents and people send email".into(),
        ));
    }
    let s = settings(&state);
    if !s.enabled {
        return Err((StatusCode::CONFLICT, "Email is off on this hub".into()));
    }
    let subject = req.subject.trim().to_string();
    if subject.is_empty() {
        return Err(bad("The subject is empty"));
    }
    no_newline("The subject", &subject, MAX_SUBJECT)?;
    if req.body.trim().is_empty() || req.body.chars().count() > MAX_BODY {
        return Err(bad(format!(
            "The body must hold 1 to {MAX_BODY} characters"
        )));
    }
    let to = req.to.trim();
    let (person, address) = if to.starts_with("human://") {
        match addresses_of(&state, to).into_iter().next() {
            Some(a) => (to.to_string(), a),
            None => {
                return Err((
                    StatusCode::FORBIDDEN,
                    format!("{to} has no address on the hub's list"),
                ))
            }
        }
    } else {
        let a = normalize_address(to);
        match person_for(&state, &a) {
            Some(p) => (p, a),
            None => {
                return Err((
                    StatusCode::FORBIDDEN,
                    "You can email only people on the hub's list".into(),
                ))
            }
        }
    };
    if !owner && !within_rate(&rt(&state).agent_sent_at, AGENT_SEND_PER_HOUR) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!("Agents may send {AGENT_SEND_PER_HOUR} emails an hour; try later"),
        ));
    }
    let (from_name, kind) = if caller.kind == "agent" {
        (
            format!("{} via MellowMesh", agent_name(&state, &caller.id)),
            ThreadKind::Agent {
                agent_id: caller.id.clone(),
                inbox: inbox_of(&caller.id),
            },
        )
    } else {
        ("MellowMesh".to_string(), ThreadKind::Info)
    };
    let id = send(
        &state,
        &s,
        Mail {
            to: address,
            person: person.clone(),
            subject,
            body: req.body,
            from_name,
            in_reply_to: None,
            references: Vec::new(),
            kind,
        },
    )
    .await
    .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(
        serde_json::json!({"sent_to": person, "message_id": id}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_follow_the_domain() {
        let (imap, smtp) = preset("gmail.com");
        assert_eq!(imap, Server::new("imap.gmail.com", 993, Security::Tls));
        assert_eq!(smtp.port, 465);
        let (imap, smtp) = preset("Example.org");
        assert_eq!(imap.host, "imap.example.org");
        assert_eq!(smtp.host, "smtp.example.org");
        assert_eq!(preset("").0, Server::default());
    }

    #[test]
    fn addresses_are_checked() {
        assert!(valid_address("lea@studio.fr"));
        assert!(valid_address("a.b+mesh@sub.example.co"));
        assert!(!valid_address("lea"));
        assert!(!valid_address("lea@localhost"));
        assert!(!valid_address("lea@studio.fr, eve@x.com"));
        assert!(!valid_address("lea @studio.fr"));
        assert!(!valid_address("<lea@studio.fr>"));
        assert!(!valid_address("a@b@c.fr"));
    }

    #[test]
    fn unencrypted_only_on_this_machine() {
        let plain = |host: &str| Server::new(host, 1143, Security::None);
        assert!(check_server("x", &plain("127.0.0.1")).is_ok());
        assert!(check_server("x", &plain("localhost")).is_ok());
        assert!(check_server("x", &plain("[::1]")).is_ok());
        assert!(check_server("x", &plain("mail.example.com")).is_err());
        assert!(check_server("x", &Server::new("bad host", 993, Security::Tls)).is_err());
        assert!(check_server("x", &Server::new("imap.x.com", 0, Security::Tls)).is_err());
    }

    #[test]
    fn an_email_is_read_with_its_thread_and_provider_verdict() {
        let raw = b"From: Lea <Lea@Studio.fr>\r\nTo: hub@gmail.com\r\nSubject: Re: Decision needed\r\nMessage-ID: <abc@studio.fr>\r\nIn-Reply-To: <mm.123@gmail.com>\r\nReferences: <first@x> <mm.123@gmail.com>\r\nAuthentication-Results: mx.google.com; dmarc=pass header.from=studio.fr\r\nAuthentication-Results: fake; dmarc=pass header.from=evil.fr\r\n\r\n1\r\n\r\nOn Mon, MellowMesh wrote:\r\n> q\r\n";
        let m = read_incoming(raw).unwrap();
        assert_eq!(m.from, "lea@studio.fr");
        assert_eq!(m.message_id.as_deref(), Some("abc@studio.fr"));
        assert_eq!(m.refs, vec!["mm.123@gmail.com", "first@x"]);
        assert!(m.auth_results.unwrap().contains("mx.google.com"));
        assert!(!m.automatic);
        let auto =
            b"From: a@b.com\r\nAuto-Submitted: auto-replied\r\nSubject: Out of office\r\n\r\naway";
        assert!(read_incoming(auto).unwrap().automatic);
    }
}
