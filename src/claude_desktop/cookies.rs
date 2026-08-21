//! Row-level access to a profile's Chromium cookie jar, so the **web view's
//! login** can cross machines the way the token cache already does.
//!
//! # The defect this exists for
//!
//! Plan 6-13 made every Claude Desktop *account* travel: `config-tokenCache*`
//! is decrypted with the pushing Mac's Safe Storage key and re-sealed under the
//! target's. That authenticates the **API** — `ai-usagebar usage` shows all four
//! accounts' quota on the second Mac — and it is what
//! [`crate::anthropic::desktop_creds`] reads.
//!
//! The app itself signs in from neither. Claude Desktop is Electron, and its
//! renderer's "who am I" is the cookie jar: `sessionKey` / `sessionKeyV3` on
//! `.claude.ai`. `super::restore_desktop_state` moves exactly two sets of
//! things when you switch accounts locally —
//!
//! ```text
//! COOKIE_FILES  = ["Cookies", "Cookies-journal"]
//! LEVELDB_DIRS  = ["Local Storage", "Session Storage", "IndexedDB"]
//! ```
//!
//! — and a sync bundle already carries both as ordinary files. The LevelDB
//! trees are plaintext and therefore arrive working. The cookie *values* are
//! not: every `encrypted_value` in that table is a Chromium safeStorage `v10`
//! blob under a key in the **pushing** Mac's login Keychain, so the file lands
//! on the target complete and inert, and the app opens on a login screen.
//!
//! Measured on the reporting user's machine, per profile: 26 rows, 4,014 bytes
//! of `encrypted_value`, every one of them `v10`; zero `v10` blobs anywhere in
//! the 17 MB of LevelDB. **The cookie table is the entire remaining delta
//! between a local account switch and a restored one.**
//!
//! # What is deliberately *not* here
//!
//! - **`bridge-state.json`** — the remote-control / cloud-session bridge. It is
//!   in `crate::sync::scope`'s `EXCLUDED_NAMES` and is not in the profile store
//!   to begin with (a switch deletes it and never snapshots it; see
//!   `super::BRIDGE_FILE`), so no bundle has ever carried one. Carrying it would
//!   restore a `cse_…` id that was already stale on the machine that wrote it,
//!   and `/remote-control` would fail to disconnect.
//! - **`ant-device-registry.json`** — browser-extension device registrations,
//!   account-keyed and *per machine*. Excluded the same way, and rightly: a
//!   registration describes the Mac that made it, which is exactly why
//!   [`super::merge::merge_device_registry`] is additive locally. Nothing
//!   arrives from another machine, so there is nothing to merge here.
//!
//! # Identity is the seven-column unique index, not `(host_key, name, path)`
//!
//! Chromium's index is
//!
//! ```text
//! (host_key, top_frame_site_key, has_cross_site_ancestor, name, path,
//!  source_scheme, source_port)
//! ```
//!
//! and on a real jar `top_frame_site_key` and `has_cross_site_ancestor` do vary
//! across rows sharing a host and name (partitioned cookies). Matching on the
//! three obvious columns would let one carried value overwrite a *different*
//! row's — so [`CookieKey`] is the whole index and the `UPDATE` names every
//! column of it.
//!
//! # Values are written, rows are never created or deleted
//!
//! `UPDATE`, never `INSERT`: the jar's twenty other columns — expiry, secure,
//! httponly, samesite, creation time — belong to the `Cookies` file, which
//! travels as a file and lands first (`crate::sync::restore::write::apply`
//! sorts every file ahead of every store). This module supplies the one column
//! a file copy cannot: the value, re-sealed under the reading machine's key.
//! A carried row that matches nothing is counted and skipped, never inserted,
//! so nothing here can invent a cookie.
//!
//! # Hermeticity
//!
//! Every function takes the database path. No test opens the real jar, and the
//! crypto is not in this module at all — [`crate::sync::keystore`] holds the
//! key and this holds the SQL, so the sealed blobs in and out of here are
//! opaque bytes and the whole file compiles and tests on every platform.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

/// How long a write waits for a running Claude Desktop to release the jar
/// before giving up. SQLite's own busy handler, as
/// [`crate::cursor::db`] uses for the same reason: this is the app's file and a
/// second lock protocol only one side observes is not a lock.
const WRITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The columns of Chromium's `cookies_unique_index`, in its order.
///
/// Ordered and `Ord` so a jar serialises to the same bytes twice: the payload
/// is hashed to decide "is this the same login?", and an unstable order would
/// make one unchanged cookie jar look like a different one on every push.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CookieKey {
    pub host_key: String,
    pub top_frame_site_key: String,
    pub has_cross_site_ancestor: i64,
    pub name: String,
    pub path: String,
    pub source_scheme: i64,
    pub source_port: i64,
}

