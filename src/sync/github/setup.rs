//! `ai-usagebar sync setup` — pair this machine with the private repository
//! named in `[sync] repo`, in the five steps UX-03 asks for.
//!
//! **Uploads nothing** (D-05). The flow is: the repository and the gate, then
//! the passphrase, then the categories, then the size, then "ready to push".
//!
//! **The ordering is the substance.** Step 1 refuses before [`SetupPrompt`] is
//! touched at all, because asking someone to choose a passphrase for a
//! repository that is about to be refused wastes their time and teaches them
//! the refusal is negotiable. Every test drives a scripted prompt double that
//! records which methods were reached, so that ordering is asserted rather than
//! reviewed.
//!
//! `roots` is the **only** way this module reaches a filesystem path: the
//! keyfile, the pairing record, the token file, the index, and the
//! `config.toml` write-back all resolve from it, so no path here is ever
//! derived from a real `$HOME` — which the AUR `check()` would run against on
//! an installer's machine.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use toml_edit::DocumentMut;
use zeroize::Zeroizing;

use crate::config::{SyncCategory, SyncConfig};
use crate::error::{AppError, Result};
use crate::sync::crypto::{KdfParams, Keyfile};
use crate::sync::index::Index;
use crate::sync::passphrase::{self, Strength};
use crate::sync::report::{self, DryRunReport};
use crate::sync::{SyncRoots, plan};

use super::gate::{self, PushClearance};
use super::pairing;
use super::token::{self, TokenSource};
use super::{Client, Endpoints, RepoRef};

/// Everything the guided flow needs from outside itself.
///
/// One trait rather than direct `stdin`/`stdout` calls, so the *order* the
/// steps run in is observable: every test passes a double that records which
/// methods were reached, and a refusal that reaches [`SetupPrompt::passphrase`]
/// fails the test rather than the review.
///
/// **Nothing outside [`TtyPrompt`] may read stdin.**
pub trait SetupPrompt {
    /// Narrate. Everything the user reads goes through here.
    fn say(&mut self, line: &str);

    /// A yes/no with a default — step 4's size confirmation, and anything else
    /// that needs one.
    fn confirm(&mut self, question: &str, default_yes: bool) -> Result<bool>;

    /// Step 2. `generated` has already been displayed with Phase 1's
    /// no-recovery warning; the implementation either accepts it or supplies
    /// its own. Called again when the strength floor refuses a supplied one.
    fn passphrase(&mut self, generated: &str) -> Result<Zeroizing<String>>;

    /// Step 3. Returns the categories to keep, in any order.
    fn categories(&mut self, current: &[SyncCategory]) -> Result<Vec<SyncCategory>>;

    /// The KDF cost a new keyfile is written at.
    ///
    /// A seam, not a question: the shipped default is 1 GiB and takes about a
    /// second and a half, which every test that reaches step 2 would otherwise
    /// pay — and the AUR `check()` runs those tests during `makepkg`. Tests
    /// override it with [`crate::sync::crypto::MIN_KDF_MEMORY_KIB`].
    fn kdf(&self) -> KdfParams {
        KdfParams::default()
    }

    /// Step 5's persistence. Also a seam, and a mandatory one: on macOS
    /// [`token::store`] writes the **real** login Keychain item, so a test that
    /// reached step 5 through the production call would clobber the user's own
    /// sync token — and the AUR `check()` runs these tests on installers'
    /// machines. The default *is* production; the double overrides it.
    fn store_token(&self, token: &str, file: &Path) -> Result<TokenSource> {
        token::store(token, file)
    }

    /// The 401 path, for the same reason: [`token::clear`] deletes the real
    /// Keychain item on macOS.
    fn clear_token(&self, file: &Path) -> Result<()> {
        token::clear(file)
    }
}

/// The production implementation. Untested by construction — it is the only
/// thing in this file that reads stdin.
#[derive(Debug, Default)]
pub struct TtyPrompt;

impl TtyPrompt {
    fn line(&self) -> Result<String> {
        let mut buf = String::new();
        std::io::stdin()
            .read_line(&mut buf)
            .map_err(|e| AppError::Other(format!("could not read your answer: {e}")))?;
        Ok(buf.trim().to_owned())
    }
}

impl SetupPrompt for TtyPrompt {
    fn say(&mut self, line: &str) {
        println!("{line}");
    }

    fn confirm(&mut self, question: &str, default_yes: bool) -> Result<bool> {
        let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
        println!("{question} {hint}");
        let answer = self.line()?;
        Ok(match answer.to_ascii_lowercase().as_str() {
            "" => default_yes,
            "y" | "yes" => true,
            _ => false,
        })
    }

