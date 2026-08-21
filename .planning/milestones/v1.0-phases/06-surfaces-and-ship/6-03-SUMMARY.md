---
phase: 06-surfaces-and-ship
plan: 03
subsystem: widget
tags: [ux-06, d-03, waybar, exit-code, structural-gate, hermetic-tests]
requires:
  - "src/config.rs :: Config::load_from — already existed (phase 1); this plan only threads it in"
  - "src/config.rs :: SyncConfig — already tolerant of unknown keys; unchanged"
provides:
  - "widget::run::run_once(cli, out, config_path) -> i32 — the injectable writer seam UX-06 is asserted through"
  - "A structural gate proving the widget render path holds no reference into the encrypted-backup module"
affects:
  - "6-01 owns the other half of D-03 (sync's non-zero exit) — untouched here"
  - "6-05 release notes: restoring onto an older build is safe, see Unknown keys below"
tech-stack:
  added: []
  patterns:
    - "include_str! structural gate with a vacuity guard and runtime-assembled forbidden literals"
    - "config-path injection as the hermeticity seam for an always-exit-0 surface"
key-files:
  created: []
  modified:
    - src/widget/run.rs
decisions:
  - "No run_with wrapper — it would be a public one-line indirection with no caller but run and no test"
  - "src/config.rs left untouched — the round trip proved SyncConfig already tolerates unknown keys"
  - "The failing-transport injection is substituted by an absent credential under injected roots"
metrics:
  duration: ~45m
  completed: 2026-08-20
status: complete
---

# Phase 6 Plan 03: The Widget's Exit-0 Invariant Summary

UX-06 is now falsifiable: `run_once` returns the exit code it is supposed to guarantee and
takes the config file as a parameter, so eight tests drive the shipped render path against
poisoned `[sync]` fixtures under a `TempDir` and assert exit 0 plus one line of `⚠` Waybar
JSON — and a structural gate proves the render path cannot reach the encrypted-backup module
at all.

## What was built

### The seam (the part that did not exist)

`run_once` could not be asserted against as written: it returned `()`, and reached
configuration only through `Config::load()`, which resolves the real `$HOME`. Asserting
hermetically *and* through the shipped path *and* on an exit code was impossible against
those signatures. Final shapes:

```rust
pub  async fn run(cli: Cli) -> i32                                                  // unchanged
priv async fn run_once(cli: &Cli, out: &mut impl Write, config_path: Option<&Path>) -> i32
priv async fn build_output(cli: &Cli, config_path: Option<&Path>) -> Result<WaybarOutput>
```

`build_output` calls `Config::load_from(path)` when a path is injected and `Config::load()`
when it is not. `load` itself delegates to `load_from`, so the `None` arm is the unchanged
production loader, not a parallel one. **The config-path seam the tests use is
`run_once`'s third parameter**, threaded to `build_output` and nowhere else.

The constant `0` now *returns from the function under test*. That is the whole point: a
later refactor that makes the exit code conditional breaks a test instead of silently
removing the module from someone's bar.

`run`'s public signature is unchanged and `src/bin/ai-usagebar.rs` is untouched
(`git diff <base> HEAD -- src/bin/` is empty).

### The tests (all through that seam, none through the private `fallback`)

| Fixture | Failure | Asserted |
|---|---|---|
| `keep_snapshots = "ten"` | wrong type | exit 0, `⚠`, "TOML error" |
| `[sync` … | half-written TOML, what an interrupted restore leaves | exit 0, `⚠`, "TOML error" |
| `keep_snapshots = 0` | refused at load (the flip would drop what it publishes) | exit 0, `⚠`, names `keep_snapshots` |
| mode 000 config (unix) | unreadable file | exit 0, `⚠`, "I/O error" |
| unknown `[sync]` keys from a newer build | none — must parse | exit 0, **not** a TOML error; run reached the credentials |
| absent credential under injected roots | non-config fetch failure | exit 0, `⚠`, names the missing file |

Every failing case additionally asserts stdout is exactly one line, ends in `\n`, parses as
JSON, carries `text == "⚠"` and a `class` — the Waybar contract, not just the exit code.

