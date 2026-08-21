---
phase: 03-github-auth-and-the-private-repo-gate
plan: 01
subsystem: transport
tags: [github, auth, private-repo-gate, tracer, frozen-seams, hermetic-tests, zero-upload]

requires:
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 01
    provides: "`SyncRoots`/`SyncRoots::at`, `SyncConfig`, `SyncAction`, `sync::cli::run`"
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 07
    provides: "`sync push --dry-run`, `report::build_status`/`render_status`"
  - codebase
    provides: "`vendor::{HTTP_CLIENT_TIMEOUT, MAX_BODY_BYTES, read_body_capped, same_origin_redirect_policy}`, `AppError`, `anthropic::keychain`'s read/write split"
provides:
  - "`src/sync/github/` — seven files, six declared submodules, every wave-2 file already created"
  - "`Endpoints`, `RepoRef`, `Client` (one `GET`, no body-carrying method)"
  - "`GithubError`'s seven frozen variants + `classify` / `from_transport` / `actionable`"
  - "`TokenSource`, `TokenChain` (four frozen fields), `token::resolve`"
  - "`RepoFacts`, `fetch_facts`, `assert_pushable`, `PushClearance` + `assert_fresh` + `MAX_CLEARANCE_AGE`"
  - "`[sync] repo` on `SyncConfig`, validated at config load"
  - "`ai-usagebar sync setup`, and `sync::cli::run_with` — the injected seam every CLI test drives"
affects: [3-02, 3-03, 3-04, 3-07, phase-4-push]

tech-stack:
  added: []
  patterns:
    - "The safety property is a type property, not an ordering property: `Client` has exactly one request method and no way to carry a body, so no reordering of statements can put an upload before the gate."
    - "A source-reading guard test (`include_str!(\"mod.rs\")`) is the cheap enforcement for 'this file must never gain method X'. Verified to actually fail on an injected `.post(`, and asserted non-vacuous so an empty split cannot pass it silently."
    - "`run` / `run_with`: the production wrapper resolves `$HOME` and is never tested; the seam takes config, roots, both hosts, the token chain, and the clock."

key-files:
  created:
    - src/sync/github/mod.rs
    - src/sync/github/http.rs
    - src/sync/github/token.rs
    - src/sync/github/keychain.rs
    - src/sync/github/gate.rs
    - src/sync/github/pairing.rs
    - src/sync/github/setup.rs
  modified:
    - src/sync/mod.rs
    - src/config.rs
    - src/sync/cli.rs
    - src/widget/cli.rs
    - src/bin/ai-usagebar.rs
    - src/sync/plan.rs
    - src/sync/report.rs

key-decisions:
  - "`get_json` returns `Vec<u8>`, not `bytes::Bytes`. `bytes` reaches the build only as a transitive `reqwest` dependency, so `use bytes::…` does not resolve; declaring it would break the zero-new-crates constraint and `cargo machete`. `vendor::read_body_capped` already returns `Vec<u8>`."
  - "`assert_pushable` returns `(PushClearance, Vec<String>)` and takes `credentials_in_bundle`. The tuple exists at the tracer so plan 3-04 adds warning cases without touching `setup.rs`, which it does not own; the parameter exists so the gate and 3-04's `check_drift` cannot contradict each other about D-04's credentials-off carve-out."
  - "`PushClearance` has a private field, no public constructor, no `Clone`/`Copy`, and `assert_pushable` is its sole constructor. A clearance that can be stashed and duplicated *is* a cached check."
  - "`assert_fresh` + `MAX_CLEARANCE_AGE = 30s` turn D-04's word 'immediately' into arithmetic Phase 4 can call. A clearance dated in the future also fails: that is a clock that moved."
  - "`Endpoints` carries `uploads_base` from day one even though Phase 3 never reads it — `uploads.github.com` is a separate host, and a Phase 4 that hard-codes it at the call site has an untestable upload path."
  - "The runtime is built inside `sync::cli::setup`, not moved into `src/bin/ai-usagebar.rs`. `Command::Sync` stays dispatched before the runtime so `status` and `push --dry-run` keep paying nothing for a reactor they never use. The bin's comment claiming sync makes no network call was true at plan 2-01 and is now corrected in place."
  - "`classify` implements only 401 / 404 / catch-all here, per plan; `headers` and `now` are bound to `let _` with a comment naming plan 3-03 as their consumer, so the frozen signature does not warn."
  - "`RepoRef::parse` is a strict allow-list (ASCII alphanumeric plus `-`, `_`, `.`), not an escape, and rejects `.`/`..` segments. The value is interpolated into a URL path (T-3-03), and it is validated at `Config::validate` so a malformed name fails at load rather than after a network round trip."
  - "No token field on `SyncConfig`. A `Contents: write` token is a different class of secret from the read-only provider API keys `config.toml` may hold inline; plan 3-02 owns its storage."

