---
phase: 05-pull-and-restore
verified: 2026-08-20T05:30:00Z
status: human_needed
score: 7/7 ROADMAP success criteria verified
behavior_unverified: 0
overrides_applied: 0
gates:
  cargo_test: pass (1520 lib + 60 integration; 0 failed; 16 ignored, all live/calibration)
  cargo_clippy: pass (--all-targets -D warnings, clean)
  cargo_fmt: pass (--check, clean)
  make_test: pass (incl. GNOME, KDE, Omarchy JS contract suites)
  verified_at_head: 5f1da8e
uncalled_public_functions: 0
negative_controls_run: 6
human_verification:
  - test: "Run the real two-machine flow against a real private GitHub repo and a real token: `sync init` + `sync push` on machine A, then `sync pull --apply` on machine B, and confirm you can continue working."
    expected: "B's tree matches A's, credentials open, Claude Code and the Desktop app resume on B without re-login."
    why_human: "Every test in the phase is mockito. Not one byte has ever crossed to api.github.com or uploads.github.com on this path. The phase goal is a real second machine; the suite proves the pair agrees with itself against a fake remote."
  - test: "Run `ai-usagebar sync pull` interactively at a real terminal, and again piped (`echo \"$PASSWORD\" | ai-usagebar sync pull --apply --yes`)."
    expected: "Interactive: password prompt, then the apply gate, then the credential gate if any. Piped: no prompt, password consumes the first line, flags answer the gates."
    why_human: "`sync::cli::pull` — the arm that reads the real process stdin and makes the `is_terminal()` decision — is private and untested by integration. Only `pull_with_parts` below it is covered. The e2e file says so explicitly in its module docs."
  - test: "CAL-1 (`cal1_range_on_private_release_asset`) and CAL-5 (`cal5_release_asset_state_and_digest`)."
    expected: "A measurement, or a deliberate decision to leave them unmeasured into v2."
    why_human: "Both need a throwaway private repo and a token; CAL-5 additionally writes and deletes release assets. Both correctly remain `#[ignore]`d and no doc claims either was measured."
warnings:
  - id: W1
    severity: warning
    kind: text-asserting-behaviour-that-does-not-exist
    statement: "5-08-PLAN must_have and 5-08-SUMMARY's table heading both claim the seven ROADMAP success criteria map one-to-one onto seven named tests whose names say which criterion they are. The `criterion_N` numbering is the plan's own enumeration, not the ROADMAP's, and the SUMMARY's row 7 is documentation rather than a criterion."
    impact: "Coverage is complete — every ROADMAP criterion has a real passing test — but a reader auditing ROADMAP criterion 7 via the SUMMARY table lands on docs instead of the interruption test."
  - id: W2
    severity: warning
    kind: thin-test-net-on-a-security-control
    statement: "The SAFE-04 ordering (backup::take before write::apply) is defended by exactly one test. With the two calls swapped the entire 1520-test lib suite stays green; only the integration test `criterion_4_*` fails."
    impact: "No behavioural defect. One deleted or skipped integration test would leave the archive-before-write control undefended."
  - id: W3
    severity: info
    kind: defence-in-depth-not-visible-to-the-e2e-test
    statement: "Removing the `is_symlink()` arm of `merge::not_a_plain_file` leaves the e2e symlink test green, because the `else` catch-all still refuses. Only the unit test at src/sync/restore/merge.rs:960, which pins the message, catches it."
    impact: "The control is layered and working. Recorded so nobody 'simplifies' the catch-all arm away believing the e2e test covers it."
---

# Phase 5: Pull and Restore — Verification Report

**Phase Goal (ROADMAP):** A second machine reproduces the user's state from the
remote — and a restore that would clobber something newer says so first, backs it
up, and can be undone.

**The user's goal, as stated for this verification:** push from one MacBook,
restore onto a second one, and end up able to continue where they stopped.

**Verified:** 2026-08-20 · **At HEAD:** `5f1da8e` · **Status:** `human_needed`
**Re-verification:** No — initial verification.

---

## Gates, re-run by the verifier

Not read from a SUMMARY. Run in this process, at the HEAD above, with the tree
clean before and after.

