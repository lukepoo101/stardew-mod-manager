//! Copies of SMAPI logs, one per game session, in the manager's data folder.
//!
//! SMAPI overwrites its log on every start, so without a copy an earlier
//! session's log is gone. Only the most recent copies are kept.

use manager_app::error::{AppError, AppResult};
use manager_app::ports::logging::SessionLogArchivePort;
use std::path::PathBuf;

pub struct FilesystemSessionLogs {
    dir: PathBuf,
}

impl FilesystemSessionLogs {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Session ids are UUIDs; anything else is refused so a name can never
    /// reach outside the folder.
    #[allow(clippy::result_large_err)]
    fn path(&self, session_id: &str) -> AppResult<PathBuf> {
        if session_id.is_empty()
            || !session_id
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == '-')
        {
            return Err(AppError::validation(
                "SESSION_ID_INVALID",
                "That session id is not valid",
            ));
        }
        Ok(self.dir.join(format!("{session_id}.log")))
    }
}

#[allow(clippy::result_large_err)]
fn io(error: std::io::Error) -> AppError {
    AppError::system("SESSION_LOG_IO", error.to_string())
}

impl SessionLogArchivePort for FilesystemSessionLogs {
    fn save(&self, session_id: &str, content: &str) -> AppResult<()> {
        let path = self.path(session_id)?;
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let temp = path.with_extension("log.part");
        std::fs::write(&temp, content).map_err(io)?;
        std::fs::rename(&temp, &path).map_err(io)
    }

    fn load(&self, session_id: &str) -> AppResult<Option<String>> {
        let path = self.path(session_id)?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io(error)),
        }
    }

    fn prune(&self, keep: usize) -> AppResult<()> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Ok(());
        };
        let mut logs: Vec<_> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        logs.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        for (_, path) in logs.into_iter().skip(keep) {
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_saved_read_back_and_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let logs = FilesystemSessionLogs::new(dir.path().join("session-logs"));
        let ids = ["0a-1", "0b-2", "0c-3"];
        for id in ids {
            logs.save(id, &format!("log {id}")).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(logs.load("0b-2").unwrap().as_deref(), Some("log 0b-2"));
        assert!(logs.load("ff").unwrap().is_none());
        logs.prune(2).unwrap();
        assert!(logs.load("0a-1").unwrap().is_none());
        assert!(logs.load("0c-3").unwrap().is_some());
        assert!(logs.save("../escape", "x").is_err());
    }
}
