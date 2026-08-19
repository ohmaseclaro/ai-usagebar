---
phase: 03-github-auth-and-the-private-repo-gate
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/config.rs
  - src/sync/mod.rs
  - src/sync/cli.rs
  - src/widget/cli.rs
  - src/bin/ai-usagebar.rs
  - src/sync/github/mod.rs
  - src/sync/github/http.rs
  - src/sync/github/token.rs
  - src/sync/github/keychain.rs
  - src/sync/github/gate.rs
  - src/sync/github/pairing.rs
  - src/sync/github/setup.rs
autonomous: true
requirements: [REPO-01, REPO-03, REPO-04, SAFE-01, UX-03]
must_haves:
  truths:
    - "`ai-usagebar sync setup` against a mock private repo prints the repo, its visibility, and the token's source — and exits zero."
    - "The same command against a mock 404 prints the exact `gh repo create --private` line and exits non-zero (D-01)."
    - "A repo reporting `private: false` refuses before anything else happens (D-04, SAFE-01)."
    - "The token's value never appears in output at any verbosity; only its source does (D-02)."
    - "Nothing in the phase opens a socket to a host that is not an injected `Endpoints` base (D-05)."
  artifacts:
    - src/sync/github/mod.rs declaring the six submodules, `Endpoints`, `RepoRef`, and the client
    - src/sync/github/http.rs with the frozen `GithubError` enum every later plan matches on
    - "`[sync] repo` on `SyncConfig` in src/config.rs"
    - "`SyncAction::Setup` in src/widget/cli.rs, dispatched from `sync::cli::run_with`"
  key_links:
    - "src/sync/mod.rs declares `pub mod github;` — without it nothing in the phase compiles"
    - "github/mod.rs declares all six submodules, so each wave-2 plan owns exactly one or two files and never touches mod.rs"
    - "`GithubError`, `TokenChain`, `Client::get_json`, and `assert_pushable` are frozen here; wave-2 plans fill bodies, never signatures"
    - "`assert_pushable` returns its warnings in the tuple, so plan 3-04 adds warning cases without touching `setup.rs`, which it does not own"
    - "`Endpoints` carries `uploads_base` even though Phase 3 never uploads — hard-coding it would make Phase 4's upload path untestable"
    - "`run_with` is the injected-seam entry every CLI test drives; `run` is the thin wrapper that resolves real paths and is never called from a test"
---

<objective>
The tracer for Phase 3: one thin path from `config.toml` through token resolution, an HTTP
request, and the visibility gate, out to a CLI surface — wired end to end against
`mockito`, production quality, zero bytes uploaded.

It also fixes every module boundary and every cross-file signature for the rest of the
phase. Wave-2 plans fill files this plan creates; none of them edits another's.

Implements **D-01** (the repo is named, never guessed), **D-02** (resolution order, and
the token is reported by source only), **D-04** (the gate is a call, not a cached fact),
**D-05** (no upload anywhere in the phase).

Purpose: prove the whole stack on one request before four plans build out from it.
Output: `src/sync/github/` module tree, `[sync] repo`, `ai-usagebar sync setup`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-CONTEXT.md
@.planning/research/github-transport.md
@CLAUDE.md
@src/vendor.rs
@src/error.rs
@src/anthropic/fetch.rs
@src/sync/mod.rs
</context>

<source_audit>
Phase-wide coverage audit. Every source item maps to a plan.

