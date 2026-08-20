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
use crate::sync::crypto::{KdfParams, Keyfile, Keys, content_address};
use crate::sync::github::setup::TtyPrompt;
use crate::sync::github::{
    self, Client, Endpoints, RepoRef, gate, pairing, token, token::TokenChain,
};
use crate::sync::index::Index;
use crate::sync::push::progress;
use crate::sync::push::{self, PushCtx, PushOutcome};
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
        SyncAction::Push { dry_run: false } => push(cfg, roots, endpoints, chain, now),
        SyncAction::Prune => prune(cfg, roots, endpoints, chain, now),
        SyncAction::Rekey => rekey(cfg, roots, endpoints, chain, now),
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
        // Same 401 handling the setup flow performs, and `source` is what makes
        // it safe: a 401 on a token resolved from the environment or from `gh`
        // must not delete the Keychain item or the token file, neither of which
        // this run used (F-1).
        //
        // **`token::clear_source` deletes the real macOS login Keychain item**
        // when the source *is* the Keychain. No test here may mock a 401 against
        // this arm; `sync setup`'s version of this call goes through
        // `SetupPrompt::clear_token`, which a test double overrides. If `status`
        // ever needs a 401 test, give it the same seam first — do not reach for
        // the production function.
        Err(e) => {
            let e = github::setup::clear_if_dead(
                e,
                source,
                &github::setup::token_path(roots),
                &token::clear_source,
            );
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
         Nothing was uploaded — `sync setup` never uploads. Run `ai-usagebar sync push` when \
         you are ready.\n",
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
    local_keyfile(path).map(|k| k.keys)
}

/// Everything the push path needs out of the local keyfile, from one read.
pub(crate) struct LocalKeyfile {
    pub keys: Keys,
    /// The parameters *this bundle* lives at, which every new snapshot root
    /// repeats. Never `KdfParams::default` — that is the whole point of the
    /// keyfile storing them.
    pub kdf: KdfParams,
    /// The asset name this keyfile would publish under, content-addressed over
    /// its canonical serialization — the same bytes `rekey` uploads, so a
    /// rewrapped keyfile and this one can never collide.
    pub asset: String,
}

fn local_keyfile(path: &Path) -> std::result::Result<LocalKeyfile, String> {
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
    open_keyfile(keyfile, &pw)
}

/// [`local_keyfile`] with the password already in hand — the rekey arm's path,
/// which prompts for the old password through 3-07's seam rather than reading
/// stdin itself.
fn local_keyfile_with(
    path: &Path,
    pw: &zeroize::Zeroizing<String>,
) -> std::result::Result<LocalKeyfile, String> {
    let raw = std::fs::read_to_string(path).map_err(|_| {
        format!(
            "this bundle has no sync keyfile ({} is absent)",
            path.display()
        )
    })?;
    let keyfile: Keyfile = serde_json::from_str(&raw)
        .map_err(|_| format!("{} is not a readable sync keyfile", path.display()))?;
    open_keyfile(keyfile, pw)
}

/// The one place a password becomes keys. Neither the password nor any derived
/// key reaches the `String` this returns: a wrong one produces `crypto`'s own
/// single indistinguishable refusal.
fn open_keyfile(
    keyfile: Keyfile,
    pw: &zeroize::Zeroizing<String>,
) -> std::result::Result<LocalKeyfile, String> {
    // Argon2id at m = 1 GiB is a deliberate cost. Announce it before it starts —
    // a command that appears frozen for a second and a half reads as a hang.
    eprintln!("sync: deriving the sync key (Argon2id — this takes a moment)…");
    let keys = keyfile.open(pw.as_bytes()).map_err(|e| e.to_string())?;
    Ok(LocalKeyfile {
        kdf: keyfile.kdf.params(),
        asset: keyfile_asset_for(&keyfile)?,
        keys,
    })
}

