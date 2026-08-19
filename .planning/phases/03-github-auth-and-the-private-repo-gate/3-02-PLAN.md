---
phase: 03-github-auth-and-the-private-repo-gate
plan: 02
type: execute
wave: 2
depends_on: [3-01]
files_modified:
  - src/sync/github/token.rs
  - src/sync/github/keychain.rs
autonomous: true
requirements: [REPO-02, REPO-04]
must_haves:
  truths:
    - "All four sources in D-02's order are reachable, and each is exercised through the injected chain rather than the real thing."
    - "A token written on macOS never appears in a process argument list."
    - "A token written on Linux lands in a mode-0600 file created in its destination directory."
    - "No test reads the real Keychain, the real token file, or spawns `gh`."
    - "The stored token is never written into `config.toml`, and nothing in this module can write there."
  artifacts:
    - src/sync/github/token.rs with the complete four-source chain and the store/clear entry points
    - src/sync/github/keychain.rs implementing the macOS read/write split for the sync-token item
  key_links:
    - "`TokenChain`'s two closure fields are the only path to the Keychain and to `gh`; production supplies them, tests supply fakes"
    - "`TokenChain::production()` is the single place that decides which platform half is live"
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

<task type="auto" tdd="true">
  <name>Task 1: The macOS Keychain half — read via `security(1)`, write via Security.framework</name>
  <files>src/sync/github/keychain.rs</files>
  <behavior>
    - `read_raw` returns `Ok(None)` when `security` exits with the item-not-found status, and `Err` for every other non-zero exit — a locked Keychain is not the same as "no token".
    - `write_raw` fails closed with an actionable error when the account selector is unavailable, rather than falling back to a form that would place the token in argv.
    - A round trip of write-then-read is exercised only behind a macOS-gated, `#[ignore]`d test; the default test set touches no real Keychain item.
  </behavior>
  <action>
Fill `src/sync/github/keychain.rs`, which plan 3-01 created as a macOS-gated module holding
only its doc comment. Model it directly on `src/anthropic/keychain.rs` — reuse that module's
shape and its reasoning, do not invent a second convention, and do not refactor the Anthropic
module to share code with this one. The two items have different service names and different
lifetimes; a shared abstraction over two callers is the wrong trade here.

Service name: the constant `ai-usagebar-sync-token`, exactly as D-02 names it. Account
selector: the macOS short user name, resolved the same way `anthropic::keychain::account`
resolves it, and used identically by the read and the write so an update cannot create a
second item the read will never find again. That divergence is a bug the Anthropic module
already paid for; its doc comment says so.

`pub fn read_raw() -> Result<Option<String>>` shells out to `/usr/bin/security
find-generic-password` with the service and account selectors and `-w`. The token arrives on
that child's **stdout**, which is a read and therefore permitted; nothing about the read puts
a secret into an argument. Return `Ok(None)` only for the item-not-found exit status
(`errSecItemNotFound`, 44) and `Err` with an actionable message for every other failure,
naming the locked-Keychain and denied-ACL cases the way the Anthropic module does.

`pub fn write_raw(token: &str) -> Result<()>` calls
`security_framework::passwords::set_generic_password` with the same service and account pair.
This is the load-bearing half of the split: the `security add-generic-password` form would
place the token in the child's argument vector where any process on the machine can read it.
Fail closed with an actionable error when the account selector is unavailable — never fall
back to the argv form.

`pub fn delete_raw() -> Result<()>` removes the item, treating item-not-found as success, so
clearing a revoked token is idempotent.

Trim a trailing newline from what `security` returns and treat an empty value as `Ok(None)`.

Keep the module `#[cfg(target_os = "macos")]`-gated as 3-01 left it, so the Linux build never
compiles `security-framework` for this path. Unit tests here cover only the pure helpers (the
exit-status mapping, the trimming); anything that touches a real Keychain item is `#[ignore]`d
and macOS-gated, because the AUR `check()` runs `cargo test` on installers' machines.
  </action>
  <verify>
    <automated>cargo test --lib sync::github::keychain && cargo build --lib</automated>
  </verify>
  <done>On macOS, `cargo test --lib sync::github::keychain` is green and no default-set test reads or writes a real Keychain item. On Linux the module is not compiled and `cargo build --lib` is clean. `grep -n 'add-generic-password' src/sync/github/keychain.rs` finds nothing.</done>
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
`AI_USAGEBAR_SYNC_TOKEN` itself. Bound the child with a timeout so a hung credential helper
cannot wedge `sync setup`.

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
| T-3-08 | Information disclosure | Keychain write | critical | mitigate | `security_framework::passwords::set_generic_password`, never the `security(1)` form that would place the token in the child's argument vector; fails closed when the account selector is unavailable |
| T-3-09 | Information disclosure | spawned `gh` | high | mitigate | Nothing is passed to the child; `vendor_secret_env_vars_to_remove` plus `AI_USAGEBAR_SYNC_TOKEN` are stripped from its environment so it inherits no credential |
| T-3-10 | Information disclosure | token file | critical | mitigate | Created via `NamedTempFile::new_in` the destination directory, `persist()`d, then explicitly `chmod` 0600; never under `/tmp`, which is world-readable, cross-filesystem, and may be swap-backed tmpfs |
| T-3-11 | Spoofing | a Keychain error read as "no token" | high | mitigate | An `Err` from a chain source propagates instead of falling through, so a locked Keychain is reported as such rather than sending the user to re-issue a live token |
| T-3-12 | Denial of service | a hung `gh` or credential helper | medium | mitigate | The child is bounded by a timeout; its absence or failure is `Ok(None)`, since `gh` is convenience and never a requirement |
| T-3-13 | Information disclosure | token in `config.toml` | high | mitigate | No token field exists on `SyncConfig` and nothing in this module accepts a config path; storage is Keychain or a mode-0600 file only (REPO-02) |
</threat_model>

<verification>
- `cargo test --lib sync::github::token` is green on both platforms; `cargo test --lib
  sync::github::keychain` is green on macOS and the module is absent on Linux.
- `grep -rn 'add-generic-password' src/sync/github/` returns nothing.
- No test in either file calls `TokenChain::production`, `std::env::var`, or
  `std::process::Command`.
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
