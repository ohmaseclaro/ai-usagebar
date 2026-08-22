---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 07
subsystem: sync
tags: [integration-tests, mockito, documentation, repo-06, repo-07, sync-04, sync-05, sync-07, crypto-04, ux-04]

requires:
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 01
    provides: "`push::run`, `PushCtx`, `Pointer`, `PRUNE_GRACE`, the write verbs, and the `sync push`/`prune`/`rekey` arms"
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 02
    provides: "`packer::build` — packs, manifest, index object, root, and the local chunk table"
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 03
    provides: "`upload::run`, `upload::ensure_keyfile`, the `Progress` implementations"
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 04
    provides: "`pointer::commit`'s bounded, merging compare-and-swap"
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 05
    provides: "`prune::plan_deletions`, `prune::run`, `prune::run_on_demand`"
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 06
    provides: "`rekey::run` and the not-revocation sentence its `include_str!` test guards"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 05
    provides: "`docs/sync-github.md` and the README's sync section, created for pairing and tokens"
provides:
  - "`tests/sync_push_e2e.rs` — the only place the six modules five parallel worktrees built are exercised together, against one stateful `mockito` fake"
  - "A measured request-count shape for a first push: nine fixed requests plus one upload and one verifying download per pack"
  - "A measured retention figure Phase 5 needs: 12 pushes at `keep_snapshots = 3` left 14 live assets against 25 ever uploaded"
  - "Two source-reading guards: no fifth object kind sealed under `chunk_key`, and both pack size constants pinned with `PACK_MAX` named as the governing one"
  - "`docs/sync-github.md` §push, §retention, §rekey, §acceptable-use — the user-facing half of Phase 4"
affects: [phase-5-restore, phase-6-surfaces]

tech-stack:
  added: []
  patterns:
    - "One stateful in-memory fake of the remote rather than per-test static mocks: the kill/resume, conflict, prune and twelve-push retention tests all need the remote to *remember*, and four independent fixtures would have disagreed about what a release looks like."
    - "Mutually exclusive `match_request` predicates wherever two mocks share a method and a path, so mockito's preference for an unsatisfied mock never silently retires the first one."
    - "Assertions on sets, not counts, wherever the count belongs to another plan."

key-files:
  created:
    - tests/sync_push_e2e.rs
  modified:
    - docs/sync-github.md
    - README.md

decisions:
  - "ROADMAP criterion 1's two numbers cannot both hold, so the test asserts the ratio REPO-06 actually requires and records the measured absolute count. 5,000 chunks is 1.25 GiB, and a fixture compressible enough to avoid writing that collapses into one pack whose single-chunk header passes CHUNK_SIZE at ~2,400 entries; separately, 'under 10 requests' is below the protocol's own nine-request floor."
  - "Criteria 2 and 3 are one test, because they are one property, and it drives *two* refused pushes before the resume — that is what the shipped code requires, and the reason is pinned in a comment and stated in the documentation."
  - "The rekey test runs at `MIN_KDF_MEMORY_KIB` (8 MiB) rather than the 8 KiB the rest of the suite uses: `Keyfile::rewrap` enforces the write-path floor and `rewrap_with_floor` is `pub(crate)` on purpose. 8 MiB of Argon2id is milliseconds."
  - "The fixture's categories include `Credentials` even though nothing seeds that collector, because with it off a public repository *warns and proceeds* — D-04's deliberate carve-out — which would have silently disarmed the incident test."

metrics:
  duration: ~3h
  completed: 2026-08-19
status: complete
---

# Phase 4 Plan 07: The Seven Success Criteria, and the User-Facing Docs Summary

Twelve integration tests drive the whole nine-step push against one stateful `mockito`
fake, and `docs/sync-github.md` gains the three commands Phase 4 ships — with every
sentence checked against the merged code rather than against a plan's prose.

## What was built

**`tests/sync_push_e2e.rs`** (1,180 lines, 12 tests, 9.9 s). A `Local` fixture — a
`TempDir` tree, a hand-wrapped cheap keyfile, a pairing record, an `Index` — and a
`Remote` fixture: one `mockito::Server` wired to a single `RemoteState` that remembers
its assets, its pointer blob, every request it answered, every name that ever landed,
and every name deleted. Every test calls `push::run`, `prune::run_on_demand` or
`rekey::run`; none calls a module in isolation.