patterns-established:
  - "Every file a later plan owns is created by the tracer with its doc comment and frozen signature, so no two plans in a wave ever edit the same file."
  - "A guard test that asserts an absence must also assert it is looking at something — otherwise a refactor that moves the code turns the guard into a no-op that still reports green."

requirements-completed: [REPO-01, REPO-03, REPO-04, SAFE-01, UX-03]

coverage:
  - id: REPO-03
    description: "`sync setup` against a mock 404, and with `[sync] repo` unset, each prints the exact `gh repo create … --private` line and exits non-zero; the tool never creates a repository"
    verification:
      - kind: unit
        ref: "src/sync/github/gate.rs#a_404_names_both_causes_and_prints_the_create_command"
        status: pass
      - kind: unit
        ref: "src/sync/github/setup.rs#an_unset_repo_names_the_config_key_and_the_create_command"
        status: pass
      - kind: unit
        ref: "src/sync/cli.rs#setup_without_a_configured_repo_exits_non_zero"
        status: pass
    human_judgment: false
  - id: SAFE-01
    description: "A repository reporting `private: false` refuses before anything else happens, and the credential-bearing arm names rotation"
    verification:
      - kind: unit
        ref: "src/sync/github/gate.rs#a_public_repository_refuses_and_says_to_rotate_when_credentials_are_in_the_bundle"
        status: pass
      - kind: unit
        ref: "src/sync/github/gate.rs#a_public_repository_refuses_distinctly_when_credentials_are_not_in_the_bundle"
        status: pass
      - kind: unit
        ref: "src/sync/github/setup.rs#a_public_repository_is_refused_before_anything_else_happens"
        status: pass
    human_judgment: false
  - id: D-05
    description: "`Client` exposes no method that can carry a request body, proven by a test rather than by review"
    verification:
      - kind: unit
        ref: "src/sync/github/mod.rs#the_client_exposes_no_method_that_can_carry_a_request_body"
        status: pass
      - kind: manual
        ref: "negative control — injecting `self.http.post(\"x\")` into `get_json` makes the guard fail with the loud message; reverted"
        status: pass
    human_judgment: false
  - id: D-04
    description: "A `PushClearance` can be asked whether it is still fresh, and cannot be forged, cloned, or cached"
    verification:
      - kind: unit
        ref: "src/sync/github/gate.rs#a_clearance_goes_stale_and_a_clearance_from_the_future_is_refused"
        status: pass
      - kind: unit
        ref: "src/sync/github/gate.rs#a_private_repository_clears_with_no_warnings_and_the_injected_clock"
        status: pass
    human_judgment: false
  - id: REPO-04
    description: "Token resolution walks D-02's order and reports its source; an existing env token needs no new secret"
    verification:
      - kind: unit
        ref: "src/sync/github/token.rs#the_environment_wins_and_reports_itself_as_the_source"
        status: pass
      - kind: unit
        ref: "src/sync/github/token.rs#a_token_file_answers_when_the_environment_is_unset"
        status: pass
      - kind: unit
        ref: "src/sync/github/token.rs#an_exhausted_chain_names_every_way_to_supply_a_token"
        status: pass
    human_judgment: false
  - id: T-3-01
    description: "The token's value never appears in output at any verbosity; only its source does"
    verification:
      - kind: unit
        ref: "src/sync/cli.rs#the_success_line_reports_the_token_source_and_never_the_token"
        status: pass
      - kind: unit
        ref: "src/sync/github/mod.rs#the_clients_debug_reports_the_token_source_and_never_the_token"
        status: pass
      - kind: unit
        ref: "src/sync/github/token.rs#no_rendering_of_the_resolved_token_type_contains_the_token"
        status: pass
    human_judgment: false
  - id: T-3-03
    description: "A config `repo` value cannot escape the URL path it is interpolated into, and fails at config load"
    verification:
      - kind: unit
        ref: "src/sync/github/mod.rs#everything_that_is_not_one_owner_slash_name_pair_is_refused_by_shape"
        status: pass
      - kind: unit
        ref: "src/config.rs#a_malformed_sync_repo_fails_at_load_naming_the_expected_shape"
        status: pass
    human_judgment: false
  - id: hermeticity
    description: "Every CLI test drives `run_with` with injected values; none reads a real `$HOME`, so the AUR `check()` cannot fail on an installer's config"
    verification:
      - kind: unit
        ref: "src/sync/cli.rs#setup_against_a_private_repository_exits_zero"
        status: pass
      - kind: manual
        ref: "grep — `TokenChain::production` / `Config::load` / `SyncRoots::resolve` / `std::env::var` appear only in production wrappers, never under `#[cfg(test)]`"
        status: pass
    human_judgment: false
  - id: UX-03
    description: "First-time setup guided end to end"
    verification:
      - kind: unit
        ref: "src/sync/cli.rs#setup_against_a_private_repository_exits_zero"
        status: partial
    notes: "Foundation only. `sync setup` runs end to end and every failure names its fix, but the *guided* flow — prompts, token capture, `config.toml` write-back — is plan 3-07's, which owns `setup.rs` outright in a later wave."
    human_judgment: false