| Gate | Result |
|---|---|
| `cargo test` | **pass** — 1520 lib, 3 + 13 + 15 + 16 + 13 integration, 0 failed, 16 ignored |
| `cargo clippy --all-targets -- -D warnings` | **pass** — clean |
| `cargo fmt --check` | **pass** — clean |
| `make test` | **pass** — plus GNOME, KDE and Omarchy JS contract suites |

All 16 ignored tests are the live/calibration set (`--test live`); nothing in the
default `cargo test` set — which the AUR `check()` runs on an installer's
machine — reaches the network, a real `$HOME`, or the Keychain.

**A note on the tree.** HEAD moved during this verification: a concurrent process
committed Phase 6 work (`46cff6c`…`5f1da8e`). `git diff bfb49ef..HEAD` touches only
`src/widget/run.rs` and a planning document — **none of the six defects injected
below leaked into a commit**, and the gate results above were re-run after that
move. The working tree is clean at the end of this report.

---

## Goal achievement — the ROADMAP's seven success criteria

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 1 | `sync pull` into an empty injected home reproduces the pushed tree byte-for-byte, every credential at 0600 | ✓ VERIFIED | `criterion_1_a_pushed_tree_restores_byte_for_byte_under_a_second_machines_roots` (tests/sync_restore_e2e.rs:1049). Machine B's four roots have different leaf names, under a different username, in a separate `TempDir`. Asserts ≥8 files across every category, byte equality, `mode == 0o600` for **every** landed file, created directories at 0700, no path containing `alice`, and the anchor advanced from the root's own sealed counter. |
| 2 | `sync pull --dry-run` lists every file created / overwritten / skipped, and writes nothing | ✓ VERIFIED | `a_dry_run_writes_nothing_at_all_and_never_downloads_a_pack_it_only_needs_for_file_data` (e2e:1200) + `sync::cli::tests::a_pull_with_no_flags_and_no_terminal_writes_nothing_and_names_the_flag` (cli.rs:2349). The listing itself is `report::render_plan`, exercised by 8 unit tests with a per-category line budget. |
| 3 | A locally-newer credential is reported before any write and is not silently overwritten | ✓ VERIFIED | `force_alone_never_overwrites_a_live_credential_and_force_credentials_needs_force` (e2e) + `sync::cli::tests::force_alone_stops_at_a_locally_newer_credential_before_the_backup_exists`. `report::render_plan` surfaces attention items **before** the gate. |
| 4 | The pre-restore backup exists before the first byte, and its printed rollback command restores the prior state exactly | ✓ VERIFIED | `criterion_4_the_backup_precedes_the_first_write_and_its_printed_command_restores_exactly`. It extracts the archive, compares members against pre-restore bytes, clobbers the destinations, then executes the rendered `tar -xzf … -C …` through `/bin/sh` and compares **contents and modes**. |
| 5 | A rolled-back snapshot, a tampered pack, or a manifest naming a missing chunk refuses and writes **zero** files | ✓ VERIFIED | Four tests: `criterion_3_a_rolled_back_pointer_is_refused_and_prune_does_not_collect_its_orphans`, `..._a_tampered_pack_refuses_and_leaves_no_plaintext_under_machine_b`, `..._a_snapshot_naming_a_pack_the_release_withholds_refuses_and_writes_nothing`, `..._a_manifest_entry_that_escapes_its_root_is_refused_and_named_in_the_report`. Emptiness asserted by **walking root B**, not by the return value. |
| 6 | Two machines that edited the same routine converge on the newer one, and the overwritten value is named in the report | ✓ VERIFIED | `sync_06_a_newer_local_file_is_skipped_and_named_and_force_overwrites_and_names_it`, which asserts `report::render_outcome(&forced)` contains `scheduled-tasks/daily routine.json` (e2e:1462). A list, not a count. |
| 7 | Killing the process mid-restore leaves no plaintext outside the destination directory and no half-written credential | ✓ VERIFIED | `criterion_6_an_interrupted_restore_leaves_no_half_written_file_and_no_anchor_move`. |

**Score: 7/7.** Each is behaviour-dependent and each has a passing behavioural
test, run by the verifier — none is VERIFIED on symbol presence.

---

## The repeated defect: code that is tested but that nothing calls

Every public and `pub(crate)` item added in the Phase 5 range
(`5db4db6..HEAD`, 51 items) was enumerated from the diff and each was grepped for
a **production** call site — a use outside a `#[cfg(test)]` module.

