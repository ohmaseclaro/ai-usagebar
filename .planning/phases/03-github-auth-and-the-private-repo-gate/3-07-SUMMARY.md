---
phase: 03-github-auth-and-the-private-repo-gate
plan: 07
subsystem: transport
tags: [sync-setup, guided-flow, prompt-seam, sync-status, safe-02, hermetic-tests, zero-upload]

requires:
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 01
    provides: "`setup::run`'s entry point, `SetupOutcome`, `cli::run_with`, `Client`, `Endpoints`, `RepoRef`"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 02
    provides: "`token::store` / `token::clear`, the live four-source `TokenChain`"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 03
    provides: "`http::actionable`'s seven-arm message table"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 04
    provides: "`assert_pushable`'s six refusals, `pairing::{read_from, write_to, check_drift}`, `DriftOutcome`, the SAFE-02 incident"
  - phase: 01-encrypted-bundle-core
    provides: "`passphrase::{generate, check, NO_RECOVERY, OFFLINE_ATTACK_NOTE}`, `crypto::Keyfile::create`"
  - phase: 02-bundle-scope-local-index-dry-run-planning
    provides: "`plan::build_with_keys`, `report::{build_status, render_dry_run}`, `SyncRoots`, `SyncConfig::includes`"
provides:
  - "`SetupPrompt` — the injected seam (`say` / `confirm` / `passphrase` / `categories` / `kdf` / `store_token` / `clear_token`) plus `TtyPrompt`"
  - "`setup::run` — UX-03's five steps, ordered, with every refusal before the first prompt"
  - "`SetupOutcome` extended: `stored_at`, `categories`, `keyfile`, `reused_pairing`, `files`, `raw_bytes`, `would_send`"
  - "`setup::{token_path, clear_if_dead}` — the token file and the 401 call site, shared with `sync status`"
  - "`report::RepoSection` + `render_repo` — the repository half of `sync status`"
  - "`cli::{status, resolve_repo_section, repo_section}` — one repository request per invocation"
  - "`SyncRoots::index_file` — the index path, injected"
  - "`From<GithubError> for AppError` now renders `actionable`, which had no call site at all"
affects: [phase-4-push]

tech-stack:
  added: []
  patterns:
    - "A prompt double that *records which methods were reached* turns an ordering claim into an assertion. `reached.is_empty()` after a refusal is the whole of T-3-35."
    - "Trait methods with production defaults (`kdf`, `store_token`, `clear_token`) are the cheapest possible seam for a dependency that is unsafe in a test: the production call site is unchanged, and the override is what a test asserts against."
    - "Reusing `render_dry_run` for setup's step 4 makes 'the same number' structural rather than a promise — there is only one renderer and one plan."

key-files:
  created: []
  modified:
    - src/sync/github/setup.rs
    - src/sync/report.rs
    - src/sync/cli.rs
    - src/sync/github/http.rs
    - src/sync/mod.rs
    - src/bin/ai-usagebar.rs
    - docs/sync-github.md