/// The keyfile's asset name — `keyfile-<content address>.json`.
pub(crate) fn keyfile_asset_for(keyfile: &Keyfile) -> std::result::Result<String, String> {
    let canonical = serde_json::to_vec(keyfile)
        .map_err(|e| format!("the sync keyfile could not be serialized: {e}"))?;
    Ok(push::keyfile_asset_name(&content_address(&canonical)))
}

// ---- the write commands ----------------------------------------------------

/// Everything a `PushCtx` needs that is resolved from the local machine.
///
/// Held as a struct so the three write arms build it identically: a second
/// place that resolves a repository, a token and a pairing record is a second
/// place for them to disagree.
struct Resolved {
    repo: RepoRef,
    client: Client,
    index: Index,
    repo_id: String,
}

fn resolve(
    cfg: &Config,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &TokenChain,
) -> std::result::Result<Resolved, String> {
    let configured = cfg.sync.repo.as_deref().ok_or_else(|| {
        "no repository is configured. Set `repo = \"owner/name\"` under [sync] in config.toml \
         and run `ai-usagebar sync setup` — this tool never creates a repository."
            .to_owned()
    })?;
    let repo = RepoRef::parse(configured).map_err(|e| e.to_string())?;

    // The pairing record is what supplies the bundle identifier bound into every
    // snapshot root. Reading it from the *record* rather than from a response is
    // the format's §5 rule: a reader binds its own identifier.
    let pairing = pairing::read_from(&pairing::default_path(roots))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "this machine is not paired with {repo} yet. Run `ai-usagebar sync setup` \
                 first — it verifies the repository is private and records which repository \
                 this bundle belongs to."
            )
        })?;

    let (value, source) = token::resolve(chain).map_err(|e| e.to_string())?;
    let client = Client::new(endpoints, value, source).map_err(|e| e.to_string())?;
    let index = Index::at(&roots.index_file).map_err(|e| e.to_string())?;
    Ok(Resolved {
        repo,
        client,
        index,
        repo_id: push::repo_id_for(pairing.repo_id),
    })
}

/// `ai-usagebar sync push`.
///
/// **Every refusal that does not need a password comes first.** The password is
/// read here, at the terminal, and never below — nothing under
/// `src/sync/push/` reads a password or an environment variable — so an
/// unconfigured or unpaired machine must be told so *before* it is asked for
/// one.
fn push(
    cfg: &Config,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &TokenChain,
    now: DateTime<Utc>,
) -> i32 {
    let parts = match resolve(cfg, roots, endpoints, chain) {
        Ok(parts) => parts,
        Err(why) => return refuse(&why),
    };
    match local_keyfile(&keyfile_path(roots)) {
        Ok(keyfile) => push_with_parts(cfg, roots, &parts, &keyfile, now),
        Err(why) => refuse(&why),
    }
}

/// The tested seam: everything injected, and no terminal — `resolve` has already
/// turned the config, the token chain and the pairing record into values.
///
/// **A run whose only failure is the prune step exits 0.** That is D2 in the
/// exit code, and it is the one place where getting it wrong is silent: the
/// push already succeeded and the user's data is safe, so leaving a few stale
/// packs costs storage, not correctness.
fn push_with_parts(
    cfg: &Config,
    roots: &SyncRoots,
    parts: &Resolved,
    keyfile: &LocalKeyfile,
    now: DateTime<Utc>,
) -> i32 {
    let rt = match runtime() {
        Ok(rt) => rt,
        Err(why) => return refuse(&why),
    };
    let ctx = context(cfg, roots, keyfile, parts, now);
    // A progress line on a terminal, plain completed-asset lines when piped.
    // `is_terminal` is read here rather than inside the reporter so tests can
    // pin either shape without a tty.
    let mut progress = progress::reporter(std::io::stderr().is_terminal());
    match rt.block_on(push::run(ctx, progress.as_mut())) {
        Ok(outcome) => {
            print!("{}", render_push(&outcome));
            0
        }
        Err(e) => refuse(&e.to_string()),
    }
}

