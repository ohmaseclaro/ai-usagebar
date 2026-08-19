//! `ai-usagebar sync …` entry point. Owned by plan 2-01, extended with
//! `push --dry-run` by plan 2-07.
//!
//! Output carries paths and byte counts only — never a file's contents. This
//! command's whole job is telling the user what *would* leave the machine, so
//! printing any of it here would defeat the point.
//!
//! **Nothing in this module opens a socket.** `--dry-run` measures a push
//! without performing one, and `sync push` without it refuses rather than
//! half-executing: there is no transport in this build to half-execute with.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::config::Config;
use crate::sync::crypto::{Keyfile, Keys};
use crate::sync::index::{self, Index};
use crate::sync::report::DryRunReport;
use crate::sync::{SyncRoots, passphrase, plan, report};
use crate::widget::cli::SyncAction;

/// Same shape as `account::run` / `tui::settings::run_cli`: an exit code, no
/// async, no Waybar exit-0 contract — a script piping this deserves a real code.
pub fn run(action: &SyncAction) -> i32 {
    match action {
        SyncAction::Status => status(),
        SyncAction::Push { dry_run: true } => dry_run(),
        SyncAction::Push { dry_run: false } => no_transport(),
    }
}

/// Config, roots, and the index — everything both subcommands need.
///
/// The index is a hint (D5): if it will not open, the scan is still the truth
/// and only the last-sync line and the would-upload column are lost.
fn open() -> std::result::Result<(Config, SyncRoots, Option<Index>), String> {
    let config = Config::load().map_err(|e| format!("could not read the config file: {e}"))?;
    let roots = SyncRoots::resolve(&config).map_err(|e| e.to_string())?;
    let index = match index::default_path().and_then(|p| Index::at(&p)) {
        Ok(i) => Some(i),
        Err(e) => {
            eprintln!("sync: local index unavailable, last-sync unknown ({e})");
            None
        }
    };
    Ok((config, roots, index))
}

fn status() -> i32 {
    let (config, roots, index) = match open() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sync: {e}");
            return 1;
        }
    };

    // UX-02 is "what would change now", so status builds a plan when it can —
    // and still prints plan 2-01's counts-only form when it cannot.
    let (plan, _) = try_plan(&roots, &config, index.as_ref());
    let report = report::build_status(&roots, &config.sync, index.as_ref(), Utc::now(), plan);
    print!("{}", report::render_status(&report));
    0
}

fn dry_run() -> i32 {
    let (config, roots, index) = match open() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sync: {e}");
            return 1;
        }
    };

    let (plan, no_key) = try_plan(&roots, &config, index.as_ref());
    let report = DryRunReport {
        status: report::build_status(&roots, &config.sync, index.as_ref(), Utc::now(), plan),
        no_key,
    };
    print!("{}", report::render_dry_run(&report));
    0
}

/// The would-upload half, or the reason there is none.
///
/// Never fatal: the file counts and raw bytes need no key at all, and a user
/// checking what is in scope should not have to authenticate (SCOPE-04).
fn try_plan(
    roots: &SyncRoots,
    config: &Config,
    index: Option<&Index>,
) -> (Option<plan::SyncPlan>, Option<String>) {
    let Some(index) = index else {
        return (None, Some("the local index could not be opened".into()));
    };
    let keys = match keys_at(&keyfile_path(roots)) {
        Ok(k) => k,
        Err(why) => return (None, Some(why)),
    };
    match plan::build_with_keys(roots, &config.sync, index, Utc::now(), &keys) {
        Ok(p) => (Some(p), None),
        Err(e) => (None, Some(e.to_string())),
    }
}

/// The local copy of the bundle keyfile, beside `config.toml`.
///
/// Derived from the injected [`SyncRoots`] rather than resolved here, so a test
/// points it at a temp directory the same way every other collector is pointed.
///
/// **Read-only.** Creating this file is the job of the guided setup that pairs
/// the repo and sets the sync password; this command only uses one that already
/// exists, and says so plainly when it does not.
fn keyfile_path(roots: &SyncRoots) -> PathBuf {
    roots.config_dir.join("sync").join("keyfile.json")
}