`fallback` is deliberately *not* the thing under test. A panic or an early return before it
would still take the bar down, and only the seam catches that.

## Threat mitigations

- **T-6-10** (widget exit code): four poisoned-config fixtures assert exit 0 through the real
  writer seam. `⚠` renders in all four.
- **T-6-11** (a restore from a newer build): `SyncConfig` already carries `#[serde(default)]`
  and does **not** deny unknown fields — `deny_unknown_fields` is applied at the outer
  `Config` level only, deliberately, so a misspelled *section* is caught while a field from a
  future version is not. A round-trip test now pins this: an unknown `[sync]` key loads and
  leaves `repo` and `keep_snapshots` intact. It also fails if anyone later adds
  `deny_unknown_fields` to `SyncConfig`.
- **T-6-12** (tooltip disclosure): the unknown-key fixture deliberately carries an
  `https://…` value and a synthetic all-zeroes `ghp_`-shaped value. All six fixtures assert
  the rendered tooltip contains no `https://`, no `ghp_` and no `github_pat_`, and the
  absent-credential fixture additionally asserts the configured repository name is not echoed
  onto the bar. The expected-absent literals are assembled from fragments at runtime, so this
  file contains none of them and the assertion cannot match its own source.
- **T-6-13** (render-path reachability): the structural gate, below.
- **T-6-14**, **T-6-SC**: accepted as planned. Zero new crates; `Cargo.toml` and `Cargo.lock`
  are unchanged.

### The structural gate

One test reads `run.rs`, `render.rs` **and `pretty.rs`** via `include_str!` (compile-time, so
no working directory and no `$HOME`), strips every line whose first non-whitespace characters
begin a comment, and asserts the remainder contains neither `crate::sync` nor `sync::`. All
three, because `run_once` calls `print_pretty` on every non-JSON render — a gate omitting
`pretty.rs` would leave a third of the shipped path unchecked. A comment above it says a
fourth render-path file has to be added there.

Two things keep it from rotting into decoration:

- **Vacuity guard.** Each file must read back still containing `WaybarOutput` before the
  absence is asserted. Without it the gate passes just as happily against an empty string.
- **Runtime-assembled forbidden literals**, so the file cannot contain the text it forbids.

