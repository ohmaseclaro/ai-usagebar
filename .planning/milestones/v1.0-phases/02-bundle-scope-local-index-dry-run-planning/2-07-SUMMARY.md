---
phase: 02-bundle-scope-local-index-dry-run-planning
plan: 07
subsystem: cli
tags: [sync, dry-run, cli, projection, zstd, padding, dedup, hermetic-tests]

requires:
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 01
    provides: "`SyncRoots`, `StatusReport`/`render_status`, `SyncAction`, the `sync` CLI entry point"
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 05
    provides: "`plan::build` and `SyncPlan` — the object this renders"
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 06
    provides: "CAL-4: project stored size as `frame(window).len() + 40`, never a compression ratio"
  - phase: 01-encrypted-bundle-core
    provides: "`Keys::chunk_id`, `chunk::frame`, `chunk::seal_chunk`, `Keyfile::open`, `passphrase::read_line`"
provides:
  - "`ai-usagebar sync push --dry-run` — D4's per-category files / raw bytes / would-send, totals, snapshot size"
  - "`plan::build_with_keys` — the single call site binding Phase 1's keyed BLAKE3 to the planner"
  - "`plan::SEAL_OVERHEAD`, `FilePlan::new_stored_bytes`, `CategoryPlan::new_stored_bytes`, `SyncPlan::total_new_stored_bytes` — the measured would-upload figure"
  - "`report::DryRunReport` / `render_dry_run`; `StatusReport.plan` so `sync status` shows the same figures"
affects: [phase-3-setup, phase-4-push]

tech-stack:
  added: []
  patterns:
    - "The would-upload figure is *measured*: every new chunk is really framed with Phase 1's `chunk::frame` while its plaintext is in hand. No ratio is ever applied to raw bytes."
    - "One model, two renderers — `sync status` and `sync push --dry-run` cannot print different numbers for the same question because they read the same `StatusReport`."
    - "A missing key removes a column and prints why; it never prints a zero. `0 bytes` and `not computed` are opposite answers to 'what will this cost me'."

key-files:
  created: []
  modified:
    - src/sync/plan.rs
    - src/sync/report.rs
    - src/sync/cli.rs
    - src/widget/cli.rs

key-decisions:
  - "The projection is `frame(chunk).len() + SEAL_OVERHEAD` per new chunk, computed inside `plan::build` while the chunk is already in the read buffer. Any other placement means re-reading every file. `build` therefore compresses now — it still never encrypts and never transmits."
  - "`SEAL_OVERHEAD = 24 + 16` is a literal because `crypto`'s `NONCE_LEN`/`TAG_LEN` are private, and it is pinned against a really-sealed chunk at four lengths so it cannot drift into a quietly wrong estimate."
  - "`build_with_keys` takes `&Keys`, not the plan's `master_key`. `subkeys()` and `name_key` are private to `crypto`; taking a raw master key would mean exporting them, and `Keys` already holds all three subkeys zeroizing, private and `Debug`-redacted. Strictly less key material in flight (T-2-29)."
  - "`build_status` gained `plan: Option<SyncPlan>` and derives its rows from the plan when it has one, rather than scanning a second time. Two walks of the same tree can disagree if a file appears between them, and the report would then print two numbers for one fact. `CategoryPlan` carries `capped` for exactly this."
  - "`DryRunReport` is `{status, no_key}`, not the plan's `{plan, index_rebuilt}`. `index_rebuilt` moved onto `SyncPlan` in 2-05, and the field the phase actually needs is the reason the would-upload column is missing."
  - "The keyfile is read from `<config_dir>/sync/keyfile.json`, derived from the injected `SyncRoots`. **Read-only** — creating it belongs to the guided setup that pairs the repo and sets the password (phase 3). This plan builds no setup flow, no interactive prompt, and no keyfile writer."
  - "The password comes from stdin only, and an empty stdin is refused *before* the KDF. Without that guard an unattended run with stdin on /dev/null would spend a gibibyte and ~1.5 s hashing the empty string before being told it was wrong."

patterns-established:
  - "A calibration figure and a user-facing figure that disagree are reconciled, not averaged: the disagreement here turned out to be cross-file dedup, and both numbers are right for their own question."
  - "Multi-line refusals wrap under their heading (`note()`), because the reasons a column is missing run past a terminal width."

requirements-completed: [SCOPE-03, SCOPE-04, UX-02]

