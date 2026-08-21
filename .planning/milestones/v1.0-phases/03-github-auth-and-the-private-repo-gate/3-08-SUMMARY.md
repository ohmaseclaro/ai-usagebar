---
phase: 03-github-auth-and-the-private-repo-gate
plan: 08
subsystem: transport
tags: [security-remediation, push-clearance, token-source, gate-ordering, capability-type, zeroize]

requires:
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 01
    provides: "`PushClearance`, `assert_pushable`'s frozen signature, `Client`, `Endpoints`, `RepoRef`"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 02
    provides: "`TokenChain`, `TokenSource`, `token::{store, clear}`, `sync::keychain`"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 03
    provides: "`GithubError`, `http::{classify, actionable, message_of}`"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 04
    provides: "`gate::fetch_facts`, `pairing::check_drift`, `DriftOutcome`"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 07
    provides: "`setup::run`'s five steps, `SetupPrompt`, `clear_if_dead`, `cli::repo_section`"
provides:
  - "`gate::Pushing` — the capability every Phase 4 write verb must take by value"
  - "`gate::PushClearance::spend(self, now) -> Result<Pushing>` — replaces `assert_fresh`, consuming"
  - "`gate::FetchError { error, token_rejected }` — `fetch_facts`'s new error type, 401 distinguishable"
  - "`token::clear_source(source, file)` + `token::owns_a_store(source)` + `token::clear_note(source, file)` — replaces `token::clear`"
  - "`SetupPrompt::clear_token(source, file)` — the seam now carries the source"
  - "`setup::clear_if_dead(err: gate::FetchError, source, file, clear)` — new signature"
  - "`SetupOutcome` no longer carries `clearance`"
affects: [phase-4-push]

tech-stack:
  added: []
  patterns:
    - "A capability that must be checked before use should be *consumed* by the check, not queried by a `&self` method. `spend(self, now) -> Result<Pushing>` cannot be forgotten; `assert_fresh(&self, …)` was, for a whole phase, while having tests."
    - "When an error type collapses two causes into one arm, carry the distinction as a field on a dedicated error rather than re-deriving it from a message. `AppError::Credentials` meant both `401` and `not a legal header value`."
    - "A destructive action parameterized by a source needs the partition stated once (`owns_a_store`) and enforced twice — at the caller, so a test double can observe the decision, and inside the destructive function, so a new caller cannot bypass it."
    - "A prompt that is an *input to a gate* must run before the gate. Reordering leaves one evaluation; recomputing leaves two to keep in agreement, which is the shape that produced the hole."

key-files:
  created: []
  modified:
    - src/sync/github/gate.rs
    - src/sync/github/setup.rs
    - src/sync/github/token.rs
    - src/sync/github/http.rs
    - src/sync/github/keychain.rs
    - src/sync/cli.rs
    - src/sync/passphrase.rs
    - docs/sync-github.md

key-decisions:
  - "**F-1's trigger moved from an `AppError` arm to a typed field.** `gate::fetch_facts` now returns `Result<RepoFacts, FetchError>` where `token_rejected` is set only for `GithubError::Unauthorized`. Matching `AppError::Credentials(_)` also caught `Client::get_json`'s illegal-header-byte failure, so a malformed token deleted the Keychain item while printing about header validity."
  - "**F-1's action is partitioned by `TokenSource`, in two places.** `token::clear_source` clears the Keychain for `Keychain`, the file for `File`, and nothing for `Env`/`GhCli`; `setup::clear_if_dead` does not even reach the seam unless `token::owns_a_store(source)`. The double guard is what makes 'the environment 401'd and nothing was deleted' assertable through the test double rather than only inside the production function."
  - "**`http::actionable`'s 401 arm no longer promises a clear.** It knows the status, not the store, and 'the stored token will be cleared' was false in both directions. `token::clear_note` — beside `clear_source`, so the two cannot drift — states what was actually done, and for `Env`/`GhCli` names the variable or the CLI to change instead."
  - "**F-2 reordered rather than recomputed.** The category prompt is step 1 and the gate is step 2, so `credentials_in_bundle` is derived once, from the answer, and handed to both `check_drift` and `assert_pushable`. A gate refusal now costs the user one question — the gate's own input — and still nothing else."
  - "**F-3 makes freshness structural.** `assert_fresh` is gone as a `&self` method; `spend(self, now)` consumes the clearance and is the sole mint site for `Pushing`. `SetupOutcome` no longer carries a clearance at all: setup uploads nothing, so the only thing keeping one could do is age."
  - "**F-10 moved every write past the confirmation.** The keyfile, the `config.toml` write-back, the token store and the pairing record are all step 5. A keyfile written before the confirm survived a decline and then refused the re-run, behind a passphrase shown once with no recovery by design."
  - "**F-6 also fixed a deadlock the audit did not name.** The parent's `child.wait()` held the very mutex the watchdog needed in order to kill, so the watchdog could never fire during the call it was supposed to bound. The read moved to its own thread behind `recv_timeout`, and `wait` became a bounded `try_wait` poll."
  - "**F-9 changed the types rather than narrowing the doc.** `env_value`, both injected closure return types and `gh_auth_token`'s buffer are `Zeroizing<String>`. The one boundary this module cannot own — `security(1)`'s output through the shared `anthropic::keychain` worker — is wrapped on the first line `sync::keychain::read_raw` controls, and the doc now names it."

