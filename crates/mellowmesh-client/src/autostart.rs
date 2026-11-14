//! Locating, probing and auto-starting the local `mellowmeshd` daemon.
//!
//! Sync functions (`is_daemon_running`, `spawn_daemon`) are kept for
//! non-async callers; async code should use the `_async` variants, which
//! never block a tokio worker thread.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(1000);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Upper bound on how much of a `/health` response is read.
const MAX_HEALTH_RESPONSE: u64 = 4096;

fn daemon_binary_name() -> &'static str {
    if cfg!(windows) {
        "mellowmeshd.exe"
    } else {
        "mellowmeshd"
    }
}

fn loopback_addr(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

fn health_request(port: u16) -> String {
    format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
}

/// True when a raw HTTP response is a `200` whose body is `OK` — what the
/// MellowMesh daemon's `/health` endpoint returns. Any other listener on the
/// port (a different service, a half-started process) does not count.
fn is_health_ok(response: &[u8]) -> bool {
    let text = String::from_utf8_lossy(response);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return false;
    };
    let status_ok = head
        .lines()
        .next()
        .map(|line| {
            let mut parts = line.split_whitespace();
            parts.next().is_some_and(|v| v.starts_with("HTTP/1.")) && parts.next() == Some("200")
        })
        .unwrap_or(false);
    if !status_ok {
        return false;
    }
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    if chunked {
        // "2\r\nOK\r\n0\r\n\r\n"
        let mut lines = body.split("\r\n");
        let size = lines.next().map(|s| s.trim());
        let data = lines.next();
        size == Some("2") && data == Some("OK")
    } else {
        body.trim() == "OK"
    }
}

/// True when a MellowMesh daemon answers `GET /health` with `OK` on
/// `127.0.0.1:port`. Blocking; prefer [`is_daemon_running_async`] in async
/// code.
pub fn is_daemon_running(port: u16) -> bool {
    let probe = || -> std::io::Result<bool> {
        let mut stream = TcpStream::connect_timeout(&loopback_addr(port), PROBE_TIMEOUT)?;
        stream.set_read_timeout(Some(PROBE_TIMEOUT))?;
        stream.set_write_timeout(Some(PROBE_TIMEOUT))?;
        stream.write_all(health_request(port).as_bytes())?;
        let mut buf = Vec::new();
        stream.take(MAX_HEALTH_RESPONSE).read_to_end(&mut buf)?;
        Ok(is_health_ok(&buf))
    };
    probe().unwrap_or(false)
}

/// Async variant of [`is_daemon_running`].
pub async fn is_daemon_running_async(port: u16) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let probe = async {
        let mut stream = tokio::net::TcpStream::connect(loopback_addr(port)).await?;
        stream.write_all(health_request(port).as_bytes()).await?;
        let mut buf = Vec::new();
        (&mut stream)
            .take(MAX_HEALTH_RESPONSE)
            .read_to_end(&mut buf)
            .await?;
        Ok::<bool, std::io::Error>(is_health_ok(&buf))
    };
    matches!(
        tokio::time::timeout(PROBE_TIMEOUT * 2, probe).await,
        Ok(Ok(true))
    )
}

/// The directory holding the daemon's data (SQLite database, owner token,
/// PID file, log). Mirrors `mellowmesh_store::sqlite::default_db_path`:
/// the parent of `MELLOWMESH_DB` if set, otherwise the platform data dir
/// for `com.mellowmesh.mellowmesh`.
pub fn default_data_dir() -> PathBuf {
    if let Ok(db) = std::env::var("MELLOWMESH_DB") {
        if let Some(parent) = PathBuf::from(db).parent() {
            if !parent.as_os_str().is_empty() {
                return parent.to_path_buf();
            }
        }
        return PathBuf::from(".");
    }
    platform_data_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "macos")]
fn platform_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/com.mellowmesh.mellowmesh"))
}

#[cfg(windows)]
fn platform_data_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("mellowmesh")
            .join("mellowmesh")
            .join("data"),
    )
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_data_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        Some(p) if p.is_absolute() => p,
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".local/share"),
    };
    Some(base.join("mellowmesh"))
}

