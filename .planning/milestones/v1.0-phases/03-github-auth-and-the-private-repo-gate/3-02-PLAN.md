---
phase: 03-github-auth-and-the-private-repo-gate
plan: 02
type: execute
wave: 2
depends_on: [3-01]
files_modified:
  - src/sync/github/token.rs
  - src/sync/github/keychain.rs
  - src/anthropic/keychain.rs
  - tests/live.rs
autonomous: true
requirements: [REPO-02, REPO-04]
must_haves:
  truths:
    - "All four sources in D-02's order are reachable, and each is exercised through the injected chain rather than the real thing."
    - "A token written on macOS never appears in a process argument list."
    - "A token written on Linux lands in a mode-0600 file created in its destination directory."
    - "No test in `src/` reads the real Keychain, the real token file, or spawns `gh`; the real-Keychain round trip lives in `tests/live.rs` behind `#[ignore]`."
    - "The Keychain read/write/delete rules exist in exactly one place in the crate, not two."
    - "The stored token is never written into `config.toml`, and nothing in this module can write there."
  artifacts:
    - src/sync/github/token.rs with the complete four-source chain and the store/clear entry points
    - src/sync/github/keychain.rs — a service-name constant and three wrappers over the existing workers
    - "three `pub(crate)` visibilities in src/anthropic/keychain.rs"
  key_links:
    - "`TokenChain`'s two closure fields are the only path to the Keychain and to `gh`; production supplies them, tests supply fakes"
    - "`TokenChain::production()` is the single place that decides which platform half is live"
    - "`anthropic::keychain`'s `*_service` workers are already service-parameterized — reusing them is what keeps the account-selector agreement rule single-sourced"
---

<objective>
Fill D-02's token chain: the macOS Keychain half, the `gh auth token` half, and the write
path that persists a token after `sync setup` collects it. Plan 3-01 froze
`TokenChain`'s fields and `resolve`'s signature and already implemented the environment and
file sources; this plan supplies the two injected closures and everything that stores a token.

Implements **D-02** (resolution order and the rejection of `keyring`/`secret-service`) and the
codebase's standing rule that a secret never enters process arguments.

Purpose: a headless machine over SSH must still find a token, which is why this is a Keychain
item and a mode-0600 file rather than a D-Bus keyring.
Output: a complete `token.rs` and a complete macOS `keychain.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-CONTEXT.md
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-01-SUMMARY.md
@.planning/research/github-transport.md
@CLAUDE.md
@src/anthropic/keychain.rs
@src/cache.rs
@src/vendor.rs
</context>

<tasks>

<task type="auto">
  <name>Task 1: The macOS Keychain half — three wrappers over the module that already does this</name>
  <files>src/anthropic/keychain.rs, src/sync/github/keychain.rs, tests/live.rs</files>
  <action>
**Do not write a second Keychain implementation.** `src/anthropic/keychain.rs` already
implements exactly this, and its three workers —`read_raw_service`, `write_raw_service`,
`delete_raw_service` — are **already parameterized by service name**. They are private only
because nothing outside that module has needed them until now. Re-deriving the
`errSecItemNotFound` mapping, the read/write account-selector agreement, and the
fail-closed-on-unset-`$USER` rule is not "following the convention", it is a second copy that
can drift. That module's own doc comment records the bug this invites: the read once omitted
`-a` while the write passed `-a ""`, so a refresh created a second, empty-account item the read
could never find again. One copy of that rule, not two.

So: in `src/anthropic/keychain.rs`, change `read_raw_service`, `write_raw_service`, and
`delete_raw_service` from private to `pub(crate)`. Nothing else in that file changes — no
behaviour, no signature, no test. Add one line to its module doc noting that the sync token
uses the same workers under a different service name, so the next reader knows why the
visibility widened.

Then `src/sync/github/keychain.rs`, which plan 3-01 created macOS-gated with only its doc
comment, becomes a service-name constant and three one-line wrappers:

- `pub const SERVICE: &str = "ai-usagebar-sync-token";` — exactly as D-02 names it.
- `pub fn read_raw() -> Result<Option<String>>` → `anthropic::keychain::read_raw_service(SERVICE)`
- `pub fn write_raw(token: &str) -> Result<()>` → `anthropic::keychain::write_raw_service(SERVICE, token)`
- `pub fn delete_raw() -> Result<()>` → `anthropic::keychain::delete_raw_service(SERVICE)`

Everything the previous draft of this plan spelled out — reads shell out to `security(1)` and
take the value from the child's **stdout**; writes go through
`security_framework::passwords::set_generic_password` so the secret never enters the child's
argument vector; the write fails closed when `$USER` is unset rather than falling back to the
argv form; item-not-found is `Ok(None)` on read and success on delete; the value is trimmed
and an empty one reads as absent — is already true of those workers. Verify that by reading
them, not by re-implementing them.

Three one-line wrappers need no unit test: there is no branch to get wrong. What does need
exercising is that the real item round-trips, and that belongs in `tests/live.rs` behind
`#[ignore]` beside the existing probes, macOS-gated. A `src/` unit test that writes a real
Keychain item would be reached by `cargo test -- --ignored` in any CI leg and by the AUR
`check()`'s environment, and it would leave an item behind on a developer's login Keychain.
Name it so its cost is obvious, have it write, read back, and delete under a
`ai-usagebar-sync-token-livetest` service name — not the production one — and assert the round
trip.
  </action>
  <verify>
    <automated>cargo build --lib && cargo test --lib -- anthropic::keychain sync::github::keychain</automated>
  </verify>
  <done>`src/sync/github/keychain.rs` is a constant plus three delegating one-liners and contains no `Command`, no `security_framework` call, and no exit-status constant of its own. `src/anthropic/keychain.rs` differs from `HEAD` only by three `pub(crate)` visibilities and one doc line. The live round-trip is `#[ignore]`d, macOS-gated, and uses a test-only service name. `cargo build --lib` is clean on both platforms.</done>
  <reversibility rating="costly">The service name `ai-usagebar-sync-token` is the address of a stored secret on a user's machine. Renaming it after ship orphans every token already written under the old name, with no way to find it. It is fixed by D-02.</reversibility>
