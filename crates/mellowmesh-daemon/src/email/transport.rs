//! Talking to the mail servers: IMAP to read the hub's inbox, SMTP to send
//! from it. An in-memory mailbox ([`Fake`]) stands in for both in tests.

use super::{Security, Settings};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpStream;

/// How long one conversation with a mail server may take.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Emails read in one pass; the rest wait for the next one.
const BATCH: usize = 50;
/// Larger emails are skipped (a command is a few lines).
const MAX_EMAIL_BYTES: usize = 5 << 20;

/// Where reading stopped: the folder's UIDVALIDITY and the last UID read.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cursor {
    pub validity: u32,
    pub last_uid: u32,
}

/// One email to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// Display name before the hub's address ("Coder via MellowMesh").
    pub from_name: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    /// Message-ID, without the angle brackets.
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

/// An in-memory mailbox for tests: what is delivered to `inbox` is read
/// once; what the hub sends lands in `sent`.
#[derive(Debug, Default)]
pub struct Fake {
    pub inbox: Mutex<Vec<Vec<u8>>>,
    pub sent: Mutex<Vec<Outgoing>>,
}

impl Fake {
    pub fn deliver(&self, raw: impl Into<Vec<u8>>) {
        self.inbox
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(raw.into());
    }

    /// Emails sent since the last call.
    pub fn take_sent(&self) -> Vec<Outgoing> {
        std::mem::take(&mut *self.sent.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// The real mail servers, or the fake mailbox.
#[derive(Clone)]
pub enum Transport {
    Real,
    Fake(Arc<Fake>),
}

impl Transport {
    /// New emails since `cursor`, and the new cursor. The first read only
    /// notes where the folder ends: mail already there is never replayed.
    pub async fn fetch(
        &self,
        s: &Settings,
        password: &str,
        cursor: Option<Cursor>,
    ) -> Result<(Vec<Vec<u8>>, Option<Cursor>), String> {
        match self {
            Transport::Fake(f) => Ok((
                std::mem::take(&mut *f.inbox.lock().unwrap_or_else(|p| p.into_inner())),
                cursor,
            )),
            Transport::Real => tokio::time::timeout(TIMEOUT, fetch_imap(s, password, cursor))
                .await
                .map_err(|_| format!("{} did not answer in time", s.imap.host))?,
        }
    }

    /// Log in and open the folder; says how many emails it holds.
    pub async fn check_inbox(&self, s: &Settings, password: &str) -> Result<String, String> {
        match self {
            Transport::Fake(_) => Ok("The inbox opens.".into()),
            Transport::Real => tokio::time::timeout(TIMEOUT, async {
                let mut session = open_imap(s, password).await?;
                let mailbox = session
                    .select(&s.folder)
                    .await
                    .map_err(|e| format!("Cannot open the folder {}: {e}", s.folder))?;
                let _ = session.logout().await;
                Ok(format!("{} opens ({} emails).", s.folder, mailbox.exists))
            })
            .await
            .map_err(|_| format!("{} did not answer in time", s.imap.host))?,
        }
    }

    pub async fn send(&self, s: &Settings, password: &str, out: &Outgoing) -> Result<(), String> {
        match self {
            Transport::Fake(f) => {
                f.sent
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(out.clone());
                Ok(())
            }
            Transport::Real => send_smtp(s, password, out).await,
        }
    }
}

/// A byte stream IMAP can run over: plain TCP or TLS.
trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + std::fmt::Debug {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + std::fmt::Debug> Io for T {}

type ImapSession = async_imap::Session<Box<dyn Io>>;

async fn tls(host: &str, tcp: TcpStream) -> Result<tokio_native_tls::TlsStream<TcpStream>, String> {
    let connector = tokio_native_tls::native_tls::TlsConnector::new()
        .map_err(|e| format!("TLS is unavailable: {e}"))?;
    tokio_native_tls::TlsConnector::from(connector)
        .connect(host, tcp)
        .await
        .map_err(|e| format!("Secure connection to {host} failed: {e}"))
}

async fn greeting<T>(client: &mut async_imap::Client<T>) -> Result<(), String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + std::fmt::Debug,
{
    match client.read_response().await {
        Some(Ok(_)) => Ok(()),
        Some(Err(e)) => Err(format!("The mail server did not greet: {e}")),
        None => Err("The mail server closed the connection".into()),
    }
}

async fn open_imap(s: &Settings, password: &str) -> Result<ImapSession, String> {
    let srv = &s.imap;
    let tcp = TcpStream::connect((srv.host.as_str(), srv.port))
        .await
        .map_err(|e| format!("Cannot reach {}:{}: {e}", srv.host, srv.port))?;
    let client = match srv.security {
        Security::Tls => {
            let stream: Box<dyn Io> = Box::new(tls(&srv.host, tcp).await?);
            let mut c = async_imap::Client::new(stream);
            greeting(&mut c).await?;
            c
        }
        Security::None => {
            let stream: Box<dyn Io> = Box::new(tcp);
            let mut c = async_imap::Client::new(stream);
            greeting(&mut c).await?;
            c
        }
        Security::Starttls => {
            let mut plain = async_imap::Client::new(tcp);
            greeting(&mut plain).await?;
            plain
                .run_command_and_check_ok("STARTTLS", None)
                .await
                .map_err(|e| format!("{} refused STARTTLS: {e}", srv.host))?;
            let stream: Box<dyn Io> = Box::new(tls(&srv.host, plain.into_inner()).await?);
            async_imap::Client::new(stream)
        }
    };
    client
        .login(s.login(), password)
        .await
        .map_err(|(e, _)| format!("{} refused the login: {e}", srv.host))
}

async fn fetch_imap(
    s: &Settings,
    password: &str,
    cursor: Option<Cursor>,
) -> Result<(Vec<Vec<u8>>, Option<Cursor>), String> {
    let mut session = open_imap(s, password).await?;
    let mailbox = session
        .select(&s.folder)
        .await
        .map_err(|e| format!("Cannot open the folder {}: {e}", s.folder))?;
    let validity = mailbox.uid_validity.unwrap_or(0);
    let next = mailbox.uid_next.unwrap_or(1);
    let start = match cursor {
        Some(c) if c.validity == validity => c.last_uid,
        // First read, or the folder was rebuilt: start from now.
        _ => {
            let _ = session.logout().await;
            return Ok((
                Vec::new(),
                Some(Cursor {
                    validity,
                    last_uid: next.saturating_sub(1),
                }),
            ));
        }
    };
    let found = session
        .uid_search(format!("UID {}:*", start.saturating_add(1)))
        .await
        .map_err(|e| format!("Cannot search the folder: {e}"))?;
    // `n:*` also matches the last email when n is past the end.
    let mut uids: Vec<u32> = found.into_iter().filter(|u| *u > start).collect();
    uids.sort_unstable();
    uids.truncate(BATCH);
    let mut emails = Vec::new();
    let mut last = start;
    if let Some(&max) = uids.last() {
        let set = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut got: Vec<(u32, Vec<u8>)> = Vec::new();
        {
            let mut stream = session
                .uid_fetch(&set, "(UID BODY.PEEK[])")
                .await
                .map_err(|e| format!("Cannot read the emails: {e}"))?;
            while let Some(item) = stream.next().await {
                let fetch = item.map_err(|e| format!("Cannot read an email: {e}"))?;
                if let (Some(uid), Some(body)) = (fetch.uid, fetch.body()) {
                    if body.len() <= MAX_EMAIL_BYTES {
                        got.push((uid, body.to_vec()));
                    }
                }
            }
        }
        got.sort_by_key(|(uid, _)| *uid);
        emails = got.into_iter().map(|(_, b)| b).collect();
        last = max;
    }
    let _ = session.logout().await;
    Ok((
        emails,
        Some(Cursor {
            validity,
            last_uid: last,
        }),
    ))
}

fn bracketed(id: &str) -> String {
    format!("<{}>", id.trim_matches(['<', '>']))
}

async fn send_smtp(s: &Settings, password: &str, out: &Outgoing) -> Result<(), String> {
    use lettre::message::{header::ContentType, Mailbox};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

    let from = Mailbox::new(
        Some(out.from_name.clone()),
        s.address
            .parse()
            .map_err(|e| format!("The hub's address is not valid: {e}"))?,
    );
    let to: Mailbox = out
        .to
        .parse()
        .map_err(|e| format!("Not a valid address ({}): {e}", out.to))?;
    let mut builder = Message::builder()
        .from(from)
        .to(to)
        .subject(out.subject.clone())
        .message_id(Some(bracketed(&out.message_id)))
        .header(ContentType::TEXT_PLAIN);
    if let Some(r) = &out.in_reply_to {
        builder = builder.in_reply_to(bracketed(r));
    }
    if !out.references.is_empty() {
        builder = builder.references(
            out.references
                .iter()
                .map(|r| bracketed(r))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    let email = builder
        .body(out.body.clone())
        .map_err(|e| format!("Cannot build the email: {e}"))?;
    let srv = &s.smtp;
    let transport = match srv.security {
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&srv.host),
        Security::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&srv.host),
        Security::None => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
            srv.host.clone(),
        )),
    }
    .map_err(|e| format!("Cannot use {}: {e}", srv.host))?
    .port(srv.port)
    .credentials(Credentials::new(
        s.login().to_string(),
        password.to_string(),
    ))
    .timeout(Some(Duration::from_secs(30)))
    .build();
    transport
        .send(email)
        .await
        .map(|_| ())
        .map_err(|e| format!("{} did not send the email: {e}", srv.host))
}