duration: 55min
completed: 2026-08-19
status: complete
---

# Phase 3 / Plan 01: The GitHub Tracer and the Private-Repo Gate

**`ai-usagebar sync setup` runs end to end — config → token → one `GET` → the
private assertion → a printed pairing — against `mockito`, and the crate contains
no method that could have sent a byte.** That is the phase's defining property,
and it is enforced by a test that was verified to actually fire, not by a review
convention.

## Task Commits

1. **`de65cbf`** — the module tree, the frozen seams, and `[sync] repo` (task 1)
2. **`7d9a40e`** — `ai-usagebar sync setup` and the `run_with` seam (task 2)

## The frozen signatures — copy these, do not re-derive them

Four wave-2 plans build against these in parallel worktrees. Everything below is
verbatim from the merged code.

### `src/sync/github/mod.rs`

```rust
pub mod gate;
pub mod http;
#[cfg(target_os = "macos")]
pub mod keychain;
pub mod pairing;
pub mod setup;
pub mod token;

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub api_base: String,      // Default: "https://api.github.com"
    pub uploads_base: String,  // Default: "https://uploads.github.com" — unread in Phase 3
}
impl Default for Endpoints { /* the two real hosts */ }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}
impl RepoRef {
    pub fn parse(raw: &str) -> Result<RepoRef>;
}
impl std::fmt::Display for RepoRef;  // renders "owner/name"

pub struct Client { /* private: http, endpoints, token, source */ }
impl std::fmt::Debug for Client;     // hand-written: endpoints + token *source* only

impl Client {
    pub fn new(
        endpoints: &Endpoints,
        token: zeroize::Zeroizing<String>,
        source: token::TokenSource,
    ) -> Result<Self>;

    pub fn source(&self) -> token::TokenSource;
    pub fn endpoints(&self) -> &Endpoints;

    pub async fn get_json(
        &self,
        path: &str,
    ) -> Result<(reqwest::StatusCode, reqwest::header::HeaderMap, Vec<u8>)>;
}
```

`Client::new` takes `&Endpoints` (it clones internally) rather than by value —
`setup.rs` holds a `&Endpoints` it does not own.

**There is no other request method, and there must not be.** `get_json` is the
single outbound call site in the phase; it sends `Authorization: Bearer …`
(marked sensitive), `Accept: application/vnd.github+json`,
`X-GitHub-Api-Version: 2022-11-28`, and `User-Agent: ai-usagebar/<version>`, and
reads the body through `vendor::read_body_capped` at `vendor::MAX_BODY_BYTES`.

### `src/sync/github/http.rs`

```rust
#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    Unauthorized { message: String },
    RateLimited { retry_after: std::time::Duration, message: String },
    Forbidden { message: String },
    NotFound { message: String },
    Conflict { message: String },
    Unexpected { status: u16, message: String },
    Transport { message: String },
}

impl From<GithubError> for AppError;  // Credentials | Http{429,403,404,409,status} | Transport

pub fn classify(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
    now: chrono::DateTime<chrono::Utc>,
) -> GithubError;

pub fn from_transport(e: &reqwest::Error) -> GithubError;
pub fn actionable(err: &GithubError) -> String;
```

**Seven variants, closed for Phase 3.** Plan 3-03 fills the 403/429 rate-limit
arithmetic and the per-variant actionable text behind these three unchanged
signatures; it adds no variant. `classify` currently handles 401, 404, and a
catch-all; `headers` and `now` are bound to `let _` with a comment naming 3-03 as
their consumer. A private helper `message_of(body)` extracts GitHub's own
`{"message": …}` and truncates at 200 chars — 3-03 can keep or replace it, it is
not part of the frozen surface.

