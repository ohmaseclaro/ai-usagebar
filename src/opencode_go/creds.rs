//! Locate the OpenCode Go API key the official OpenCode CLI already holds.
//!
//! The CLI keeps credentials in its data directory (`$XDG_DATA_HOME/opencode`,
//! else `~/.local/share/opencode`). Current releases store them in the SQLite
//! database `opencode.db` (`OPENCODE_DB` overrides it), table `credential`,
//! one row per integration with a JSON `value` such as
//! `{"type":"key","key":"..."}`. Older releases wrote
//! `auth.json`, shaped `{ "<provider id>": { "type": "api", "key": "..." } }`;
//! the database imports it once and never writes it again, so the database is
//! searched first. A Go key lives under the integration `opencode-go`; the
//! console account key filed under `opencode` (Zen) is accepted after it.
//!
//! This module reads both read-only and never writes, copies, or caches the
//! key, exactly like anthropic, cursor, kiro, and devin treat the credential
//! store the official CLI already wrote. The entry must declare a key-bearing
//! type: an OAuth or well-known entry keeps its material in other fields, so
//! it is not read as an API key.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::cache::home_dir;
use crate::error::{AppError, Result};

/// Integration ids holding a key the usage endpoint accepts, most specific
/// first.
const PROVIDER_IDS: &[&str] = &["opencode-go", "opencode"];

/// Entry types whose `key` field is a usable API key. Anything else — in
/// particular OpenCode's `oauth` and `wellknown` types, which hold the secret
/// in `token`/`refresh` — must not be read as a key.
const KEY_TYPES: &[&str] = &["key", "api", "zen"];

const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";

/// Read the OpenCode Go key out of one credential store: the CLI's SQLite
/// database or a legacy `auth.json`, told apart by the file's header.
///
/// A missing or malformed file, no matching entry, or an entry whose type
/// does not carry a key yields `None` rather than an error: one broken shape
/// must not mask a working path later in the search list.
pub fn read_key_from(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.starts_with(SQLITE_MAGIC) {
        return read_key_from_db(path);
    }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    PROVIDER_IDS
        .iter()
        .find_map(|id| key_from_entry(value.get(*id)?))
}

fn read_key_from_db(path: &Path) -> Option<String> {
    // Read-only: the running CLI owns this database; SQLite allows concurrent
    // readers alongside it.
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    PROVIDER_IDS.iter().find_map(|id| {
        let value: String = conn
            .query_row(
                "SELECT value FROM credential WHERE integration_id = ?1 \
                 ORDER BY active DESC, time_created DESC LIMIT 1",
                [id],
                |row| row.get(0),
            )
            .ok()?;
        key_from_entry(&serde_json::from_str(&value).ok()?)
    })
}

fn key_from_entry(entry: &Value) -> Option<String> {
    let entry = entry.as_object()?;
    if !entry
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| KEY_TYPES.contains(&t))
    {
        return None;
    }
    let key = entry.get("key")?.as_str()?.trim();
    (!key.is_empty()).then(|| key.to_string())
}

/// The CLI's database: `OPENCODE_DB` when set (a relative name resolves
/// against the data dir, as the CLI does), else `opencode.db` there.
pub fn default_db_path() -> Result<PathBuf> {
    Ok(db_path(
        std::env::var_os("OPENCODE_DB"),
        &opencode_data_dir()?,
    ))
}

fn db_path(var: Option<OsString>, data: &Path) -> PathBuf {
    match var.filter(|v| !v.is_empty() && v != ":memory:") {
        Some(v) => data.join(v),
        None => data.join("opencode.db"),
    }
}

fn opencode_data_dir() -> Result<PathBuf> {
    Ok(data_dir(std::env::var_os("XDG_DATA_HOME"), home_dir()?).join("opencode"))
}

/// The OpenCode CLI's documented data directory: `$XDG_DATA_HOME` when it is
/// absolute, else `~/.local/share` — on every platform, the troubleshooting
/// docs put `auth.json` in `.local/share` on macOS, Linux, and Windows alike.
pub fn default_auth_path() -> Result<PathBuf> {
    Ok(opencode_data_dir()?.join("auth.json"))
}

/// Pure XDG rule: the variable wins only when absolute, as the XDG Base
/// Directory spec requires; otherwise the documented default under home.
fn data_dir(var: Option<OsString>, home: PathBuf) -> PathBuf {
    if let Some(dir) = var.map(PathBuf::from).filter(|dir| dir.is_absolute()) {
        return dir;
    }
    home.join(".local").join("share")
}

/// The effective search list: a configured override when it names at least
/// one path, else the CLI's database, then its legacy `auth.json`.
pub fn effective_paths(configured: Option<&[PathBuf]>) -> Result<Vec<PathBuf>> {
    match configured {
        Some(paths) if !paths.is_empty() => Ok(paths.to_vec()),
        _ => Ok(vec![default_db_path()?, default_auth_path()?]),
    }
}

