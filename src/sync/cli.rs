//! `ai-usagebar sync …` entry point. Owned by plan 2-01, extended with
//! `push --dry-run` by plan 2-07 and `setup` by plan 3-01.
//!
//! Output carries paths and byte counts only — never a file's contents. This
//! command's whole job is telling the user what *would* leave the machine, so
//! printing any of it here would defeat the point.
//!
//! **Only `setup` opens a socket, and only to `GET`.** `--dry-run` measures a
//! push without performing one, `sync push` without it refuses rather than
//! half-executing, and `setup` verifies the remote is private without uploading
//! a byte — the client it uses has no method that can carry a request body.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::config::{Config, SyncCategory};
use crate::sync::crypto::{Keyfile, Keys};
use crate::sync::github::setup::TtyPrompt;
use crate::sync::github::{
    self, Client, Endpoints, RepoRef, gate, pairing, token, token::TokenChain,
};
use crate::sync::index::Index;
use crate::sync::report::{DryRunReport, RepoSection};
use crate::sync::{SyncRoots, passphrase, plan, report};
use crate::widget::cli::SyncAction;

/// Same shape as `account::run` / `tui::settings::run_cli`: an exit code, no
/// Waybar exit-0 contract — a script piping this deserves a real code. The
/// widget's exit-0 invariant is a property of `widget::run::fallback`; `sync`
/// reports failure honestly (D-06).
///
/// **The thin wrapper that resolves the real world.** No test calls it: it
/// reads `$HOME` through `Config::load` and `TokenChain::production`, and the
/// AUR `check()` runs `cargo test` on installers' machines. Everything below
/// hangs off [`run_with`].
pub fn run(action: &SyncAction) -> i32 {
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sync: could not read the config file: {e}");
            return 1;
        }
    };
    let roots = match SyncRoots::resolve(&config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("sync: {e}");
            return 1;
        }
    };
    run_with(
        action,
        &config,
        &roots,
        &Endpoints::default(),
        &TokenChain::production(),
        Utc::now(),
    )
}

/// Every dependency injected: the config, the roots, both GitHub hosts, the
/// token chain, and the clock. This is what tests drive.
pub fn run_with(
    action: &SyncAction,
    cfg: &Config,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &TokenChain,
    now: DateTime<Utc>,
) -> i32 {
    match action {
        SyncAction::Status => status(cfg, roots, endpoints, chain, now),
        SyncAction::Setup => setup(cfg, roots, endpoints, chain, now),
        SyncAction::Push { dry_run: true } => dry_run(cfg, roots, now),
        SyncAction::Push { dry_run: false } => no_transport(),
    }
}

/// One current-thread runtime, built only where one is actually needed.
///
/// `src/bin/ai-usagebar.rs` dispatches `Command::Sync` before it constructs a
/// runtime, which keeps `push --dry-run` and an unconfigured `status` paying
/// nothing for a reactor they never use.
fn runtime() -> std::result::Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("could not start the async runtime ({e})"))
}

/// The index is a hint (D5): if it will not open, the scan is still the truth
/// and only the last-sync line and the would-upload column are lost.
fn open_index(roots: &SyncRoots) -> Option<Index> {
    match Index::at(&roots.index_file) {
        Ok(i) => Some(i),
        Err(e) => {
            eprintln!("sync: local index unavailable, last-sync unknown ({e})");
            None
        }
    }
}

/// `sync status` — the category listing, plus what plan 3-07 knows about the
/// repository.
///
/// The two halves fail independently on purpose: a dead token still leaves the
/// category listing visible (a user should be able to see what *would* be sent
/// without authenticating), but a repository-section failure is still a
/// non-zero exit (D-06, REPO-05, T-3-41).
fn status(
    config: &Config,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &TokenChain,
    now: DateTime<Utc>,
) -> i32 {
    let index = open_index(roots);
    // UX-02 is "what would change now", so status builds a plan when it can —
    // and still prints plan 2-01's counts-only form when it cannot.
    let (plan, _) = try_plan(roots, config, index.as_ref(), now);
    let repo = resolve_repo_section(config, roots, endpoints, chain, now);
    let failed = repo.failure.is_some();
    let report = report::build_status(roots, &config.sync, index.as_ref(), now, plan, Some(repo));
    print!("{}", report::render_status(&report));
    i32::from(failed)
}