### `src/sync/github/token.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource { Env, Keychain, File, GhCli }

impl TokenSource {
    pub fn label(&self) -> &'static str;  // "env" | "Keychain" | "file" | "gh"
}

#[derive(Default)]
pub struct TokenChain {
    pub env_value: Option<String>,
    pub keychain: Option<Box<dyn Fn() -> Result<Option<String>>>>,
    pub file_path: Option<PathBuf>,
    pub gh: Option<Box<dyn Fn() -> Result<Option<String>>>>,
}

impl TokenChain {
    pub fn production() -> TokenChain;  // env var + <config_dir>/sync-token; keychain/gh are None
}

pub fn resolve(chain: &TokenChain)
    -> Result<(zeroize::Zeroizing<String>, TokenSource)>;
```

`TokenChain` derives `Default`, which is how every test builds a partial chain
(`TokenChain { env_value: …, ..Default::default() }`). Both closure fields carry
`#[allow(clippy::type_complexity)]`. `resolve` trims, skips empty sources, and
propagates a non-`NotFound` I/O error from the token file rather than silently
falling through to `gh` — an unreadable-but-present token file is a
misconfiguration the user must see.

**Plan 3-02:** fill `keychain` and `gh` in `production()`, and add `store` /
`clear`. Do not add a field, do not add a `TokenSource` variant, do not change
`resolve`'s signature. `src/sync/github/keychain.rs` exists with its doc comment
and is yours; it must wrap `anthropic::keychain`'s existing
`read_raw_service` / `write_raw_service` / `delete_raw_service` (already
parameterized by service name) rather than reimplement the
read-via-`security(1)` / write-via-Security.framework split.

### `src/sync/github/gate.rs`

```rust
pub const MAX_CLEARANCE_AGE: std::time::Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFacts {
    pub id: u64,
    pub private: bool,
    pub visibility: String,
    pub owner_login: String,
    pub owner_id: u64,
    pub archived: bool,
    pub fork: bool,
    pub admin_permission: bool,   // permissions.admin, false when the object is absent
}

pub async fn fetch_facts(
    client: &Client,
    repo: &RepoRef,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<RepoFacts>;

#[derive(Debug)]
pub struct PushClearance { /* private: checked_at */ }

impl PushClearance {
    pub fn checked_at(&self) -> chrono::DateTime<chrono::Utc>;
    pub fn assert_fresh(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        max_age: std::time::Duration,
    ) -> Result<()>;
}

pub fn assert_pushable(
    facts: &RepoFacts,
    repo: &RepoRef,
    credentials_in_bundle: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(PushClearance, Vec<String>)>;
```

`RepoFacts` is `PartialEq` so a test can assert the whole struct at once. It is
mapped from a private `RawRepo`/`RawOwner`/`RawPermissions` deserialize triple,
which is why the flat shape and the nested wire shape can differ without serde
gymnastics — plan 3-04 can extend the raw structs freely.

**Plan 3-04:** `assert_pushable`'s signature is byte-identical to the above and
must stay so. The tracer implements only the `private` refusal, in both its arms
(a public repository with credentials in the bundle names rotation and the
un-publishable bytes; without them it refuses with a distinct message about chat
indexes and config, which 3-04 turns into the warn-and-proceed arm). `visibility`,
`owner_login`/`owner_id`, `archived`, and `fork` are yours.

### The Phase 4 contract — this is the whole of D-04

`PushClearance` being unforgeable and non-`Clone` stops duplication but says
nothing about age. So, explicitly, and this is not optional:

> **Phase 4's upload entry point takes a `PushClearance` by value, calls
> `assert_fresh(now, MAX_CLEARANCE_AGE)` before the first byte, and re-runs
> `fetch_facts` + `assert_pushable` *inside* the push call — it never carries a
> clearance obtained at `sync setup`.**

A repository can be flipped public from the web UI between setup and push. A
clearance minted at setup and presented at push time is precisely the cached
check D-04 forbids; taking it by value and re-running the gate is what makes the
check structurally prior to the first byte rather than merely earlier in a
narrative.

### `src/sync/github/setup.rs`

```rust
#[derive(Debug)]
pub struct SetupOutcome {
    pub repo: RepoRef,
    pub token_source: TokenSource,
    pub visibility: String,
    pub warnings: Vec<String>,
    pub clearance: PushClearance,
}

pub async fn run(
    cfg: &SyncConfig,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &token::TokenChain,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<SetupOutcome>;
```

