---
phase: 03-github-auth-and-the-private-repo-gate
plan: 02
subsystem: transport
tags: [github, auth, keychain, token-chain, subprocess-timeout, mode-0600, hermetic-tests]

requires:
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 01
    provides: "`TokenSource`, `TokenChain`'s four frozen fields, `resolve`, the empty `keychain.rs`"
  - codebase
    provides: "`anthropic::keychain`'s `read_raw_service`/`write_raw_service`/`delete_raw_service`, `cache::atomic_write`, `vendor::vendor_secret_env_vars_to_remove`, `config::resolved_path`"
provides:
  - "`TokenChain::production()` with all four of D-02's sources live"
  - "`token::store(&str, &Path) -> Result<TokenSource>` and `token::clear(&Path) -> Result<()>`"
  - "`sync::github::keychain` — `SERVICE` plus six delegating one-liners"
  - "three `pub(crate)` workers in `anthropic::keychain`"
  - "the `#[ignore]`d real-Keychain round trip in `tests/live.rs`"
affects: [3-03, 3-07, phase-4-push]

tech-stack:
  added: []
  patterns:
    - "A subprocess timeout with no crate: `spawn` + `Arc<Mutex<Child>>` + a watchdog thread on `mpsc::recv_timeout` that calls `Child::kill`. The stdout pipe is taken out *before* the child is shared, so the blocking read never holds the lock the watchdog needs."
    - "A platform-split entry point (`store`) keeps its file half in a separate private fn (`store_file`), so the half a macOS unit test *can* exercise is not welded to the half it must not."

key-files:
  modified:
    - src/anthropic/keychain.rs
    - src/sync/github/keychain.rs
    - src/sync/github/token.rs
    - tests/live.rs

key-decisions:
  - "`sync/github/keychain.rs` has six one-liners, not three. The plan's `read_raw`/`write_raw`/`delete_raw` are what `token.rs` calls; the `*_service` trio underneath them exists because `tests/live.rs` is a *separate crate* and cannot see `anthropic::keychain`'s `pub(crate)` workers — so the live probe needs a service-parameterized seam of its own to stay off the production item. Widening the anthropic workers to `pub` instead was the alternative and is strictly worse."
  - "`resolve` needed no change: plan 3-01 already implemented all four arms, including the `Err`-propagates rule. Only the tests that prove the order were missing."
  - "The exhausted-chain message gains its Keychain line from a cfg'd `keychain_hint()` — naming a Keychain item on Linux, where source 2 cannot exist, is not naming a fix (D-06)."
  - "`store_file` writes through `cache::atomic_write` rather than hand-rolling `NamedTempFile::new_in` + `persist`. That helper already creates the parent, puts the temp file in the destination's own directory, and `sync_all`s — the same reuse `anchor::write_to` makes."
  - "The `gh` watchdog wakes on an `mpsc` disconnect rather than sleeping the full timeout, so a 40 ms `gh auth token` does not leave a thread parked for five seconds."
  - "`wait_with_output` was not used: it consumes the `Child`, and the watchdog needs `&mut` access to the same one. The main thread reads the taken stdout pipe and then `wait()`s under the lock — same bytes, same bound, no second owner."

patterns-established:
  - "Precedence tests make *every* later source able to answer, each with a distinct value, so an ordering assertion cannot pass because the winner was the only one that could speak."

requirements-completed: [REPO-02, REPO-04]