**Observed failing, by hand, before commit** (the plan's `<done>` requires this): the
forbidden path was appended as a non-comment `use` line to each of the three files in turn —
each produced `FAILED. 0 passed; 1 failed` with the file named — and a comment mentioning the
same path was confirmed *not* to trip it. Working tree restored after each.

Scope note carried forward from the objective: the claim is about the **render path**, not
the binary. `6-CONTEXT.md`'s D3 calls the widget and `sync` "different binaries" — they are
not; `ai-usagebar` is one binary and `sync` is a subcommand of it. The *contract* split D3
describes is correct and is not re-opened. **6-05 must not repeat the "different binary"
wording in the README.** What the two paths genuinely share is the config file on disk, which
is exactly what the Task 1 fixtures cover.

## Decisions

**The failing-transport substitution (on the record, not a quiet omission).** The ROADMAP asks
for "a test that injects a failing transport". A valid config selecting a vendor whose
credential file does not exist under the injected roots fails the fetch *before any socket is
opened*, giving the same end-to-end path through `run_once` → `build_output` →
`anthropic_output` → `fetch_snapshot` with no network. A true transport injection would need a
client seam `build_output` does not have; adding one to reach an outcome an absent credential
already produces would be scaffolding. The credential target is `Explicit`, which is never
eligible for the macOS Keychain fallback, so this test cannot reach a real Keychain either.

**No `run_with`.** The plan's artifact list named `pub async fn run_with(cli, config_path)`.
It was built, then deleted before commit: nothing called it but `run`, no test used it, and
its stated purpose — leaving `src/bin/ai-usagebar.rs` untouched — is satisfied by `run`
passing `None` directly. A public one-line wrapper with one internal caller and no test is
exactly the defect class this milestone is being audited for. Same reasoning removed a
`config_path` parameter from `run_cycle` and `run_watch`: nothing would ever have passed
`Some`, and a test for the cycle branch could not be written safely (`active::cycle` writes to
the real cache path, so a test exercising that branch is one refactor away from mutating the
user's own state). A `load_config` helper was inlined for the same reason — it ended up with
one caller.

**`src/config.rs` unchanged.** The plan allowed a tolerance change "only if a round trip
proves it missing". It did not: the round trip passed on the first run. **For 6-05's release
notes: restoring a config onto an older build is safe** — unknown `[sync]` keys are ignored,
known ones keep their values, and the widget renders normally.

**The sync subcommand's non-zero exit contract is asserted in `src/sync/cli.rs` by 6-01, not
here.** Two plans editing one file in one wave is the merge conflict this phase's wave layout
exists to avoid.

## Call-site audit

Demanded explicitly: every public function or config field added, and its production call sites.

| Added | Visibility | Production call sites |
|---|---|---|
| — | — | **No new public API and no new config field.** |
| `config_path` param on `run_once` | private | 2 (`run`, `run_watch`), both passing `None`; 8 tests pass `Some` |
| `config_path` param on `build_output` | private | 1 (`run_once`) |
| `-> i32` on `run_once` | private | returned by `run` (the process exit code) and asserted by 8 tests |

Nothing added has zero call sites. `src/config.rs` was not modified, so no config field was
written-but-never-read.

## Deviations from Plan

**1. [Rule 1 — dead code] `run_with` not created, despite being a named artifact**
- **Found during:** Task 1, at the call-site audit
- **Issue:** `pub async fn run_with` had one caller (`run`), no test, and no external user
- **Fix:** deleted; `run` passes `None` to `run_once` directly. `run`'s signature and
  `src/bin/ai-usagebar.rs` are unchanged either way, which was `run_with`'s stated purpose
- **Files modified:** src/widget/run.rs
- **Commit:** 46cff6c

**2. [Rule 1 — dead parameter] `config_path` not threaded into `run_cycle`/`run_watch`**
- **Found during:** Task 1
- **Issue:** first draft threaded it through all three branches of `run`; nothing would ever
  pass `Some`, and the cycle branch cannot be tested hermetically because `active::cycle`
  writes to the real cache path
- **Fix:** both reverted to their original signatures; only the render path carries the seam
- **Commit:** 46cff6c

**3. [planned substitution] failing transport → absent credential.** Recorded above and in the
plan's own `<source_audit>`; not a silent omission.

## Verification

| Gate | Result |
|---|---|
| `cargo test --lib widget::run` | 22 passed, 0 failed |
| `cargo test --lib` | **1520 passed**, 0 failed (baseline 1512, +8) |
| `cargo test` | **1580 passed**, 0 failed, 16 ignored (baseline 1572, +8) |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `make test` | green, including the GNOME, KDE and Omarchy contract suites |
| structural gate observed failing | yes — on a non-comment line in each of the three files |
| `git diff --stat` scope | `src/widget/run.rs` only |
| `Cargo.toml` / `Cargo.lock` | unchanged |
| `git diff -- src/bin/` | empty |
| `src/sync/`, `macos/`, `gnome-extension/`, `kde-plasmoid/`, `omarchy/` | untouched |

Hermeticity: no test resolves `$HOME` or `$XDG`, opens a socket, or reaches the Keychain.
Config, cache and credential paths are all injected under a `TempDir`; `--json` is passed
explicitly so no assertion depends on TTY detection; `src/widget/cli.rs` declares no clap
`env =` bindings, so no ambient variable can change what these tests parse. The mode-000 case
skips itself if the file still reads back (running as root), rather than asserting something
untrue.

## Known Stubs

None. No stub, placeholder, skipped test, or unrun `<verify>` was left behind.

## Threat Flags

None. No new network endpoint, auth path, file-access pattern, or schema change at a trust
boundary. The only production change is a parameter and a return type on two private
functions.

## Self-Check: PASSED

- `src/widget/run.rs` — FOUND
- Commit 00ac6bd — FOUND
- Commit 46cff6c — FOUND
- Commit 7b90ded — FOUND