**Zero orphans.** The full enumeration and its production call site:

| Symbol | Production call site |
|---|---|
| `restore::run` | `sync/cli.rs:934` (plan pass), `:1011` (apply pass) |
| `fetch::resolve` | `restore/mod.rs:367` |
| `merge::plan` | `restore/mod.rs:370` |
| `write::apply` | `restore/mod.rs:396` |
| `backup::take` | `restore/mod.rs:393` |
| `backup::take_with` | `backup.rs:50` (via `take`) |
| `backup::rollback_command` | `restore/mod.rs:200` (via `BackupRecord::rollback_command`) |
| `BackupRecord::rollback_command` | `report.rs:144`, `:171` |
| `layout::from_manifest_path` | `merge.rs:192` **and** `write.rs:135` (defence in depth at the write boundary) |
| `layout::accept_for_write` | `merge.rs:189` |
| `report::render_plan` | `cli.rs:949`, `:959`, and `report.rs:195` inside `confirm_apply` |
| `report::render_outcome` | `cli.rs:1015` |
| `report::confirm_apply` | `cli.rs:954` |
| `report::confirm_credentials` | `cli.rs:979`, `:981` |
| `report::APPLY_COMMAND` | `report.rs:532` |
| `report::MAX_ITEM_LINES_PER_CATEGORY` | `report.rs:497`, `:505` |
| `report::MAX_ATTENTION_ITEMS` | `report.rs:265`, `:369`, `:416` (+ bounds) |
| `Disposition::writes` | `write.rs:119`, `restore/mod.rs:390`, `merge.rs:380`, `report.rs:500`, `:686` |
| `PackSource::{empty,add,holds_pack,keys,packs,bytes,sealed,chunk}` | `fetch.rs:287/450/311/295/433/440/472`, `write.rs:208` |
| `index::reset_at` | `cli.rs:812` |
| `Index::rehashing` | `cli.rs:295`, `:599` |
| `plan::build_with_keys` | `cli.rs:411`, `push/mod.rs:322` |
| `push::anchor_path` | `cli.rs:919`, `push/mod.rs:500`, `:513` |
| `push::assert_no_rollback` | `push/mod.rs:312`, `rekey.rs:111`, `prune.rs:248` |
| `upload::assert_keyfile_is_current` | `push/mod.rs:318`, `upload.rs:222` |
| `packer::highest_counter` | `packer.rs:411`, `push/mod.rs:501`, `:511` |
| `packer::root_for` | `push/mod.rs:373` |
| `packer::manifest_path` | `packer.rs:118` |
| `gate::Pushing::covers` | `github/write.rs:261`, `:368`, `:452`, `:624` |
| `guard::{rs_files,rs_files_in,production_code}` | test-only guards, by design (`crypto.rs:1301`, `passphrase.rs:449`, `github/mod.rs:385`, `gate.rs:778`) |

The two already-deleted surfaces stay deleted: `RestoreOptions` carries **no**
`force_rehash` field, and the `Pull` clap variant declares no `--force-rehash`
flag — the match arm at `cli.rs:112` destructures only the six that exist, and
records in a comment why the seventh is absent.

**The class is not closed, though.** Commit `8d63023`, a Phase 6 commit that
landed during this verification, is titled *"the wrapper that had no caller"*.
That is instance eleven. The enumeration above is clean for Phase 5's own
surfaces; it says nothing about Phase 6.

---

## The ten specific behaviours, each checked in merged code