/// `ai-usagebar sync prune`.
///
/// Unlike the automatic prune after a push, a failure here **is** a failure:
/// the user asked for exactly this.
fn prune(
    cfg: &Config,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &TokenChain,
    now: DateTime<Utc>,
) -> i32 {
    let parts = match resolve(cfg, roots, endpoints, chain) {
        Ok(parts) => parts,
        Err(why) => return refuse(&why),
    };
    let keyfile = match local_keyfile(&keyfile_path(roots)) {
        Ok(k) => k,
        Err(why) => return refuse(&why),
    };
    let rt = match runtime() {
        Ok(rt) => rt,
        Err(why) => return refuse(&why),
    };
    let ctx = context(cfg, roots, &keyfile, &parts, now);
    match rt.block_on(push::prune::run_on_demand(
        &ctx,
        cfg.sync.keep_snapshots as usize,
    )) {
        Ok(deleted) => {
            println!("pruned:     {deleted} pack(s) no kept snapshot still referenced");
            0
        }
        Err(e) => refuse(&e.to_string()),
    }
}

/// `ai-usagebar sync rekey`.
///
/// **This arm owns both password prompts**, and they come *after* every refusal
/// that does not need one — an unconfigured or unpaired machine is told so
/// before it is asked for a password it would then discard. They go through plan
/// 3-07's prompt seam: TTY or stdin, never a command-line argument and never an
/// environment variable, which is Phase 1's rule and is not relaxed. Phase 1's
/// strength floor is applied to the new password before the call, so a refused
/// password costs no network round trip.
fn rekey(
    cfg: &Config,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &TokenChain,
    now: DateTime<Utc>,
) -> i32 {
    use crate::sync::github::setup::SetupPrompt;

    let parts = match resolve(cfg, roots, endpoints, chain) {
        Ok(parts) => parts,
        Err(why) => return refuse(&why),
    };
    let rt = match runtime() {
        Ok(rt) => rt,
        Err(why) => return refuse(&why),
    };

    let mut prompt = TtyPrompt;
    prompt.say(
        "Changing the sync password rewraps the master key. Not one pack byte moves — and \
         this is NOT revocation: anyone who already holds a copy of the old keyfile can still \
         open it with the old password.",
    );
    prompt.say("The CURRENT sync password:");
    let old_pw = match prompt.passphrase("") {
        Ok(pw) => pw,
        Err(e) => return refuse(&e.to_string()),
    };
    prompt.say("The NEW sync password:");
    let new_pw = match prompt.passphrase("") {
        Ok(pw) => pw,
        Err(e) => return refuse(&e.to_string()),
    };
    // Phase 1's floor, at the parameters the new keyfile will be written at.
    match passphrase::check(&new_pw, prompt.kdf()) {
        passphrase::Strength::Rejected(why) => return refuse(&format!("refused: {why}")),
        passphrase::Strength::Weak(why) => prompt.say(&format!("     {why}")),
        passphrase::Strength::Strong => {}
    }

    let keyfile = match local_keyfile_with(&keyfile_path(roots), &old_pw) {
        Ok(k) => k,
        Err(why) => return refuse(&why),
    };
    let ctx = context(cfg, roots, &keyfile, &parts, now);
    match rt.block_on(push::rekey::run(&ctx, &old_pw, &new_pw)) {
        Ok(asset) => {
            println!("keyfile:    republished as {asset}");
            println!(
                "note:       this is not revocation — an old copy of the keyfile still opens \
                 under the old password."
            );
            0
        }
        Err(e) => refuse(&e.to_string()),
    }
}

/// One non-zero exit, one message, and nothing else: no token, no prefix of one,
/// no header dump, no response body echoed unsanitized. Remote-supplied text
/// arrives already through `http::message_of`'s `sanitize_untrusted_field`.
fn refuse(why: &str) -> i32 {
    eprintln!("sync: {why}");
    1
}