    fn passphrase(&mut self, _generated: &str) -> Result<Zeroizing<String>> {
        println!(
            "Press Enter to take the generated passphrase, or type your own now.\n\
             A typed passphrase is echoed — this build has no hidden-input dependency, \
             which is one more reason to take the generated one."
        );
        // Never argv, never an environment variable (T-3-37, Phase 1's rule).
        let typed = passphrase::read_line(std::io::stdin().lock())?;
        Ok(typed)
    }

    fn categories(&mut self, current: &[SyncCategory]) -> Result<Vec<SyncCategory>> {
        let mut chosen = current.to_vec();
        for category in SyncCategory::ALL {
            let on = chosen.contains(&category);
            let keep = self.confirm(&format!("  include `{}`?", category.label()), on)?;
            match (keep, on) {
                (true, false) => chosen.push(category),
                (false, true) => chosen.retain(|c| *c != category),
                _ => {}
            }
        }
        // Canonical D1 order, whatever order the answers arrived in.
        Ok(SyncCategory::ALL
            .into_iter()
            .filter(|c| chosen.contains(c))
            .collect())
    }
}

/// What setup learned. Carries the [`PushClearance`] rather than a `bool`,
/// because a `bool` is a cached check and D-04 forbids one.
///
/// Nothing here is a secret: the token is reported as a *source*, the
/// passphrase is not represented at all, and no keyfile byte is carried.
#[derive(Debug)]
pub struct SetupOutcome {
    pub repo: RepoRef,
    /// Where the token was *resolved* from; the value is never rendered.
    pub token_source: TokenSource,
    /// Where it was *stored* for next time — the Keychain on macOS, the
    /// mode-0600 file elsewhere.
    pub stored_at: TokenSource,
    pub visibility: String,
    pub warnings: Vec<String>,
    pub clearance: PushClearance,
    pub categories: Vec<SyncCategory>,
    /// The local keyfile. Still local — Phase 4 uploads it.
    pub keyfile: PathBuf,
    /// True when an existing pairing record was reused rather than issued.
    pub reused_pairing: bool,
    /// Phase 2's own figures for the chosen categories, not a second estimate.
    pub files: usize,
    pub raw_bytes: u64,
    pub would_send: u64,
}