patterns-established:
  - "Guard tests that assert an *absence of a shape*, not just a behaviour: `freshness_is_the_only_exit_from_a_clearance` reads gate.rs's own shipped source and fails if an `assert_fresh`-shaped method returns."
  - "Untrusted remote text is attributed and delimited (`GitHub said: \"…\"` via `{:?}`), not merely sanitized, so it cannot read as the tool's own advice."

requirements-satisfied: [SAFE-01, SAFE-02, REPO-02, REPO-05, D-02, D-04, D-06, UX-03]
---

# Plan 3-08 — Phase 3 security remediation

Nine of the audit's ten findings. F-7 stays carried forward: it is a Phase 4
guard extension over `write.rs`, which plan 4-01 owns and this plan does not
touch.

## The contract Phase 4 must build against

**This is the part plan 4-01 needs.** Three signatures changed in ways that
shape the push path.

### 1. A write verb takes a `Pushing`, not a `PushClearance`

```rust
pub struct Pushing(());                                    // gate.rs, private field

impl PushClearance {
    pub fn spend(self, now: DateTime<Utc>) -> Result<Pushing>;   // consumes, checks MAX_CLEARANCE_AGE
    pub fn checked_at(&self) -> DateTime<Utc>;
}
```

`assert_fresh` **no longer exists**. `MAX_CLEARANCE_AGE` is no longer a
parameter — `spend` applies it, so there is no call that can pass a laxer one.

Every verb in `src/sync/push/` and `src/sync/github/write.rs` that sends a byte
must take a `Pushing` by value:

```rust
pub async fn upload_pack(client: &Client, /* … */, _permit: Pushing) -> Result<()>;
```

and the push entry point mints it immediately before the first byte:

```rust
let facts = gate::fetch_facts(&client, &repo, now).await?;          // note: FetchError, see below
pairing::check_drift(record.as_ref(), &facts, credentials_in_bundle, now)?;
let (clearance, warnings) = gate::assert_pushable(&facts, &repo, credentials_in_bundle, now)?;
let permit = clearance.spend(now)?;                                  // the only way to get one
```

`SetupOutcome` no longer has a `clearance` field, so there is nothing to carry
over from `sync setup` even by accident. Both types are `#[must_use]`.

### 2. `gate::fetch_facts` returns `Result<RepoFacts, gate::FetchError>`

```rust
pub struct FetchError { pub error: AppError, pub token_rejected: bool }
impl From<FetchError> for AppError { /* … */ }   // `?` still works in an AppError context
```

`token_rejected` is true **only** for a 401. Anything that clears a stored token
must branch on it, never on `AppError::Credentials(_)`.

### 3. Clearing a token needs its `TokenSource`

```rust
pub fn clear_source(source: TokenSource, file_path: &Path) -> Result<()>;
pub fn owns_a_store(source: TokenSource) -> bool;   // Keychain | File
pub fn clear_note(source: TokenSource, file_path: &Path) -> String;

pub(crate) fn clear_if_dead(
    err: gate::FetchError,
    source: TokenSource,
    token_file: &Path,
    clear: &dyn Fn(TokenSource, &Path) -> Result<()>,
) -> AppError;
```

`token::clear` is gone. A push that 401s mid-upload routes through
`clear_if_dead` with the source `token::resolve` returned — the same shape
`setup.rs` and `cli.rs` use.

## What each finding was, and what closed it

### F-1 (BLOCKER) — a 401 destroyed a credential it did not come from