/// Detection probe: any effective path holding a usable key. The parse here
/// is just the one provider entry, never a subprocess — every frontend start
/// runs this, and the fetch reads the same file and reports honestly.
pub fn probe(configured: Option<&[PathBuf]>) -> bool {
    effective_paths(configured)
        .ok()
        .is_some_and(|paths| paths.iter().any(|path| read_key_from(path).is_some()))
}

/// Production resolver: configured env var, then inline `api_key`, then the
/// CLI's own credential store. Tests use [`resolve_from`], which takes the values as
/// inputs so an ambient `OPENCODE_GO_API_KEY` cannot leak into them.
pub fn resolve_key(
    env_var_name: &str,
    inline: Option<&str>,
    configured: Option<&[PathBuf]>,
) -> Result<String> {
    let paths = effective_paths(configured)?;
    resolve_from(
        env_var_name,
        std::env::var(env_var_name).ok().as_deref(),
        inline,
        &paths,
    )
}

/// Env, then inline config, then each path in turn — the shared lookup used
/// by both call-sites.
pub fn resolve_from(
    env_var_name: &str,
    env_key: Option<&str>,
    inline: Option<&str>,
    paths: &[PathBuf],
) -> Result<String> {
    if let Some(key) = non_empty(env_key).or_else(|| non_empty(inline)) {
        return Ok(key.to_string());
    }
    for path in paths {
        if let Some(key) = read_key_from(path) {
            return Ok(key);
        }
    }
    Err(AppError::Credentials(signed_out_message(env_var_name)))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

fn signed_out_message(env_var_name: &str) -> String {
    format!(
        "OpenCode Go is not signed in. Run `opencode auth login` and choose \
         OpenCode Go, or set {env_var_name}."
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn write(dir: &Path, name: &str, value: Value) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    }

    #[test]
    fn reads_the_current_api_key_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "auth.json",
            serde_json::json!({
                "opencode": {"type": "api", "key": "sk-live-key"}
            }),
        );
        assert_eq!(read_key_from(&path).unwrap(), "sk-live-key");
    }

    #[test]
    fn accepts_the_legacy_zen_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "auth.json",
            serde_json::json!({"opencode": {"type": "zen", "key": "zen-key"}}),
        );
        assert_eq!(read_key_from(&path).unwrap(), "zen-key");
    }

    #[test]
    fn oauth_and_wellknown_entries_are_not_api_keys() {
        let dir = tempfile::tempdir().unwrap();
        let oauth = write(
            dir.path(),
            "oauth.json",
            serde_json::json!({"opencode": {"type": "oauth", "token": "tok"}}),
        );
        assert!(read_key_from(&oauth).is_none());
        // A well-known entry keeps the env-var name in `key` and the actual
        // secret in `token`; reading `key` would send the wrong value.
        let wellknown = write(
            dir.path(),
            "wellknown.json",
            serde_json::json!({"opencode": {"type": "wellknown", "key": "OPENAI_API_KEY", "token": "sk-real"}}),
        );
        assert!(read_key_from(&wellknown).is_none());
    }

    #[test]
    fn missing_provider_entry_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "auth.json",
            serde_json::json!({"google": {"type": "api", "key": "sk-google"}}),
        );
        assert_eq!(read_key_from(&path), None);
    }

    #[test]
    fn blank_key_or_malformed_file_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let blank = write(
            dir.path(),
            "blank.json",
            serde_json::json!({"opencode": {"type": "api", "key": "  "}}),
        );
        assert_eq!(read_key_from(&blank), None);

        let malformed = dir.path().join("malformed.json");
        std::fs::write(&malformed, "not json").unwrap();
        assert_eq!(read_key_from(&malformed), None);
        assert_eq!(read_key_from(&dir.path().join("missing.json")), None);
    }

    fn seed_db(path: &Path, rows: &[(&str, Value, Option<bool>, i64)]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE credential (id TEXT PRIMARY KEY, integration_id TEXT, \
             label TEXT NOT NULL, value TEXT NOT NULL, connector_id TEXT, method_id TEXT, \
             active INTEGER, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL)",
        )
        .unwrap();
        for (index, (integration, value, active, created)) in rows.iter().enumerate() {
            conn.execute(
                "INSERT INTO credential VALUES (?1, ?2, 'label', ?3, NULL, NULL, ?4, ?5, ?5)",
                rusqlite::params![
                    format!("row-{index}"),
                    integration,
                    value.to_string(),
                    active,
                    created
                ],
            )
            .unwrap();
        }
    }

    #[test]
    fn reads_the_go_key_from_the_cli_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode.db");
        seed_db(
            &path,
            &[
                (
                    "opencode",
                    serde_json::json!({"type": "key", "key": "zen-key"}),
                    Some(true),
                    1,
                ),
                (
                    "opencode-go",
                    serde_json::json!({"type": "key", "key": "old-key"}),
                    Some(false),
                    3,
                ),
                (
                    "opencode-go",
                    serde_json::json!({"type": "key", "key": "go-key"}),
                    Some(true),
                    2,
                ),
            ],
        );
        assert_eq!(read_key_from(&path).unwrap(), "go-key");
    }

    #[test]
    fn database_oauth_rows_and_other_integrations_are_not_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode.db");
        seed_db(
            &path,
            &[
                (
                    "opencode-go",
                    serde_json::json!({"type": "oauth", "access": "tok"}),
                    Some(true),
                    1,
                ),
                (
                    "anthropic",
                    serde_json::json!({"type": "key", "key": "other"}),
                    Some(true),
                    1,
                ),
            ],
        );
        assert_eq!(read_key_from(&path), None);
        let empty = dir.path().join("empty.db");
        Connection::open(&empty)
            .unwrap()
            .execute_batch("CREATE TABLE t (x)")
            .unwrap();
        assert_eq!(read_key_from(&empty), None);
    }

    #[test]
    fn auth_json_prefers_the_go_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "auth.json",
            serde_json::json!({
                "opencode": {"type": "api", "key": "zen-key"},
                "opencode-go": {"type": "api", "key": "go-key"}
            }),
        );
        assert_eq!(read_key_from(&path).unwrap(), "go-key");
    }

    #[test]
    fn db_path_follows_opencode_db() {
        let data = PathBuf::from("data");
        assert_eq!(db_path(None, &data), data.join("opencode.db"));
        assert_eq!(
            db_path(Some(":memory:".into()), &data),
            data.join("opencode.db")
        );
        assert_eq!(db_path(Some("beta.db".into()), &data), data.join("beta.db"));
        #[cfg(not(windows))]
        assert_eq!(
            db_path(Some("/x/o.db".into()), &data),
            PathBuf::from("/x/o.db")
        );
    }

    #[test]
    fn data_dir_prefers_an_absolute_xdg_data_home() {
        // An absolute path is platform-shaped: `C:\…`/UNC on Windows, `/…`
        // elsewhere — `is_absolute` is false for `/…` on Windows.
        #[cfg(windows)]
        let absolute = PathBuf::from(r"C:\custom\data");
        #[cfg(not(windows))]
        let absolute = PathBuf::from("/custom/data");
        let home = PathBuf::from(if cfg!(windows) {
            r"C:\Users\you"
        } else {
            "/home/you"
        });
        assert_eq!(
            data_dir(Some(absolute.clone().into_os_string()), home.clone()),
            absolute
        );
        // Relative values are discarded, per the XDG spec.
        assert_eq!(
            data_dir(Some(OsString::from("relative/data")), home.clone()),
            home.join(".local").join("share")
        );
        assert_eq!(
            data_dir(None, home.clone()),
            home.join(".local").join("share")
        );
    }

    #[test]
    fn effective_paths_override_names_at_least_one_path() {
        let paths = vec![PathBuf::from("/a/auth.json"), PathBuf::from("/b/auth.json")];
        assert_eq!(effective_paths(Some(&paths)).unwrap(), paths);
    }

    #[test]
    fn resolve_env_beats_inline_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "auth.json",
            serde_json::json!({"opencode": {"type": "api", "key": "file-key"}}),
        );
        let paths = [path];
        assert_eq!(
            resolve_from(
                "OPENCODE_GO_API_KEY",
                Some("env-key"),
                Some("inline-key"),
                &paths
            )
            .unwrap(),
            "env-key"
        );
        assert_eq!(
            resolve_from("OPENCODE_GO_API_KEY", None, Some("inline-key"), &paths).unwrap(),
            "inline-key"
        );
        assert_eq!(
            resolve_from("OPENCODE_GO_API_KEY", None, None, &paths).unwrap(),
            "file-key"
        );
    }

    #[test]
    fn probe_sees_a_live_key_and_ignores_an_absent_one() {
        let dir = tempfile::tempdir().unwrap();
        let live = write(
            dir.path(),
            "live.json",
            serde_json::json!({"opencode": {"type": "api", "key": "sk-key"}}),
        );
        let empty = write(
            dir.path(),
            "empty.json",
            serde_json::json!({"google": {"type": "api", "key": "sk-key"}}),
        );
        assert!(probe(Some(std::slice::from_ref(&live))));
        assert!(!probe(Some(&[empty])));
        assert!(!probe(Some(&[dir.path().join("missing.json")])));
    }

    #[test]
    fn resolve_failure_names_both_ways_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "auth.json",
            serde_json::json!({"google": {"type": "api", "key": "sk-key"}}),
        );
        let paths = [path];
        let error = resolve_from("OPENCODE_GO_API_KEY", None, None, &paths).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("opencode auth login"), "{message}");
        assert!(message.contains("OPENCODE_GO_API_KEY"), "{message}");
    }
}