| Claim | Status | Where |
|---|---|---|
| A dry run writes nothing and fetches no data pack — structural, not a flag check | ✓ | `fetch.rs:305` gates the **third** download round behind `if ctx.opts.apply`; rounds 1–2 (index object, manifest) are metadata only. `restore/mod.rs:373` returns before the write half. The write path is not reachable past that early return. |
| A bundle pushed under one username restores under a different one; the manifest holds root-prefixed relative paths and `manifest_path` **errors** rather than falling back to absolute | ✓ | `packer::manifest_path` (packer.rs:428) returns `Err("refusing to record … it lies under none of the sync roots")`. `layout::from_manifest_path` refuses absolute, `..`, NUL, backslash, drive-letter, unknown prefix — before touching the filesystem, building the result one component at a time. `criterion_1` asserts no landed path contains `alice`. |
| Two machines racing publish at **distinct** counters and both survive | ✓ | `packer::root_for:411` derives the counter from the **arriving** pointer (`highest_counter(arriving, …) + 1`), and `push::run:373` calls it inside the CAS rebuild closure, so it re-derives on every 409. `two_machines_racing_publish_distinct_counters_and_a_pull_takes_the_newest` arms a real 409 with the winner's sha and asserts the published counters are unique. |
| Snapshot selection breaks ties on the root's sealed `created_at`, not the pointer's list order | ✓ | `fetch.rs:214` — `(root.counter, root.created_at, &record.root) > (best.counter, best.created_at, &best_record.root)`. Both leading terms come out of the **opened** root. Confirmed by negative control NC-2. |
| The anchor is read on all three paths that publish a pointer | ✓ | `push::run` → `push/mod.rs:312`; `rekey` → `rekey.rs:111`; `prune::run_on_demand` → `prune.rs:248`. All three call the one `super::assert_no_rollback`. |
| A machine with a stale keyfile is refused a push, not silently skipped | ✓ | `upload::assert_keyfile_is_current` at `push/mod.rs:318` (before the packer, so the refusal costs nothing) **and** again at `upload.rs:222`. `a_machine_that_missed_a_rekey_is_refused_a_push_and_can_still_pull` proves the refusal *and* that the machine is not stranded — a pull consults no local keyfile. |
| `--force` alone never overwrites a live credential; that needs `--force-credentials` too | ✓ | `merge::decide` — the `credential && !opts.force_credentials` arm returns `NeedsCredentialConfirm`, never `Overwrite`. `confirm_credentials` never reads `assume_yes` (pinned by a source-scanning test at report.rs:1173). clap enforces `requires = "force"` on `--force-credentials`. `write::apply` **errors** if a `NeedsCredentialConfirm` item ever reaches it. Confirmed by NC-4. |
| A symlinked destination is `RejectedPath` and no flag promotes it | ✓ | `merge::local_at` uses `fs::symlink_metadata`, never `fs::metadata`. `RejectedPath` is the one disposition carrying a reason and `Disposition::writes()` returns false for it — `--force` promotes only `SkipLocalNewer`. The e2e test loops all three flag combinations and asserts the link and its target are untouched. Confirmed by NC-3b. |
| Both confirmation gates precede `backup::take`, so a decline leaves neither archive nor write | ✓ | Structural: `backup::take` lives inside `restore::run` at step 5, and `restore::run` is only invoked with `apply: true` at `cli.rs:1011` — **after** both gates. `force_alone_stops_at_a_locally_newer_credential_before_the_backup_exists` asserts `files_under(&backups_dir(&roots)).is_empty()` after the decline. |
| Piped, the passphrase owns stdin and the gates get no reader | ✓ | `cli.rs:860` — `interactive = stdin().is_terminal()`; when false, `PullIo { gate: None }` and the credential gate is handed `std::io::empty()`, which reaches EOF and refuses. `sync_password` reads `stdin().lock()` on both paths and rejects an empty password before spending a gibibyte on Argon2id. |

---

## Documentation, checked sentence by sentence against merged code

### `docs/sync-format.md` §11

Every falsifiable claim was checked. All hold.