/// The repository section, or the reason there is none.
///
/// An **unconfigured** machine gets an empty section and no runtime at all — a
/// network error reported by a machine that never named a repository would be a
/// lie.
fn resolve_repo_section(
    config: &Config,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &TokenChain,
    now: DateTime<Utc>,
) -> RepoSection {
    if config.sync.repo.is_none() {
        return RepoSection::default();
    }
    match runtime() {
        Ok(rt) => rt.block_on(repo_section(config, roots, endpoints, chain, now)),
        Err(why) => RepoSection {
            configured: config.sync.repo.clone(),
            failure: Some(why),
            ..RepoSection::default()
        },
    }
}

/// **Exactly one request to the repository endpoint.** `fetch_facts` is called
/// once and its result is handed to both the drift check and the report; the
/// gate is not re-run per consumer.
async fn repo_section(
    config: &Config,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &TokenChain,
    now: DateTime<Utc>,
) -> RepoSection {
    let mut section = RepoSection {
        configured: config.sync.repo.clone(),
        ..RepoSection::default()
    };
    let Some(configured) = config.sync.repo.as_deref() else {
        return section;
    };
    let repo = match RepoRef::parse(configured) {
        Ok(repo) => repo,
        Err(e) => return section.failed(e.to_string()),
    };

    let record = match pairing::read_from(&pairing::default_path(roots)) {
        Ok(record) => record,
        Err(e) => return section.failed(e.to_string()),
    };
    section.last_verified = record.as_ref().map(|p| p.checked_at);

    let (value, source) = match token::resolve(chain) {
        Ok(pair) => pair,
        Err(e) => return section.failed(e.to_string()),
    };
    // The label, never the value. `RepoSection` has no field that could hold one.
    section.token_source = Some(source.label());

    let client = match Client::new(endpoints, value, source) {
        Ok(client) => client,
        Err(e) => return section.failed(e.to_string()),
    };
    let facts = match gate::fetch_facts(&client, &repo, now).await {
        Ok(facts) => facts,
        // Same 401 promise the setup flow keeps: `actionable` says the stored
        // token will be cleared, so whichever command printed that must clear it.
        //
        // **`token::clear` deletes the real macOS login Keychain item.** No
        // test here may mock a 401 against this arm; `sync setup`'s version of
        // this call goes through `SetupPrompt::clear_token`, which a test double
        // overrides. If `status` ever needs a 401 test, give it the same seam
        // first — do not reach for the production function.
        Err(e) => {
            let e =
                github::setup::clear_if_dead(e, &github::setup::token_path(roots), &token::clear);
            return section.failed(e.to_string());
        }
    };
    section.visibility = Some(facts.visibility.clone());

    // One source, both gate calls — deriving it twice reintroduces the
    // contradiction plan 3-04 removed.
    let credentials_in_bundle = config.sync.includes(SyncCategory::Credentials);
    match pairing::check_drift(record.as_ref(), &facts, credentials_in_bundle, now) {
        // The SAFE-02 incident renders verbatim: a repository going public is
        // the same event whichever command noticed it.
        Err(e) => return section.failed(e.to_string()),
        Ok(drift) => section.warnings.extend(drift.warnings),
    }
    match gate::assert_pushable(&facts, &repo, credentials_in_bundle, now) {
        Err(e) => return section.failed(e.to_string()),
        Ok((_clearance, warnings)) => section.warnings.extend(warnings),
    }
    section
}

fn dry_run(config: &Config, roots: &SyncRoots, now: DateTime<Utc>) -> i32 {
    let index = open_index(roots);
    let (plan, no_key) = try_plan(roots, config, index.as_ref(), now);
    let report = DryRunReport {
        status: report::build_status(roots, &config.sync, index.as_ref(), now, plan, None),
        no_key,
    };
    print!("{}", report::render_dry_run(&report));
    0
}

/// `sync setup` — pair with the configured private repository.
///
/// **The runtime is built here.** `src/bin/ai-usagebar.rs` dispatches
/// `Command::Sync` before it constructs one, which was correct when `sync` only
/// scanned the filesystem. Keeping the dispatch where it is means `status` and
/// `push --dry-run` still pay nothing for a runtime they do not use.
///
/// The success line reports the token's **source** and never its value; the
/// failure path prints the message and nothing else — no token, no prefix of
/// one, no header dump (T-3-01).
fn setup(
    cfg: &Config,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &TokenChain,
    now: DateTime<Utc>,
) -> i32 {
    let rt = match runtime() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("sync: {e}");
            return 1;
        }
    };
    let mut prompt = TtyPrompt;
    match rt.block_on(github::setup::run(
        &cfg.sync,
        roots,
        endpoints,
        chain,
        &mut prompt,
        now,
    )) {
        Ok(outcome) => {
            print!("{}", render_setup(&outcome));
            0
        }
        Err(e) => {
            eprintln!("sync: {e}");
            1
        }
    }
}