fn context<'a>(
    cfg: &'a Config,
    roots: &'a SyncRoots,
    keyfile: &'a LocalKeyfile,
    parts: &'a Resolved,
    now: DateTime<Utc>,
) -> PushCtx<'a> {
    PushCtx {
        client: &parts.client,
        repo: &parts.repo,
        cfg: &cfg.sync,
        roots,
        keys: &keyfile.keys,
        kdf: keyfile.kdf,
        index: &parts.index,
        repo_id: parts.repo_id.clone(),
        keyfile_asset: keyfile.asset.clone(),
        // Filled by `push::run` from the remote, after the gate. A caller that
        // populated it would have had to make a request before the gate.
        previous: None,
        now,
    }
}

/// Pure, so the no-secret assertion is on a value rather than on captured
/// stdout. `PushOutcome` has no field that could hold a token or a passphrase.
fn render_push(outcome: &PushOutcome) -> String {
    let mut out = format!(
        "\nuploaded:   {} pack(s), {}\nskipped:    {} pack(s) already present\n\
         snapshots:  {} kept\npruned:     {} pack(s)\n",
        outcome.packs_uploaded,
        report::human_bytes(outcome.bytes_uploaded),
        outcome.packs_skipped,
        outcome.snapshots_kept,
        outcome.packs_deleted,
    );
    // D2: a prune failure is a warning on a successful push, never a failure.
    if let Some(warning) = &outcome.prune_warning {
        out.push_str(&format!(
            "warning:    the push succeeded and the snapshot is published, but cleaning up \
             superseded data did not: {warning}\n\
             \x20           This costs storage, not correctness. `ai-usagebar sync prune` \
             retries it.\n"
        ));
    }
    out.push_str("\nThe snapshot is published.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::github::setup::{Double, Script};
    use std::fs;
    use std::sync::Mutex;
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
                env_value: Some(zeroize::Zeroizing::new(TOKEN.into())),
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
                env_value: Some(zeroize::Zeroizing::new(TOKEN.into())),
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
    fn a_push_on_an_unconfigured_machine_refuses_before_any_request() {
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
                    env_value: Some(zeroize::Zeroizing::new(TOKEN.into())),
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

    // ---- 4-01: the push, end to end ---------------------------------------

    /// A seeded tree, a cheap keyfile, and a pairing record — everything a push
    /// resolves from the local machine, all inside the injected `TempDir`.
    ///
    /// Microsecond KDF parameters, never production ones: the AUR `check()` runs
    /// these tests on an installer's machine.
    fn seeded(dir: &TempDir) -> (SyncRoots, LocalKeyfile) {
        let roots = roots_at(dir);
        fs::create_dir_all(&roots.config_dir).unwrap();
        fs::write(&roots.config_file, b"[anthropic]\nenabled = true\n").unwrap();

        let cheap = crate::sync::crypto::KdfParams {
            m_kib: 8,
            t: 1,
            p: 1,
        };
        let (file, keys) =
            Keyfile::create_with_floor(b"correct horse battery staple", cheap, cheap.m_kib)
                .unwrap();
        let asset = keyfile_asset_for(&file).unwrap();
        // `ensure_keyfile` publishes the keyfile from disk, exactly as `sync
        // setup` writes it — so the fixture has to have written it. Holding the
        // `Keyfile` only in memory made every push fail at the keyfile hop.
        let keyfile_path = keyfile_path(&roots);
        fs::create_dir_all(keyfile_path.parent().unwrap()).unwrap();
        fs::write(&keyfile_path, serde_json::to_vec_pretty(&file).unwrap()).unwrap();
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
        (
            roots,
            LocalKeyfile {
                keys,
                kdf: cheap,
                asset,
            },
        )
    }

    /// Drives the push at `push_with_parts`, one step below the arm that reads
    /// the sync password off stdin — the same carve-out the `Setup` arm takes,
    /// and for the same reason: no test may drive a terminal.
    fn push_against(cfg: &Config, roots: &SyncRoots, keyfile: &LocalKeyfile, base: &str) -> i32 {
        let parts = resolve(
            cfg,
            roots,
            &Endpoints {
                api_base: base.into(),
                uploads_base: base.into(),
            },
            &TokenChain {
                env_value: Some(zeroize::Zeroizing::new(TOKEN.into())),
                ..TokenChain::default()
            },
        )
        .expect("the fixture is configured and paired");
        push_with_parts(cfg, roots, &parts, keyfile, NOW)
    }

    /// Every asset a fixture has accepted, in upload order: id `n` is index
    /// `n - 1`. Keyed by name because a push uploads several packs.
    type Sent = std::sync::Arc<Mutex<Vec<(String, Vec<u8>)>>>;

    /// `…/releases/9/assets?name=pack-<hex>.bin` — the name GitHub assigns is
    /// the one the caller asked for, so the fixture echoes it rather than
    /// inventing one.
    fn asset_name_in(path_and_query: &str) -> String {
        path_and_query
            .split_once("name=")
            .map(|(_, rest)| rest.split('&').next().unwrap_or(rest).to_string())
            .unwrap_or_else(|| panic!("an upload names its asset: {path_and_query}"))
    }

    fn asset_id_in(path: &str) -> usize {
        path.rsplit('/')
            .next()
            .and_then(|id| id.parse().ok())
            .unwrap_or_else(|| panic!("an asset path ends in its id: {path}"))
    }

    fn asset_json_sized(id: u64, name: &str, size: usize) -> String {
        format!(
            r#"{{"id":{id},"name":"{name}","size":{size},"state":"uploaded",
                "created_at":"2023-11-14T22:13:20Z"}}"#
        )
    }

    fn asset_json(id: u64, name: &str) -> String {
        format!(
            r#"{{"id":{id},"name":"{name}","size":1,"state":"uploaded",
                "created_at":"2023-11-14T22:13:20Z"}}"#
        )
    }

    /// Everything the outbound path touches: the release read, the pointer
    /// read, the resume listing, the uploads and the verifying downloads.
    ///
    /// **A push uploads more than one asset.** Plan 4-02 packs the manifest and
    /// the index object alongside the data chunks, so even a one-file bundle
    /// produces several packs. The fixture therefore keys everything by asset
    /// name and hands out ids in upload order, rather than assuming a single
    /// `pack-x.bin` with id 1 — an assumption that silently made the verifying
    /// download (D3) compare one pack's bytes against another's.
    ///
    /// The verification hop serves back the very bytes it was handed, so the
    /// recorder is per-test rather than a shared static: these tests run in
    /// parallel.
    fn mock_upload_path(server: &mut mockito::ServerGuard) -> Sent {
        let sent: Sent = std::sync::Arc::default();

        server
            .mock("GET", "/repos/o/n/releases/tags/ai-usagebar-sync-v1")
            .with_status(200)
            .with_body(r#"{"id":9}"#)
            .create();
        server
            .mock("GET", "/repos/o/n/contents/sync/pointer.json")
            .with_status(404)
            .with_body(r#"{"message":"Not Found"}"#)
            .create();
        // 4-03's resume scan lists the release's assets before the first
        // upload. Empty, because this fixture is a first push and nothing has
        // landed yet. `expect(1)` deliberately: the incident path in
        // `a_repository_that_turns_public_mid_push_deletes_and_does_not_flip`
        // lists a *second* time, and mockito prefers a matching mock that is
        // still missing hits — so that test's own listing answers the second
        // call without this one having to know about it.
        server
            .mock("GET", "/repos/o/n/releases/9/assets")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body("[]")
            // How many listings a push makes depends on how far it gets: the
            // resume scan always, `ensure_keyfile`'s own listing only if the
            // re-gate passes. `expect_at_least(1)` so the incident path, which
            // stops before the keyfile hop, still leaves its own listing mock
            // unsatisfied — mockito prefers a matching mock still missing hits.
            .expect_at_least(1)
            .create();

        // The body is recorded from `with_body_from_request`, not from
        // `match_request`: mockito evaluates every mock's matcher against every
        // request clearing method and path, so recording in a matcher counts
        // requests this mock never answers.
        let recorder = std::sync::Arc::clone(&sent);
        server
            .mock("POST", mockito::Matcher::Regex("/releases/9/assets".into()))
            .with_status(201)
            .with_body_from_request(move |req| {
                let name = asset_name_in(req.path_and_query());
                let mut held = recorder.lock().unwrap();
                held.push((name.clone(), req.body().unwrap().clone()));
                asset_json_sized(held.len() as u64, &name, held.last().unwrap().1.len())
                    .into_bytes()
            })
            .create();

        let echo = std::sync::Arc::clone(&sent);
        server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"/releases/assets/\d+$".into()),
            )
            .with_status(200)
            .with_body_from_request(move |req| {
                let id = asset_id_in(req.path());
                echo.lock()
                    .unwrap()
                    .get(id - 1)
                    .map(|(_, bytes)| bytes.clone())
                    .unwrap_or_default()
            })
            .create();

        sent
    }

    /// The whole outbound path: gate, plan, pack, upload, verify, re-gate, flip.
    /// One file, one pack, one asset, one compare-and-swap.
    #[test]
    fn a_push_uploads_one_asset_and_flips_the_pointer_with_a_precondition() {
        let dir = TempDir::new().unwrap();
        let (roots, keyfile) = seeded(&dir);
        let mut server = mockito::Server::new();

        // Two visibility reads: one before the first byte, one before the flip.
        // The gate is re-earned inside the push, never carried from `sync setup`.
        let gate = server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(PRIVATE_BODY)
            .expect(2)
            .create();
        let sent = mock_upload_path(&mut server);
        let flip = server
            .mock("PUT", "/repos/o/n/contents/sync/pointer.json")
            .with_status(201)
            .with_body(r#"{"content":{"sha":"blob1"}}"#)
            .expect(1)
            .create();

        assert_eq!(
            push_against(&cfg_with_repo(Some("o/n")), &roots, &keyfile, &server.url()),
            0
        );
        gate.assert();
        flip.assert();
        let uploaded = sent.lock().unwrap();
        assert!(
            uploaded.iter().any(|(name, _)| name.starts_with("pack-")),
            "pack bytes reached the uploads host: {:?}",
            uploaded.iter().map(|(n, _)| n).collect::<Vec<_>>()
        );
        // The call-site guard. `ensure_keyfile` shipped once with no caller at
        // all, its own tests green because they called it directly — the third
        // time this milestone has produced a tested function nothing invokes.
        // A first push that skips it publishes a pointer naming a keyfile asset
        // that does not exist, and no second machine can bootstrap from it.
        assert!(
            uploaded
                .iter()
                .any(|(name, _)| name.starts_with("keyfile-")),
            "the wrapped master key is published, not merely addressed: {:?}",
            uploaded.iter().map(|(n, _)| n).collect::<Vec<_>>()
        );
    }

    /// SAFE-02 through the push path: a repository that reads readable on the
    /// re-gate deletes what this run uploaded and never reaches the flip.
    #[test]
    fn a_repository_that_turns_public_mid_push_deletes_and_does_not_flip() {
        let dir = TempDir::new().unwrap();
        let (roots, keyfile) = seeded(&dir);
        let mut server = mockito::Server::new();

        // Private on the first read…
        server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(PRIVATE_BODY)
            .expect(1)
            .create();
        let sent = mock_upload_path(&mut server);
        // …and public on the second.
        let public = server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(
                r#"{"id":1,"private":false,"visibility":"public",
                    "owner":{"login":"o","id":7},"archived":false,"fork":false}"#,
            )
            .expect(1)
            .create();

        // The incident path lists, then deletes only what this run uploaded.
        // The listing echoes every asset the fixture accepted — a push packs the
        // manifest and index object as well as the data, so "what this run
        // uploaded" is several assets, not one.
        let named = std::sync::Arc::clone(&sent);
        let listing = server
            .mock("GET", mockito::Matcher::Regex("per_page=100".into()))
            .with_status(200)
            .with_body_from_request(move |_| {
                let held = named.lock().unwrap();
                let rows: Vec<String> = held
                    .iter()
                    .enumerate()
                    .map(|(i, (name, bytes))| asset_json_sized(i as u64 + 1, name, bytes.len()))
                    .collect();
                format!("[{}]", rows.join(",")).into_bytes()
            })
            .expect(1)
            .create();
        // Asserted as a *set equal to what was uploaded*, not as a count: the
        // number of packs is 4-02's business, and a fixture that pins it would
        // fail on any future packing change while proving nothing extra.
        let deleted: std::sync::Arc<Mutex<Vec<usize>>> = std::sync::Arc::default();
        let recorder = std::sync::Arc::clone(&deleted);
        server
            .mock(
                "DELETE",
                mockito::Matcher::Regex(r"/releases/assets/\d+$".into()),
            )
            .with_status(204)
            .with_body_from_request(move |req| {
                recorder.lock().unwrap().push(asset_id_in(req.path()));
                Vec::new()
            })
            .create();
        let flip = server
            .mock("PUT", "/repos/o/n/contents/sync/pointer.json")
            .with_status(201)
            .expect(0)
            .create();

        assert_ne!(
            push_against(&cfg_with_repo(Some("o/n")), &roots, &keyfile, &server.url()),
            0
        );
        public.assert();
        listing.assert();
        flip.assert();

        let mut destroyed = deleted.lock().unwrap().clone();
        destroyed.sort_unstable();
        let uploaded: Vec<usize> = (1..=sent.lock().unwrap().len()).collect();
        assert!(!uploaded.is_empty(), "the run uploaded before it re-gated");
        assert_eq!(
            destroyed, uploaded,
            "every asset this run uploaded is destroyed, and nothing else is"
        );
    }

    /// A failed flip is a failed push. `with_retry` still never retries a
    /// conflict; the **one** re-drive is plan 4-04's bounded compare-and-swap in
    /// `pointer::commit` — re-read, rebuild on whoever won, `PUT` once more —
    /// and it stops there rather than looping. Two `PUT`s and no third.
    #[test]
    fn a_failed_pointer_put_exits_non_zero_after_exactly_one_bounded_retry() {
        let dir = TempDir::new().unwrap();
        let (roots, keyfile) = seeded(&dir);
        let mut server = mockito::Server::new();

        server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(PRIVATE_BODY)
            .create();
        let _sent = mock_upload_path(&mut server);
        let flip = server
            .mock("PUT", "/repos/o/n/contents/sync/pointer.json")
            .with_status(409)
            .with_body(r#"{"message":"is at abc but expected def"}"#)
            .expect(2)
            .create();

        assert_ne!(
            push_against(&cfg_with_repo(Some("o/n")), &roots, &keyfile, &server.url()),
            0
        );
        flip.assert();
    }

    /// SYNC-04, the other half of D3: a pack that does not read back as what was
    /// sent fails the push **before** the flip, so no pointer can ever reference
    /// a pack that did not verify. Killing a run anywhere above the `PUT` leaves
    /// the remote pointer byte-identical to what it was.
    #[test]
    fn a_pack_that_does_not_verify_never_reaches_the_pointer_put() {
        let dir = TempDir::new().unwrap();
        let (roots, keyfile) = seeded(&dir);
        let mut server = mockito::Server::new();

        server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(PRIVATE_BODY)
            .create();
        server
            .mock("GET", "/repos/o/n/releases/tags/ai-usagebar-sync-v1")
            .with_status(200)
            .with_body(r#"{"id":9}"#)
            .create();
        server
            .mock("GET", "/repos/o/n/contents/sync/pointer.json")
            .with_status(404)
            .with_body(r#"{"message":"Not Found"}"#)
            .create();
        // 4-03's resume scan, with nothing landed: every pack uploads.
        server
            .mock("GET", "/repos/o/n/releases/9/assets")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body("[]")
            .create();
        server
            .mock("POST", mockito::Matcher::Regex("/releases/9/assets".into()))
            .with_status(201)
            .with_body(asset_json(1, "pack-x.bin"))
            .create();
        // Serves something other than what was uploaded.
        //
        // `expect_at_least`, not `expect(1)`: uploads run in a bounded window,
        // so when the first verification fails the ones already in flight have
        // also been read back. Pinning the count would pin 4-03's concurrency
        // cap into an unrelated test. What this test asserts is the flip.
        let verify = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"/releases/assets/\d+$".into()),
            )
            .with_status(200)
            .with_body("not the bytes that were sent")
            .expect_at_least(1)
            .create();
        let flip = server
            .mock("PUT", "/repos/o/n/contents/sync/pointer.json")
            .with_status(201)
            .expect(0)
            .create();

        assert_ne!(
            push_against(&cfg_with_repo(Some("o/n")), &roots, &keyfile, &server.url()),
            0
        );
        verify.assert();
        flip.assert();
    }

    /// D2 in the exit code: a prune failure is a warning, never a failed push.
    #[test]
    fn a_prune_failure_is_a_warning_line_and_the_push_still_exits_zero() {
        let rendered = render_push(&PushOutcome {
            packs_uploaded: 2,
            bytes_uploaded: 4096,
            snapshots_kept: 3,
            prune_warning: Some("GitHub refused the delete (403)".into()),
            ..PushOutcome::default()
        });
        assert!(rendered.contains("warning:"), "{rendered}");
        assert!(rendered.contains("403"), "{rendered}");
        assert!(rendered.contains("storage, not correctness"), "{rendered}");
        assert!(rendered.contains("published"), "{rendered}");
    }

    /// T-4-10. `PushOutcome` has no field that *could* hold either secret, and
    /// this is what keeps it that way.
    #[test]
    fn the_rendered_outcome_carries_neither_the_token_nor_the_passphrase() {
        let rendered = render_push(&PushOutcome {
            packs_uploaded: 1,
            prune_warning: Some("nothing secret here".into()),
            ..PushOutcome::default()
        });
        assert!(!rendered.contains(TOKEN), "{rendered}");
        assert!(!rendered.contains(&TOKEN[..8]), "{rendered}");
        assert!(!rendered.contains("correct horse"), "{rendered}");
    }

    /// **The dispatch exists and is reached.** `run_with` routes a bare `push`, a
    /// `prune` and a `rekey` into the real arms; an unconfigured repository
    /// refuses before any prompt and before any request, which is what makes
    /// this safe to drive through the production entry point.
    #[test]
    fn the_three_write_actions_dispatch_and_refuse_an_unconfigured_machine() {
        let dir = TempDir::new().unwrap();
        for action in [
            SyncAction::Push { dry_run: false },
            SyncAction::Prune,
            SyncAction::Rekey,
        ] {
            assert_ne!(
                drive(&action, &cfg_with_repo(None), &dir, "http://127.0.0.1:1"),
                0,
                "{action:?} must refuse an unconfigured machine"
            );
        }
    }

    /// The bundle identifier comes from the pairing record, so an unpaired
    /// machine is told to pair rather than handed a confusing failure after a
    /// network round trip.
    #[test]
    fn an_unpaired_machine_is_told_to_run_setup_rather_than_pushing() {
        let dir = TempDir::new().unwrap();
        let err = resolve(
            &cfg_with_repo(Some("o/n")),
            &roots_at(&dir),
            &Endpoints::default(),
            &TokenChain {
                env_value: Some(zeroize::Zeroizing::new(TOKEN.into())),
                ..TokenChain::default()
            },
        )
        .err()
        .expect("nothing was paired");
        assert!(err.contains("sync setup"), "{err}");
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