/// Resolve the daemon binary: the absolute `mellowmeshd` next to the current
/// executable when it exists; otherwise fall back to a `PATH` lookup, with a
/// warning since that may pick up an unexpected binary.
fn resolve_daemon_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(daemon_binary_name());
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    eprintln!(
        "warning: {} not found next to this executable; falling back to PATH lookup",
        daemon_binary_name()
    );
    PathBuf::from(daemon_binary_name())
}

/// Open the daemon log file (`mellowmeshd.log` in the data dir) for the
/// spawned daemon's stdout/stderr; falls back to discarding output.
fn daemon_log_stdio() -> (Stdio, Stdio) {
    let dir = default_data_dir();
    let open = || -> std::io::Result<(Stdio, Stdio)> {
        std::fs::create_dir_all(&dir)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("mellowmeshd.log"))?;
        let err = file.try_clone()?;
        Ok((Stdio::from(file), Stdio::from(err)))
    };
    open().unwrap_or_else(|_| (Stdio::null(), Stdio::null()))
}

/// Spawn the daemon detached from this process: on unix it gets its own
/// process group, so a Ctrl-C delivered to the CLI or MCP host's foreground
/// group does not kill it. A background thread reaps it if it exits while
/// this process is still alive (no zombie); if this process exits first the
/// daemon is re-parented and keeps running.
fn launch_daemon(port: u16) -> anyhow::Result<()> {
    let bin_path = resolve_daemon_binary();
    let mut cmd = Command::new(&bin_path);
    cmd.arg("--port").arg(port.to_string());

    let (stdout, stderr) = daemon_log_stdio();
    cmd.stdin(Stdio::null()).stdout(stdout).stderr(stderr);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start {}: {e}", bin_path.display()))?;
    std::thread::Builder::new()
        .name("mellowmeshd-reaper".to_string())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(())
}

fn startup_timeout_error(port: u16) -> anyhow::Error {
    anyhow::anyhow!(
        "Timeout waiting for mellowmeshd to start on port {port} (see {})",
        default_data_dir().join("mellowmeshd.log").display()
    )
}

/// Start the daemon on `port` unless one is already healthy there, and wait
/// until it answers `/health`. Blocking; prefer [`spawn_daemon_async`] in
/// async code.
pub fn spawn_daemon(port: u16) -> anyhow::Result<()> {
    if is_daemon_running(port) {
        return Ok(());
    }
    launch_daemon(port)?;
    let start = std::time::Instant::now();
    while start.elapsed() < STARTUP_TIMEOUT {
        if is_daemon_running(port) {
            return Ok(());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    Err(startup_timeout_error(port))
}

/// Async variant of [`spawn_daemon`]: polls with `tokio::time::sleep` and
/// async probes, so it never blocks a runtime worker.
pub async fn spawn_daemon_async(port: u16) -> anyhow::Result<()> {
    if is_daemon_running_async(port).await {
        return Ok(());
    }
    launch_daemon(port)?;
    let start = tokio::time::Instant::now();
    while start.elapsed() < STARTUP_TIMEOUT {
        if is_daemon_running_async(port).await {
            return Ok(());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    Err(startup_timeout_error(port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_ok_requires_200_and_ok_body() {
        assert!(is_health_ok(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 2\r\n\r\nOK"
        ));
        assert!(is_health_ok(
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n2\r\nOK\r\n0\r\n\r\n"
        ));
        assert!(!is_health_ok(b"HTTP/1.1 200 OK\r\n\r\n<html>hello</html>"));
        assert!(!is_health_ok(b"HTTP/1.1 404 Not Found\r\n\r\nOK"));
        assert!(!is_health_ok(b"SSH-2.0-OpenSSH_9.6\r\n"));
        assert!(!is_health_ok(b""));
    }

    #[test]
    fn nothing_listening_is_not_running() {
        // Bind then drop to get a port that is (almost certainly) closed.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(!is_daemon_running(port));
    }

    #[tokio::test]
    async fn non_mellowmesh_listener_is_not_running() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 512];
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\nhello")
                    .await;
            }
        });
        assert!(!is_daemon_running_async(port).await);
    }

    #[tokio::test]
    async fn healthy_listener_is_running() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 512];
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nOK")
                    .await;
            }
        });
        assert!(is_daemon_running_async(port).await);
    }
}