coverage:
  - id: SCOPE-04
    description: "`sync push --dry-run` prints per category the file count, raw bytes and the bytes that would actually upload, then totals and the snapshot size"
    verification:
      - kind: unit
        ref: "src/sync/report.rs#a_dry_run_renders_files_raw_bytes_and_would_send_per_category"
        status: pass
      - kind: unit
        ref: "src/sync/report.rs#the_rendered_totals_are_the_plans_own_figures"
        status: pass
      - kind: manual
        ref: "./target/release/ai-usagebar sync push --dry-run — 1649 files, 99.4 MiB, 18.0 MiB would send"
        status: pass
    human_judgment: false
  - id: SCOPE-04b
    description: "A dry-run with no key still produces per-category counts and raw bytes and names what is missing, rather than a wrong zero"
    verification:
      - kind: unit
        ref: "src/sync/report.rs#without_a_key_the_counts_still_render_and_the_missing_column_is_named"
        status: pass
      - kind: unit
        ref: "src/sync/cli.rs#a_missing_keyfile_explains_itself_without_failing_the_command"
        status: pass
    human_judgment: false
  - id: SCOPE-03
    description: "The would-upload figure is Phase 1's real framed-and-sealed size, not a compression ratio applied to raw bytes"
    verification:
      - kind: unit
        ref: "src/sync/plan.rs#the_projection_is_the_framed_size_not_a_compression_ratio"
        status: pass
      - kind: unit
        ref: "src/sync/plan.rs#an_incompressible_chunk_stores_more_than_its_plaintext"
        status: pass
      - kind: unit
        ref: "src/sync/plan.rs#sealing_a_chunk_costs_exactly_the_framed_size_plus_the_overhead"
        status: pass
    human_judgment: false
  - id: SYNC-02
    description: "A second dry-run over an unchanged tree reports zero bytes to upload and zero files opened"
    verification:
      - kind: unit
        ref: "src/sync/plan.rs#a_no_op_projects_zero_stored_bytes_under_the_real_chunker"
        status: pass
      - kind: unit
        ref: "src/sync/report.rs#the_rendered_totals_are_the_plans_own_figures"
        status: pass
      - kind: manual
        ref: "second pass over the real bundle: 0 files opened, 0 B, 0.5 s vs 1.7 s"
        status: pass
    human_judgment: false
  - id: UX-02
    description: "`sync status` shows the same would-change figures plus the last-sync time"
    verification:
      - kind: unit
        ref: "src/sync/report.rs#no_last_sync_renders_as_never"
        status: pass
      - kind: manual
        ref: "./target/release/ai-usagebar sync status — same table, plus `last sync: never` and the index path"
        status: pass
    human_judgment: false
  - id: T-2-31
    description: "`sync push` without `--dry-run` exits non-zero with an actionable message and attempts nothing"
    verification:
      - kind: unit
        ref: "src/sync/cli.rs#a_push_without_dry_run_refuses_non_zero_and_points_at_the_dry_run"
        status: pass
      - kind: unit
        ref: "src/widget/cli.rs#sync_subcommands_parse_and_the_dry_run_flag_is_opt_in"
        status: pass
    human_judgment: false
  - id: D3-reporting
    description: "`excluded_files`/`excluded_bytes` render for transcripts only, and never as '30 days'"
    verification:
      - kind: unit
        ref: "src/sync/report.rs#only_transcripts_report_what_the_bounds_left_behind"
        status: pass
    human_judgment: false

duration: 75min
completed: 2026-08-19
status: complete
---

# Phase 2 / Plan 07: `sync push --dry-run` Summary

**The command runs end to end on this machine's real bundle: 1649 files, 99.4 MiB
raw, **18.0 MiB would send**, in 1.7 s — and a second run opens 0 files, sends
0 B and takes 0.5 s. The would-send figure is measured by really framing every
new chunk with Phase 1's own `chunk::frame`, never by applying a ratio to raw
bytes.**

## Task Commits

1. **`ad81f4c`** — the stored-size measurement and `build_with_keys` (task 1, `plan.rs`)
2. **`b80e315`** — D4's renderer and the would-upload column in `sync status` (task 1, `report.rs`)
3. **`e50b5a3`** — `sync push --dry-run`, and the bare push that refuses (task 2)
4. **`520154d`** — pin the integration test's clock

## The number, and why it is 18.0 MiB and not 30.07 MiB

Measured 2026-08-19, Apple M3 Max / macOS 26.5.2, release build, against the real
default bundle:

```
                     files         raw  would send
  config           1 files       183 B       168 B
  credentials    107 files    23.8 MiB    10.8 MiB
  routines         8 files    12.4 KiB     9.4 KiB
  chat_index    1533 files    75.6 MiB     7.2 MiB
  transcripts          off

  total         1649 files    99.4 MiB    18.0 MiB

  snapshot: 1649 files, 99.4 MiB of local state
  a push would send 18.0 MiB in 704 new chunks (51.4 MiB of plaintext, from 1649 files read)
```

Plan 2-06's CAL-4 says **30.07 MiB** for the same bundle. The plan's `<output>`
asks whether a large disagreement means one of the two is measuring the wrong
thing. **Neither is.** Three of the four categories agree to the byte:

| category | CAL-4 stored | dry-run would-send |
|---|---:|---:|
| `config` | 168 B | 168 B |
| `credentials` | 10.84 MiB | 10.84 MiB |
| `routines` | 9.44 KiB | 9.44 KiB |
| `chat_index` | **19.22 MiB** | **7.2 MiB** |

The whole difference is `chat_index`, and it is **cross-file deduplication**:

```
chunks:            1716 total across all files, 704 distinct
chat_index raw:    79,273,647 B
…distinct:         28,890,926 B   (2.74x — 1012 duplicate chunks)
…stored:            7,498,096 B   (3.85x, against CAL-4's 3.93x for the category)
```

CAL-4 framed every chunk of every file independently, so it answers *"how big is
this bundle stored"*. The dry-run sums `new_chunk_ids`, which are deduplicated
across files, so it answers *"how many bytes would a push send"* — and Phase 4
uploads exactly that list, once per id. The per-byte projection is identical in
both (`frame` + 40, same 3.85–3.93x on the same category); only the input set
differs. The user's 1533 `local_*.json` session indexes across four accounts
share 1012 identical 256 KiB blocks.

**This does not make the estimate optimistic in the forbidden way.** The thing
2-06 warned about is applying a *ratio* to raw bytes, which is wrong low because
`frame` pads to the next power of two. That is not done anywhere here: every
figure comes from a real `frame(chunk).len() + 40`.

## The projection, concretely

```rust
pub const SEAL_OVERHEAD: u64 = 24 + 16;          // nonce ‖ … ‖ Poly1305 tag
let stored = chunk::frame(&buf)?.len() as u64 + SEAL_OVERHEAD;
```

Run inside `chunk_from`, while the chunk is still in the read buffer — the only
placement that does not mean re-reading every file. Three tests hold it:

- `sealing_a_chunk_costs_exactly_the_framed_size_plus_the_overhead` pins the
  constant against a really-sealed chunk at 1 B / 1 KB / 100 KiB / 256 KiB,
  because `NONCE_LEN` and `TAG_LEN` are private to `crypto` and a literal that
  nobody checks is a literal that silently rots.
- `the_projection_is_the_framed_size_not_a_compression_ratio` takes 256 KiB of
  one byte: zstd flattens it to ~26 B, `frame` rounds to a power of two, and the
  test asserts the framed length *is* a power of two and that the reported figure
  exceeds zstd's own output. A ratio cannot produce that number.
- `an_incompressible_chunk_stores_more_than_its_plaintext` is the other end:
  random bytes store **above** their plaintext size, which a ratio would also
  get wrong.

Cost: zstd level 3 over the new bytes only. 1.7 s for the whole 99.4 MiB first
run; **0 s on a no-op**, because nothing is read and therefore nothing is framed.

## Deviations from Plan

**1. [Interface] `build_with_keys(…, keys: &Keys)` rather than `build_with_master_key(…, master_key)`**
- **Issue:** `crypto::subkeys()` and the `name_key` field are private, and there
  is no public master-key type. Deriving `name_key` inside `plan.rs` would mean
  exporting the key hierarchy's internals.
- **Fix:** take `&Keys`, Phase 1's own public type, and call its `chunk_id`. All
  three subkeys stay `Zeroizing`, private and `Debug`-redacted; the closure
  borrows and returns an address. Strictly less key material in flight (T-2-29),
  and still exactly one call site (T-2-32).

**2. [Model] `DryRunReport { status, no_key }`, not `{ plan, index_rebuilt }`**
- **Issue:** 2-05 moved `index_rebuilt` onto `SyncPlan`, so the plan's second
  field was already redundant. And the shape the phase actually needs is the
  no-key case: counts without a plan, plus the reason.
- **Fix:** `DryRunReport` wraps `StatusReport` (which gained
  `plan: Option<SyncPlan>`) and adds `no_key: Option<String>`. One model, two
  renderers, so the two commands cannot print different numbers for one fact.

**3. [Scope, additive] `CategoryPlan.capped` and the `new_stored_bytes` family**
- **Issue:** `SyncPlan` had no stored-size field at all, and `build_status`
  needed `walk_capped` without re-walking the tree.
- **Fix:** `FilePlan.new_stored_bytes`, `CategoryPlan.new_stored_bytes`,
  `CategoryPlan.capped`, `SyncPlan.total_new_stored_bytes`. `build_status` gained
  `plan: Option<SyncPlan>` and derives its rows from the plan when it has one,
  so the keyed path walks the tree **once**.