| Claim | Verified against |
|---|---|
| Seven-step order, anchor read at 1, advanced at 7 | `restore/mod.rs:362-418` — identical order, identical rationale |
| The four ceilings: 256 snapshots, 128 manifest chunks, 256 index chunks, 512 packs | `fetch.rs:69,86,97,110` — exact match |
| Byte ceiling = 256 × `PACK_MAX` = 12 GiB | `fetch.rs:113` `MAX_RESTORE_BYTES: u64 = 256 * PACK_MAX as u64`; `pack.rs:75` `PACK_MAX = 48 MiB`. 256 × 48 MiB = 12 GiB ✓ |
| "512 packs at `PACK_MAX` is 24 GiB, twice the byte ceiling" | arithmetic checks ✓ |
| "at `PACK_TARGET`, 512 packs is 16 GiB" | `PACK_TARGET = 32 MiB`; 512 × 32 MiB = 16 GiB ✓ |
| "`should_seal` compares against `PACK_MAX` and never reads `PACK_TARGET`" | `pack.rs:273-275` — literally `current_len.saturating_add(next_blob_len) > PACK_MAX` ✓ |
| "the pointer's `offset`, `clen`, `true_len` are never used to slice" | no such read in `fetch.rs`; pinned by `the_pointers_unauthenticated_offsets_never_index_into_anything` which sets all three to `MAX` and asserts inertness. `merge.rs:388` reads `clen` from the **authenticated index object**, for a display estimate, not to slice ✓ |
| "A restore can never change the remote. Four read verbs and no write verb" | four reads: `pointer::load`, `find_release`, `asset_index`, `download_asset`. `fetch.rs` imports only two **constants** from `github::write`, no verb. `find_release` documents why it is not `ensure_release` ✓ |
| The eight-row disposition table, first match wins | `merge::decide` — row for row, including "equal is not newer" (`local_mtime <= remote_mtime` ⇒ `Update`) ✓ |
| "Restored files are 0600, directories 0700, the manifest's mode is not consulted" | `write.rs` chmods the tempfile **before** the first byte; `packer::mode_of`'s recorded mode is never read on the restore side ✓ |
| "the archive is chmodded 0600 the moment it exists, inside a 0700 directory made before `tar` creates anything" | `backup.rs` ✓ |
| "A restore that creates everything and overwrites nothing has no archive, and says so" | `backup::take` returns `Option`; `report.rs:147` renders the `None` case ✓ |
| The self-correcting note — "An earlier version of this comment said there was [a monthly retention tail], and `grep -rn monthly src/sync/` finds nothing" | `grep -rn monthly src/sync/` returns nothing ✓. The comment is honest about its own prior error. |

### `docs/configuration.md` — the `sync pull` section

The flag table matches `widget/cli.rs`'s `Pull` variant exactly, including the
two constraints clap enforces:

- `--dry-run` carries `conflicts_with = "apply"` ✓ (pinned by `apply_and_dry_run_together_are_refused_by_clap`)
- `--force-credentials` carries `requires = "force"` ✓ (same test)
- "There is no `--force-rehash` on `sync pull`, deliberately" ✓ — no such field on `RestoreOptions`, no such arg on `Pull`
- "A piped run with none of them prints the plan, names `--apply`, and exits 0" ✓ — `a_pull_with_no_flags_and_no_terminal_writes_nothing_and_names_the_flag` asserts exit 0 and the presence of `report::APPLY_COMMAND`
- "the credential one wants the word `overwrite` typed out" ✓ — `the_credential_gate_takes_the_typed_word_and_nothing_else` proves `yes\n` is rejected
- "It never deletes" ✓ — no `fs::remove*` anywhere in `src/sync/restore/`; `merge`'s module doc records the non-deletion as a decision
- "`keep_snapshots`: zero refused at load, `prune` clamps to at least 1" ✓ — `config.rs:1093` and `prune.rs:93` (`keep.max(1)`)
- "the keyfile sits next to `config.toml`, at `sync/keyfile.json`" ✓ — `cli.rs:426`

### `README.md`

Two changes, both accurate: the sentence that previously stopped mid-word
("…no permission that could") now completes, and the new `sync pull` bullet
states the dry-run default, the skip-and-name behaviour, the
`--force` + `--force-credentials` pairing, and the archive — all four verified
above.

---

## Negative controls — six run by the verifier, all reverted

Every injection was made in `src/`, the named test watched to go red, and the
edit undone with `git checkout`. **`git status --porcelain` is empty at the end
of this report**, `cargo fmt --check` and `cargo clippy -D warnings` are clean,
and `git diff bfb49ef..HEAD` confirms nothing I injected reached a commit despite
a concurrent process committing mid-audit.