| Test | Criterion |
|---|---|
| `a_first_push_issues_one_upload_per_pack_and_never_one_per_chunk` | 1 (REPO-06) |
| `a_push_killed_before_the_flip_leaves_the_previous_pointer_byte_identical` | 2 and 3 (SYNC-04, SYNC-05) |
| `a_resume_deletes_a_torn_asset_rather_than_skipping_on_its_name` | 3, the torn-upload half |
| `a_stale_sha_conflict_re_plans_and_the_prune_spares_the_competitor` | 4 (REPO-07) |
| `a_rekey_leaves_every_pack_byte_identical_and_destroys_the_old_keyfile` | 5 (CRYPTO-04, D5) |
| `repeated_syncs_of_a_growing_file_leave_fewer_assets_than_were_ever_uploaded` | 6 (SYNC-07) |
| `a_long_push_reports_advancing_counts_and_every_failure_names_an_action` | 7 (UX-04) |
| `an_unreferenced_asset_survives_inside_the_grace_window_and_not_outside_it` | `PRUNE_GRACE` end to end |
| `an_on_demand_prune_with_no_published_pointer_deletes_nothing_and_creates_no_release` | 4-06's declined release creation |
| `the_push_path_seals_only_the_formats_four_object_kinds` | Phase 1 carry-forward (T-4-53) |
| `both_pack_size_constants_are_pinned_and_pack_max_is_the_one_that_governs` | Phase 1 carry-forward (T-4-54) |
| `nothing_in_this_suite_resolves_a_real_home_or_a_real_token` | T-4-51 |

**`docs/sync-github.md`** gains `## What sync push does` (with re-running and progress
output), `## Retention, and what a prune deletes` (with the two guards), `## Changing
the sync password`, and a rewritten `## GitHub's acceptable-use policy`. **`README.md`**
gains one line per command and a link — no second copy.

## The two numbers the plan asked for

**REPO-06.** A first push of **193 chunks** produced **2 packs** and cost **13 HTTP
requests** in total. The shape, pinned as an equality rather than a bound:

```
9 fixed + 2 × packs
```