| Source | Item | Covered by |
|---|---|---|
| GOAL | Paired with a private repo, token structurally unable to create one, refuses anything not verifiably private, zero bytes uploaded | 3-01 … 3-07 |
| REQ | REPO-01 token scoped to the single sync repo, no create/administer permission | 3-01, 3-04, 3-05 |
| REQ | REPO-02 token stored as Keychain item / mode-0600 file, never a tracked file | 3-02 |
| REQ | REPO-03 the tool never creates a repository; a missing repo is an actionable error | 3-01, 3-04, 3-05 |
| REQ | REPO-04 an existing env / `gh` token is reused so setup needs no new secret | 3-01, 3-02 |
| REQ | REPO-05 network, auth, and rate-limit failures are actionable and exit non-zero | 3-03 |
| REQ | SAFE-01 a credential-bearing push is refused unless the repo is verified private, checked immediately before every push | 3-01, 3-04 |
| REQ | SAFE-02 a previously-private repo found public aborts and names the credentials to rotate | 3-04 |
| REQ | UX-03 first-time setup guided end to end | 3-07 |
| REQ | REPO-06 bulk data as few large objects, inside the content-creation limits | **foundation only** — `Endpoints.uploads_base` (3-01) and the content-creation-aware backoff (3-03). The observable requirement is an upload, which D-05 forbids here; it lands in Phase 4, exactly as ROADMAP traceability assigns it. |
| REQ | REPO-07 snapshot pointer published with a CAS precondition | **foundation only** — the 409 arm of `GithubError` is frozen in 3-01 so Phase 4 adds no variant. Observable in Phase 4 per ROADMAP traceability. |
| RESEARCH | REST over existing reqwest 0.12 + rustls, zero new crates | 3-01 |
| RESEARCH | `Endpoints { api_base, uploads_base }`, both pointed at one mockito server | 3-01 |
| RESEARCH | The full assertion set: private, visibility, owner.login, owner.id, archived, fork | 3-04 |
| RESEARCH | 403/429 backoff: `retry-after` → `x-ratelimit-reset` → jittered exponential, min 60 s | 3-03 |
| RESEARCH | macOS write via Security.framework, read via `security(1)`; mode-0600 file elsewhere | 3-02 |
| RESEARCH | CAL-1 — `Range:` on a private-repo release asset | 3-06 |
| CONTEXT | D-01 repo named explicitly, missing value prints the create command | 3-01 |
| CONTEXT | D-02 token order env → Keychain → 0600 file → `gh auth token` | 3-01, 3-02 |
| CONTEXT | D-03 fine-grained PAT `Contents: read/write` + `Metadata: read`; warn, never fail, on excess | 3-04, 3-05 |
| CONTEXT | D-04 the private check re-runs immediately before every push, never cached | 3-04 |
| CONTEXT | D-05 zero bytes leave the machine in this phase | 3-01 (whole-phase invariant) |
| CONTEXT | D-06 every failure path names the fix, and exits non-zero | 3-03 |

**Naming reconciliation:** ROADMAP §Phase 3 calls the guided command `sync init`; `3-CONTEXT.md`
D-05 calls it `sync setup`. CONTEXT is the locked artifact, so the command ships as
`sync setup`. Recorded here so the divergence is deliberate, not a drift.
</source_audit>

<tasks>

<task type="tracer" tdd="true">
  <name>Task 1: The module tree, the frozen seams, and one real request end to end</name>
  <files>src/sync/mod.rs, src/sync/github/mod.rs, src/sync/github/http.rs, src/sync/github/token.rs, src/sync/github/keychain.rs, src/sync/github/gate.rs, src/sync/github/pairing.rs, src/sync/github/setup.rs</files>
  <behavior>
    - A `GET` against a mockito server returning `{"id":1,"private":true,"visibility":"private","owner":{"login":"o","id":7},"archived":false,"fork":false}` yields `RepoFacts` with those five fields populated.
    - The same endpoint returning `private: false` yields a refusal error, not a `RepoFacts`.
    - A 404 yields the distinct "repo missing or token not scoped to it" error carrying the create command.
    - `RepoRef::parse("owner/name")` succeeds; `""`, `"name"`, `"a/b/c"`, and a value with whitespace each fail with a message naming the expected shape.
    - `token::resolve` over a `TokenChain` whose `env_value` is set returns that value paired with `TokenSource::Env`; with `env_value` unset and `file_path` pointing at a temp file it returns the file's contents paired with `TokenSource::File`.
    - `format!("{:?}", …)` of the resolved token type does not contain the token's characters.
  </behavior>
  <action>
Add `pub mod github;` to `src/sync/mod.rs` and create `src/sync/github/` with seven files.
Every file is created here so that each wave-2 plan owns whole files and no two plans ever
edit the same one. Files this plan does not fill carry their frozen public signature plus a
working minimum; the gaps are functional, never architectural.