key-decisions:
  - "**`actionable` had zero call sites.** `From<GithubError> for AppError` used `err.to_string()`, so 3-03's entire message table — every 'names the fix' string D-06 and REPO-05 rest on — was unreachable from any command. That conversion is the single choke point between a `GithubError` and the text a user reads, so it now runs `actionable(&err)`. Fixing it at the call sites instead would have meant reconstructing a `GithubError` from an `AppError` in two places."
  - "**`SetupPrompt` carries `store_token` / `clear_token`, not just prompts.** `token::store` writes the real macOS login Keychain and `token::clear` deletes it; a test reaching step 5 through the production functions clobbers the developer's own token and would do the same during the AUR `check()`. The defaults *are* production, so no call site changed."
  - "**`clear_if_dead(err, path, clear)` takes the clear as a parameter** and lives in `setup.rs`, shared with `cli.rs`'s status arm. One decision point for 'a 401 means the stored token is dead', two callers, and a test that asserts a 403 does *not* clear — which is what the 403 message promises when it says 'keep it'."
  - "**Re-running setup reuses the *pairing record*; an existing *keyfile* stops the flow.** The two are separate objects. `DriftOutcome::first_contact == false` is what 'already paired, reusing it' means; T-3-38 is about the keyfile, whose overwrite is unrecoverable."
  - "**Step 4 renders `report::render_dry_run` over `plan::build_with_keys`'s own plan.** No second estimate exists to disagree with the first, and `sync setup` and `sync push --dry-run` cannot print two answers to 'what will this cost me'."
  - "**`sync status` runs `check_drift` then `assert_pushable`, in that order, on one `fetch_facts`.** Both take the same `cfg.includes(SyncCategory::Credentials)` value. The gate is not re-run per consumer — asserted by `mockito`'s `.expect(1)`."
  - "**An unconfigured `status` builds no runtime and makes no request.** A machine that never named a repository reporting a network error would be a lie, and it is not a non-zero exit either."
  - "**`SyncRoots` gained `index_file`** rather than a seventh `run_with` parameter. `SyncRoots::at` (test-only in practice, six callers) derives it from `config_dir`; `SyncRoots::resolve` (production-only) takes `index::default_path()`. Without it every `sync status` test would create `~/.cache/ai-usagebar/sync/index.sqlite3` on an installer's machine."

patterns-established:
  - "When a message in file A promises an action owned by file B, the test that proves the pairing belongs with B and asserts against a recorder — not a review note."

requirements-completed: [UX-03, REPO-02, REPO-05, SAFE-01, SAFE-02]