The nine are: two visibility reads (the gate and the re-gate), one pointer read, one
release lookup, three asset listings (the resume scan, `ensure_keyfile`'s, and the
prune's), the keyfile upload, and the flip. The two-per-pack are the upload and D3's
verifying download.

**SYNC-07.** Twelve pushes of a growing file at `keep_snapshots = 3` left **14 live
assets against 25 ever uploaded**, with 3 snapshots kept. Phase 5 should size a restore
against the live figure, which is what the pointer names — not against the cumulative
one.

## Where the code diverges from the plans

The plan asked for every sentence to be checked against the merged code. Six divergences
were named in advance and confirmed; **two more were found by running the thing**.

Confirmed as stated, and written that way:

1. **`upload::ensure_keyfile` runs after the re-gate**, and both it and
   `progress::reporter` are wired now with a call-site guard in `cli.rs`. The e2e suite
   asserts the keyfile upload independently (`uploads == packs + 1`).
2. **A push uploads several assets.** The fixture keys everything by name and hands out
   ids per upload.
3. **Reuse asks two questions** — `reusable()` intersects the local chunk table with the
   packs the *arriving pointer* names. The killed-push test depends on exactly this: the
   killed run's packs are unpublished, so nothing is reusable and the run repacks.
4. **`rekey` does not call `ensure_keyfile`.** Said in the test's doc comment and in
   `docs/sync-github.md`.
5. **`prune::run_on_demand` returns `Ok(0)` before `ensure_release`** when no pointer is
   published. Asserted (`no /releases/tags request`), and its consequence stated in the
   documentation.
6. **`keep` is clamped to ≥1.** Stated in the retention section alongside the config's
   own refusal of `0`.

### New finding 1 — ROADMAP criterion 1's two numbers are mutually unreachable

The criterion asks for ~5,000 chunks in under 10 requests. Neither half survives contact:

- 5,000 chunks is **1.25 GiB** at `CHUNK_SIZE`. Writing that during `makepkg`'s
  `check()` is not acceptable on an installer's machine. Making the payload compressible
  enough to avoid writing it collapses the bundle into a single pack — and a pack's
  header is *still a single sealed chunk*, which `frame()` refuses past `CHUNK_SIZE`, so
  the ceiling on a one-pack bundle is roughly **2,400 entries**, not 5,000. This is the
  same single-chunk header 4-CONTEXT.md flags as the risk gap-closure 1-09 did not reach
  — it turns out to bite from *compressible data*, not only from a raised `PACK_MAX`.
- **"Under 10 requests" is below the protocol's own floor of nine**, and the floor is
  nine before a single pack moves. Even a one-pack bundle costs 11.

So the test asserts what REPO-06's requirement text actually says — "a small number of
large objects, never one request per chunk" — as a ratio, and pins the absolute count
exactly so a protocol that grows a round trip has to say so. No source change was made;
this is a roadmap-wording defect, not a code defect.

### New finding 2 — a first re-run after an interruption reuses nothing

`plan::build` emits `file_plans` in **two passes**: every file the index already knows,
in scan order, then every file that changed. So the first run that sees a file puts it
*last*, and the next run puts it in *scan order*. The manifest is built from that list
and is packed alongside the data chunks, so the two runs seal packs at different content
addresses — and the resume's `decide()` finds nothing present.

Measured, with a push refused at the flip four times over the same tree:

| run | packs uploaded |
|---|---|
| 0 (first sight of the new file) | 2 new |
| 1 (re-plan, order now stable) | 2 new |
| 2 | **0** |
| 3 (flip allowed) | **0** |

SYNC-05 therefore holds from the *second* re-run onward, and the cost of the first is
one extra upload of the bundle's changed part. Nothing is lost and nothing is corrupted;
it is bandwidth. The e2e test drives two refused pushes before the resume and asserts
`packs_uploaded == 0` and zero `POST`s, with the reason pinned in a comment;
`docs/sync-github.md` states it in a user's words under "Re-running an interrupted push".

Fixing it belongs in `4-02`/`2-05` (sort `file_plans` by path, or build the manifest
from `scan.files` order rather than plan order) and is out of this plan's file scope.
It is a one-line change with a measurable payoff and should be raised for Phase 5, which
also has to read that manifest.

## Deviations from Plan

### Auto-fixed issues

**1. [Rule 1 — Bug] The fixture's refusal only refused once**

- **Found during:** Task 1, while measuring the resume
- **Issue:** Two mockito mocks sharing `PUT /contents/sync/pointer.json` were separated
  only by the refusing one's matcher. Mockito prefers a matching mock that has not met
  its expectation, so after one hit the refusal retired and the *success* mock answered —
  turning "every flip is refused" into "the first flip is refused" and making three
  subsequent runs look like a packing non-determinism bug that does not exist.
- **Fix:** Every mock sharing a method and path now carries a matcher that partitions
  the space, so exactly one can ever match. Documented as fixture trap 2 in the module
  docs.
- **Files modified:** `tests/sync_push_e2e.rs`
- **Commit:** 161c554

**2. [Rule 2 — Missing critical coverage] The incident path was disarmed by the fixture's own config**

- **Found during:** Task 1
- **Issue:** The fixture ran with `categories = [Config]`. With the credentials category
  off, `assert_pushable` and `check_drift` both *warn and proceed* on a public repository
  — D-04's deliberate carve-out. So the criterion-7 failure-path assertion passed a
  repository that had turned public straight through and never reached
  `went_public_mid_push`.
- **Fix:** `Credentials` is in the fixture's categories, and the fake flips visibility
  between the two gate reads rather than before the first, so the *re-gate* is what
  fires. The test now also asserts the incident deleted what the run uploaded and left
  the previous snapshot's packs alone.
- **Files modified:** `tests/sync_push_e2e.rs`
- **Commit:** 161c554

**3. [Rule 3 — Blocking] The rekey test could not run at the suite's KDF parameters**

- **Found during:** Task 1
- **Issue:** `Keyfile::rewrap` enforces `MIN_KDF_MEMORY_KIB`, and `rewrap_with_floor` is
  `pub(crate)` — deliberately, since a floor a caller passes its own value for is not a
  floor. A rekey at 8 KiB is refused.
- **Fix:** That one test builds its fixture at `MIN_KDF_MEMORY_KIB` (8 MiB), which is
  milliseconds of Argon2id, rather than reaching for the private seam. Explained at the
  constant.
- **Files modified:** `tests/sync_push_e2e.rs`
- **Commit:** 161c554

**4. [Rule 2 — Missing critical functionality] The seal guard was a substring check that could not see the root**

- **Found during:** Task 1
- **Issue:** The first draft matched sealing call sites by fragment. `Root::new(…)
  .seal(ctx.keys)?` spans several lines, so its sealing line is a bare `.seal(ctx.keys)?;`
  that no per-kind fragment recognises — the guard failed on correct code, and a version
  loosened enough to pass would have stopped catching anything.
- **Fix:** The guard pins the **complete set** of sealing lines in production code
  (everything before each file's `#[cfg(test)]`), each annotated with the kind it seals,
  and asserts both directions: nothing new, and nothing lost. A guard that stops matching
  the code fails as loudly as one that finds a fifth kind.
- **Files modified:** `tests/sync_push_e2e.rs`
- **Commit:** 161c554

**5. [Rule 1 — Stale documentation] The README and the doc header still said push had not shipped**

- **Found during:** Task 2
- **Issue:** `README.md` said "pushing arrives in a later release" and
  `docs/sync-github.md` opened with "This release pairs with the repository and verifies
  it". Both were true before this phase and are false now.
- **Fix:** Rewritten. The README lists the four commands and links; the doc's header
  names them and points at `docs/sync-format.md` as the specification.
- **Files modified:** `README.md`, `docs/sync-github.md`
- **Commit:** a7955aa

### Not fixed, deliberately

The two-pass `file_plans` ordering (new finding 2) is a `src/` change and this plan's
file scope is three files, none of them under `src/`. It is documented in the test, in
the user-facing doc, and above.

## Known stubs

None. Every test in the file drives real code against the fake; nothing is skipped and
no `<verify>` went unrun.

## Threat flags

None. This plan adds no network endpoint, no auth path, and no schema; it reads two
source trees at compile time through `include_str!` and writes prose.

## Verification

| Gate | Result |
|---|---|
| `cargo test` | **1,406 passed, 0 failed, 16 ignored** — 1,365 lib (unchanged baseline) + 12 new in `sync_push_e2e` + 29 pre-existing integration |
| `cargo test --test sync_push_e2e` | 12 passed, 0 failed, 9.9 s |
| `cargo test` with `$HOME` unset | 12 passed — the suite is hermetic against the AUR `check()` |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `make test` | green — cargo plus the GNOME, KDE, and Omarchy Node contract suites |
| `cargo machete` | not installed on this machine; `Cargo.toml` is unchanged and no dependency was added, so there is nothing for it to find |
| `Cargo.toml` / `Cargo.lock` | unchanged |
| Files under `src/` | none modified |

## What Phase 5 should carry forward

- **14 live assets, not 25.** A restore fetches what the pointer names.
- **The manifest ordering defect.** Phase 5 reads the manifest; a fix in `packer::build`
  (or in `plan::build`) makes resume free *and* makes two machines with identical trees
  produce identical packs. Worth doing before restore depends on the current order.
- **The single-chunk pack header has a second trigger.** 4-CONTEXT.md frames it as a
  risk of raising `PACK_MAX`. It is also reachable from highly compressible data at the
  *current* `PACK_MAX`, at roughly 2,400 entries in one pack. `PackWriter::finish` errors
  cleanly rather than truncating, so it is a refusal and not corruption — but it is a
  refusal a user could hit.

## Self-Check: PASSED

- `tests/sync_push_e2e.rs` — FOUND
- `docs/sync-github.md` — FOUND (modified)
- `README.md` — FOUND (modified)
- commit `161c554` — FOUND
- commit `a7955aa` — FOUND