**`github/mod.rs`** — declares five unconditional submodules (`gate`, `http`, `pairing`,
`setup`, `token`) plus `keychain` gated on `#[cfg(target_os = "macos")]` — six files, five of
them compiled on every target. It holds:

`Endpoints { pub api_base: String, pub uploads_base: String }` with a `Default` of the two
real GitHub hosts (`https://api.github.com` and `https://uploads.github.com`). Both fields
exist from day one: uploads is a separate host, and Phase 4 cannot test its upload path
against a mock server if that host is baked into a string literal at the call site. Nothing
in Phase 3 reads `uploads_base`; document that in the field's doc comment.

`RepoRef { pub owner: String, pub name: String }` with `parse(&str) -> Result<RepoRef>`
accepting exactly one `owner/name` pair — reject empty, missing or extra separators, and any
segment carrying whitespace, a path separator, or a control character, since the value is
interpolated into a URL path. `Display` renders it back as `owner/name`.

`Client { http: reqwest::Client, endpoints: Endpoints, token: Zeroizing<String>, source: TokenSource }`
built by `Client::new(endpoints, token, source) -> Result<Client>`. Its reqwest client reuses
`crate::vendor::HTTP_CLIENT_TIMEOUT` and `crate::vendor::same_origin_redirect_policy()` — the
same-origin policy is what stops a bearer token following a cross-host redirect. Give `Client`
a hand-written `Debug` that prints the endpoints and the token *source* and nothing else; the
derived one would print the token.

`Client::get_json(&self, path: &str) -> Result<(reqwest::StatusCode, reqwest::header::HeaderMap, Vec<u8>)>`
is the single outbound call site for the whole phase. It sends `Authorization: Bearer …`,
`Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`, and a
`User-Agent` of `ai-usagebar/` plus `env!("CARGO_PKG_VERSION")` (GitHub rejects a request
without one). It reads the body through `crate::vendor::read_body_capped` with
`crate::vendor::MAX_BODY_BYTES` — every response in this phase is a few kilobytes of JSON.
The body type is `Vec<u8>` because that is exactly what `read_body_capped` returns. Do **not**
reach for `bytes::Bytes`: it is a transitive dependency of `reqwest`, not a declared one, so
`use bytes::…` will not resolve, and declaring it would both break the phase's zero-new-crates
constraint and be flagged by `cargo machete`.

There is no `post`, `put`, or `patch` method on `Client` in this phase, and no code path
that hands it a request body. That absence is D-05: the type itself cannot upload. Lock it
down with a test — `Client`'s inherent impl exposes exactly one request method, and a
compile-time guard is worth more than a review convention. The cheap version that actually
catches a regression: a `#[test]` reading this module's own source via
`include_str!("mod.rs")` and asserting the impl block contains no `.post(`, `.put(`,
`.patch(`, `.body(`, or `.multipart(` outside a string literal. Keep the check crude and the
message loud; its job is to fail when someone adds an upload here rather than in Phase 4's
own module.

**`github/http.rs`** — the failure taxonomy's *type*, frozen here so no wave-2 plan changes a
variant another plan matches on. Define `pub enum GithubError` with exactly these variants and
never add one in this phase: `Unauthorized { message: String }` (401), `RateLimited { retry_after: Duration, message: String }` (403 or 429 that carries limit
headers), `Forbidden { message: String }` (403 without them — a missing permission),
`NotFound { message: String }`, `Conflict { message: String }` (the 409 Phase 4's
compare-and-swap pointer write will need; unreached here, present so Phase 4 adds no variant),
`Unexpected { status: u16, message: String }`, and `Transport { message: String }`. Implement
`std::error::Error` via `thiserror`, and `From<GithubError> for AppError` mapping onto the
existing `AppError::Http` / `Transport` / `Credentials` arms.