**4. [Scope, reduced] No interactive password prompt, and no keyfile writer**
- **Issue:** the plan's task 2 assumes a keyfile exists and a password can be
  prompted for. Neither is true in this phase: creating the keyfile is the guided
  setup's job in phase 3, and no no-echo TTY reader exists in the crate (adding
  one means a new dependency or termios code, which phase 3 will own alongside
  the flow that needs it).
- **Fix:** `keys_at` reads a keyfile if one is present at
  `<config_dir>/sync/keyfile.json` and takes the password from stdin; otherwise
  the dry-run takes the no-key path the plan already specifies as a first-class
  outcome. On this machine today that is the path taken, and it says so:

  ```
    would upload: not computed — this bundle has no sync keyfile yet (…/sync/keyfile.json is absent)
      only the third column needs one; the counts and raw bytes above need no password at all
  ```

  Phase 3's setup gains the keyed column for free the moment it writes that file.
  If it chooses a different location, `keyfile_path` is the one function to move.

**Total deviations:** 4 — one shrinking the key surface, one reconciling the
model with 2-05, one additive, one deferring an artifact its owning phase has not
built yet.

## Security notes

- **T-2-29** (key handling) — the password arrives on stdin only, never argv and
  never an env var; it lives in `Zeroizing<String>` and is dropped at the end of
  `keys_at`. A wrong password produces `crypto`'s single indistinguishable
  refusal. No key, subkey or chunk plaintext reaches a `Debug` impl, an error or
  a printed line. An **empty** stdin is refused before the KDF runs, so an
  unattended invocation cannot burn a gibibyte hashing the empty string.
- **T-2-30** (dry-run output) — the report prints category labels, counts and
  byte totals. No file path from inside a category, no chunk id, no file content.
  The only paths printed are the index and the keyfile, both already the user's
  own configuration.
- **T-2-31** (bare `sync push`) — exits 1 with a message pointing at `--dry-run`.
  It parses rather than being a clap error so the refusal gets to explain itself.
  There is no transport in `src/sync/`, and `grep` finds no `reqwest`, no socket
  and no URL anywhere under it.
- **T-2-32** (a wrong would-upload figure) — ids come from `Keys::chunk_id`
  through the single `build_with_keys` call site; `CHUNK_BYTES` is already 2-05's
  re-export of `CHUNK_SIZE`; the byte figure is Phase 1's own `frame` output plus
  a pinned AEAD overhead.
- **T-2-33** (Argon2id cost) — accepted and announced: `sync: deriving the sync
  key (Argon2id — this takes a moment)…` prints before the derivation starts.
- **T-2-SC** — no dependency added. `IsTerminal` is stdlib.
- **Phase 1 NEW-3 (the deferred AAD object-type separator) is still not
  triggered:** this plan seals nothing. `chunk::frame` is called for its *length*;
  `chunk::seal_chunk` appears only in tests, and no new kind of object is sealed
  under `chunk_key`.

## Verification

- `cargo test --lib sync` — **199 passed**, 0 failed (was 175 before this plan).
- `cargo test --lib widget::cli` — **21 passed**.
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo fmt --check` — clean.
- **Widget exit-0 invariant intact:** `./target/release/ai-usagebar` with no
  subcommand still exits 0. `sync` is dispatched before the widget path and
  returns its own real code (0 for both reads, 1 for a bare push).
- **Hermetic:** every new test builds its roots via `SyncRoots::at` on a
  `TempDir`, its index via `Index::at`, its keys from a `KdfParams { m_kib: 8 }`
  keyfile, and its `now` from a fixed timestamp. Nothing calls `default_path`,
  `SyncRoots::resolve`, `Config::load` or reads an env var. The AUR `check()`
  reads no installer file and pays no KDF.
- The real-bundle figures above were produced by a throwaway binary in the
  scratchpad (deleted) that pointed `build_with_keys` at a temp index; it wrote
  nothing to the user's config dir and never touched the real sync index.

## Next Phase Readiness

- **Phase 3 (`sync setup`)** — write the keyfile to
  `<config_dir>/sync/keyfile.json` and the third column lights up with no change
  here. That flow also owns the interactive password prompt this plan
  deliberately did not build.
- **Phase 4 (push)** — upload `SyncPlan::new_chunk_ids`, once per id;
  `total_new_stored_bytes` is what that will weigh on the wire, so a pack-count
  line can be derived from it against `PACK_TARGET`. `set_last_sync` and
  `evict_unseen` still belong to whoever actually pushes — a dry-run that stamped
  `last_sync` would lie, and this one does not.
- **Known gap:** the `chunk` table still has no writer (carried from 2-05), so
  "already uploaded" is known only from the `file` table's rows. Until Phase 4
  populates it, a chunk shared with a file that failed its append check is
  re-planned as new.

---
*Phase: 02-bundle-scope-local-index-dry-run-planning*
*Completed: 2026-08-19*