</task>

<task type="auto" tdd="true">
  <name>Task 2: The `gh` half, the write path, and the chain end to end</name>
  <files>src/sync/github/token.rs</files>
  <behavior>
    - With `env_value` set and all three later sources also able to answer, `resolve` returns the environment value and `TokenSource::Env` — order is asserted, not assumed.
    - With `env_value` cleared, the injected keychain closure answers and `resolve` returns `TokenSource::Keychain`.
    - With `env_value` and the keychain closure both empty, a mode-0600 temp file answers and `resolve` returns `TokenSource::File`.
    - With the first three empty, the injected `gh` closure answers and `resolve` returns `TokenSource::GhCli`.
    - With all four empty, `resolve` errors with a message naming how to supply a token, and the message contains no token-shaped value.
    - A keychain closure returning `Err` propagates rather than silently falling through to the next source.
    - `store` on a non-macOS target writes the file at mode 0600 and leaves nothing behind at any other mode.
  </behavior>
  <action>
Complete `src/sync/github/token.rs` behind the signatures plan 3-01 froze. Do not add a field
to `TokenChain`, do not add a variant to `TokenSource`, and do not change `resolve`'s
signature — plans 3-03 and 3-04 are building against them in parallel worktrees.

**`TokenChain::production()`** now supplies all four. `env_value` reads
`AI_USAGEBAR_SYNC_TOKEN`. `keychain` is `Some` of a closure calling
`super::keychain::read_raw` on macOS and `None` elsewhere. `file_path` stays as 3-01 set it.
`gh` is `Some` of a closure calling the new `gh_auth_token` helper. This function is the only
place in the module that touches the environment or a real path, and no test calls it.

**`gh_auth_token() -> Result<Option<String>>`** runs `gh auth token` and returns its trimmed
stdout, or `Ok(None)` when the binary is absent or exits non-zero — `gh` is convenience and
never a requirement, so its absence is not an error. Two rules govern the spawn. First, the
token travels on the child's **stdout**; nothing is passed to it. Second, strip this project's
own provider secrets from the child's environment using the existing
`crate::vendor::vendor_secret_env_vars_to_remove(&[])` helper, so a subprocess this tool
spawns never inherits credentials it has no business seeing. Also strip
`AI_USAGEBAR_SYNC_TOKEN` itself.

Bound the child, and use this mechanism specifically: `std::process::Command::spawn`, then a
watchdog `std::thread` that sleeps a few seconds and calls `Child::kill` if the process is
still running, then `wait_with_output` on the main thread. `Command::output` has no timeout of
its own and there is no dependency in this tree that adds one — **do not add a crate for
this**, and do not reach for `tokio::process`, because `resolve` is synchronous and the
`TokenChain` closures that call it are not async. Roughly fifteen lines of `std`. A `gh` that
never returns is not hypothetical: it shells out to whatever credential helper the user
configured, and one of those hanging on a locked keyring would otherwise wedge `sync setup`
with no output.

**`resolve`** already walks env then file. Extend it to the full D-02 order — env, keychain,
file, `gh` — returning the first non-empty trimmed value with its `TokenSource`. A closure
that returns `Err` propagates immediately: an error from the Keychain means "the Keychain
could not answer", not "there is no token", and silently continuing would send the user to
re-issue a token they already have. That distinction is the same one
`anthropic::keychain::read_raw` documents. When all four are empty, error with a message that
names the environment variable, the Keychain item, the file path, and `gh auth token` — D-06
requires the failure to name the fix.

