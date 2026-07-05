use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

pub fn is_daemon_running(port: u16) -> bool {
    let addr = format!("127.0.0.1:{port}");
    if let Ok(socket_addr) = addr.parse() {
        TcpStream::connect_timeout(&socket_addr, Duration::from_millis(1000)).is_ok()
    } else {
        false
    }
}

pub fn spawn_daemon(port: u16) -> anyhow::Result<()> {
    if is_daemon_running(port) {
        return Ok(());
    }

    let mut bin_path = PathBuf::from(if cfg!(windows) {
        "mellowmeshd.exe"
    } else {
        "mellowmeshd"
    });
    if let Ok(mut current_exe) = std::env::current_exe() {
        current_exe.pop(); // Remove filename, get parent dir
        let local_bin = current_exe.join(if cfg!(windows) {
            "mellowmeshd.exe"
        } else {
            "mellowmeshd"
        });
        if local_bin.exists() {
            bin_path = local_bin;
        }
    }

    let mut cmd = Command::new(&bin_path);
    cmd.arg("--port").arg(port.to_string());

    // Redirect standard streams to null to run as daemon
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.spawn()?;

    // Poll for readiness (up to 3 seconds)
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if is_daemon_running(port) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    Err(anyhow::anyhow!(
        "Timeout waiting for mellowmeshd to start on port {port}"
    ))
}
