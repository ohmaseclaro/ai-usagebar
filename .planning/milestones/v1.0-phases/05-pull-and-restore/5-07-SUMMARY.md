---
phase: 05-pull-and-restore
plan: 07
subsystem: sync/cli
status: complete
tags: [restore, cli, gates, consent, stdin, index-recovery, call-sites]
requires:
  - "sync::restore::{run, RestoreOptions, RestoreCtx, Disposition, ItemPlan} (5-01, frozen)"
  - "sync::restore::report::{render_plan, render_outcome, confirm_apply, confirm_credentials, APPLY_COMMAND} (5-06)"
  - "sync::push::anchor_path (4-08) — the anchor file restore reads and advances"
  - "sync::passphrase::read_line (Phase 1) — stdin only, never argv, never an env var"
provides:
  - "`ai-usagebar sync pull` — the command; SyncAction::Pull with seven flags"
  - "sync::index::reset_at — `--rebuild-index`"
  - "sync::index::Index::rehashing — `--force-rehash`"
  - "`ai-usagebar sync push --rebuild-index --force-rehash`"
affects:
  - "5-08 — the end-to-end assertions; the exit-code table and gate order below are what it pins"
tech-stack:
  added: []
  patterns:
    - "one `is_terminal` read decides both who owns stdin and whether a gate may ask"
    - "the gate that renders the report is the only thing that renders it"
    - "a recovery switch on the shared handle rather than threaded through every planner's argument list"
key-files:
  created: []
  modified:
    - src/sync/cli.rs
    - src/sync/index.rs
    - src/sync/plan.rs
    - src/widget/cli.rs
decisions:
  - "a piped stdin belongs to the sync password; the gates read only a terminal, and an unattended run answers with flags"
  - "`--force-rehash` lives on `sync push`, not on `sync pull` — nothing on the pull path reads it, and a flag with no reader is this milestone's most repeated defect"
  - "the force_rehash switch sits on `Index` (suppressing `lookup`/`cached`) rather than on `plan::build`'s argument list, so it covers every planner and adds no field to `PushCtx`"
  - "a declined apply gate exits 0; a declined credential gate exits non-zero"
metrics:
  duration: ~2h
  completed: 2026-08-20
---

# Phase 5 Plan 07: `sync pull` — Summary

`ai-usagebar sync pull` exists. Everything under `src/sync/restore/` had been
built, tested and reachable by nothing; it is now reachable by the command the
report has been printing since 5-06.

---

## THE COMMAND, AS IT SHIPS

```
ai-usagebar sync pull [--apply | --dry-run] [--force [--force-credentials]]
                      [--allow-rollback] [-y|--yes] [--rebuild-index]
```

| flag | what it does | clap constraint |
|---|---|---|
| `--apply` | the only flag that lets a byte reach disk | conflicts with `--dry-run` |
| `--dry-run` | already the default; accepted for symmetry with `push --dry-run` | conflicts with `--apply` |
| `--force` | promotes a locally-newer **non-credential** item to `Overwrite` | — |
| `--force-credentials` | the second consent for a locally-newer credential | **requires** `--force` |
| `--allow-rollback` | opens an older snapshot of the **same** bundle | never waives `repo_id` |
| `-y`, `--yes` | answers the apply gate | does **not** answer the credential gate |
| `--rebuild-index` | discards the local SQLite index and reopens it empty | — |

`sync push` gained `--rebuild-index` and `--force-rehash`, which is where the
second one changes what the planner does.

**`--dry-run` maps to nothing.** A dry run is the *absence* of `apply`, not a
flag anything checks (D1), and `restore/mod.rs`'s early return is what makes the
write path unreachable. The flag is accepted so `push --dry-run` and
`pull --dry-run` read the same way, and clap refuses it alongside `--apply` so a
run that passes both is an error rather than a guess.

### The order, which is the safety property

```
1. restore::run(apply: false)     — always, including under --apply
2. the apply gate                 — renders the report, then asks (D6)
3. the credential gate            — before backup::take, before the first byte
4. restore::run(apply: true)      — the backup and the writes, in 5-01's order
5. report::render_outcome
```

Positions 2 and 3 are both *before* step 4, so a user who stops at either has
had **nothing written and no archive taken** (T-5-60, T-5-61). The credential
gate's decline is asserted by walking the roots *and* the backups directory
afterwards, not by reasoning about it.

### The exit-code table 5-08 asserts

