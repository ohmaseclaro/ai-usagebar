//! The local change-detection index (D5). Schema owned by plan 2-03; plan 2-01
//! opens the file and reads the one key `sync status` needs.
//!
//! It lives under `~/.cache`, **not** `~/.config`, precisely so it is never
//! itself synced. It is a *hint*: deleting it must degrade to a full re-scan,
//! never to a wrong answer.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::error::{AppError, Result};

/// `~/.cache/ai-usagebar/sync/index.sqlite3`, via the same resolver the vendor
/// caches use.
pub fn default_path() -> Result<PathBuf> {
    Ok(crate::cache::xdg_cache_dir()?
        .join("ai-usagebar")
        .join("sync")
        .join("index.sqlite3"))
}

/// The index database. Constructed with [`Index::at`] everywhere, including
/// production — [`default_path`] is the only thing that knows about `$HOME`.
pub struct Index {
    conn: Connection,
    path: PathBuf,
}

impl Index {
    /// Open (creating if absent) the index at `path`.
    ///
    /// The file is created and set to mode 0600 **before** the connection is
    /// opened: its rows carry account UUIDs inside full paths, so the mode is
    /// never briefly wrong on a shared machine.
    pub fn at(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| AppError::io_at(dir, e))?;
        }
        create_private(path)?;
        let conn = Connection::open(path)
            .map_err(|e| AppError::Other(format!("could not open {}: {e}", path.display())))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v BLOB)",
            [],
        )
        .map_err(|e| AppError::Other(format!("could not initialise {}: {e}", path.display())))?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// Where this index lives — so a report can name it without resolving
    /// `$HOME` itself, which is what keeps the report builder hermetic.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// When the last successful sync completed, if the index knows. `None` is
    /// a normal first-run answer, not an error.
    pub fn last_sync(&self) -> Option<DateTime<Utc>> {
        let raw: String = self
            .conn
            .query_row("SELECT v FROM meta WHERE k = 'last_sync'", [], |row| {
                row.get(0)
            })
            .ok()?;
        DateTime::parse_from_rfc3339(&raw)
            .ok()
            .map(|t| t.with_timezone(&Utc))
    }
}

/// Create the file mode-0600 if it does not exist. SQLite is happy to adopt a
/// zero-length file, which is what lets the mode be right from byte zero.
fn create_private(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    match opts.open(path) {
        Ok(_) => Ok(()),
        // Lost a race with another process that just created it — fine.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(AppError::io_at(path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn opening_a_fresh_index_creates_the_file_and_reports_no_last_sync() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("index.sqlite3");
        let index = Index::at(&path).unwrap();
        assert!(path.exists());
        assert!(index.last_sync().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn the_index_file_is_created_mode_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("index.sqlite3");
        let _index = Index::at(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    }

    #[test]
    fn a_recorded_last_sync_reads_back() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("index.sqlite3");
        let index = Index::at(&path).unwrap();
        index
            .conn
            .execute(
                "INSERT INTO meta (k, v) VALUES ('last_sync', '2026-08-19T12:00:00Z')",
                [],
            )
            .unwrap();
        assert_eq!(
            index.last_sync().map(|t| t.to_rfc3339()),
            Some("2026-08-19T12:00:00+00:00".to_string())
        );
    }

    #[test]
    fn a_garbage_last_sync_value_reads_as_unknown_not_an_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("index.sqlite3");
        let index = Index::at(&path).unwrap();
        index
            .conn
            .execute(
                "INSERT INTO meta (k, v) VALUES ('last_sync', 'not-a-date')",
                [],
            )
            .unwrap();
        assert!(index.last_sync().is_none());
    }
}