coverage:
  - id: REPO-04
    description: "All four of D-02's sources resolve, in order, each through the injected chain"
    verification:
      - kind: unit
        ref: "src/sync/github/token.rs#the_environment_wins_and_reports_itself_as_the_source"
        status: pass
      - kind: unit
        ref: "src/sync/github/token.rs#the_keychain_outranks_the_file_and_gh"
        status: pass
      - kind: unit
        ref: "src/sync/github/token.rs#a_token_file_answers_when_the_environment_is_unset"
        status: pass
      - kind: unit
        ref: "src/sync/github/token.rs#gh_answers_when_nothing_ahead_of_it_did"
        status: pass
      - kind: unit
        ref: "src/sync/github/token.rs#an_exhausted_chain_names_every_way_to_supply_a_token"
        status: pass
    human_judgment: false
  - id: REPO-02
    description: "A stored token lands in the Keychain or a mode-0600 file, never in `config.toml`"
    verification:
      - kind: unit
        ref: "src/sync/github/token.rs#a_stored_token_file_is_created_at_mode_0600_in_its_own_directory"
        status: pass
      - kind: live
        ref: "tests/live.rs#sync_token_keychain_live_round_trip"
        status: deferred
        notes: "`#[ignore]`d and macOS-only by construction. Run `cargo test --test live sync_token -- --ignored`; it writes only `ai-usagebar-sync-token-livetest` and deletes it again."
      - kind: manual
        ref: "grep — `token.rs` accepts no config path and names no config type; `SyncConfig` still has no token field"
        status: pass
    human_judgment: false
  - id: T-3-08
    description: "A token written on macOS never appears in a process argument list"
    verification:
      - kind: manual
        ref: "`grep -c 'Command\\|security_framework' src/sync/github/keychain.rs` is 0 — the write is `anthropic::keychain::write_raw_service`, i.e. `security_framework::passwords::set_generic_password`, which is also the only implementation of that rule in the crate"
        status: pass
    human_judgment: false
  - id: T-3-09
    description: "The spawned `gh` inherits no credential"
    verification:
      - kind: manual
        ref: "src/sync/github/token.rs#gh_auth_token — `env_remove` over `vendor_secret_env_vars_to_remove(&[])` plus `AI_USAGEBAR_SYNC_TOKEN`; stdin null, nothing in argv but `auth token`"
        status: pass
    human_judgment: false
  - id: T-3-11
    description: "A Keychain error is not read as an absent token"
    verification:
      - kind: unit
        ref: "src/sync/github/token.rs#a_keychain_error_stops_the_chain_instead_of_falling_through"
        status: pass
    human_judgment: false
  - id: hermeticity
    description: "No test in `src/` reads the real Keychain, the real token file, or spawns a process"
    verification:
      - kind: manual
        ref: "scan of every `#[cfg(test)]` block in `src/sync/github/` for `TokenChain::production` / `std::env::var` / `Command::new` / `keychain::*_raw` — no hits"
        status: pass
    human_judgment: false

duration: 40min
completed: 2026-08-19
status: complete
---

# Phase 3 / Plan 02: The GitHub Token Chain

**All four of D-02's sources are live, and the macOS half is six delegating
lines over the module that already implemented it.** The interesting artifact of
this plan is how little code it contains: the Keychain rules, the atomic write,
and the environment-stripping list were all already in the tree.

## Task Commits

1. **`9354c63`** — the Keychain half, as wrappers (task 1)
2. **`34ffad7`** — the `gh` half, the write path, and D-02's order proven (task 2)

## What plan 3-07 needs from here

```rust
// src/sync/github/token.rs
pub fn store(token: &str, file_path: &Path) -> Result<TokenSource>;
pub fn clear(file_path: &Path) -> Result<()>;
```

`store` returns where it actually went: `TokenSource::Keychain` on macOS (where
`file_path` is ignored), `TokenSource::File` everywhere else. Print that, never
the token. `clear` removes **both** halves and is idempotent — on macOS it
deletes the Keychain item *and* the file, because a file copied from another
machine is still something `resolve` would find.

**The token-file path** is the one `TokenChain::production()` computes, and 3-07
should compute it the same way rather than hard-coding a string:

```rust
crate::config::resolved_path()
    .and_then(|p| p.parent().map(|d| d.join("sync-token")))
```

That is *the directory `config.toml` actually resolved to*, joined with
`sync-token` — normally `~/.config/ai-usagebar/sync-token`, but
`~/Library/Application Support/ai-usagebar/sync-token` on a macOS install with no
legacy `~/.config` file, since `config::resolved_path` prefers whichever of the
two exists. Deriving it from the config path is what keeps the token beside the
config the user is actually using.

`store`'s `file_path` is a *token* path and nothing else. Nothing in the module
accepts a config path or can write TOML; `SyncConfig` still has no token field.

## The Keychain half

`src/anthropic/keychain.rs` already implemented exactly this — reads through
`security(1)`, writes through `security_framework::passwords::set_generic_password`,
both selecting by the same `(service, account)` pair, failing closed when `$USER`
is unset. Its three `*_service` workers were already parameterized by service
name and private only by accident. They are now `pub(crate)`; that file's entire
diff is three visibility keywords and one doc paragraph.

So `src/sync/github/keychain.rs` is:

```rust
pub const SERVICE: &str = "ai-usagebar-sync-token";   // D-02, and the address of a real secret

pub fn read_raw()  -> Result<Option<String>> { read_raw_service(SERVICE) }
pub fn write_raw(token: &str) -> Result<()>  { write_raw_service(SERVICE, token) }
pub fn delete_raw() -> Result<()>            { delete_raw_service(SERVICE) }

pub fn read_raw_service(service: &str)   -> Result<Option<String>>  { worker::read_raw_service(service) }
pub fn write_raw_service(s: &str, t: &str) -> Result<()>            { worker::write_raw_service(s, t) }
pub fn delete_raw_service(service: &str) -> Result<()>              { worker::delete_raw_service(service) }
```

Six, not the three the plan drew, and the reason is the live test:
`tests/live.rs` is a separate crate, so `pub(crate)` workers are invisible to it,
and the round trip must run under `ai-usagebar-sync-token-livetest` rather than
the item a real user's token lives in. A parameterized seam one module further
out was cheaper than widening `anthropic::keychain` to `pub`.

Why reuse rather than a second implementation, in the module's own words: the
read once omitted `-a` while the write passed `-a ""`, so a refresh created a
second, empty-account item the read could never find again. Re-deriving the
worker re-derives the bug.

## The `gh` half

`Command::output` has no timeout, no crate in this tree adds one, adding one
would break the zero-new-crates rule and `cargo machete`, and `resolve` is
synchronous so `tokio::process` is not reachable from it. The bound is therefore
`std` only:

- `spawn`, then `child.stdout.take()` **before** the child goes into an
  `Arc<Mutex<_>>` — the blocking read must not hold the lock the watchdog needs.
- a watchdog thread on `mpsc::recv_timeout(5s)`: `Timeout` means the child is
  still running, so `kill` it; `Disconnected` means the caller finished and
  dropped its end, so exit immediately rather than parking for the full five
  seconds.
- the main thread reads the pipe to EOF (a killed child closes it, which is what
  ends the read), then `wait()`s under the lock.

`wait_with_output`, which the plan named, consumes the `Child` and so cannot
coexist with a watchdog that needs `&mut` to the same one; the loop above reads
the same bytes with the same bound.

A missing `gh` and a non-zero exit are both `Ok(None)`: source 4 is convenience,
so "not installed" and "not logged in" are equally just "nothing here". The child
gets `stdin: null`, `stderr: null`, `stdout: piped`, nothing in argv but
`auth token`, and an environment with every `VENDOR_SECRET_ENV_VARS` entry and
`AI_USAGEBAR_SYNC_TOKEN` removed.

## Verification

```
cargo test --lib -- anthropic::keychain sync::github    # 28 passed, 0 failed
cargo test --lib sync::                                 # 223 passed, 0 failed
cargo clippy --all-targets -- -D warnings               # clean
cargo fmt --check                                       # clean
```

- `git diff HEAD~2 -- src/anthropic/keychain.rs` — three `pub(crate)`s and one
  doc paragraph, nothing else.
- `grep -c 'Command\|security_framework' src/sync/github/keychain.rs` → 0.
- Every `#[cfg(test)]` block under `src/sync/github/` scanned for
  `TokenChain::production` / `std::env::var` / `Command::new` /
  `keychain::*_raw` — no hits. The only `std::env::var` and the only
  `Command::new` in `token.rs` are in `production()` and `gh_auth_token`, neither
  of which any test calls.
- `Cargo.toml` unchanged. Zero new crates.

## Not done here, deliberately

- **The Linux build was not compiled.** This worktree is macOS and no
  `x86_64-unknown-linux-gnu` toolchain is installed. The non-macOS arms are
  `cfg`-selected siblings of arms that do compile, and `store_file` is compiled
  here under `cfg(any(not(target_os = "macos"), test))` and covered by a test, so
  the Linux-only code paths are `keychain: None`, an empty `keychain_hint()`, and
  the two-line `store`/`clear` dispatch. CI's Linux leg is the confirmation.
- **`gh_auth_token` has no test**, by design: testing it means spawning a
  process. Its one testable seam would be the exit-status mapping, and there is
  no mapping — any failure is `Ok(None)`. The plan's suggested
  `classify_security_exit` helper was not written for the same reason: the
  `errSecItemNotFound` mapping it would have factored lives in
  `anthropic::keychain`, already written, already the only copy, and copying it
  out to make it testable would be the exact duplication task 1 exists to
  prevent.
- **The `anthropic::keychain` error strings still say "Claude credentials"**, so
  a locked Keychain during `sync setup` reports the wrong noun. Fixing it means
  editing that module's behaviour, which task 1 forbids; the sentence still names
  the right fix (unlock the Keychain / allow ai-usagebar), which is what D-06
  requires. Worth a one-line generalization in a later plan that owns the file.