Also define three further signatures, frozen here and filled by plan 3-03:
`pub fn classify(status: StatusCode, headers: &HeaderMap, body: &[u8], now: DateTime<Utc>) -> GithubError`,
`pub fn from_transport(e: &reqwest::Error) -> GithubError`, and
`pub fn actionable(err: &GithubError) -> String`. Implement `classify` here for the statuses
this tracer exercises — 401, 404, and a catch-all — `from_transport` as the plain
`Transport` mapping, and `actionable` as the error's own display text. Plan 3-03 fills in the
rate-limit arithmetic and the message text that names each fix. `now` is a parameter, never
`Utc::now()` inside, mirroring `antigravity::parse_cache_at`.

`Client::get_json` maps any `reqwest` failure through `from_transport`, so the transport arm
has one call site and plan 3-03 can give it actionable text without touching this file.

**`github/token.rs`** — `pub enum TokenSource { Env, Keychain, File, GhCli }` with a
`label()` returning `"env"`, `"Keychain"`, `"file"`, `"gh"`, and
`pub struct TokenChain { pub env_value: Option<String>, pub keychain: Option<Box<dyn Fn() -> Result<Option<String>>>>, pub file_path: Option<PathBuf>, pub gh: Option<Box<dyn Fn() -> Result<Option<String>>>> }`.
All four fields exist now and their types do not change again: the two closures are the
injection seam that keeps every test off the real Keychain and out of a subprocess.

`pub fn resolve(chain: &TokenChain) -> Result<(Zeroizing<String>, TokenSource)>` walks the four
in D-02's order and returns the first non-empty value with its source, trimming trailing
newlines. Fill `env_value` and `file_path` completely here — reading a file and trimming a
string needs no platform code. Leave `keychain` and `gh` as the `None` that
`TokenChain::production()` currently supplies; plan 3-02 supplies both closures and the write
path. When every source is empty, error with the message that names how to supply one.

`TokenChain::production() -> TokenChain` reads `AI_USAGEBAR_SYNC_TOKEN` into `env_value` and
sets `file_path` to the sync-token file beside `config.toml`. It is the only function here
that touches the environment or a real path, and no test calls it.

The token is `Zeroizing<String>` throughout (`zeroize` is already a dependency). No type in
this module derives `Debug` while holding the value.

**`github/keychain.rs`** — created, `#![cfg(target_os = "macos")]`-gated, holding only the
module doc that states the read-via-`security(1)` / write-via-Security.framework split and
points at `src/anthropic/keychain.rs` as the convention to follow. Plan 3-02 fills it. An
empty macOS-only module compiles clean on Linux and on macOS.

**`github/gate.rs`** — `pub struct RepoFacts { pub id: u64, pub private: bool, pub visibility: String, pub owner_login: String, pub owner_id: u64, pub archived: bool, pub fork: bool, pub admin_permission: bool }`
deserialized from `GET /repos/{owner}/{name}` (`admin_permission` from `permissions.admin`,
defaulting to `false` when the object is absent). Add
`pub async fn fetch_facts(client: &Client, repo: &RepoRef, now: DateTime<Utc>) -> Result<RepoFacts>`
performing that request through `Client::get_json`, mapping a non-2xx through
`http::classify`, and turning a `NotFound` into the D-01 message: the repo is missing or the
token is not scoped to it — GitHub returns the same 404 for both and we must say both — plus
the literal `gh repo create <owner>/<name> --private` line with the configured owner and name
substituted.

Add, with this **exact** frozen signature:

`pub fn assert_pushable(facts: &RepoFacts, repo: &RepoRef, credentials_in_bundle: bool, now: DateTime<Utc>) -> Result<(PushClearance, Vec<String>)>`

Three things about it are decided here and are not plan 3-04's to revisit.

*It returns its warnings in the tuple.* Plan 3-04 adds warning cases to this function, and
3-04's worktree contains this plan's `setup.rs` — a file 3-04 does not own. If warnings
arrived by a signature change, 3-04's branch would not compile. So the tuple exists from the
tracer, and `setup.rs` here already destructures it as `let (clearance, warnings) = …` and
renders `warnings`, which is empty at the tracer. That is the whole reason for a `Vec` nobody
fills yet.