coverage:
  - id: UX-03
    description: "`sync setup` walks repo → password → categories → size → ready against a mock private repository, stores the token, and exits zero"
    verification:
      - kind: unit
        ref: "src/sync/github/setup.rs#the_five_steps_run_in_order_and_end_at_ready_to_push"
        status: pass
      - kind: unit
        ref: "src/sync/cli.rs#the_success_line_reports_the_token_source_and_never_the_token"
        status: pass
    human_judgment: false
  - id: T-3-35
    description: "Every refusal stops before the password step, asserted by a prompt double that records which methods were reached"
    verification:
      - kind: unit
        ref: "src/sync/github/setup.rs#every_refusal_stops_before_the_password_step_is_reached"
        status: pass
      - kind: unit
        ref: "src/sync/github/setup.rs#a_public_repository_with_credentials_on_stops_before_the_passphrase"
        status: pass
      - kind: unit
        ref: "src/sync/github/setup.rs#an_existing_keyfile_stops_the_flow_rather_than_being_overwritten"
        status: pass
    human_judgment: false
  - id: REPO-02
    description: "The token appears in no rendered line, no narrated line, no config file, and no process argument at any verbosity"
    verification:
      - kind: unit
        ref: "src/sync/github/setup.rs#the_outcome_carries_neither_the_token_nor_the_passphrase"
        status: pass
      - kind: unit
        ref: "src/sync/cli.rs#the_success_line_reports_the_token_source_and_never_the_token"
        status: pass
      - kind: unit
        ref: "src/sync/cli.rs#status_reports_the_repository_visibility_token_source_and_last_verified"
        status: pass
    human_judgment: false
  - id: REPO-05
    description: "`sync status` reports the repository, its visibility, the token's source, and when the pairing was last verified; every repository-section failure is non-zero"
    verification:
      - kind: unit
        ref: "src/sync/cli.rs#status_reports_the_repository_visibility_token_source_and_last_verified"
        status: pass
      - kind: unit
        ref: "src/sync/cli.rs#status_without_a_token_names_every_source_and_exits_non_zero"
        status: pass
      - kind: unit
        ref: "src/sync/cli.rs#an_unreachable_github_still_prints_the_listing_and_exits_non_zero"
        status: pass
      - kind: unit
        ref: "src/sync/cli.rs#status_without_a_configured_repo_names_the_key_and_still_lists_categories"
        status: pass
    human_judgment: false
  - id: SAFE-02
    description: "A repository that has turned public since pairing is reported by `sync status` as the incident, verbatim, not as a generic error"
    verification:
      - kind: unit
        ref: "src/sync/cli.rs#a_repository_that_turned_public_is_reported_as_the_incident_and_exits_non_zero"
        status: pass
    human_judgment: false
  - id: SAFE-01
    description: "D-04's credentials carve-out survives both gate calls in the setup flow's own call order"
    verification:
      - kind: unit
        ref: "src/sync/github/setup.rs#a_public_repository_with_credentials_off_proceeds_with_the_warning"
        status: pass
      - kind: unit
        ref: "src/sync/github/setup.rs#a_public_repository_with_credentials_on_stops_before_the_passphrase"
        status: pass
    human_judgment: false
  - id: "3-CONTEXT cross-plan promise"
    description: "`actionable`'s 401 arm says the stored token will be cleared, and the call site clears it; a 403 does not"
    verification:
      - kind: unit
        ref: "src/sync/github/setup.rs#a_401_clears_the_stored_token_and_nothing_else_does"
        status: pass
      - kind: unit
        ref: "src/sync/github/setup.rs#the_401_message_promises_the_clear_the_call_site_performs"
        status: pass
      - kind: unit
        ref: "src/sync/github/setup.rs#a_403_does_not_clear_the_token_the_message_told_the_user_to_keep"
        status: pass
    human_judgment: false
  - id: T-3-36
    description: "Neither secret, nor an eight-character prefix of either, reaches the rendered outcome; no keyfile byte either"
    verification:
      - kind: unit
        ref: "src/sync/github/setup.rs#the_outcome_carries_neither_the_token_nor_the_passphrase"
        status: pass
    human_judgment: false
  - id: T-3-37
    description: "The passphrase is accepted only from the prompt seam; `TtyPrompt` reads it through `passphrase::read_line` on stdin, never argv or an environment variable"
    verification:
      - kind: manual
        ref: "grep — `std::io::stdin` appears in `setup.rs` only inside `TtyPrompt`; no `std::env::var` and no CLI flag anywhere in the file"
        status: pass
    human_judgment: false
  - id: T-3-38
    description: "An existing keyfile stops the flow with what exists and why, before a new passphrase is even generated"
    verification:
      - kind: unit
        ref: "src/sync/github/setup.rs#an_existing_keyfile_stops_the_flow_rather_than_being_overwritten"
        status: pass
    human_judgment: false
  - id: T-3-39
    description: "The `config.toml` write-back is `toml_edit` in place, preserving comments and unrelated keys, atomic, mode 0600"
    verification:
      - kind: unit
        ref: "src/sync/github/setup.rs#a_toggled_category_lands_in_the_injected_config_and_reads_back"
        status: pass
    human_judgment: false
  - id: T-3-40
    description: "The closing line states explicitly that nothing was uploaded (D-05)"
    verification:
      - kind: unit
        ref: "src/sync/cli.rs#the_success_line_reports_the_token_source_and_never_the_token"
        status: pass
      - kind: unit
        ref: "src/sync/github/setup.rs#declining_the_size_confirmation_stops_without_pairing"
        status: pass
    human_judgment: false
  - id: T-3-41
    description: "A repository-section failure exits non-zero even though the category listing still renders"
    verification:
      - kind: unit
        ref: "src/sync/cli.rs#a_repository_that_turned_public_is_reported_as_the_incident_and_exits_non_zero"
        status: pass
      - kind: unit
        ref: "src/sync/cli.rs#status_without_a_token_names_every_source_and_exits_non_zero"
        status: pass
    human_judgment: false
  - id: hermeticity
    description: "No test drives a terminal, reads a real `$HOME`/`$XDG`, reaches the network, or touches the real Keychain"
    verification:
      - kind: unit
        ref: "src/sync/github/setup.rs#nothing_is_written_outside_the_injected_temp_directory"
        status: pass
      - kind: manual
        ref: "`security find-generic-password -s ai-usagebar-sync-token` shows an unchanged `mdat` across a full `cargo test --lib -- sync:: config::` run"
        status: pass
    human_judgment: false