| situation | exit |
|---|---|
| completed dry run (piped, or `--dry-run`) | **0** — it did exactly what it was asked |
| apply gate declined at the terminal | **0** — a choice the tool honoured |
| completed apply | 0 |
| credential gate declined | **1** — it stopped a restore the user had asked for |
| wrong password / no pointer / 403 / tampered pack / unreachable remote | 1 |
| partial restore (`failed_at` set) | 1 |

One message, once, to stderr, through the existing `refuse`. Remote-supplied
manifest paths in that message go through `display::sanitize_untrusted_field`.

---

## THE THREE HAZARDS 5-06 HANDED OVER

### 1. `APPLY_COMMAND` names a command that now exists

`report::APPLY_COMMAND` is `"ai-usagebar sync pull --apply"`, printed in the
footer of every dry run. The guard is a test that **parses the constant with
clap**:

```rust
let argv: Vec<&str> = restore::report::APPLY_COMMAND.split_whitespace().collect();
let cli = crate::widget::cli::Cli::try_parse_from(&argv)?;
assert!(matches!(cli.command, Some(Command::Sync { action: SyncAction::Pull { apply: true, .. } })));
```

If the subcommand is ever renamed, or `--apply` is ever spelled differently, or
the const drifts, that test fails. It is the assertion this milestone has needed
eight times.

### 2. The plan is rendered exactly once

`confirm_apply` writes the whole report and *then* asks, so the arm that offers
the gate does **not** also `print!(render_plan(..))`. `render_plan` is called
directly on exactly the two paths that never reach the gate: the piped dry run,
and `--apply`. `the_plan_is_printed_exactly_once_on_the_path_that_asks` counts
`"DRY RUN"` occurrences and asserts 1.

### 3. Stdin has one owner, decided by `is_terminal`

The collision is real: the sync password is read from stdin, and both gates read
from stdin. Over a pipe there is one stream and no way to tell the two apart, so
whichever reads second eats the other's line.

**The password wins the stream.** It is the one input that cannot be supplied any
other way — Phase 1's rule keeps it out of argv and out of the environment — so:

- **stdin is a terminal** → the password is asked for, then the gates read the
  next lines the user types. Sequential reads over a terminal; nothing is
  consumed twice, and both confirmations are offered normally.
- **stdin is a pipe** → the password takes the first line, and the gates are
  never given a reader. A piped run is *never asked anything*: without `--apply`
  it prints the plan, names `--apply`, and exits 0; with `--apply` it applies;
  and `--force-credentials` is how it answers the credential question, which
  `confirm_credentials` reads itself.

One `std::io::stdin().is_terminal()` read decides both halves, so they cannot
disagree about which stream they are sharing. The mechanism is the injected
`PullIo { out, gate: Option<&mut dyn BufRead> }`, and `gate: None` **is** the
piped shape — `a_piped_run_is_never_asked_anything_so_the_password_keeps_the_pipe`
asserts the output contains neither `[y/N]` nor `Type the word`, and then drives
the same fixture with a reader to prove the assertion is about the pipe rather
than about an empty plan.

`sync_password` is a new four-line function and deliberately **does not** go
through `keys_at`/`local_keyfile`, which the plan's action text suggested: those
read `<config>/sync/keyfile.json`, and a second machine has none — that is the
entire situation a restore exists for. A pull consults no local keyfile at all;
the wrapper comes off the remote and the password unwraps it.

---

## WHAT 4-08 CHANGED, AND HOW THIS HONOURS IT

- **The anchor is reused, never reimplemented.** `ctx.anchor_path` comes from
  `push::anchor_path(roots, &parts.repo)` — the same function, the same file
  (`<config_dir>/sync-anchor-<owner>-<name>.json`), keyed on the locally
  configured `RepoRef` and never on the remote's claimed `repo_id`.
  `anchor::{read_from, accept, write_to}` are Phase 1's, called by
  `restore/mod.rs` and `fetch.rs`. Nothing here re-derives a high-water mark.
  `allow_rollback_never_rescues_a_bundle_identity_mismatch` writes an anchor at
  `push::anchor_path` and watches the pull read it, which is what proves the two
  sides agree on the path rather than merely both having one.