**This one is not frozen.** Plan 3-07 owns the file outright in a later wave and
adds a prompt seam; its only caller is `sync/cli.rs`, which 3-07 also owns.
Nothing in wave 2 calls it. `roots` is currently `let _`-bound with a comment: it
is in the signature so the pairing record (3-04) and the `config.toml` write-back
(3-07) have an injected directory instead of reaching for a real `$HOME`.

### `src/sync/cli.rs`

```rust
pub fn run(action: &SyncAction) -> i32;          // production wrapper, never tested

pub fn run_with(
    action: &SyncAction,
    cfg: &Config,                                 // the whole Config, not just `.sync`
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &TokenChain,
    now: chrono::DateTime<chrono::Utc>,
) -> i32;
```

`run_with` takes `&Config` because `status` and `push --dry-run` need the whole
thing; the `Setup` arm passes `&cfg.sync` down to `github::setup::run`. A private
`render_setup(&SetupOutcome) -> String` is pure so the no-token-in-output
assertion is on a value rather than on captured stdout. `status` and `dry_run`
now take `(cfg, roots, now)` too, so nothing under `run_with` reads the clock.

### `[sync] repo`

```toml
[sync]
repo = "owner/name"
```

`pub repo: Option<String>` on `SyncConfig`. No default, nothing derived (D-01),
and no token field anywhere in `config.toml`. `Config::validate` runs
`RepoRef::parse` on it behind an `if let Some(..)`, so a malformed value fails at
load with `[sync] repo — … expected exactly "owner/name"`.

## What `sync setup` prints

```
repo:       octocat/ai-usagebar-sync
visibility: private
token:      present (env)
Nothing was uploaded — this command only verifies the pairing.
```

Warnings, when plan 3-04 starts emitting them, render as `warning:    …` lines
between the token line and the closing note. `setup.rs` already destructures
`(clearance, warnings)` and `render_setup` already iterates them, so 3-04 adds
warning cases without this file changing.

## Deviations from the plan

- **The `keychain` cfg-gate lives in `mod.rs`**, as `#[cfg(target_os = "macos")]
  pub mod keychain;`, rather than as an inner `#![cfg(…)]` in the file. Both were
  described in the plan; gating at the declaration keeps the file itself free of
  attribute noise and is what `anthropic::keychain` already does.
- **The CLI's `setup` test is a plain `#[test]` with `mockito::Server::new()`**,
  not a `#[tokio::test]`. `run_with` builds its own runtime, so a test with a
  reactor already installed on its thread would panic on nesting — and
  `rt-multi-thread` is not an enabled tokio feature in this crate, so the
  `flavor = "multi_thread"` escape does not exist. The blocking mock server is
  the correct shape for testing a synchronous entry point.
- **`Client::new` takes `&Endpoints`.** The plan wrote `Client::new(endpoints,
  token, source)` without specifying ownership; a reference is what the one
  caller has.
- **The guard test carries a non-vacuity assertion** (`production.contains("pub
  async fn get_json")`) beyond what the plan asked for. Without it, a refactor
  that moved `Client` out of `mod.rs` would turn the guard into a no-op that
  still reports green.

## Verification

```
cargo test --lib -- sync:: config::tests widget::cli   # 308 passed, 0 failed
cargo clippy --all-targets -- -D warnings   # clean
cargo fmt --check                           # clean
```

- `grep -rn 'Utc::now' src/sync/github/` — one hit, in a doc comment explaining
  why it is never called. Every time-dependent function takes `now`.
- `TokenChain::production` / `Config::load` / `SyncRoots::resolve` /
  `std::env::var` appear only in the two production wrappers (`sync::cli::run`,
  `TokenChain::production`); none under `#[cfg(test)]`. No test opens a socket to
  anything but a `mockito` base or a dead loopback port, spawns a process, or
  resolves a real home directory.
- `Cargo.toml` is unchanged. Zero new crates.
- The guard test's negative control: injecting `self.http.post("x")` into
  `get_json` makes it fail with the loud D-05 message; reverted.

## Not done here, deliberately

- **CAL-1** (does a private-repo release asset honour `Range:` after the 302 to
  signed storage?) is assigned to plan 3-06 by the phase's source audit, and
  `tests/live.rs` is 3-06's file. The HTTP plumbing it needs now exists, so 3-06
  can write the `#[ignore]`d probe against `Client` directly.
- `keychain.rs` and `pairing.rs` are doc comments only — 3-02 and 3-04 own them.
- The `Administration`-permission warning (D-03) is 3-04's: `RepoFacts` carries
  `admin_permission` and nothing yet reads it.