/// The five steps, in order.
pub async fn run(
    cfg: &SyncConfig,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &token::TokenChain,
    prompt: &mut dyn SetupPrompt,
    now: DateTime<Utc>,
) -> Result<SetupOutcome> {
    // ---- Step 1: the repository, and the gate ---------------------------
    // Nothing below this block is reached by a refusal, and no prompt method is
    // called inside it (T-3-35).
    let Some(configured) = cfg.repo.as_deref() else {
        return Err(AppError::Other(no_repo_message()));
    };
    let repo = RepoRef::parse(configured)?;

    // Computed **once** and passed to both gate calls. Derived twice, plan
    // 3-04's credentials-off carve-out dies: `check_drift` carves it out and
    // `assert_pushable` would then overrule it.
    let credentials_in_bundle = cfg.includes(SyncCategory::Credentials);

    let token_file = token_path(roots);
    let (value, source) = token::resolve(chain)?;
    // Kept so step 5 can persist it. `Zeroizing`, so the copy dies with it.
    let keep = value.clone();
    let client = Client::new(endpoints, value, source)?;

    let facts = match gate::fetch_facts(&client, &repo, now).await {
        Ok(facts) => facts,
        Err(e) => return Err(clear_if_dead(e, &token_file, &|p| prompt.clear_token(p))),
    };

    let pairing_file = pairing::default_path(roots);
    let record = pairing::read_from(&pairing_file)?;
    // check_drift first, then assert_pushable — that order, always.
    let drift = pairing::check_drift(record.as_ref(), &facts, credentials_in_bundle, now)?;
    let (clearance, gate_warnings) =
        gate::assert_pushable(&facts, &repo, credentials_in_bundle, now)?;

    let mut warnings = drift.warnings.clone();
    warnings.extend(gate_warnings);

    prompt.say(&format!(
        "1/5  {repo} is {} — the gate passed.",
        facts.visibility
    ));
    for warning in &warnings {
        prompt.say(&format!("     warning: {warning}"));
    }
    if !drift.first_contact {
        prompt.say(
            "     this machine is already paired with it; reusing that pairing rather than \
             issuing a second one.",
        );
    }

    // ---- Step 2: the passphrase, and the keyfile ------------------------
    let keyfile_path = crate::sync::cli::keyfile_path(roots);
    if keyfile_path.exists() {
        return Err(AppError::Other(existing_keyfile_message(&keyfile_path)));
    }

    let kdf = prompt.kdf();
    let generated = passphrase::generate()?;
    prompt.say("\n2/5  A sync password protects the bundle.");
    prompt.say(passphrase::NO_RECOVERY);
    prompt.say(passphrase::OFFLINE_ATTACK_NOTE);
    // Shown exactly once — not re-displayed on a re-prompt below.
    prompt.say(&format!("\n     generated passphrase:  {}\n", &*generated));

    let chosen_pw = loop {
        let candidate = prompt.passphrase(&generated)?;
        let candidate = if candidate.is_empty() {
            generated.clone()
        } else {
            candidate
        };
        match passphrase::check(&candidate, kdf) {
            Strength::Rejected(why) => prompt.say(&format!("     refused: {why}")),
            Strength::Weak(why) => {
                prompt.say(&format!("     {why}"));
                break candidate;
            }
            Strength::Strong => break candidate,
        }
    };

    let (keyfile, keys) = Keyfile::create(chosen_pw.as_bytes(), kdf)?;
    write_keyfile(&keyfile_path, &keyfile)?;
    prompt.say(&format!("     keyfile written: {}", keyfile_path.display()));

    // ---- Step 3: the categories -----------------------------------------
    prompt.say("\n3/5  What gets bundled. `credentials` is the deliberate one — turning it on syncs saved logins.");
    let categories = prompt.categories(&cfg.categories)?;
    let chosen_cfg = SyncConfig {
        categories: categories.clone(),
        ..cfg.clone()
    };
    if categories != cfg.categories {
        write_categories(&roots.config_file, &categories)?;
        prompt.say(&format!("     saved to {}", roots.config_file.display()));
    }

    // ---- Step 4: the size ------------------------------------------------
    let index = Index::at(&roots.index_file)?;
    let sync_plan = plan::build_with_keys(roots, &chosen_cfg, &index, now, &keys)?;
    let (files, raw_bytes, would_send) = (
        sync_plan
            .categories
            .iter()
            .filter(|c| chosen_cfg.includes(c.category))
            .map(|c| c.files)
            .sum(),
        sync_plan
            .categories
            .iter()
            .filter(|c| chosen_cfg.includes(c.category))
            .map(|c| c.raw_bytes)
            .sum(),
        sync_plan.total_new_stored_bytes,
    );

    prompt.say("\n4/5  What a first push would send:");
    // The dry-run's own renderer over the dry-run's own plan — so the number
    // here and the number `sync push --dry-run` prints cannot disagree.
    prompt.say(&report::render_dry_run(&DryRunReport {
        status: report::build_status(roots, &chosen_cfg, Some(&index), now, Some(sync_plan), None),
        no_key: None,
    }));
    if !prompt.confirm("     Pair this machine with that scope?", true)? {
        return Err(AppError::Other(
            "setup stopped at the size confirmation. Nothing was uploaded — this command \
             never uploads — and the repository was not touched."
                .into(),
        ));
    }

    // ---- Step 5: ready ---------------------------------------------------
    let stored_at = prompt.store_token(&keep, &token_file).map_err(|e| {
        AppError::Other(format!(
            "could not save the GitHub sync token: {e}\n\
             The secret being saved here is the *sync token*, not a Claude credential — \
             the wording above comes from the shared Keychain helper."
        ))
    })?;
    pairing::write_to(&pairing_file, &drift.record)?;

    Ok(SetupOutcome {
        repo,
        token_source: source,
        stored_at,
        visibility: facts.visibility,
        warnings,
        clearance,
        categories,
        keyfile: keyfile_path,
        reused_pairing: !drift.first_contact,
        files,
        raw_bytes,
        would_send,
    })
}

/// `<config_dir>/sync-token` — the same file [`token::TokenChain::production`]
/// computes from the resolved config path, but taken from the injected roots
/// so nothing here reads a real `$HOME`.
pub(crate) fn token_path(roots: &SyncRoots) -> PathBuf {
    roots.config_dir.join("sync-token")
}