**`pub fn store(token: &str, file_path: &Path) -> Result<TokenSource>`** persists a token
collected by `sync setup`. On macOS it calls `keychain::write_raw` and returns
`TokenSource::Keychain`; on every other target it writes `file_path` atomically —
`tempfile::NamedTempFile::new_in` the destination's own directory, then `persist()`, then an
explicit `set_permissions` to 0600 — and returns `TokenSource::File`. Create the parent
directory first. Never `/tmp`: it is world-readable, is often a different filesystem so
`persist` degrades to a copy that leaves the original behind, and may be tmpfs that survives
in swap. This mirrors the atomic-write convention `cache.rs` and the Settings overlay already
use.

**`pub fn clear(file_path: &Path) -> Result<()>`** removes both halves idempotently, for the
401 path plan 3-03 defines: a revoked token should be cleared, not retried.

Nothing in this module writes to `config.toml`, and nothing accepts a path that could be one.
A `Contents: write` GitHub token is a different class of secret from the read-only provider
keys `config.toml` is allowed to hold inline.

Every test builds its `TokenChain` by hand with closures over `Vec`/`Result` fixtures and a
`TempDir` for `file_path`. No test calls `production()`, reads an environment variable, or
spawns a process.
  </action>
  <verify>
    <automated>cargo test --lib sync::github::token</automated>
  </verify>
  <done>All four sources resolve in D-02's order, each asserted by a test that shadows the later ones so precedence is proven rather than incidental. A keychain error propagates. The file written by `store` is mode 0600. `cargo test --lib sync::github::token` is green and no test in the module spawns a subprocess or reads a real credential.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| Keychain / token file → process | The stored secret enters memory |
| process → spawned `gh` | A child process inherits an environment and an argument vector |
| process → disk | A written token is a durable secret whose mode decides who can read it |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-3-08 | Information disclosure | Keychain write | critical | mitigate | Delegated to `anthropic::keychain::write_raw_service`, which uses `security_framework::passwords::set_generic_password` and never the `security(1)` form that would place the token in the child's argument vector, and fails closed when the account selector is unavailable. Reuse rather than reimplementation is the mitigation: a second copy of this rule is a second place for it to drift |
| T-3-09 | Information disclosure | spawned `gh` | high | mitigate | Nothing is passed to the child; `vendor_secret_env_vars_to_remove` plus `AI_USAGEBAR_SYNC_TOKEN` are stripped from its environment so it inherits no credential |
| T-3-10 | Information disclosure | token file | critical | mitigate | Created via `NamedTempFile::new_in` the destination directory, `persist()`d, then explicitly `chmod` 0600; never under `/tmp`, which is world-readable, cross-filesystem, and may be swap-backed tmpfs |
| T-3-11 | Spoofing | a Keychain error read as "no token" | high | mitigate | An `Err` from a chain source propagates instead of falling through, so a locked Keychain is reported as such rather than sending the user to re-issue a live token |
| T-3-12 | Denial of service | a hung `gh` or credential helper | medium | mitigate | The child is bounded by a timeout; its absence or failure is `Ok(None)`, since `gh` is convenience and never a requirement |
| T-3-13 | Information disclosure | token in `config.toml` | high | mitigate | No token field exists on `SyncConfig` and nothing in this module accepts a config path; storage is Keychain or a mode-0600 file only (REPO-02) |
</threat_model>

<verification>
- `cargo test --lib sync::github::token` is green on both platforms; `cargo build --lib` is
  clean on Linux, where `sync/github/keychain.rs` is not compiled.
- `git diff --stat HEAD -- src/anthropic/keychain.rs` shows only the three visibility changes
  and one doc line.
- `grep -c 'Command\|security_framework' src/sync/github/keychain.rs` is 0.
- No test in `src/` calls `TokenChain::production`, `std::env::var`, or
  `std::process::Command`; the real-Keychain round trip is `#[ignore]`d in `tests/live.rs`
  under a test-only service name.
</verification>

<success_criteria>
A token can be found on a machine with only an environment variable, only a Keychain item,
only a mode-0600 file, or only `gh` — and the order between them is D-02's, proven by tests
that make more than one source able to answer at once. A token stored on macOS never enters a
process argument list; one stored elsewhere is unreadable by another user.
</success_criteria>

<output>
Create `.planning/phases/03-github-auth-and-the-private-repo-gate/3-02-SUMMARY.md` when done.
Record the final `store` / `clear` signatures and the resolved token-file path, which plan
3-07 calls from the guided setup.
</output>