/// Open the keyfile at `path` with a password read from stdin.
///
/// The password arrives on stdin only — never argv, never an environment
/// variable (T-2-29) — is held in a `Zeroizing<String>`, and never reaches an
/// error message: a wrong one produces `crypto`'s own single indistinguishable
/// refusal. Neither the keyfile's bytes nor any derived key is formatted into
/// the `String` this returns.
fn keys_at(path: &Path) -> std::result::Result<Keys, String> {
    let raw = std::fs::read_to_string(path).map_err(|_| {
        format!(
            "this bundle has no sync keyfile yet ({} is absent)\n\
             only the third column needs one; the counts and raw bytes above need \
             no password at all",
            path.display()
        )
    })?;
    let keyfile: Keyfile = serde_json::from_str(&raw)
        .map_err(|_| format!("{} is not a readable sync keyfile", path.display()))?;

    if std::io::stdin().is_terminal() {
        return Err(
            "the sync password must be piped in on stdin; this build has no interactive \
             prompt"
                .into(),
        );
    }
    let pw = passphrase::read_line(std::io::stdin().lock()).map_err(|e| e.to_string())?;
    // Not merely a nicety: without this an unattended run with stdin on
    // /dev/null would spend a gibibyte and a second and a half hashing the
    // empty string before being told it was wrong.
    if pw.is_empty() {
        return Err("no sync password arrived on stdin".into());
    }

    // Argon2id at m = 1 GiB is a deliberate cost. Announce it before it starts —
    // a command that appears frozen for a second and a half reads as a hang.
    eprintln!("sync: deriving the sync key (Argon2id — this takes a moment)…");
    keyfile.open(pw.as_bytes()).map_err(|e| e.to_string())
}

/// `sync push` without `--dry-run`.
///
/// Non-zero and nothing attempted. There is no transport in this build, so
/// there is nothing here that could half-execute a push or reach a network —
/// and the private-repo gate that must precede any upload does not exist yet
/// either.
fn no_transport() -> i32 {
    eprintln!(
        "sync: this build cannot upload. The push transport — packs, the atomic \
         snapshot flip, and the private-repo gate that has to pass before a single \
         byte moves — is not in it yet.\n\
         \x20     Use `ai-usagebar sync push --dry-run` to see exactly what a push \
         would send."
    );
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn roots_at(dir: &TempDir) -> SyncRoots {
        SyncRoots::at(
            dir.path().join("config.toml"),
            dir.path().to_path_buf(),
            dir.path().join("desktop"),
            dir.path().join("profiles"),
            dir.path().join("claude-home"),
        )
    }

    #[test]
    fn the_keyfile_sits_beside_the_config_and_is_never_resolved_from_home() {
        let dir = TempDir::new().unwrap();
        let path = keyfile_path(&roots_at(&dir));
        assert!(path.starts_with(dir.path()), "{}", path.display());
        assert!(path.ends_with("sync/keyfile.json"), "{}", path.display());
    }

    /// The common case in this build: no bundle has been set up, so the column
    /// is unavailable — and the message says the counts still are.
    #[test]
    fn a_missing_keyfile_explains_itself_without_failing_the_command() {
        let dir = TempDir::new().unwrap();
        let err = keys_at(&keyfile_path(&roots_at(&dir))).expect_err("no keyfile was written");
        assert!(err.contains("no sync keyfile"), "{err}");
        assert!(err.contains("need no password"), "{err}");
    }

    /// T-2-30: a refusal names the file, never its bytes.
    #[test]
    fn an_unreadable_keyfile_is_refused_without_echoing_its_contents() {
        let dir = TempDir::new().unwrap();
        let path = keyfile_path(&roots_at(&dir));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{\"wrapped_master_key\": \"not-a-keyfile-at-all\"}").unwrap();

        let err = keys_at(&path).expect_err("that is not a keyfile");
        assert!(err.contains("not a readable sync keyfile"), "{err}");
        assert!(!err.contains("not-a-keyfile-at-all"), "{err}");
    }

    #[test]
    fn a_push_without_dry_run_refuses_non_zero_and_points_at_the_dry_run() {
        assert_ne!(run(&SyncAction::Push { dry_run: false }), 0);
    }
}