/// `SELECT` and `UPDATE` share one spelling of the index, so the two can never
/// drift into matching on different columns.
const KEY_COLUMNS: &str =
    "host_key, top_frame_site_key, has_cross_site_ancestor, name, path, source_scheme, source_port";

/// Every row of the jar, as its identity and its still-sealed value.
///
/// Sorted by [`CookieKey`], for the digest stability described on it.
///
/// A missing database is an **empty vector** and not an error: that profile has
/// simply never opened the app. A database that exists but will not open *is*
/// an error — reporting an unreadable jar as "no cookies here" is how a push
/// ships a bundle that silently drops the login it was asked to carry.
///
/// Read-only, so this is safe beside a running Claude Desktop.
pub fn read_sealed(path: &Path) -> Result<Vec<(CookieKey, Vec<u8>)>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| opening(path, e))?;
    let sql = format!("SELECT {KEY_COLUMNS}, encrypted_value FROM cookies");
    let mut stmt = conn.prepare(&sql).map_err(|e| reading(path, e))?;
    let mut rows = stmt.query([]).map_err(|e| reading(path, e))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| reading(path, e))? {
        let key = CookieKey {
            host_key: row.get(0).map_err(|e| reading(path, e))?,
            top_frame_site_key: row.get(1).map_err(|e| reading(path, e))?,
            has_cross_site_ancestor: row.get(2).map_err(|e| reading(path, e))?,
            name: row.get(3).map_err(|e| reading(path, e))?,
            path: row.get(4).map_err(|e| reading(path, e))?,
            source_scheme: row.get(5).map_err(|e| reading(path, e))?,
            source_port: row.get(6).map_err(|e| reading(path, e))?,
        };
        let sealed: Vec<u8> = row.get(7).map_err(|e| reading(path, e))?;
        if sealed.is_empty() {
            continue;
        }
        out.push((key, sealed));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Is there a cookie jar with anything in it — **without reading a value**?
///
/// The question `sync status` asks, and it may not read a credential to answer
/// it: the macOS menu bar runs `sync status --json` on every menu open.
/// `SELECT 1 … LIMIT 1` returns no value at all.
pub fn has_rows(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| opening(path, e))?;
    conn.query_row("SELECT 1 FROM cookies LIMIT 1", [], |_| Ok(true))
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(false),
            // No `cookies` table at all is a database that is not a cookie jar.
            // "Nothing here" is the honest reading, not a failure to report.
            other => Err(AppError::Credentials(format!(
                "could not check the Claude Desktop cookie jar at {}: {other}",
                path.display()
            ))),
        })
}