duration: 90min
completed: 2026-08-19
status: complete
---

# Phase 3 / Plan 07: The Guided Setup, and What `sync status` Now Knows

**Two statements that had outrun their implementation are now kept, and the
second one was not on the list.**

The one this plan was warned about: `http::actionable`'s 401 arm tells the user
the stored token "will be cleared", and there was no call site clearing it. There
is one now, and a test asserts the pairing.

The one found while wiring it: **`actionable` had no call site at all.**
`From<GithubError> for AppError` rendered `err.to_string()`, so the entire 3-03
message table — six distinct outcomes each naming a fix, the whole of D-06 and
REPO-05 — was unreachable from every command in the crate. A user hitting a 403
got `HTTP 403: GitHub refused this request (403): …` and no instruction. That is
the sixth instance of this pattern in the milestone and the second caught before
shipping.

## Task Commits

1. **`830d109`** — the guided flow, the repository section, and `actionable`'s first call site
2. **`c258cb5`** — the confirmed size is the planner's own total, and the narration is token-free
3. **`ed5014e`** — no test may reach the real macOS login Keychain

## `SetupPrompt` — the seam, verbatim

```rust
pub trait SetupPrompt {
    fn say(&mut self, line: &str);
    fn confirm(&mut self, question: &str, default_yes: bool) -> Result<bool>;
    fn passphrase(&mut self, generated: &str) -> Result<Zeroizing<String>>;
    fn categories(&mut self, current: &[SyncCategory]) -> Result<Vec<SyncCategory>>;

    // Defaults are production; a test overrides them.
    fn kdf(&self) -> KdfParams { KdfParams::default() }
    fn store_token(&self, token: &str, file: &Path) -> Result<TokenSource> { token::store(token, file) }
    fn clear_token(&self, file: &Path) -> Result<()> { token::clear(file) }
}

pub struct TtyPrompt;   // the only thing in the file that reads stdin
```

The last three are not prompts. They are the dependencies a test *may not
perform*: 1 GiB of Argon2 per test, a write to the real macOS login Keychain, and
a delete from it. Giving them production defaults means no call site knows the
seam exists, and overriding them in the double turns each into an assertion —
`stored == [token_path]` fails loudly if the override is ever dropped, rather
than silently writing a Keychain again.

`run`'s signature gained exactly one parameter:

```rust
pub async fn run(
    cfg: &SyncConfig,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &token::TokenChain,
    prompt: &mut dyn SetupPrompt,      // new
    now: DateTime<Utc>,
) -> Result<SetupOutcome>;
```

`Script` / `Double` live at module scope under `#[cfg(test)]` and are
`pub(crate)`, because `sync::cli`'s tests drive the same flow — the same
cross-module test-helper reuse `cache::temp_file` already makes.

## `SetupOutcome` — the shape Phase 4 reads

```rust
pub struct SetupOutcome {
    pub repo: RepoRef,
    pub token_source: TokenSource,   // where it was resolved from
    pub stored_at: TokenSource,      // where it was persisted to
    pub visibility: String,
    pub warnings: Vec<String>,       // drift warnings, then gate warnings
    pub clearance: PushClearance,
    pub categories: Vec<SyncCategory>,
    pub keyfile: PathBuf,
    pub reused_pairing: bool,
    pub files: usize,
    pub raw_bytes: u64,
    pub would_send: u64,
}
```

Nothing here can hold a secret: the token is a source, the passphrase is not
represented, and no keyfile byte is carried. That is asserted against both the
values *and* their eight-character prefixes.

## The local keyfile path — Phase 4 uploads this

```
<roots.config_dir>/sync/keyfile.json      // mode 0600, atomic, local only
```

Canonically `sync::cli::keyfile_path(roots)`, now `pub(crate)` so `setup.rs`
writes exactly the file `sync push --dry-run` already reads. Beside it:

```
<roots.config_dir>/sync-pairing.json      // pairing::default_path
<roots.config_dir>/sync-token             // setup::token_path, non-macOS
<roots.index_file>                        // ~/.cache/ai-usagebar/sync/index.sqlite3
```

**Phase 4 re-runs the gate before its flip** and uploads the keyfile. It must not
carry the `PushClearance` minted here — see 3-01's Phase 4 contract.

## The five steps, and what each one refuses

| step | does | refuses |
|---|---|---|
| 1 | parse `[sync] repo`, resolve the token, one `GET`, `check_drift` **then** `assert_pushable` | everything — before a single prompt method is called |
| 2 | Phase 1's generated passphrase or a supplied one, `Keyfile::create`, write at 0600 | an existing keyfile, unrecoverably |
| 3 | toggle categories, `toml_edit` write-back | — |
| 4 | `plan::build_with_keys`, rendered through `report::render_dry_run` | a declined confirmation |
| 5 | `store_token`, `pairing::write_to`, print "ready to push" | — |

`credentials_in_bundle` is `cfg.includes(SyncCategory::Credentials)`, computed
once at the top of step 1 and passed to both gate calls. `check_drift` runs
first, always.

The generated passphrase is displayed once, with `NO_RECOVERY` and
`OFFLINE_ATTACK_NOTE` verbatim from Phase 1; a refused one is re-prompted without
re-displaying it. `TtyPrompt` echoes a typed passphrase and says so — there is no
hidden-input crate in this tree and adding one would break the zero-new-crates
rule; taking the generated one needs no typing at all.

## `sync status` — the repository section

```rust
pub struct RepoSection {
    pub configured: Option<String>,
    pub visibility: Option<String>,
    pub token_source: Option<&'static str>,   // the label; there is no value field
    pub last_verified: Option<DateTime<Utc>>,
    pub warnings: Vec<String>,
    pub failure: Option<String>,
}
impl RepoSection { pub fn failed(self, why: String) -> RepoSection; }
```

`build_status` takes it as a sixth parameter; `render_status` appends
`render_repo`. `push --dry-run` passes `None` and contacts nothing.

```
  repo:      owner/name
  visible:   private
  token:     present (env)
  verified:  2026-08-19T12:00:00+00:00
```

Unconfigured renders the D-01 block naming `[sync] repo` and the `gh repo create`
line, and exits **zero** — an unconfigured machine has not failed at anything.
Every other failure exits non-zero while leaving Phase 2's category listing fully
rendered above it: a user whose token expired should still be able to see what
would be sent. The SAFE-02 incident renders verbatim from
`pairing::went_public_incident`.

## The `anthropic::keychain` noun — softened, not fixed

3-02 flagged that `anthropic::keychain`'s errors say "Claude credentials", so a
locked Keychain during `sync setup` names the wrong secret. That module is not
this plan's, so the wart is softened at the call site instead: step 5 wraps a
`store_token` failure with

> The secret being saved here is the *sync token*, not a Claude credential — the
> wording above comes from the shared Keychain helper.

The inner message still names the right fix (unlock the Keychain / allow
ai-usagebar), which is what D-06 requires. A one-line generalization in the
module that owns it remains the real fix.

## `docs/sync-github.md` — reconciled

Everything 3-05 assumed about names shipped exactly: `ai-usagebar sync setup`,
`ai-usagebar sync status`, `[sync] repo = "owner/name"`, `AI_USAGEBAR_SYNC_TOKEN`,
service `ai-usagebar-sync-token`, `~/.config/ai-usagebar/sync-token`, and the
`gh auth token` fallback. Three things in the document diverged from the code and
the **document** was corrected, per the plan's instruction:

- It promised **"the tool will warn you if it detects a token with Administration
  permissions."** 3-04 deliberately did not ship that warning and explained why;
  the document now explains the same thing.