/// Keep the promise `http::actionable`'s 401 arm makes.
///
/// That message tells the user the stored token "will be cleared". This is the
/// only call site that does it — without this the sentence is a lie, and a user
/// who re-authenticates believing they start clean walks into the same failure.
///
/// `clear` is a parameter because [`token::clear`] deletes the **real** login
/// Keychain item on macOS, which no unit test may touch. Production passes
/// `token::clear`; the test passes a recorder, which is what makes the pairing
/// assertable rather than reviewable.
///
/// A failure to clear is deliberately swallowed: the original 401 is the thing
/// the user needs to read, and burying it under a filesystem error would be
/// worse than a token file that outlived its usefulness.
pub(crate) fn clear_if_dead(
    err: AppError,
    token_file: &Path,
    clear: &dyn Fn(&Path) -> Result<()>,
) -> AppError {
    if matches!(err, AppError::Credentials(_)) {
        let _ = clear(token_file);
    }
    err
}

/// D-01: nothing is guessed, and the failure names the exact fix.
fn no_repo_message() -> String {
    "[sync] repo is unset, and ai-usagebar never guesses or creates one.\n\
     Create a private repository, then name it in config.toml:\n\
     \x20   gh repo create <owner>/<name> --private\n\
     \n\
     \x20   [sync]\n\
     \x20   repo = \"<owner>/<name>\""
        .to_owned()
}

/// T-3-38. Overwriting a keyfile makes every bundle written under the old one
/// permanently unreadable, and there is no recovery by design.
fn existing_keyfile_message(path: &Path) -> String {
    format!(
        "a sync keyfile already exists at {} and setup will not overwrite it.\n\
         Overwriting it would make every bundle written under the old password permanently \
         unreadable — there is no recovery, by design.\n\
         This machine is already set up. To start over with a new password you must delete \
         that file yourself, knowing the existing bundle becomes unreadable.",
        path.display()
    )
}

/// Atomically, then mode 0600 — the same belt-and-braces `anchor::write_to` and
/// the Settings overlay apply. The temp file lands in the destination's own
/// directory, never `/tmp`.
fn write_keyfile(path: &Path, keyfile: &Keyfile) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(keyfile)?;
    crate::cache::atomic_write(path, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| AppError::io_at(path, e))?;
    }
    Ok(())
}

/// Write `categories` back into `[sync]`, preserving comments and key order the
/// way `tui::settings::save_to_path` already does (T-3-39).
fn write_categories(config_file: &Path, categories: &[SyncCategory]) -> Result<()> {
    let original = match std::fs::read_to_string(config_file) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(AppError::io_at(config_file, e)),
    };
    let mut doc: DocumentMut = if original.trim().is_empty() {
        DocumentMut::new()
    } else {
        original.parse().map_err(|e: toml_edit::TomlError| {
            AppError::Other(format!("config.toml not parseable: {e}"))
        })?
    };

    let mut array = toml_edit::Array::new();
    for category in categories {
        array.push(category.label());
    }
    doc.entry("sync")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or_else(|| AppError::Other("config.toml has a non-table [sync]".into()))?["categories"] =
        toml_edit::value(array);

    crate::cache::atomic_write(config_file, doc.to_string().as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(config_file) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(config_file, perms);
        }
    }
    Ok(())
}

/// The scripted [`SetupPrompt`] every test in this crate drives.
///
/// `reached` is the whole point: it records which prompt methods ran, in order,
/// so "the gate refused before the password step" is an assertion rather than a
/// claim. Lives at module scope (rather than inside `mod tests`) because
/// `sync::cli`'s tests drive the same flow — the same reuse `cache::temp_file`
/// already makes.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Script {
    pub reached: Vec<String>,
    pub said: Vec<String>,
    /// Answers for `passphrase`, in order. Exhausted ⇒ the generated one.
    pub passphrases: Vec<String>,
    /// `None` keeps whatever the config already had.
    pub categories: Option<Vec<SyncCategory>>,
    pub confirm: bool,
    /// Token-file paths the flow asked to store / clear. Recorded rather than
    /// acted on — no test may reach a real Keychain.
    pub stored: Vec<PathBuf>,
    pub cleared: Vec<PathBuf>,
}

#[cfg(test)]
impl Script {
    pub fn new() -> std::rc::Rc<std::cell::RefCell<Script>> {
        std::rc::Rc::new(std::cell::RefCell::new(Script {
            confirm: true,
            ..Script::default()
        }))
    }
}

#[cfg(test)]
pub(crate) struct Double(pub std::rc::Rc<std::cell::RefCell<Script>>);