- **`--allow-rollback` is the same escape, with the same limit.** A lower counter
  of the same bundle is refused by default (naming the flag, in
  `anchor::accept`'s own words) and accepted under it; a `repo_id` mismatch is
  refused **with or without** the flag, asserted over both option sets.
- **The stale-keyfile refusal is not contradicted.** Nothing this plan prints
  mentions `sync push` or `sync rekey`, and a pull needs no local keyfile — so
  the machine that missed a rekey can still *restore* (the remote wrapper is the
  new one and the new password opens it); it is its next *push* that refuses,
  exactly as `upload::assert_keyfile_is_current` says.

---

## FLAG SEMANTICS THAT DID NOT DRIFT

- **A dry run is the absence of `apply`.** `run_with`'s `Pull` arm binds
  `dry_run: _` with a comment saying so. Every path calls `restore::run` with
  `apply: false` first, and a dry run downloads no file content (5-02).
- **`--force` does not grant `--force-credentials`**, and clap enforces
  5-03's rule that `--force-credentials` requires `--force` before a byte is
  read. `force_alone_stops_at_a_locally_newer_credential_before_the_backup_exists`
  drives one fixture through both: `--force` alone replaces the locally-newer
  *routine* and stops at the locally-newer *credential*, with the live token
  intact, the routine also untouched (the run stopped before any write, not part
  way through), and the backups directory empty. The same fixture with
  `--force-credentials` replaces both, names both, and prints the `tar -xzf`
  undo.
- **`--yes` does not answer the credential gate.**
  `the_credential_gate_takes_the_typed_word_and_nothing_else` sets
  `assume_yes: true` and types `yes` — refused; only `overwrite` passes.
- **`RejectedPath` is promoted by nothing.** This plan adds no flag that touches
  it: `--force` reaches `merge::decide` and never a refusal, and the CLI's only
  in-flight mutation is `opts.force_credentials = true` after an affirmative at
  the credential gate.
- **A declined credential gate re-plans rather than being hand-edited.** The
  affirmative sets the option and step 4 calls `restore::run` again; no
  disposition is mutated in place, so none can drift from its recorded reason.

---

## THE TWO INDEX-RECOVERY ESCAPES

**`index::reset_at(path) -> Result<Index>`** — refuses a directory *by name*
(never a recursive removal), removes the file (and its `-journal`/`-wal`/`-shm`
siblings, through the existing `discard`), and reopens through `Index::at`, which
creates the schema at mode 0600. It **removes the file rather than issuing SQL
against it**: an index corrupt enough to need this escape is one SQLite may
refuse to open, and a recovery path must not depend on the thing it is recovering
from (T-5-64). Absent is not an error. The doc comment points at `Index::at`'s
automatic version of the same idea.

**`Index::rehashing(self) -> Self`** — makes `lookup` and `cached` return `None`
for the life of the handle. It **suppresses reads and clears nothing** (T-5-65):
`a_rehashing_run_leaves_the_cache_warm_for_the_next_one` reopens the database
afterwards and plans again with `files_opened == 0`.

`a_rehashing_plan_is_the_plan_a_deleted_index_would_have_produced` asserts the
two are equal with `PartialEq` over the whole `SyncPlan`, not just through
`files_opened`, and its companion pins the non-vacuous half (a warm index without
the flag still opens nothing).

**A lost index never changes what a pull writes.**
`a_deleted_index_produces_an_identical_restore` runs the same snapshot onto two
machines — one with a warm index, one whose index file is deleted and rebuilt —
and compares exit code and every restored file byte for byte. Restore hashes what
is on disk and asks the index nothing, which is the property (T-5-63).

---

## Deviations from Plan

### [Rule 3 — blocking] `src/widget/cli.rs` was modified

The plan scoped this to `src/sync/cli.rs`, `src/sync/index.rs` and
`src/sync/plan.rs`. `SyncAction` — the clap subcommand enum — lives in
`src/widget/cli.rs`, and **the command cannot exist without a variant there**.
5-01's summary already assigned that file to this plan; the orchestrator's file
list did not. The edit is the `Pull` variant with its seven flags, two new flags
on `Push`, and the two `matches!` patterns in that file's own tests that name
`Push`'s fields exhaustively. No other code in the file changed.

### [Rule 1 — correctness] `--force-rehash` is **not** offered on `sync pull`

The plan says to wire both recovery flags onto `RestoreOptions` from the pull
arm. `--rebuild-index` is wired there and does something real. `--force-rehash`
is not, and here is why: **nothing on the pull path reads it.**
`grep -rn "force_rehash" src/sync/restore/` finds one hit — the field
declaration. Restore hashes what is on disk and never consults the index, which
is exactly the property the plan's own key_link states ("they change no restore
behaviour"). A `--force-rehash` on `pull` would therefore have been a flag with
no reader, which is the same defect as a printed command that does not exist —
the one this plan was written to close.

It is instead on `sync push`, where it changes what the planner does, and it is
reachable from `push`, `push --dry-run` and (through the shared handle) anything
else that opens the index. `RestoreOptions::force_rehash` is set to `false` in
the `Pull` dispatch with a comment saying it is read by nobody; it is listed as a
finding below rather than silently populated.

### [Rule 1 — correctness] The `force_rehash` switch sits on `Index`, not on `plan::build`'s arguments

The plan asks for a `force_rehash: bool` threaded into `plan::build`. Written
that way it reaches only the callers who remember to pass it, and the caller that
matters most — `push::run`, at `push/mod.rs:322` — would have needed a new field
on `PushCtx`, which four sibling modules' test fixtures construct as struct
literals and which 4-08 has just finished rewriting.

`Index::rehashing()` puts the switch on `lookup` and `cached` themselves, which
*are* "the one place that consults them" the plan names. Every planner is covered
— `sync push`, `push --dry-run`, `status` — no signature moved, no out-of-scope
file changed, and `plan::build_with_keys` is still the path the flag travels.

### `an_unreachable_remote_…` was replaced by a 403

A dead port is a retryable transport failure, so `pointer::load` spends the
production 60/120/240-second backoff before giving up — seven minutes inside the
AUR `check()`, which runs on installers' machines. That policy belongs to
`github::write::with_retry` and already has its own test. The pull arm's
"refuse and write nothing" property is asserted against a 403, which returns
immediately, alongside a 404 pointer, a wrong password, and a tampered pack.

### TDD gates

Task 1 has both: `67ed13d` is the RED commit (10 failing), `2bd94de` the GREEN.
Task 2 (`2b47a0a`) was committed as a single `feat` — its tests and
implementation were written together and no honest RED run preceded them.

---

## CALL-SITE AUDIT — every public function Phase 5 added

The mandate: enumerate the production call sites of every public function Phase 5
added and report any with zero. Comment lines and `#[cfg(test)] mod tests` blocks
were stripped before counting.

| symbol | production callers |
|---|---|
| `restore::run` | `sync/cli.rs` ×2 (the plan pass and the apply pass) |
| `layout::from_manifest_path` | `merge.rs`, `write.rs` |
| `layout::accept_for_write` | `merge.rs` |
| **`layout::to_manifest_path`** | **ZERO — see below** |
| `fetch::resolve` | `restore/mod.rs` |
| `merge::plan` | `restore/mod.rs` |
| `write::apply` | `restore/mod.rs` |
| `backup::take` | `restore/mod.rs` |
| `backup::take_with` | `backup::take` |
| `backup::rollback_command` | `BackupRecord::rollback_command`, `report.rs` |
| `report::render_plan` | `sync/cli.rs` ×2, `confirm_apply` |
| `report::render_outcome` | `sync/cli.rs` |
| `report::confirm_apply` | `sync/cli.rs` |
| `report::confirm_credentials` | `sync/cli.rs` ×2 (terminal and piped arms) |
| `report::APPLY_COMMAND` | `render_plan`'s footer |
| `report::MAX_ATTENTION_ITEMS` / `MAX_ITEM_LINES_PER_CATEGORY` | `report.rs` |
| `Disposition::writes` | `restore/mod.rs`, `merge.rs`, `write.rs`, `report.rs` |
| `BackupRecord::rollback_command` | `report.rs` |
| `PackSource::{empty, add, holds_pack, keys, packs, bytes, sealed}` | `fetch.rs`, `merge.rs`, `restore/mod.rs` |
| `PackSource::chunk` | `write.rs` |
| `index::reset_at` | `sync/cli.rs` (`maybe_reset_index`, from both `pull` and `push`) |
| `Index::rehashing` | `sync/cli.rs` (`rehashing`, from `push` and `push --dry-run`) |

### Finding 1 — `layout::to_manifest_path` has zero production callers

Every reference is a test inside `layout.rs`. The function delegates to
`push::packer::manifest_path`, which *is* on the push path, so it is a thin
mirror whose only purpose is the drift test that pins push and restore to the
same four prefixes. It is not a security control and no behaviour depends on it,
but it is a public function nothing calls. `src/sync/restore/layout.rs` is not
this plan's file. **Recommended for 5-08:** either delete it and have the drift
test call `push::packer::manifest_path` directly — which is what it actually
pins — or keep it with a doc comment saying it exists as the readable inverse of
`from_manifest_path` and is deliberately test-only.

### Finding 2 — `RestoreOptions::force_rehash` is written and never read

Set to `false` by the `Pull` dispatch, declared in `restore/mod.rs`, read by
nothing. 5-01 froze the field before it was known that restore would never
consult the index. **Recommended for 5-08:** delete the field, or document it in
`RestoreOptions` as reserved. It is not populated from a flag here precisely so
that no user is handed a switch that does nothing.

Nothing else in Phase 5's surface is uncalled. `sync pull` is the call site for
the rest of it.

---

## Known Stubs

None.

## Threat Flags

None. No new network endpoint, no new auth path, no new file-access pattern, no
schema at a trust boundary. This plan adds one new *write* trigger — the `pull`
command — and every byte it writes goes through `restore::run`'s existing,
already-audited path.

Register coverage:

| ID | mitigation | test |
|---|---|---|
| T-5-60 | the credential gate runs before the backup, and `--force` does not answer it | `force_alone_stops_at_a_locally_newer_credential_before_the_backup_exists` |
| T-5-61 | a decline leaves no archive — the backups directory is walked afterwards | same |
| T-5-62 | `--allow-rollback` is passed through to `anchor::accept`, whose `repo_id` arm errors regardless | `allow_rollback_never_rescues_a_bundle_identity_mismatch` |
| T-5-63 | a deleted index produces an identical restore | `a_deleted_index_produces_an_identical_restore` |
| T-5-64 | `reset_at` removes the file rather than issuing SQL against it | `reset_at_refuses_a_directory_and_removes_nothing`, `reset_at_replaces_a_populated_index_with_an_empty_private_one` |
| T-5-65 | `rehashing` suppresses reads and clears nothing | `a_rehashing_run_leaves_the_cache_warm_for_the_next_one` |
| T-5-66 | no `--password` flag, no env fallback; `sync_password` reads stdin only | the passphrase guard (`no_password_input_path_reads_the_process_environment`) scans `sync/cli.rs` |
| T-5-67 | every failure is 1 with one message; a declined apply gate is 0 | the four refusal tests plus `an_affirmative_at_the_terminal_…` |
| T-5-SC | no new crates | `git diff Cargo.toml Cargo.lock` empty |

---

## Verification

| gate | result |
|---|---|
| `cargo test` | **1512 lib / 1556 total, 0 failing** (baseline 1484 / 1528 — **+28**) |
| `cargo test --lib sync::cli` | 39 passed |
| `cargo test --lib -- sync::index sync::plan` | 52 passed |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `make test` | exit 0, plus the GNOME, KDE and Omarchy JS contract suites |
| `env -u HOME -u XDG_CONFIG_HOME -u XDG_CACHE_HOME cargo test --lib sync` | 539 passed — hermeticity holds |
| `Cargo.toml` / `Cargo.lock` | unchanged; no new crates |
| `cargo machete` | not installed on this machine; `Cargo.toml` byte-identical, so no dependency could have become unused |

Every new test is hermetic: both machines' roots are `TempDir`s, both
`Endpoints` point at one mockito server (or the parked `127.0.0.1:1`), `NOW` is
a fixed constant and the "locally newer" mtimes are stamped from it with
`set_modified` rather than read from the wall clock, KDF parameters are
`{ m_kib: 8, t: 1, p: 1 }` throughout, and no test reads a real `$HOME`, a real
token, or the macOS Keychain. The remote in every fixture is a bundle the
**push** side really produced (`plan::build_with_keys` → `packer::build` →
`packer::root_for`), so an adversarial case is one mutation of real bytes rather
than a hand-rolled fixture that could agree with a broken reader.

## Commits

| commit | what |
|---|---|
| `2b47a0a` | `feat(5-07)` — `index::reset_at`, `Index::rehashing`, and the four plan-level recovery tests |
| `67ed13d` | `test(5-07)` — the `Pull` clap variant and 10 failing tests (RED) |
| `2bd94de` | `feat(5-07)` — the `sync pull` arm, both gates, the stdin decision, and the push-side recovery wiring (GREEN) |

## Self-Check: PASSED

- `src/sync/cli.rs`, `src/sync/index.rs`, `src/sync/plan.rs`, `src/widget/cli.rs` — all present and modified.
- `2b47a0a`, `67ed13d`, `2bd94de` — all in `git log` on `gsd/5-07`.
- `git diff --diff-filter=D` across the three commits — no deletions.
- `ai-usagebar sync pull --help` renders; `ai-usagebar sync --help` lists `pull`.
