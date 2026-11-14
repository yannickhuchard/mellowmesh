//! PID file contract: while running, the daemon records its process id in
//! `mellowmeshd.pid`, in the same directory as its SQLite database (next to
//! `owner.token`). The file is removed on graceful shutdown.

use std::path::{Path, PathBuf};

pub const PID_FILE_NAME: &str = "mellowmeshd.pid";

/// Location of the PID file for a daemon using the database at `db_path`.
pub fn pid_file_path(db_path: &Path) -> PathBuf {
    match db_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(PID_FILE_NAME),
        _ => PathBuf::from(PID_FILE_NAME),
    }
}

/// Write the current process id to `path` (via a temporary file + rename,
/// so readers never observe a partially written file).
pub fn write_pid_file(path: &Path) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("pid.{}.tmp", std::process::id()));
    std::fs::write(&tmp, format!("{}\n", std::process::id()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Remove the PID file if it still names this process.
pub fn remove_pid_file(path: &Path) {
    if let Ok(content) = std::fs::read_to_string(path) {
        if content.trim() == std::process::id().to_string() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_file_lives_next_to_the_database() {
        let dir = std::env::temp_dir().join(format!(
            "mm-pid-{}",
            ulid::Ulid::new().to_string().to_lowercase()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("mellowmesh.db");
        let pid = pid_file_path(&db);
        assert_eq!(pid, dir.join("mellowmeshd.pid"));
        write_pid_file(&pid).unwrap();
        assert_eq!(
            std::fs::read_to_string(&pid).unwrap().trim(),
            std::process::id().to_string()
        );
        remove_pid_file(&pid);
        assert!(!pid.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