*It takes `credentials_in_bundle`.* This is D-04 read exactly as written: a public repository
aborts **when credentials are in the bundle**, and is allowed-with-a-warning when the
credentials category is off. Without this parameter the assertion and plan 3-04's
`check_drift` contradict each other — one hard-refuses on `private == false` knowing nothing
about the bundle, the other carves out the credentials-off case — and because `setup.rs` calls
`check_drift` and then `assert_pushable`, the second would silently kill the first's carve-out.
One function decides, and it is this one. In this tracer, `false` still refuses a public
repository (with a distinct message) rather than warning, because the tracer has no bundle to
inspect; plan 3-04 implements both arms.

*It is the sole constructor of `PushClearance { checked_at: DateTime<Utc> }`*, whose field is
**private**, with no public constructor and no `Clone` or `Copy` derive. A clearance that can
be stashed and duplicated is a cached check, which is what D-04 forbids.

Non-`Clone` stops forging and duplication but says nothing about age, and D-04's word is
"immediately". Add `pub fn assert_fresh(&self, now: DateTime<Utc>, max_age: Duration) -> Result<()>`
and a `pub const MAX_CLEARANCE_AGE: Duration` of a small number of seconds. It is three lines
while the type is still cheap to change, and it turns "immediately" from prose in a plan into
something Phase 4's upload entry point can call. Say so in the summary: Phase 4 takes a
`PushClearance` by value **and** calls `assert_fresh` before the first byte.

**`github/pairing.rs`** — created with its module doc stating what the record is for and its
mode-0600 requirement; plan 3-04 fills it.

**`github/setup.rs`** — `pub async fn run(cfg: &SyncConfig, roots: &SyncRoots, endpoints: &Endpoints, chain: &TokenChain, now: DateTime<Utc>) -> Result<SetupOutcome>`
performing, in order: read `cfg.repo` (absent ⇒ the D-01 error naming the config key and the
create command), `RepoRef::parse` it, `token::resolve` the chain, build a `Client`, call
`gate::fetch_facts`, call `gate::assert_pushable` with
`cfg.includes(SyncCategory::Credentials)` as `credentials_in_bundle`, destructure the
`(PushClearance, Vec<String>)`, render the warnings, and return a `SetupOutcome` carrying the
`RepoRef`, the `TokenSource`, the visibility string, the warnings, and the `PushClearance`.

`roots: &SyncRoots` is in the signature from the start and is the **only** way this module
reaches a filesystem path — the keyfile, the pairing record, the token file, and the
`config.toml` write-back plan 3-07 adds all resolve from it. Without it in the signature the
executor has no injected directory and reaches for a real `$HOME`, which the AUR `check()`
runs on installers' machines.

Unlike everything above, this signature is **not** frozen: plan 3-07 owns `setup.rs` outright,
in a later wave, and its only caller is `sync/cli.rs`, which 3-07 also owns. It adds a prompt
seam parameter there. Nothing in wave 2 calls it.

Everything in this module is `async` because `reqwest` is. Follow the existing vendor
fetchers' shape.

Write the tests as the behaviour block describes, using `mockito::Server::new_async()` with
both `Endpoints` fields pointed at `server.url()`, and `tempfile::TempDir` for the token file.
No test constructs `TokenChain::production()`, reads an environment variable, or resolves a
real home directory.
  </action>
  <verify>
    <automated>cargo test --lib sync::github</automated>
  </verify>
  <done>`cargo test --lib sync::github` is green. `src/sync/github/` holds seven files, all seven declared in `github/mod.rs`. `GithubError` carries its seven frozen variants and `TokenChain` its four frozen fields. No test in the module opens a socket to a host other than the mockito base, spawns a process, or reads a real `$HOME` path.</done>
  <reversibility rating="costly">`GithubError`'s variants and `TokenChain`'s fields are the seams three wave-2 plans build against in parallel; changing either after this plan merges forces all three to be reworked. They come straight from `github-transport.md` §5.2 and D-02 and are not to be improvised.</reversibility>
</task>