/// Re-seal `rows` into an **existing** jar, leaving every other column and
/// every unnamed row exactly as it was.
///
/// Returns `(updated, missing)` — how many carried rows matched a row in the
/// jar, and how many matched nothing. A miss is not an error: the caller
/// decides, and the only thing that must never happen is a silent claim of
/// success, so a call where nothing at all matched is an `Err`.
///
/// # All or nothing
///
/// One transaction. A failure leaves the jar exactly as it was rather than half
/// re-sealed under two machines' keys — a jar in which some cookies open and
/// others do not is worse than either whole one, because the app would
/// authenticate and then behave unpredictably.
///
/// # If Claude Desktop is running
///
/// SQLite's busy handler waits [`WRITE_BUSY_TIMEOUT`] for the writer lock and
/// then gives up with an error the caller turns into a refusal of this one
/// item. The converse is the case this cannot solve and the caller must state:
/// a running app holds the jar in memory and may write it back over ours.
/// Quit Claude Desktop before restoring into it.
///
/// # A stale `Cookies-journal` is SQLite's problem, and SQLite solves it
///
/// The bundle carries `Cookies-journal` beside `Cookies` because the local
/// switch does. If the pushing Mac's snapshot caught a transaction in flight,
/// that journal is *hot* — and opening the database here is exactly what makes
/// SQLite roll it back, which is the correct recovery and the same one Chromium
/// would perform. Our transaction then runs on the recovered database and its
/// commit removes the journal, so the restored pair is left self-consistent
/// rather than carrying another machine's half-written transaction forward.
/// Nothing here inspects or deletes the journal by hand; a second recovery
/// protocol beside SQLite's own would be the bug, not the fix.
pub fn write_sealed(path: &Path, rows: &[(CookieKey, Vec<u8>)]) -> Result<(usize, usize)> {
    if !path.exists() {
        return Err(AppError::Credentials(format!(
            "no Claude Desktop cookie jar at {}. Sign in to the app once for this profile so it \
             creates one, then restore again — ai-usagebar will not fabricate a cookie jar.",
            path.display()
        )));
    }
    let mut conn = Connection::open(path).map_err(|e| {
        AppError::Credentials(format!(
            "could not open the Claude Desktop cookie jar at {} for writing: {e}",
            path.display()
        ))
    })?;
    conn.busy_timeout(WRITE_BUSY_TIMEOUT).map_err(|e| {
        AppError::Credentials(format!("could not set the cookie jar busy timeout: {e}"))
    })?;
    let tx = conn.transaction().map_err(|e| running(path, e))?;
    // Every column of `cookies_unique_index`, in its order — the same identity
    // `KEY_COLUMNS` selects, spelled out here because a bound parameter cannot
    // be interpolated into a column list.
    let sql = "UPDATE cookies SET encrypted_value = ?1 WHERE \
         host_key = ?2 AND top_frame_site_key = ?3 AND has_cross_site_ancestor = ?4 AND \
         name = ?5 AND path = ?6 AND source_scheme = ?7 AND source_port = ?8";
    let (mut updated, mut missing) = (0usize, 0usize);
    {
        let mut stmt = tx.prepare(sql).map_err(|e| running(path, e))?;
        for (key, sealed) in rows {
            // `sealed` is a live session and appears in no message on any arm.
            // `key.name` is a cookie name — `sessionKey`, `__cf_bm` — which is
            // not a secret, and `{:?}` escapes it because it is bundle data.
            let n = stmt
                .execute(rusqlite::params![
                    sealed,
                    key.host_key,
                    key.top_frame_site_key,
                    key.has_cross_site_ancestor,
                    key.name,
                    key.path,
                    key.source_scheme,
                    key.source_port,
                ])
                .map_err(|e| {
                    AppError::Credentials(format!(
                        "could not write the cookie {:?} (is Claude Desktop running?): {e}",
                        key.name
                    ))
                })?;
            if n == 0 {
                missing += 1;
            } else {
                updated += n;
            }
        }
    }
    if updated == 0 {
        // Committing here would report a restored login where none landed. The
        // transaction is dropped, so the jar is untouched.
        return Err(AppError::Credentials(format!(
            "none of the {} carried cookies matched a row in {} — leaving it untouched",
            rows.len(),
            path.display()
        )));
    }
    tx.commit().map_err(|e| running(path, e))?;
    Ok((updated, missing))
}

fn opening(path: &Path, e: rusqlite::Error) -> AppError {
    AppError::Credentials(format!(
        "could not open the Claude Desktop cookie jar at {}: {e}",
        path.display()
    ))
}

fn reading(path: &Path, e: rusqlite::Error) -> AppError {
    AppError::Credentials(format!(
        "could not read the Claude Desktop cookie jar at {}: {e}",
        path.display()
    ))
}