#[cfg(test)]
impl SetupPrompt for Double {
    fn say(&mut self, line: &str) {
        self.0.borrow_mut().said.push(line.to_owned());
    }
    fn confirm(&mut self, question: &str, _default_yes: bool) -> Result<bool> {
        let mut s = self.0.borrow_mut();
        s.reached.push(format!("confirm:{question}"));
        Ok(s.confirm)
    }
    fn passphrase(&mut self, _generated: &str) -> Result<Zeroizing<String>> {
        let mut s = self.0.borrow_mut();
        s.reached.push("passphrase".into());
        let next = if s.passphrases.is_empty() {
            String::new()
        } else {
            s.passphrases.remove(0)
        };
        Ok(Zeroizing::new(next))
    }
    fn categories(&mut self, current: &[SyncCategory]) -> Result<Vec<SyncCategory>> {
        let mut s = self.0.borrow_mut();
        s.reached.push("categories".into());
        Ok(s.categories.clone().unwrap_or_else(|| current.to_vec()))
    }
    /// 8 MiB rather than a gibibyte: the AUR `check()` runs these.
    fn kdf(&self) -> KdfParams {
        KdfParams {
            m_kib: crate::sync::crypto::MIN_KDF_MEMORY_KIB,
            t: 1,
            p: 1,
        }
    }
    /// Recorded, never performed. The production default would write the real
    /// macOS login Keychain.
    fn store_token(&self, _token: &str, file: &Path) -> Result<TokenSource> {
        self.0.borrow_mut().stored.push(file.to_path_buf());
        Ok(TokenSource::File)
    }
    fn clear_token(&self, file: &Path) -> Result<()> {
        self.0.borrow_mut().cleared.push(file.to_path_buf());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::cell::RefCell;
    use std::fs;
    use std::rc::Rc;
    use tempfile::TempDir;

    const FIXTURE: &str = "github_pat_fixture_not_a_real_token";
    const PRIVATE: &str = r#"{"id":1,"private":true,"visibility":"private",
        "owner":{"login":"o","id":7},"archived":false,"fork":false}"#;
    const PUBLIC: &str = r#"{"id":1,"private":false,"visibility":"public",
        "owner":{"login":"o","id":7},"archived":false,"fork":false}"#;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn roots_at(dir: &TempDir) -> SyncRoots {
        SyncRoots::at(
            dir.path().join("config.toml"),
            dir.path().to_path_buf(),
            dir.path().join("desktop"),
            dir.path().join("profiles"),
            dir.path().join("claude-home"),
        )
    }

    fn chain() -> token::TokenChain {
        token::TokenChain {
            env_value: Some(FIXTURE.into()),
            ..token::TokenChain::default()
        }
    }

    fn endpoints_at(base: &str) -> Endpoints {
        Endpoints {
            api_base: base.into(),
            uploads_base: base.into(),
        }
    }

    fn cfg_for(repo: Option<&str>) -> SyncConfig {
        SyncConfig {
            repo: repo.map(str::to_owned),
            ..SyncConfig::default()
        }
    }

    /// One private-repo mock, one scripted run.
    async fn drive(
        cfg: &SyncConfig,
        dir: &TempDir,
        body: &str,
        status: usize,
        script: &Rc<RefCell<Script>>,
    ) -> Result<SetupOutcome> {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n")
            .with_status(status)
            .with_body(body)
            .create_async()
            .await;
        run(
            cfg,
            &roots_at(dir),
            &endpoints_at(&server.url()),
            &chain(),
            &mut Double(Rc::clone(script)),
            now(),
        )
        .await
    }

    // ---- the happy path --------------------------------------------------

    #[tokio::test]
    async fn the_five_steps_run_in_order_and_end_at_ready_to_push() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("config.toml"), "[sync]\n").unwrap();
        let script = Script::new();

        let out = drive(&cfg_for(Some("o/n")), &dir, PRIVATE, 200, &script)
            .await
            .unwrap();

        let reached = script.borrow().reached.clone();
        assert_eq!(reached[0], "passphrase", "{reached:?}");
        assert_eq!(reached[1], "categories", "{reached:?}");
        assert!(reached[2].starts_with("confirm:"), "{reached:?}");
        assert_eq!(reached.len(), 3, "{reached:?}");

        assert_eq!(out.visibility, "private");
        assert_eq!(out.token_source, TokenSource::Env);
        assert!(!out.reused_pairing);
        assert!(out.keyfile.starts_with(dir.path()));
        assert!(out.keyfile.exists());
        // Phase 2's own number, and the same number the user was shown: step 4
        // renders the dry-run's own report over the dry-run's own plan, so the
        // figure here and the figure `sync push --dry-run` prints cannot
        // disagree.
        assert!(out.files >= 1, "{out:?}");
        assert!(out.would_send > 0, "{out:?}");
        let said = script.borrow().said.join("\n");
        assert!(
            said.contains(&crate::sync::report::human_bytes(out.would_send)),
            "the confirmed size is the plan builder's own total: {said}"
        );
        assert!(said.contains("uploads nothing"), "{said}");
        // The token never reaches the narration either, not even a prefix.
        assert!(!said.contains(FIXTURE), "{said}");
        assert!(!said.contains(&FIXTURE[..8]), "{said}");
    }

    /// T-3-36. Neither secret, nor an eight-character prefix of either, reaches
    /// the value the CLI renders.
    #[tokio::test]
    async fn the_outcome_carries_neither_the_token_nor_the_passphrase() {
        let dir = TempDir::new().unwrap();
        let script = Script::new();
        script
            .borrow_mut()
            .passphrases
            .push("a-supplied-passphrase-long-enough".into());

        let out = drive(&cfg_for(Some("o/n")), &dir, PRIVATE, 200, &script)
            .await
            .unwrap();

        let rendered = format!("{out:?}");
        assert!(!rendered.contains(FIXTURE), "{rendered}");
        assert!(!rendered.contains(&FIXTURE[..8]), "{rendered}");
        assert!(!rendered.contains("a-supplied-passphrase-long-enough"));
        assert!(!rendered.contains("a-suppli"), "{rendered}");
        // …and no keyfile byte either.
        let keyfile = fs::read_to_string(&out.keyfile).unwrap();
        let wrapped: serde_json::Value = serde_json::from_str(&keyfile).unwrap();
        let wrapped = wrapped["wrapped_master_key"].as_str().unwrap();
        assert!(!rendered.contains(wrapped), "{rendered}");
    }

    /// T-3-35, the whole ordering claim: a refusal never reaches step 2.
    #[tokio::test]
    async fn every_refusal_stops_before_the_password_step_is_reached() {
        for (body, status) in [(PUBLIC, 200), ("{}", 404), ("{}", 401)] {
            let dir = TempDir::new().unwrap();
            let script = Script::new();
            let err = drive(&cfg_for(Some("o/n")), &dir, body, status, &script)
                .await
                .expect_err("this repository is not pairable");
            assert!(
                script.borrow().reached.is_empty(),
                "status {status} reached {:?}",
                script.borrow().reached
            );
            assert!(!crate::sync::cli::keyfile_path(&roots_at(&dir)).exists());
            let _ = err;
        }
    }

    /// D-01, and the 404 arm both carry the create command; neither offers to
    /// create anything.
    #[tokio::test]
    async fn a_missing_repository_names_the_create_command_and_never_offers_to_run_it() {
        let dir = TempDir::new().unwrap();
        let script = Script::new();
        let err = drive(&cfg_for(Some("o/n")), &dir, "{}", 404, &script)
            .await
            .expect_err("404");
        let text = err.to_string();
        assert!(text.contains("gh repo create"), "{text}");

        // …and with no repository configured at all, against a dead port.
        let err = run(
            &cfg_for(None),
            &roots_at(&dir),
            &endpoints_at("http://127.0.0.1:1"),
            &chain(),
            &mut Double(Rc::clone(&script)),
            now(),
        )
        .await
        .expect_err("no repository is configured");
        let text = err.to_string();
        assert!(
            text.contains("gh repo create <owner>/<name> --private"),
            "{text}"
        );
        assert!(text.contains("[sync]"), "{text}");
        assert!(script.borrow().reached.is_empty());
    }

    // ---- the 401 promise -------------------------------------------------

    /// The cross-plan promise from 3-CONTEXT: `http::actionable`'s 401 arm says
    /// the stored token "will be cleared", and this is the call site that does
    /// it. The clear is injected because the real one reaches the macOS login
    /// Keychain.
    #[test]
    fn a_401_clears_the_stored_token_and_nothing_else_does() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sync-token");
        let cleared: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
        let record = |p: &Path| {
            cleared.borrow_mut().push(p.to_path_buf());
            Ok(())
        };

        let dead = AppError::Credentials("GitHub rejected the sync token (401)".into());
        let returned = clear_if_dead(dead, &path, &record);
        assert_eq!(cleared.borrow().as_slice(), std::slice::from_ref(&path));
        // The 401 still reaches the user; clearing does not swallow it.
        assert!(returned.to_string().contains("401"), "{returned}");

        for keep in [
            AppError::Http {
                status: 403,
                body: "forbidden".into(),
            },
            AppError::Http {
                status: 404,
                body: "not found".into(),
            },
            AppError::Transport("connection reset".into()),
        ] {
            clear_if_dead(keep, &path, &record);
        }
        assert_eq!(
            cleared.borrow().len(),
            1,
            "only a 401 clears — a 403 must keep a working token"
        );
    }

    /// The promise's other half: the message the user actually reads says the
    /// token will be cleared, so the wiring above is not decoration.
    #[tokio::test]
    async fn the_401_message_promises_the_clear_the_call_site_performs() {
        let dir = TempDir::new().unwrap();
        let script = Script::new();
        let err = drive(&cfg_for(Some("o/n")), &dir, "{}", 401, &script)
            .await
            .expect_err("401");
        assert!(matches!(err, AppError::Credentials(_)), "{err:?}");
        assert!(err.to_string().contains("will be cleared"), "{err}");
        // …and the flow actually cleared it, at the injected token path.
        assert_eq!(
            script.borrow().cleared.as_slice(),
            std::slice::from_ref(&token_path(&roots_at(&dir)))
        );
        assert!(
            script.borrow().stored.is_empty(),
            "a dead token is not stored"
        );
    }

    /// A 403 keeps a working token (T-3-16): the message says so, and the flow
    /// agrees with the message.
    #[tokio::test]
    async fn a_403_does_not_clear_the_token_the_message_told_the_user_to_keep() {
        let dir = TempDir::new().unwrap();
        let script = Script::new();
        let err = drive(&cfg_for(Some("o/n")), &dir, "{}", 403, &script)
            .await
            .expect_err("403");
        assert!(err.to_string().contains("keep it"), "{err}");
        assert!(script.borrow().cleared.is_empty(), "a 403 must not clear");
    }

    // ---- step 2 ----------------------------------------------------------

    #[tokio::test]
    async fn the_keyfile_is_written_owner_only_inside_the_injected_directory() {
        let dir = TempDir::new().unwrap();
        let script = Script::new();
        let out = drive(&cfg_for(Some("o/n")), &dir, PRIVATE, 200, &script)
            .await
            .unwrap();

        assert!(out.keyfile.starts_with(dir.path()), "{:?}", out.keyfile);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&out.keyfile).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // Phase 1's no-recovery warning, shown once alongside the passphrase.
        let said = script.borrow().said.join("\n");
        assert!(said.contains("There is no recovery"), "{said}");
        assert_eq!(
            said.matches("There is no recovery").count(),
            1,
            "shown exactly once"
        );
        assert!(said.contains("guess at it as long as they like"), "{said}");
    }

    #[tokio::test]
    async fn a_passphrase_under_the_floor_is_refused_and_re_prompted_not_accepted() {
        let dir = TempDir::new().unwrap();
        let script = Script::new();
        script.borrow_mut().passphrases = vec![
            "short".into(),
            "also-too-short".into(),
            "a-long-enough-supplied-passphrase".into(),
        ];

        let out = drive(&cfg_for(Some("o/n")), &dir, PRIVATE, 200, &script)
            .await
            .unwrap();
        assert!(out.keyfile.exists());

        let reached = script.borrow().reached.clone();
        assert_eq!(
            reached.iter().filter(|r| *r == "passphrase").count(),
            3,
            "two refusals then an accepted one: {reached:?}"
        );
        let said = script.borrow().said.join("\n");
        assert!(said.contains("refused:"), "{said}");
    }

    /// T-3-38: an unrecoverable overwrite is refused, and refused *before* a
    /// new passphrase is generated.
    #[tokio::test]
    async fn an_existing_keyfile_stops_the_flow_rather_than_being_overwritten() {
        let dir = TempDir::new().unwrap();
        let existing = crate::sync::cli::keyfile_path(&roots_at(&dir));
        fs::create_dir_all(existing.parent().unwrap()).unwrap();
        fs::write(&existing, "{\"already\":\"here\"}").unwrap();

        let script = Script::new();
        let err = drive(&cfg_for(Some("o/n")), &dir, PRIVATE, 200, &script)
            .await
            .expect_err("a keyfile is already there");
        assert!(err.to_string().contains("will not overwrite"), "{err}");
        assert!(script.borrow().reached.is_empty(), "before step 2's prompt");
        assert_eq!(
            fs::read_to_string(&existing).unwrap(),
            "{\"already\":\"here\"}"
        );
    }

    // ---- step 3 ----------------------------------------------------------

    #[tokio::test]
    async fn a_toggled_category_lands_in_the_injected_config_and_reads_back() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("config.toml");
        fs::write(&config, "# keep me\n[sync]\ntranscript_days = 7\n").unwrap();

        let script = Script::new();
        script.borrow_mut().categories = Some(vec![SyncCategory::Config, SyncCategory::Routines]);

        let out = drive(&cfg_for(Some("o/n")), &dir, PRIVATE, 200, &script)
            .await
            .unwrap();
        assert_eq!(
            out.categories,
            vec![SyncCategory::Config, SyncCategory::Routines]
        );

        let written = fs::read_to_string(&config).unwrap();
        assert!(written.contains("# keep me"), "comments survive: {written}");
        assert!(written.contains("transcript_days = 7"), "{written}");

        let reloaded = Config::load_from(&config).unwrap();
        assert_eq!(
            reloaded.sync.categories,
            vec![SyncCategory::Config, SyncCategory::Routines]
        );
        assert!(!reloaded.sync.includes(SyncCategory::Credentials));
    }

    // ---- D-04's carve-out, through both gate calls ------------------------

    /// Credentials **off**: a public repository proceeds, with the warning.
    #[tokio::test]
    async fn a_public_repository_with_credentials_off_proceeds_with_the_warning() {
        let dir = TempDir::new().unwrap();
        let cfg = SyncConfig {
            repo: Some("o/n".into()),
            categories: vec![SyncCategory::Config],
            ..SyncConfig::default()
        };
        let script = Script::new();
        let out = drive(&cfg, &dir, PUBLIC, 200, &script).await.unwrap();

        assert!(!out.warnings.is_empty(), "the public warning");
        assert!(
            out.warnings.iter().any(|w| w.contains("public")),
            "{:?}",
            out.warnings
        );
        assert!(!script.borrow().reached.is_empty(), "the flow continued");
    }

    /// Credentials **on**: the same repository stops at step 1.
    #[tokio::test]
    async fn a_public_repository_with_credentials_on_stops_before_the_passphrase() {
        let dir = TempDir::new().unwrap();
        let script = Script::new();
        let err = drive(&cfg_for(Some("o/n")), &dir, PUBLIC, 200, &script)
            .await
            .expect_err("credentials are in the bundle by default");
        assert!(err.to_string().contains("REFUSING TO PUSH"), "{err}");
        assert!(script.borrow().reached.is_empty());
    }

    // ---- pairing reuse, and the filesystem boundary -----------------------

    #[tokio::test]
    async fn a_second_run_reuses_the_pairing_rather_than_issuing_another() {
        let dir = TempDir::new().unwrap();
        let script = Script::new();
        let first = drive(&cfg_for(Some("o/n")), &dir, PRIVATE, 200, &script)
            .await
            .unwrap();
        assert!(!first.reused_pairing);

        // The keyfile now exists, so a second full run stops there — remove it
        // to exercise the pairing branch on its own.
        fs::remove_file(&first.keyfile).unwrap();
        let again = Script::new();
        let second = drive(&cfg_for(Some("o/n")), &dir, PRIVATE, 200, &again)
            .await
            .unwrap();
        assert!(second.reused_pairing);
        assert!(
            again
                .borrow()
                .said
                .iter()
                .any(|s| s.contains("already paired")),
            "{:?}",
            again.borrow().said
        );
    }

    /// Every path this flow writes resolves from the injected `SyncRoots`.
    #[tokio::test]
    async fn nothing_is_written_outside_the_injected_temp_directory() {
        let dir = TempDir::new().unwrap();
        let roots = roots_at(&dir);
        let script = Script::new();
        let out = drive(&cfg_for(Some("o/n")), &dir, PRIVATE, 200, &script)
            .await
            .unwrap();

        for path in [
            out.keyfile.clone(),
            pairing::default_path(&roots),
            token_path(&roots),
            roots.config_file.clone(),
            roots.index_file.clone(),
        ] {
            assert!(
                path.starts_with(dir.path()),
                "escaped the TempDir: {}",
                path.display()
            );
        }
        assert!(
            pairing::default_path(&roots).exists(),
            "the record persisted"
        );
        assert!(roots.index_file.exists(), "the index is inside too");
        assert_eq!(
            script.borrow().stored.as_slice(),
            std::slice::from_ref(&token_path(&roots)),
            "the token was stored at the injected path and nowhere else"
        );
    }

    /// D-05: declining the size confirmation leaves the remote untouched, and
    /// says so.
    #[tokio::test]
    async fn declining_the_size_confirmation_stops_without_pairing() {
        let dir = TempDir::new().unwrap();
        let script = Script::new();
        script.borrow_mut().confirm = false;

        let err = drive(&cfg_for(Some("o/n")), &dir, PRIVATE, 200, &script)
            .await
            .expect_err("the user declined");
        assert!(err.to_string().contains("Nothing was uploaded"), "{err}");
        assert!(!pairing::default_path(&roots_at(&dir)).exists());
    }
}