| # | Injection | Result |
|---|---|---|
| NC-1 | `fetch::resolve`'s third round: `if ctx.opts.apply` → `if true` | **RED** — `a_dry_run_writes_nothing_at_all_and_never_downloads_a_pack_it_only_needs_for_file_data` failed at e2e:1246 |
| NC-2 | Tiebreak drops the sealed `created_at`: `(counter, created_at, root)` → `(counter, root)` | **RED** — `two_snapshots_at_one_counter_resolve_the_same_way_in_either_order` failed: *"a tied counter let the plaintext list's order pick the snapshot"* |
| NC-3a | `not_a_plain_file`'s `md.file_type().is_symlink()` → `false` | **RED at unit level** (`a_symlink_at_the_destination_is_refused_under_every_force`), **but the e2e symlink test stayed green** — the `else` catch-all still refuses. Recorded as W3; the control is layered, the e2e test alone does not see the narrowing. |
| NC-3b | `merge::local_at`: `fs::symlink_metadata` → `fs::metadata` (the real-world defect) | **RED** — `a_symlink_at_a_destination_is_refused_and_no_flag_promotes_it` failed at e2e:1982 |
| NC-4 | `merge::decide`'s credential arm disabled so `force` alone satisfies it | **RED** — three tests failed: `merge::tests::force_alone_never_overwrites_a_locally_newer_credential`, `cli::tests::force_alone_stops_at_a_locally_newer_credential_before_the_backup_exists`, `cli::tests::the_credential_gate_takes_the_typed_word_and_nothing_else` |
| NC-5 | `restore::run`: `backup::take` moved **after** `write::apply` | **RED** — `criterion_4_…` failed at e2e:1334. But the **lib suite stayed fully green (1520 passed)**. Recorded as W2. |
| NC-6 | `restore::run` step 7's `applied.failed_at.is_none()` → `true` | **RED** — `criterion_6_an_interrupted_restore_leaves_no_half_written_file_and_no_anchor_move` failed at e2e:1022 |

NC-4 and NC-6 independently reproduce two of the four controls `5-08-SUMMARY`
reported. They are real.

---

## CAL-1 and CAL-5

Both still unrun, and correctly so.

- `cargo test --test live -- --list --ignored` lists 16 tests, **all** ignored, including `cal1_range_on_private_release_asset` and `cal5_release_asset_state_and_digest`. Neither is reachable from a default `cargo test`.
- `tests/live.rs:723` — `#[ignore = "live API; needs a throwaway private repo and token — run with --ignored"]`
- `tests/live.rs:1491` — `#[ignore = "live API; writes and deletes release assets — run with --ignored"]`
- `docs/sync-format.md:594` — *"Still not measured, and nothing below is a measurement… Phases 4 and 5 did not run it either. No status code, no `Content-Range`, no byte count."*
- `docs/sync-format.md:627` — *"Not measured either, at the end of Phase 5, and for the same reason as CAL-1."*
- `docs/sync-calibration.md:164-166` records CAL-1 as offered-and-declined.

**No doc implies either was measured.** `docs/sync-format.md:336` is careful to
call `PACK_TARGET` advisory precisely *because* CAL-1's fallback is unmeasured.

---

## Anti-patterns

`grep -rn "TODO|FIXME|XXX|TBD|HACK|PLACEHOLDER|unimplemented!|todo!"` over
`src/sync/` and `tests/sync_restore_e2e.rs` returns **nothing**. No debt markers,
no leftover `// TEMP:` from any process, mine or concurrent.

---

## Warnings

### W1 — the criterion mapping table names the wrong criteria

`5-08-PLAN.md`'s must_have reads:

> "The seven ROADMAP success criteria for this phase map one-to-one onto seven
> named tests, and the test names say which criterion they are."

and `5-08-SUMMARY.md` heads its table **"THE SEVEN ROADMAP CRITERIA, AND THE TEST
THAT PROVES EACH."** Neither is true as written. The `criterion_N` numbering in
the test names is the *plan's own* enumeration of its must_haves, and it collides
with the ROADMAP's numbering rather than matching it:

| ROADMAP # | What it says | Test actually covering it | Its name says |
|---|---|---|---|
| 1 | round trip, 0600 | `criterion_1_…` | 1 ✓ |
| 2 | dry-run lists and writes nothing | `a_dry_run_writes_nothing_…` | *unnumbered* |
| 3 | locally-newer credential reported first | `force_alone_never_overwrites_…`, `sync_06_…` | *unnumbered* |
| 4 | backup before first byte | `criterion_4_…` | 4 ✓ |
| 5 | rollback / tampered / missing chunk refuse | four `criterion_3_…` tests | **3** |
| 6 | two machines converge on the newer routine | `sync_06_…` | *unnumbered* |
| 7 | kill mid-restore | `criterion_6_…` | **6** |