<task type="auto" tdd="true">
  <name>Task 2: `[sync] repo`, and `ai-usagebar sync setup` as a real command</name>
  <files>src/config.rs, src/sync/cli.rs, src/widget/cli.rs</files>
  <behavior>
    - A `config.toml` carrying `[sync]` with `repo = "owner/name"` round-trips that value onto `SyncConfig`.
    - A `config.toml` with a `[sync]` section and no `repo` key still loads, leaving `repo` as `None`.
    - `run_with` on `SyncAction::Setup` with `repo` unset returns a non-zero exit code and a message containing the create command.
    - `run_with` on `SyncAction::Setup` against a mockito private repo returns `0`, driven entirely from injected `Config`, `SyncRoots`, `Endpoints`, and `TokenChain` values.
  </behavior>
  <action>
Add `pub repo: Option<String>` to `SyncConfig` in `src/config.rs`, with a doc comment stating
D-01 plainly: the repository is named by the user, there is no default and nothing is derived,
because the tool holds no permission that could create one. `SyncConfig` already carries
`#[serde(default)]`, so an existing config without the key keeps loading. Do **not** add any
token field to `SyncConfig` — a `Contents: write` token is a different class of secret from the
read-only provider API keys `config.toml` is allowed to hold inline, and plan 3-02 owns its
storage.

Extend `SyncConfig::validate`-adjacent handling so a present-but-unparseable `repo` fails at
config load with the message from `RepoRef::parse`, not later at the first request. If
`SyncConfig` has no validation hook yet, call `RepoRef::parse` from `Config::validate` behind
an `if let Some(...)`.

In `src/widget/cli.rs` add `Setup` to the `SyncAction` enum that plan 2-01 introduced, with a
doc comment reading as the command's help text: pair this machine with the private GitHub
repository named in `[sync] repo`, verify it is private, and store the token. Nothing is
uploaded. Add the variant only — the `Command::Sync` arm and its dispatch in
`src/bin/ai-usagebar.rs` already exist from plan 2-01 and are not touched by this phase.

In `src/sync/cli.rs`, split the entry point in two. The shipped `run(action: &SyncAction) -> i32`
resolves the real world — `Config::load()`, `SyncRoots::resolve`, `Endpoints::default()`,
`TokenChain::production()`, `Utc::now()` — and hands them to:

`pub fn run_with(action: &SyncAction, cfg: &Config, roots: &SyncRoots, endpoints: &Endpoints, chain: &TokenChain, now: DateTime<Utc>) -> i32`

`run_with` holds all the logic and is what every test drives. `run` is a thin wrapper no test
calls. This is not ceremony: without it, a test of the `Setup` arm has to go through
`Config::load()` and `TokenChain::production()`, which read a real `$HOME` — and the AUR
`check()` runs `cargo test` during `makepkg` on installers' machines, so such a test fails the
*install* for anyone whose config differs. It is the same seam the project already uses in
`Cache::at`, `creds::read_from`, `SyncRoots::at`, and `Cli::resolve_vendor_with`.

`run_with`'s `Setup` arm drives the async `github::setup::run`, prints the outcome, and returns
`0` or a non-zero code. The error path prints the message and nothing else — no token, no
prefix of one, no header dump.

**Build the runtime here.** `src/bin/ai-usagebar.rs` dispatches `Command::Sync` *before* it
constructs the tokio runtime, under a comment saying sync does local filesystem scanning only
and so needs no runtime — true when plan 2-01 wrote it, false as of this plan. So `run_with`
constructs its own current-thread runtime with `tokio::runtime::Builder` and `block_on`s the
async call. Do **not** move the dispatch: leaving it where it is keeps runtime construction out
of the two subcommands that still do not need one, and the widget's own runtime-failure
fallback below it is unaffected. Fix that now-false comment in place — one line, and it is the
thing that would otherwise mislead the next reader into removing the local runtime.

The success line reports the repository, its visibility, and the token's **source**, in the
shape `token: present (Keychain)`. Assert in a test that the rendered success output does not
contain the token's characters.