Two wrong predicates compounding, with no attacker and in the documented
configuration: pair on macOS (token → Keychain), later export
`AI_USAGEBAR_SYNC_TOKEN` (CI, an `.envrc`, or a 90-day PAT that expired — both
recommended by this project's own docs), run `sync status`. The env token 401s,
`token::clear` deletes the Keychain item *and* the token file, the dead env var
keeps answering, and the 401 text sends the user to mint a third token.

- **Trigger:** `matches!(err, AppError::Credentials(_))` → `err.token_rejected`,
  set at the classifier for `GithubError::Unauthorized` alone.
- **Action:** `token::clear_source` clears only the store that produced the
  value. `clear_if_dead` does not reach the seam at all unless
  `owns_a_store(source)`.
- **Message:** `actionable`'s 401 arm dropped "will be cleared";
  `token::clear_note` says what was done — or, for `Env`/`GhCli`, that nothing
  was and which value to change, because a replacement written anywhere else is
  never reached while the environment answers first.

Tests: `only_a_401_clears_and_it_clears_only_the_store_it_came_from` (the full
source × outcome matrix, including the malformed-token case that used to fire),
`a_401_on_an_environment_token_clears_nothing_and_names_the_variable`,
`a_401_on_a_file_token_clears_that_file_and_says_so`,
`clearing_an_env_or_gh_source_removes_nothing_this_tool_stores` (the inner
guard, independent of the caller), `only_a_401_marks_the_token_as_rejected`.

### F-2 (HIGH) — the gate was decided against a category set the flow replaced

`credentials_in_bundle` was computed before the category prompt and was the only
gate input that prompt could change — and the one deciding D-04's public-repo
carve-out. With default answers: public repo + `categories = ["config"]` took the
carve-out, minted a clearance; the user then added `credentials`; the dry run
enumerated the credential files; setup stored the token, wrote the pairing record
at `private: false` (so SAFE-02 could never fire for it), and printed "paired and
ready to push".

The category prompt is now step 1 and the gate is step 2. One evaluation, of the
answer, handed to both `check_drift` and `assert_pushable`.

The cost is that a gate refusal now reaches `SetupPrompt::categories` —
`every_refusal_stops_before_the_password_step_is_reached` asserts no
`passphrase`, and asserts `reached.is_empty()` for the failures that are *not*
gate decisions (404, 401), which still ask nothing at all. The local
keyfile-exists precondition moved to the very top for the same reason: it is a
fact about this machine, not about the repository.

Tests: `adding_credentials_at_the_prompt_re_decides_the_public_repository` (the
composed path, verbatim), `removing_credentials_at_the_prompt_re_decides_it_too`
(the carve-out still lives, from the other direction).

### F-3 (HIGH, Phase 4 precondition) — `assert_fresh` had zero production callers

Same class as this phase's one shipped defect (`http::actionable` with no call
site) and it survived for the same reason: it had tests, so it read as wired.
Non-`Clone` prevents duplication and a private field prevents forgery; neither
prevents *holding*. See the contract above.

`freshness_is_the_only_exit_from_a_clearance` reads gate.rs's own shipped source
and fails if an `assert_fresh`-shaped method comes back or if `Pushing` gains a
second mint site.

### F-5 (medium) — first contact is narrated

Deleting `sync-pairing.json` makes `check_drift` return `first_contact: true`
and skip the `owner_id`/`repo_id` comparison entirely, and no positive
first-contact line existed anywhere — a silently reset pairing was visually
identical to a first-ever setup. Step 2 now names the repository id, the owner
login and the owner id, and says that a record which disappeared did not remove
itself.

### F-10 (low) — nothing persists before the confirmation

The keyfile was written before the size confirmation, and setup then refused to
re-run while it existed, stranding a user who declined behind a passphrase shown
once. `Keyfile::create` still runs at step 3 (its keys feed the step-4 plan), but
`write_keyfile`, `write_categories`, `store_token` and `pairing::write_to` are
all step 5. `declining_the_size_confirmation_stops_without_pairing` now also
asserts no keyfile, no config change, no stored token — and that a second run
succeeds.

### F-4 · F-6 · F-8 · F-9 (low)

- **F-4** — `message_of` returns `GitHub said: "…"`. `{:?}` escapes any quote the
  sanitizer let through, so the closing delimiter is always the remote's real end
  and 200 attacker-chosen characters cannot read as the tool's own advice.
- **F-6** — the watchdog bounded the process, not the `read_to_string`; a
  credential helper `gh` spawned keeps the inherited stdout pipe open past the
  kill. The read moved to its own thread behind `recv_timeout`. It also fixed a
  deadlock the audit did not name: the parent's `child.wait()` held the mutex the
  watchdog needed to kill, so the watchdog could not fire during the call it was
  meant to bound. `wait` is now a bounded `try_wait` poll.
- **F-8** — `src/sync/github/setup.rs` joins the passphrase-input guard's file
  list. It passes today; the list is what keeps it passing.
- **F-9** — the types changed rather than the doc. Ninth instance of the
  doc-asserts-absent-behaviour class in this milestone, and the third fixed in
  this plan alone (F-1's "will be cleared", F-3's "cannot be satisfied by a stale
  check", F-9's "end to end") — the recurring shape is a sentence describing a
  mechanism that the code declares but never reaches.

## Verification

```
cargo test                  1285 passing, 0 failing, 15 ignored   (baseline 1279 / 0 / 15)
cargo clippy --all-targets -- -D warnings   clean
cargo fmt --check                           clean
make test                                   green (GNOME, KDE, Omarchy JS suites included)
```

Six new tests, no test removed, no `#[ignore]` added. Every test remains
hermetic: mockito for HTTP, injected roots/clock/endpoints/chain, and no test
reaches a real `$HOME`, Keychain, token or socket.

## Not done here

**F-7** — extending the REPO-03 guard to pin body verbs to `write.rs` and to
assert `reqwest::Client` is constructed nowhere else under `src/sync/`. It needs
`write.rs` to exist, and plan 4-01 owns that file. The structural claim holds
today: `Client` exposes only `get_json`, and the guard in `mod.rs` still passes.