/// Pure so a test can assert on it without capturing stdout.
///
/// Neither secret is representable here: `SetupOutcome` carries the token's
/// *source* and no passphrase at all (T-3-36).
fn render_setup(outcome: &github::setup::SetupOutcome) -> String {
    let mut out = format!(
        "\nrepo:       {}\nvisibility: {}\ntoken:      present ({}), saved to the {}\n",
        outcome.repo,
        outcome.visibility,
        outcome.token_source.label(),
        outcome.stored_at.label(),
    );
    out.push_str(&format!(
        "categories: {}\n",
        outcome
            .categories
            .iter()
            .map(|c| c.label())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push_str(&format!(
        "keyfile:    {}  (local only — keep the password, there is no recovery)\n",
        outcome.keyfile.display()
    ));
    out.push_str(&format!(
        "first push: {} of {} in {} files\n",
        report::human_bytes(outcome.would_send),
        report::human_bytes(outcome.raw_bytes),
        outcome.files,
    ));
    for warning in &outcome.warnings {
        out.push_str(&format!("warning:    {warning}\n"));
    }
    if outcome.reused_pairing {
        out.push_str("pairing:    reused — this machine was already paired.\n");
    }
    out.push_str(
        "\nThis machine is paired and ready to push.\n\
         Nothing was uploaded — `sync setup` never uploads, and the push transport is not \
         in this build yet.\n",
    );
    out
}

/// The would-upload half, or the reason there is none.
///
/// Never fatal: the file counts and raw bytes need no key at all, and a user
/// checking what is in scope should not have to authenticate (SCOPE-04).
fn try_plan(
    roots: &SyncRoots,
    config: &Config,
    index: Option<&Index>,
    now: DateTime<Utc>,
) -> (Option<plan::SyncPlan>, Option<String>) {
    let Some(index) = index else {
        return (None, Some("the local index could not be opened".into()));
    };
    let keys = match keys_at(&keyfile_path(roots)) {
        Ok(k) => k,
        Err(why) => return (None, Some(why)),
    };
    match plan::build_with_keys(roots, &config.sync, index, now, &keys) {
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
pub(crate) fn keyfile_path(roots: &SyncRoots) -> PathBuf {
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
    use crate::sync::github::setup::{Double, Script};
    use std::fs;
    use tempfile::TempDir;

    const TOKEN: &str = "github_pat_fixture_not_a_real_token";
    const PRIVATE_BODY: &str = r#"{"id":1,"private":true,"visibility":"private",
        "owner":{"login":"o","id":7},"archived":false,"fork":false}"#;

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

    /// Injected end to end. Nothing here reads a real `$HOME`, which is what
    /// keeps the AUR `check()` from failing on an installer's machine.
    fn drive(action: &SyncAction, cfg: &Config, dir: &TempDir, base: &str) -> i32 {
        run_with(
            action,
            cfg,
            &roots_at(dir),
            &Endpoints {
                api_base: base.into(),
                uploads_base: base.into(),
            },
            &TokenChain {
                env_value: Some(TOKEN.into()),
                ..TokenChain::default()
            },
            DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        )
    }

    const NOW: DateTime<Utc> = match DateTime::from_timestamp(1_700_000_000, 0) {
        Some(t) => t,
        None => panic!("a fixed timestamp"),
    };

    /// The repository section alone, injected end to end.
    fn section_for(cfg: &Config, dir: &TempDir, base: &str) -> RepoSection {
        resolve_repo_section(
            cfg,
            &roots_at(dir),
            &Endpoints {
                api_base: base.into(),
                uploads_base: base.into(),
            },
            &TokenChain {
                env_value: Some(TOKEN.into()),
                ..TokenChain::default()
            },
            NOW,
        )
    }

    fn cfg_with_repo(repo: Option<&str>) -> Config {
        Config {
            sync: crate::config::SyncConfig {
                repo: repo.map(str::to_owned),
                ..Default::default()
            },
            ..Config::default()
        }
    }

    #[test]
    fn a_push_without_dry_run_refuses_non_zero_and_points_at_the_dry_run() {
        let dir = TempDir::new().unwrap();
        assert_ne!(
            drive(
                &SyncAction::Push { dry_run: false },
                &cfg_with_repo(None),
                &dir,
                "http://127.0.0.1:1",
            ),
            0
        );
    }

    /// D-01: a missing `[sync] repo` is a non-zero exit that names the fix, and
    /// no request is made at all — the endpoint here is a dead port. Safe to
    /// drive through `run_with`, which builds a real `TtyPrompt`, precisely
    /// because the refusal happens before any prompt method is reached.
    #[test]
    fn setup_without_a_configured_repo_exits_non_zero() {
        let dir = TempDir::new().unwrap();
        assert_ne!(
            drive(
                &SyncAction::Setup,
                &cfg_with_repo(None),
                &dir,
                "http://127.0.0.1:1"
            ),
            0
        );
    }

    /// T-3-01 and T-3-36. `render_setup` is pure precisely so this can be
    /// asserted rather than reasoned about.
    ///
    /// Drives `github::setup::run` with the scripted double rather than
    /// `run_with`'s `Setup` arm: that arm constructs a `TtyPrompt`, and a test
    /// that let it read stdin would also pay the shipped 1 GiB KDF.
    #[test]
    fn the_success_line_reports_the_token_source_and_never_the_token() {
        let dir = TempDir::new().unwrap();
        let mut server = mockito::Server::new();
        let _m = server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(PRIVATE_BODY)
            .create();

        let script = Script::new();
        script
            .borrow_mut()
            .passphrases
            .push("a-supplied-passphrase-long-enough".into());
        let rt = runtime().unwrap();
        let outcome = rt
            .block_on(github::setup::run(
                &cfg_with_repo(Some("o/n")).sync,
                &roots_at(&dir),
                &Endpoints {
                    api_base: server.url(),
                    uploads_base: server.url(),
                },
                &TokenChain {
                    env_value: Some(TOKEN.into()),
                    ..TokenChain::default()
                },
                &mut Double(std::rc::Rc::clone(&script)),
                DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            ))
            .unwrap();
        let rendered = render_setup(&outcome);

        assert!(rendered.contains("o/n"), "{rendered}");
        assert!(rendered.contains("private"), "{rendered}");
        assert!(rendered.contains("token:      present (env)"), "{rendered}");
        assert!(rendered.contains("ready to push"), "{rendered}");
        assert!(
            rendered.contains("Nothing was uploaded"),
            "D-05: {rendered}"
        );
        assert!(!rendered.contains(TOKEN), "{rendered}");
        assert!(!rendered.contains(&TOKEN[..8]), "{rendered}");
        assert!(
            !rendered.contains("a-supplied-passphrase-long-enough"),
            "{rendered}"
        );
        assert!(!rendered.contains("a-suppli"), "{rendered}");
    }

    // ---- 3-07: `sync status` learns about the repository -----------------

    /// The `[sync] repo` key is named, and Phase 2's category listing is still
    /// there. No request is made at all — the endpoint is a dead port.
    #[test]
    fn status_without_a_configured_repo_names_the_key_and_still_lists_categories() {
        let dir = TempDir::new().unwrap();
        let cfg = cfg_with_repo(None);
        let roots = roots_at(&dir);
        let repo = resolve_repo_section(
            &cfg,
            &roots,
            &Endpoints {
                api_base: "http://127.0.0.1:1".into(),
                uploads_base: "http://127.0.0.1:1".into(),
            },
            &TokenChain::default(),
            NOW,
        );
        assert!(repo.failure.is_none(), "unconfigured is not a failure");

        let text = report::render_status(&report::build_status(
            &roots,
            &cfg.sync,
            None,
            NOW,
            None,
            Some(repo),
        ));
        assert!(text.contains("not configured"), "{text}");
        assert!(text.contains("[sync]"), "{text}");
        for category in SyncCategory::ALL {
            assert!(text.contains(category.label()), "{text}");
        }
        assert_eq!(
            drive(&SyncAction::Status, &cfg, &dir, "http://127.0.0.1:1"),
            0,
            "an unconfigured machine is not a failure"
        );
    }

    /// The four repository facts, and exactly one request for them.
    #[test]
    fn status_reports_the_repository_visibility_token_source_and_last_verified() {
        let dir = TempDir::new().unwrap();
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(PRIVATE_BODY)
            .expect(1)
            .create();

        let roots = roots_at(&dir);
        pairing::write_to(
            &pairing::default_path(&roots),
            &pairing::Pairing {
                repo_id: 1,
                owner_id: 7,
                private: true,
                checked_at: NOW,
            },
        )
        .unwrap();

        let code = drive(
            &SyncAction::Status,
            &cfg_with_repo(Some("o/n")),
            &dir,
            &server.url(),
        );
        assert_eq!(code, 0);
        // One invocation, one repository request — the gate is not run twice.
        m.assert();

        let repo = section_for(&cfg_with_repo(Some("o/n")), &dir, &server.url());
        let text = report::render_status(&report::build_status(
            &roots,
            &cfg_with_repo(Some("o/n")).sync,
            None,
            NOW,
            None,
            Some(repo),
        ));
        assert!(text.contains("repo:      o/n"), "{text}");
        assert!(text.contains("visible:   private"), "{text}");
        assert!(text.contains("token:     present (env)"), "{text}");
        assert!(text.contains(&NOW.to_rfc3339()), "{text}");
        assert!(!text.contains(TOKEN), "{text}");
        assert!(!text.contains(&TOKEN[..8]), "{text}");
    }

    /// SAFE-02: a repository that has turned public since pairing is the
    /// incident, verbatim — not a generic error, and not a paraphrase.
    #[test]
    fn a_repository_that_turned_public_is_reported_as_the_incident_and_exits_non_zero() {
        let dir = TempDir::new().unwrap();
        let mut server = mockito::Server::new();
        let _m = server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(
                r#"{"id":1,"private":false,"visibility":"public",
                    "owner":{"login":"o","id":7},"archived":false,"fork":false}"#,
            )
            .create();

        let roots = roots_at(&dir);
        pairing::write_to(
            &pairing::default_path(&roots),
            &pairing::Pairing {
                repo_id: 1,
                owner_id: 7,
                private: true,
                checked_at: NOW,
            },
        )
        .unwrap();

        let cfg = cfg_with_repo(Some("o/n"));
        let repo = section_for(&cfg, &dir, &server.url());
        let failure = repo.failure.clone().expect("the incident");
        assert!(
            failure.starts_with("STOP — the backup repository was private"),
            "{failure}"
        );
        assert!(
            failure.contains("Rotating is the only thing that undoes this"),
            "{failure}"
        );

        // …and the category listing survives it, while the exit code does not.
        let text = report::render_status(&report::build_status(
            &roots,
            &cfg.sync,
            None,
            NOW,
            None,
            Some(repo),
        ));
        assert!(
            text.contains("config"),
            "the categories are still there: {text}"
        );
        assert_ne!(drive(&SyncAction::Status, &cfg, &dir, &server.url()), 0);
    }

    /// REPO-04: no token is a non-zero exit that names every place one is
    /// looked for — and the category listing is still printed.
    #[test]
    fn status_without_a_token_names_every_source_and_exits_non_zero() {
        let dir = TempDir::new().unwrap();
        let cfg = cfg_with_repo(Some("o/n"));
        let roots = roots_at(&dir);
        let repo = resolve_repo_section(
            &cfg,
            &roots,
            &Endpoints {
                api_base: "http://127.0.0.1:1".into(),
                uploads_base: "http://127.0.0.1:1".into(),
            },
            // Empty: nothing can answer.
            &TokenChain::default(),
            NOW,
        );
        let failure = repo.failure.clone().expect("no token resolved");
        assert!(failure.contains("AI_USAGEBAR_SYNC_TOKEN"), "{failure}");
        assert!(failure.contains("sync-token"), "{failure}");
        assert!(failure.contains("gh auth"), "{failure}");

        let text = report::render_status(&report::build_status(
            &roots,
            &cfg.sync,
            None,
            NOW,
            None,
            Some(repo),
        ));
        assert!(text.contains("token:     none found"), "{text}");
        assert!(text.contains("credentials"), "the listing survives: {text}");
        assert_ne!(
            run_with(
                &SyncAction::Status,
                &cfg,
                &roots,
                &Endpoints {
                    api_base: "http://127.0.0.1:1".into(),
                    uploads_base: "http://127.0.0.1:1".into(),
                },
                &TokenChain::default(),
                NOW,
            ),
            0
        );
    }

    /// A network failure fills the repository section with the reason and
    /// exits non-zero, without hiding what would be sent.
    #[test]
    fn an_unreachable_github_still_prints_the_listing_and_exits_non_zero() {
        let dir = TempDir::new().unwrap();
        let cfg = cfg_with_repo(Some("o/n"));
        assert_ne!(
            drive(&SyncAction::Status, &cfg, &dir, "http://127.0.0.1:1"),
            0
        );
        let repo = section_for(&cfg, &dir, "http://127.0.0.1:1");
        let failure = repo.failure.clone().expect("a dead port");
        assert!(failure.contains("Could not reach GitHub"), "{failure}");
        assert!(
            failure.contains("Nothing was uploaded"),
            "D-06 names the fix: {failure}"
        );
        assert_eq!(
            repo.token_source,
            Some("env"),
            "resolved before the request"
        );
    }
}