The widget's exit-0 invariant is a property of `widget::run::fallback`, not of `sync`. `sync`
returns non-zero on failure, per D-06. Do not route any of this through the widget path.
  </action>
  <verify>
    <automated>cargo test --lib -- sync:: config::tests</automated>
  </verify>
  <done>`ai-usagebar sync setup` exists and runs. A fixture config with `[sync] repo = "o/n"` parses; one with a malformed value fails at load naming the expected shape. A test drives `run_with` against a mockito private repo and asserts the printed output names the repo, the visibility, and the token source, and contains no substring of the token. No test calls `run`, `Config::load`, `SyncRoots::resolve`, or `TokenChain::production`.</done>
  <reversibility rating="costly">`[sync] repo` and the `sync setup` subcommand name are user-facing surfaces; `Config` denies unknown sections and a renamed key breaks existing config files. Both names come from D-01 and D-05.</reversibility>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| `config.toml` → process | `repo` is user data interpolated into a URL path |
| stored token → process → network | The bearer token crosses into an HTTP request and must reach no other destination |
| GitHub response → process | Attacker-controlled JSON when the remote is hostile or the host is spoofed |
| process → terminal / logs | Any rendered line is an exfiltration path for the token |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-3-01 | Information disclosure | `Client` `Debug`, CLI output | critical | mitigate | Hand-written `Debug` printing endpoints and `TokenSource` only; token held as `Zeroizing<String>`; a test asserts the success output contains no substring of the token |
| T-3-02 | Information disclosure | bearer token following a redirect | high | mitigate | `vendor::same_origin_redirect_policy()` stops the hop, so the token is never replayed to signed storage or a redirect target |
| T-3-03 | Tampering | `RepoRef::parse` | high | mitigate | Exactly one separator, no whitespace, path separators, or control characters, so a config value cannot escape the URL path it is interpolated into |
| T-3-04 | Elevation of privilege | any write method on `Client` | critical | mitigate | `Client` exposes `get_json` only and no request-body call site exists in the phase — D-05 is enforced by the type, not by ordering |
| T-3-05 | Spoofing | `GET /repos` response | high | mitigate | `RepoFacts` is asserted before a `PushClearance` exists; `PushClearance` has a private field, no public constructor, and no `Clone`, so a clearance cannot be forged or cached (D-04) |
| T-3-06 | Denial of service | oversized response body | medium | mitigate | `vendor::read_body_capped` at `MAX_BODY_BYTES`; every response here is kilobytes of JSON |
| T-3-07 | Information disclosure | token in argv or a child's environment | critical | mitigate | No subprocess is spawned in this plan, and `Client` receives the token as a value, never through a command line |
| T-3-SC | Tampering | dependency surface | low | accept | Zero new crates: `reqwest` 0.12 + rustls, `zeroize`, `chrono`, `serde_json`, `tempfile`, and `mockito` are all already resolved in `Cargo.toml`. No package-manager install task exists in this phase, so no legitimacy gate applies |
</threat_model>

<verification>
- `cargo test --lib sync::github` and `cargo test --lib config::tests` are green.
- `grep -rn --include='*.rs' 'Utc::now' src/sync/github/` returns nothing outside a
  `production`-suffixed function or the CLI entry point: every time-dependent function takes
  `now`.
- No test under `src/sync/github/` constructs `TokenChain::production()`, reads an
  environment variable, or calls `home_dir`.
</verification>

<success_criteria>
`ai-usagebar sync setup` resolves the configured repository, resolves a token from the
environment or a mode-0600 file, verifies the repository reports itself private, and says so —
against a mock server, with no upload path in the type system. A missing repo, a missing
token, and a public repo each fail distinctly and exit non-zero.
</success_criteria>

<output>
Create `.planning/phases/03-github-auth-and-the-private-repo-gate/3-01-SUMMARY.md` when done.
Record in it the exact public signatures of `Endpoints`, `RepoRef`, `Client`, `GithubError`,
`classify`, `TokenChain`, `TokenSource`, `RepoFacts`, `PushClearance`, and
`github::setup::run` — three wave-2 plans build against them in parallel worktrees and must
not have to re-derive them from the diff.
</output>