fn running(path: &Path, e: rusqlite::Error) -> AppError {
    AppError::Credentials(format!(
        "could not write the Claude Desktop cookie jar at {} (is Claude Desktop running?): {e}",
        path.display()
    ))
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Chromium's own `cookies` schema, verbatim from a real Claude Desktop jar
    /// — including the seven-column unique index, which is the thing under
    /// test. Trimming it to the columns this module names would make the tests
    /// agree with the code about a shape neither had checked.
    pub const SCHEMA: &str = "CREATE TABLE cookies(creation_utc INTEGER NOT NULL, \
        host_key TEXT NOT NULL, top_frame_site_key TEXT NOT NULL, name TEXT NOT NULL, \
        value TEXT NOT NULL, encrypted_value BLOB NOT NULL, path TEXT NOT NULL, \
        expires_utc INTEGER NOT NULL, is_secure INTEGER NOT NULL, is_httponly INTEGER NOT NULL, \
        last_access_utc INTEGER NOT NULL, has_expires INTEGER NOT NULL, \
        is_persistent INTEGER NOT NULL, priority INTEGER NOT NULL, samesite INTEGER NOT NULL, \
        source_scheme INTEGER NOT NULL, source_port INTEGER NOT NULL, \
        last_update_utc INTEGER NOT NULL, source_type INTEGER NOT NULL, \
        has_cross_site_ancestor INTEGER NOT NULL); \
        CREATE UNIQUE INDEX cookies_unique_index ON cookies(host_key, top_frame_site_key, \
        has_cross_site_ancestor, name, path, source_scheme, source_port);";

    pub fn key(host: &str, name: &str) -> CookieKey {
        CookieKey {
            host_key: host.to_string(),
            top_frame_site_key: format!("https://{}", host.trim_start_matches('.')),
            has_cross_site_ancestor: 0,
            name: name.to_string(),
            path: "/".to_string(),
            source_scheme: 2,
            source_port: 443,
        }
    }

    /// A jar with `rows` in it, at `path`.
    pub fn seed(path: &Path, rows: &[(CookieKey, Vec<u8>)]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        for (k, v) in rows {
            conn.execute(
                "INSERT INTO cookies (creation_utc, host_key, top_frame_site_key, name, value, \
                 encrypted_value, path, expires_utc, is_secure, is_httponly, last_access_utc, \
                 has_expires, is_persistent, priority, samesite, source_scheme, source_port, \
                 last_update_utc, source_type, has_cross_site_ancestor) \
                 VALUES (1, ?1, ?2, ?3, '', ?4, ?5, 2, 1, 1, 3, 1, 1, 1, 0, ?6, ?7, 4, 0, ?8)",
                rusqlite::params![
                    k.host_key,
                    k.top_frame_site_key,
                    k.name,
                    v,
                    k.path,
                    k.source_scheme,
                    k.source_port,
                    k.has_cross_site_ancestor,
                ],
            )
            .unwrap();
        }
    }

    /// Every column of every row, as text, so a test can assert that the parts
    /// it did not carry came through byte-identical.
    pub fn dump(path: &Path) -> Vec<String> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT creation_utc, host_key, top_frame_site_key, name, value, \
                 quote(encrypted_value), path, expires_utc, is_secure, is_httponly, \
                 last_access_utc, has_expires, is_persistent, priority, samesite, source_scheme, \
                 source_port, last_update_utc, source_type, has_cross_site_ancestor \
                 FROM cookies ORDER BY host_key, name, path",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                let mut cells = Vec::new();
                for i in 0..20 {
                    cells.push(
                        row.get::<_, rusqlite::types::Value>(i)
                            .map(|v| format!("{v:?}"))
                            .unwrap_or_default(),
                    );
                }
                Ok(cells.join("|"))
            })
            .unwrap();
        rows.map(std::result::Result::unwrap).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{dump, key, seed};
    use super::*;

    fn jar(rows: &[(CookieKey, Vec<u8>)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cookies");
        seed(&path, rows);
        (dir, path)
    }

    #[test]
    fn a_missing_jar_is_no_cookies_and_not_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cookies");
        assert!(read_sealed(&path).unwrap().is_empty());
        assert!(!has_rows(&path).unwrap());
    }

    #[test]
    fn a_jar_that_is_not_one_is_an_error_and_never_an_empty_reading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cookies");
        std::fs::write(&path, b"not a database at all").unwrap();
        // The push must not be able to report "no login here" for a jar it
        // could not open — that is how a bundle ships without the credential.
        assert!(read_sealed(&path).is_err());
    }

    #[test]
    fn rows_round_trip_and_come_back_sorted() {
        let (_dir, path) = jar(&[
            (key(".claude.ai", "sessionKey"), b"v10AAA".to_vec()),
            (key(".claude.ai", "__cf_bm"), b"v10BBB".to_vec()),
        ]);
        let got = read_sealed(&path).unwrap();
        assert_eq!(
            got.iter().map(|(k, _)| k.name.as_str()).collect::<Vec<_>>(),
            ["__cf_bm", "sessionKey"],
            "sorted by the index, so the same jar hashes the same twice"
        );
        assert!(has_rows(&path).unwrap());
    }

    #[test]
    fn writing_a_value_leaves_every_other_column_of_every_row_untouched() {
        let session = key(".claude.ai", "sessionKey");
        let other = key(".claude.ai", "__cf_bm");
        let (_dir, path) = jar(&[
            (session.clone(), b"v10-from-mac-a".to_vec()),
            (other.clone(), b"v10-untouched".to_vec()),
        ]);
        let before = dump(&path);

        let (updated, missing) =
            write_sealed(&path, &[(session.clone(), b"v10-resealed-on-b".to_vec())]).unwrap();
        assert_eq!((updated, missing), (1, 0));

        let after = dump(&path);
        assert_eq!(before.len(), after.len(), "no row created or deleted");
        // The row we did not carry is byte-identical, every column of it.
        let untouched = |rows: &[String]| {
            rows.iter()
                .find(|r| r.contains("__cf_bm"))
                .unwrap()
                .to_string()
        };
        assert_eq!(untouched(&before), untouched(&after));

        let sealed: Vec<_> = read_sealed(&path).unwrap();
        let got = sealed.iter().find(|(k, _)| *k == session).unwrap();
        assert_eq!(got.1, b"v10-resealed-on-b");
        // And the row we did carry changed in exactly one column.
        let row_of = |rows: &[String]| {
            rows.iter()
                .find(|r| r.contains("sessionKey"))
                .unwrap()
                .split('|')
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let (b, a) = (row_of(&before), row_of(&after));
        let differing = b.iter().zip(&a).filter(|(x, y)| x != y).count();
        assert_eq!(differing, 1, "exactly one column of that row moved");
    }

    #[test]
    fn identity_is_the_whole_index_so_a_partitioned_twin_is_not_overwritten() {
        // Same host, same name, same path — different partition. On a real jar
        // these coexist; matching on `(host_key, name, path)` would clobber one
        // with the other's value.
        let mut partitioned = key(".claude.ai", "sessionKey");
        partitioned.top_frame_site_key = "https://other.example".to_string();
        partitioned.has_cross_site_ancestor = 1;
        let plain = key(".claude.ai", "sessionKey");
        let (_dir, path) = jar(&[
            (plain.clone(), b"v10-plain".to_vec()),
            (partitioned.clone(), b"v10-partitioned".to_vec()),
        ]);

        let (updated, missing) =
            write_sealed(&path, &[(plain.clone(), b"v10-new".to_vec())]).unwrap();
        assert_eq!((updated, missing), (1, 0));

        let got = read_sealed(&path).unwrap();
        assert_eq!(got.len(), 2);
        let find = |k: &CookieKey| got.iter().find(|(g, _)| g == k).unwrap().1.clone();
        assert_eq!(find(&plain), b"v10-new");
        assert_eq!(
            find(&partitioned),
            b"v10-partitioned",
            "the twin in another partition kept its own value"
        );
    }

    #[test]
    fn a_carried_row_that_matches_nothing_is_counted_and_never_inserted() {
        let present = key(".claude.ai", "sessionKey");
        let (_dir, path) = jar(&[(present.clone(), b"v10-old".to_vec())]);
        let (updated, missing) = write_sealed(
            &path,
            &[
                (present, b"v10-new".to_vec()),
                (key(".claude.ai", "never-seen-here"), b"v10-x".to_vec()),
            ],
        )
        .unwrap();
        assert_eq!((updated, missing), (1, 1));
        assert_eq!(read_sealed(&path).unwrap().len(), 1, "nothing invented");
    }

    #[test]
    fn a_write_that_matches_nothing_at_all_refuses_and_changes_nothing() {
        let (_dir, path) = jar(&[(key(".claude.ai", "sessionKey"), b"v10-mine".to_vec())]);
        let before = dump(&path);
        let err = write_sealed(&path, &[(key(".claude.ai", "stranger"), b"v10-x".to_vec())])
            .unwrap_err()
            .to_string();
        assert!(err.contains("matched a row"), "{err}");
        assert_eq!(dump(&path), before, "the jar is exactly as it was");
    }

    #[test]
    fn a_missing_jar_is_never_fabricated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cookies");
        let err = write_sealed(&path, &[(key(".claude.ai", "sessionKey"), b"v10".to_vec())])
            .unwrap_err()
            .to_string();
        assert!(err.contains("will not fabricate"), "{err}");
        assert!(!path.exists());
    }

    #[test]
    fn no_message_on_any_arm_carries_a_cookie_value() {
        let secret = b"v10-this-is-a-live-session".to_vec();
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("Cookies");
        let mut messages = vec![
            write_sealed(
                &missing,
                &[(key(".claude.ai", "sessionKey"), secret.clone())],
            )
            .unwrap_err()
            .to_string(),
        ];
        let (_d, path) = jar(&[(key(".claude.ai", "other"), b"v10-x".to_vec())]);
        messages.push(
            write_sealed(&path, &[(key(".claude.ai", "sessionKey"), secret.clone())])
                .unwrap_err()
                .to_string(),
        );
        std::fs::write(dir.path().join("bad"), b"nope").unwrap();
        messages.push(
            read_sealed(&dir.path().join("bad"))
                .unwrap_err()
                .to_string(),
        );
        for m in messages {
            assert!(
                !m.contains("this-is-a-live-session"),
                "a message carried a cookie value: {m}"
            );
        }
    }
}