`criterion_2_…` (idempotence) and `criterion_5_…` (the anchor) are D7 and a
Phase-1 carried risk respectively — good tests, but not ROADMAP criteria. The
SUMMARY's row 7 is *"the two-machine flow is documented"*, which is not a
criterion at all.

**Coverage is not the problem — labelling is.** Every ROADMAP criterion has a
real, passing, behaviourally-meaningful test, verified above. But this is the
same class the milestone keeps producing: prose asserting a property of the code
that the code does not have. Someone auditing ROADMAP criterion 7 through the
SUMMARY's table is sent to a documentation row.

**Suggested fix (not applied — verifiers do not edit):** rename the six
`criterion_N_*` tests to the ROADMAP's numbering, or rename them to
`sc_N_*` / drop the numbers and let the SUMMARY carry an explicit
ROADMAP-criterion → test-name table.

### W2 — the archive-before-write ordering has a one-test net

NC-5 swapped `backup::take` and `write::apply` in `restore::run`. The whole
1520-test **lib** suite stayed green; only the integration test
`criterion_4_the_backup_precedes_the_first_write_and_its_printed_command_restores_exactly`
failed. SAFE-04's ordering is a security property with exactly one guard.

`sync::cli::tests::force_alone_stops_at_a_locally_newer_credential_before_the_backup_exists`
does *not* catch it — it pins a different property (the **gates** precede
`backup::take`, which NC-5 leaves intact).

### W3 — the symlink refusal's catch-all masks a narrowing

Removing the `is_symlink()` arm of `merge::not_a_plain_file` entirely leaves the
e2e symlink test green: `symlink_metadata` on a link reports neither dir nor
regular file, so the `else` arm still returns `Some("neither a regular file nor a
directory")` and the disposition is still `RejectedPath`. Only the unit test at
`merge.rs:960`, which asserts the *message* names a symbolic link, catches it.

This is defence in depth working. It is recorded so that a future simplification
of the `else` arm is understood to remove a real layer.

---

## What is not proven, and needs a human

The three items in the frontmatter's `human_verification` block, restated:

1. **The phase goal has never been exercised against a real remote.** Every one
   of the 60 integration assertions runs against `mockito`. The two-machine
   round trip is genuine in every respect *except* that the GitHub on the other
   end is a fake one this repository wrote. The user's stated goal — push from
   one MacBook, restore onto a second, continue working — requires one real run.
   Nothing in the code suggests it will fail; nothing in the test suite proves it
   will not.

2. **The real-stdin arm of `sync pull` is untested.** `sync::cli::pull` — the
   function that calls `stdin().is_terminal()` and reads the process's real
   standard input — is private and covered by nothing. Only `pull_with_parts`
   beneath it is tested. The e2e module docs state this deliberately, with a
   sound reason (the AUR `check()` runs `cargo test` with a real stdin on an
   installer's machine). The interactive prompts and the piped path both need
   one manual run.

3. **CAL-1 and CAL-5.** Still unmeasured after five phases. Not a blocker —
   whole-pack fetch is correct either way, and both CAL-5 branches ship the
   conservative arm — but the decision to carry them into v2 unmeasured should be
   made deliberately rather than by default.

---

## Verdict

**7/7 ROADMAP success criteria verified against merged code and re-run tests.
Zero blockers. Zero uncalled public functions in the phase's own surfaces. Six
independent negative controls run and reverted, tree clean.**

The restore path is the most carefully-built thing in this milestone: the safety
ordering is structural rather than conventional, the ceilings are derived rather
than guessed, the hostile-input boundary is one function and it refuses before
touching the filesystem, and the two places a reader could believe an
unauthenticated field both explicitly do not.

Status is `human_needed` rather than `passed` for one reason: **the phase's goal
is a second physical machine, and no byte of this feature has ever crossed to
github.com.** That is a verification the codebase cannot perform on itself.

---

_Verified: 2026-08-20T05:30:00Z at `5f1da8e` · Verifier: Claude (gsd-verifier)_
_Gates re-run in-process: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `make test` — all pass._
_Working tree clean; six injected defects all reverted and confirmed absent from `bfb49ef..HEAD`._