- It said the token file is for "Linux and other platforms". It is *written*
  there off macOS but *read* on every platform, so a file copied from another
  machine still works on a Mac.
- Its reason for refusing a fork was API quotas. 3-04's actual reason is that a
  fork shares its upstream's object network.

Added: the five steps of `sync setup`, and the four token source **labels**
(`env`, `Keychain`, `file`, `gh`) as `sync status` prints them.

## Deviations from the plan

- **`src/sync/github/http.rs`** was edited — one line, plus a comment — to route
  `From<GithubError> for AppError` through `actionable`. Not in `files_modified`,
  but the plan requires the flow to exit "with the message plan 3-03 produced",
  and the alternative was reconstructing a `GithubError` from an `AppError` at
  two call sites. 3-03 is merged and no wave-3 plan owns the file.
- **`src/sync/mod.rs`** gained `SyncRoots::index_file`. Without it every `sync
  status` test writes `~/.cache/ai-usagebar/sync/index.sqlite3` on the machine
  running `cargo test` — which the AUR `check()` does on installers' machines.
  `SyncRoots::at` derives it from `config_dir` so all six existing test callers
  compile unchanged; `SyncRoots::resolve` uses `index::default_path()`, so
  production is byte-identical to before.
- **`src/bin/ai-usagebar.rs`**'s comment was corrected: `status` now makes a
  request too, when a repository is configured.
- **`run_with`'s `Setup` arm has no test.** It constructs a `TtyPrompt`, and a
  test driving it would read stdin and pay the shipped 1 GiB KDF. It is now the
  same class of thin production wrapper as `run` — dispatch plus a runtime. The
  arm's refusal path (`[sync] repo` unset) *is* driven through `run_with`,
  because it returns before any prompt method is reached.
- **Three commits, not per-task.** Task 1 and task 2 interlock through
  `build_status`'s signature and `RepoSection`; splitting them would have made
  the first commit not compile.

## Two things Phase 4 must know

1. **`plan::build_with_keys` mutates the index**, and `sync setup` now calls it —
   exactly as `sync push --dry-run` already did since 2-07. After a setup or a
   dry-run, the index records the chunks it planned, so a subsequent `build`
   returns an *empty* plan for unchanged files. A push that derives "what to
   upload" from a fresh `build` alone would therefore upload nothing. This is
   pre-existing and not introduced here, but setup makes it the *first* thing a
   new user does, so Phase 4's upload path has to reconcile against what was
   actually transmitted rather than against what was last planned.
2. **`sync status`'s 401 arm calls the production `token::clear`**, which deletes
   the real macOS Keychain item. It carries a comment saying so. If a later plan
   wants a 401 test against `status`, give that arm the same seam
   `SetupPrompt::clear_token` has first.

## Verification

```
cargo test --lib -- sync::github::setup sync::report sync::cli   # 41 passed, 0 failed
cargo test --lib -- sync:: config::                              # 338 passed, 0 failed
cargo clippy --all-targets -- -D warnings                        # clean
cargo fmt --check                                                # clean
```

- `Cargo.toml` unchanged. Zero new crates.
- `tests/live.rs` untouched — plan 3-06 owns it.
- `.planning/STATE.md`, `ROADMAP.md`, `REQUIREMENTS.md` untouched.
- Hermeticity, checked rather than assumed: `security find-generic-password -s
  ai-usagebar-sync-token` reports an unchanged `mdat` across a full
  `cargo test --lib -- sync:: config::` run, and
  `nothing_is_written_outside_the_injected_temp_directory` walks the keyfile, the
  pairing record, the token path, `config.toml` and the index and asserts every
  one starts with the `TempDir`.

## Not done here, deliberately

- **Nothing is uploaded** (D-05). `sync setup` ends at "ready to push".
- **The D-03 administrative-permission warning** is still outstanding, per 3-04,
  and `docs/sync-github.md` now says so rather than promising it.
- **`TtyPrompt` has no test** — it is the one thing in the file that reads stdin,
  and it is the reason everything else in the file is testable.
