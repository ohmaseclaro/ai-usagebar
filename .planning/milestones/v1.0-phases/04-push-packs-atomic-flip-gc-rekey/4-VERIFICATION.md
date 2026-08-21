---
phase: 04-push-packs-atomic-flip-gc-rekey
verified: 2026-08-19T00:00:00Z
status: human_needed
score: 7/7 success criteria verified (criterion 1 on intent — its literal numbers are unsatisfiable)
behavior_unverified: 0
overrides_applied: 0
gates_rerun:
  - command: "cargo fmt --check"
    result: "exit 0"
  - command: "cargo clippy --all-targets -- -D warnings"
    result: "exit 0"
  - command: "make test"
    result: "exit 0 — 1365 lib + 12 sync_push_e2e + 13 sync_e2e + 13 sync_vectors + 3 + GNOME/KDE/Omarchy JS gates"
  - command: "env -u HOME -u XDG_CONFIG_HOME -u XDG_CACHE_HOME cargo test"
    result: "exit 0 — milestone hermeticity invariant holds"
  - command: "cargo machete"
    result: "NOT RUN — cargo-machete is not installed on this machine (release-checklist item, not a phase gate)"
human_verification:
  - test: "Decide the rewording of ROADMAP Phase 4 success criterion 1"
    expected: "The criterion states a ratio and a shape that the shipped protocol can actually meet"
    why_human: "4-07's claim is independently confirmed and is in fact stronger than reported. Only a human can amend the roadmap contract."
  - test: "Decide whether the cross-machine prune/reuse race is accepted for Phase 5 or closed here"
    expected: "Either a recorded acceptance, or a re-plan of the bundle (not just the pointer) on a 409"
    why_human: "Narrow multi-machine concurrency; needs a product call on residual risk before restore ships"
  - test: "Fix the four documentation defects listed under Anti-Patterns"
    expected: "The stale ensure_keyfile comment, the request-count claim, the keep-clamp claim, and the truncated README sentence are corrected"
    why_human: "Wording decisions; none is a code defect"
warnings:
  - "`Index::known_chunks` has zero production call sites — instance 8 of this milestone's recurring defect"
  - "`src/sync/push/upload.rs` doc-comment asserts prune never collects a resurrected orphan keyfile; 4-05 made that false"
  - "`docs/sync-github.md`'s 13-request figure is off by one for a genuinely first push (release creation)"
  - "`docs/sync-github.md`'s 'clamped to at least one everywhere else' is not true of `push::run`'s rebuild closure"
  - "`README.md` line 237 ends mid-sentence"
environment_anomaly:
  - "An uncommitted edit to src/sync/push/prune.rs planting `/user/repos` appeared in the working tree mid-verification and then reverted. Not authored by this verifier, not committed. The REPO-03 guard was confirmed non-vacuous against that exact fragment. Tree is clean at 67addcf."
---

# Phase 4: Push — Packs, Atomic Flip, GC, Rekey — Verification Report

**Phase Goal:** The user's encrypted bundle reaches their private repo; an interrupted push
never leaves a snapshot a pull could read; and the remote does not grow without bound.

**Status:** `human_needed` — the goal is achieved in the codebase. What is outstanding is
three decisions, not three defects.

---

## Gates, re-run rather than read

| Gate | Result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --all-targets -- -D warnings` | exit 0, no warnings |
| `make test` | exit 0 — 1365 lib, 12 `sync_push_e2e`, 13 `sync_e2e`, 13 `sync_vectors`, 3 CLI, plus the GNOME, KDE and Omarchy JS contract suites |
| `cargo test` with `$HOME`, `$XDG_CONFIG_HOME`, `$XDG_CACHE_HOME` unset | exit 0 — the milestone's hermeticity invariant holds |
| `cargo machete` | **not run** — `cargo-machete` is not installed here. No new dependency was added this phase (`Cargo.toml` is untouched in `04fa4c…HEAD`), so the risk is nil, but the check was not performed and I will not report it as passing. |

---

## The measured numbers, reproduced

Both headline numbers were re-derived by temporarily instrumenting the tests to print
their observations, then reverting. The tree is clean at `67addcf`.

| Claim | Reproduced | Result |
|---|---|---|
| A first push of 193 chunks produces 2 packs and 13 requests | `chunks=193 packs_uploaded=2 packs_skipped=0 requests=13 uploads=3` | **confirmed** |
| Shape `9 fixed + 2×packs` | `13 == 9 + 2*2` | **confirmed** |
| Twelve pushes at `keep_snapshots = 3` leave 14 live against 25 ever uploaded | `live=14 ever=25 snapshots=3` | **confirmed** |

`uploads=3` is two packs plus the keyfile, which is `ensure_keyfile` demonstrably firing
on the production path rather than only in its own tests.

### The resume sort is load-bearing, and the test proves it

`plan::build`'s per-category sort (`67addcf`) was removed, the e2e suite re-run, and the
sort restored:

```
test a_push_killed_before_the_flip_leaves_the_previous_pointer_byte_identical ... FAILED
test result: FAILED. 11 passed; 1 failed
```

The test drives **one** refused push (`st.refuse_put = true`, one `expect_err`) and then
asserts `packs_uploaded == 0` and `uploads() == 0` on the **first** resume. It is a
genuine regression test for the ordering fix, not a restatement of it. Confirmed.

---

## Success criteria

| # | Criterion | Verdict |
|---|---|---|
| 1 | One upload per pack, never one per chunk; ~5,000 chunks under 10 requests | **PASS on intent, FAIL as literally worded** — see below |
| 2 | Killing after uploads, before the flip, leaves the previous pointer intact | **PASS** |
| 3 | Re-running re-uploads only what is missing or non-`uploaded` | **PASS** |
| 4 | A stale-`sha` 409 re-reads and re-plans; the competitor's assets survive | **PASS** |
| 5 | `sync rekey` rewraps the same master key; the old keyfile asset is gone | **PASS** |
| 6 | Prune shrinks the asset list; remote size tracks live data | **PASS** |
| 7 | Advancing counts, and every failure path exits non-zero with an action | **PASS** |

### Criterion 1 — 4-07's claim is correct, and understated

I assessed this independently. It holds, and the floor is one request higher than 4-07 said.

- **5,000 chunks is 1.25 GiB.** `5000 × 262,144 = 1,310,720,000` bytes. Writing that inside
  `makepkg`'s `check()` is not acceptable.
- **Making it compressible collapses it into one pack, and the header blows.** `should_seal`
  compares pack *bytes* against `PACK_MAX`; highly compressible chunks never reach 48 MiB, so
  entries accumulate. The pack header is still a **single** sealed chunk — the one object
  gap-closure 1-09 deliberately did not reach. At the measured ~124 bytes of JSON per entry
  (191 entries → 23,669 bytes), `CHUNK_SIZE`'s 262,144 is passed at roughly **2,100** entries.
  So the two halves of the criterion are mutually unreachable, exactly as reported.
- **"Under 10 requests" is below the protocol's own floor — and the floor is 10, not 9.**
  The nine fixed requests 4-07 enumerates assume the release already exists. On a *genuinely*
  first push there is none, so `ensure_release` spends a `GET /releases/tags/…` that 404s
  **and** a `POST /releases`. The true first-push floor is `10 + 2×packs ≥ 12`, and a bundle
  always produces at least two packs (data+manifest, then the index object), so **14**.
  The measured 13 comes from a fixture whose tag mock always answers 200.

**Proposed rewording:**

> 1. A first push issues **one upload request per pack**, never one per chunk: a bundle of
>    ~190 chunks completes in 13 HTTP requests against an existing release — nine fixed
>    (two visibility reads, one pointer read, one release lookup, three asset listings, the
>    keyfile upload, and the flip) plus one upload and one verifying download per pack, with
>    one further request the first time the release itself has to be created. Request count
>    tracks packs, never chunks.

This is the phase's single roadmap-contract deviation and the reason the status is
`human_needed` rather than `passed`.

---

## Call sites of every public function added in Phase 4

The milestone's most repeated defect is a function that exists and is tested but that nothing
calls. Every `pub`/`pub(crate)` function added under `src/sync/push/`, `src/sync/github/write.rs`
and `src/sync/index.rs` was enumerated and its production callers traced.

| Function | Production call site | Status |
|---|---|---|
| `push::run` | `src/sync/cli.rs:568` | wired |
| `push::gate_now` | `push/mod.rs:280,304`, `rekey.rs:97`, `prune.rs:216` | wired |
| `push::pack_asset_name` | `push/mod.rs:437`, `upload.rs:99,265,277` | wired |
| `push::keyfile_asset_name` | `upload.rs:159`, `rekey.rs:95`, `cli.rs:467` | wired |
| `push::repo_id_for` | `cli.rs:517` | wired |
| `packer::build` | `push/mod.rs:288` | wired |
| `packer::manifest_path` | `packer.rs:114` | wired |
| `pointer::load` | `push/mod.rs:283`, `prune.rs:225`, `rekey.rs:99` | wired |
| `pointer::commit` | `push/mod.rs`, `prune.rs:248`, `rekey.rs:150` | wired |
| `upload::run` | `push/mod.rs:301` | wired |
| `upload::ensure_keyfile` | `push/mod.rs:320` | **wired** (was the reported gap; `e038f49` closed it, and `uploads=3` above proves it fires) |
| `progress::reporter` | `cli.rs:567` | **wired** (was the reported gap; same commit) |
| `progress::render` | `Counters::line`, both reporters | wired |
| `prune::plan_deletions` | `prune.rs:150,245` | wired |
| `prune::run` | `push/mod.rs:387`, `prune.rs:258` | wired |
| `prune::run_on_demand` | `cli.rs:601` | wired |
| `rekey::run` | `cli.rs` rekey arm | wired |
| `write::{ensure_release, list_assets, upload_asset, delete_asset, download_asset, get_contents, put_contents, with_retry}` | all reached from `push/mod.rs`, `upload.rs`, `prune.rs`, `rekey.rs`, `pointer.rs` | wired |
| `Index::record_chunks` | `packer.rs:174` | wired |
| `Index::chunk_locations` | `packer.rs:223` | wired |
| `Index::forget_chunks` | `prune.rs:191` | wired |
| **`Index::known_chunks`** | **none** | **ORPHANED** |

### ⚠️ `Index::known_chunks` — instance 8

```
src/sync/index.rs:419   pub fn known_chunks(&self, ids: &[ChunkId]) -> HashSet<ChunkId>
```

Every reference is a test: seven in `index.rs`'s own module, one in `prune.rs:801`. There is
no production caller anywhere in `src/`.

This is not an oversight that slipped through — it is a documented one. `4-02-PLAN.md:220`
states "`build` calls `known_chunks` and `record_chunks`", and `4-02-SUMMARY.md:98` records
that the real implementation needed locations rather than membership, so `chunk_locations`
was added and "`known_chunks` is one line on top of it and keeps the frozen surface honest
for callers that only need the yes/no."

Severity is low — it is a one-line delegation to a function that *is* called, so it cannot
drift into wrongness independently. But it is the same shape as the seven prior instances:
a public function, fully tested, that production never reaches. Either give it the caller
the plan promised, or delete it and let `chunk_locations` be the surface. It should not
survive into Phase 5 as the ninth.

---

## Documentation, checked sentence by sentence

Verified accurate: the nine-step order and the claim that the keyfile is published *after*
the re-gate (`push/mod.rs` step 6b); "downloads each one back and checks it matches"
(`upload_one` compares `content_address(&fetched)` against `pack.id`); the resume rule
"name, size *and* upload state" (`decide`); "a re-run after an interrupted push re-sends
nothing" (proven above); "nothing younger than 24 hours is deleted" (`PRUNE_GRACE`); the
`sync push` result block, which is byte-for-byte `cli::render_push`; the prune warning text,
also byte-for-byte; the progress line `uploading 2/3 assets — 24.0 MiB of 36.0 MiB`, which
is `progress::render`'s exact format, on stderr, `\r`-rewritten on a tty and plain otherwise;
the on-demand-prune-does-nothing-without-a-pointer paragraph; the rekey ordering and the
"not revocation" framing; the twelve-push measurement (25 → 14).

Four defects found.

| # | File | Claim | Reality | Severity |
|---|---|---|---|---|
| D1 | `src/sync/push/upload.rs` (`ensure_keyfile` doc) | "the old wrapper comes back as an orphan asset **prune never collects**" | False since 4-05. `plan_deletions` sweeps `Kind::Keyfile` whose name ≠ `kept.keyfile` once past `PRUNE_GRACE`; `an_orphan_keyfile_no_pointer_names_is_swept_like_a_pack` asserts it. The comment describes a hole that was closed one wave later. | ⚠️ Warning |
| D2 | `docs/sync-github.md` | "A first push of ~190 chunks costs 13 HTTP requests in total: nine fixed" | True only when the release already exists. A literal first push adds `POST /releases` — 10 fixed, 14 total. The same mislabel sits in `tests/sync_push_e2e.rs`'s doc comment ("one release lookup"). | ⚠️ Warning |
| D3 | `docs/sync-github.md` | "`0` is refused when the config is loaded, and the value is clamped to at least one **everywhere else**" | `plan_deletions` clamps (`keep.max(1)`), and both prune paths go through it. `push::run`'s own rebuild closure does **not**: `if snapshots.len() > keep { drain(..len-keep) }` empties the list at `keep == 0`. Unreachable through config (`validate()` rejects 0, `Default` is 10), so this is a docs overstatement rather than a live bug — but the sentence is not true as written. | ⚠️ Warning |
| D4 | `README.md:237` | "…and its token holds no permission that could" | The sentence ends there. Missing "…create one." | ⚠️ Warning |

Minor, not counted: `src/widget/cli.rs`'s `Prune` help says "Only **packs** … are removed";
prune also sweeps orphan keyfiles.

---

## Known open items — each re-confirmed against the merged code

| Item | Confirmed? | Evidence |
|---|---|---|
| `prune::run_on_demand` returns `Ok(0)` before `ensure_release` when no pointer is published, so an orphan keyfile on a bundle that never published a pointer stays until a first push succeeds | **still true, and correctly described** | `prune.rs:225-229` returns `Ok(0)` on `None` strictly before the `ensure_release` at 232. `an_on_demand_prune_with_no_published_pointer_deletes_nothing_and_creates_no_release` asserts no `/releases/tags` request. `docs/sync-github.md` describes it accurately, including the consequence. |
| `rekey` does not call `ensure_keyfile` — the local keyfile is the old one until after the flip | **still true, and correct** | `rekey.rs:117` calls `upload_asset` directly with the freshly rewrapped bytes; `write_local` runs at 162, *after* `pointer::commit`. Calling `ensure_keyfile` would upload the **old** wrapper. Documented in the criterion-5 e2e test's doc comment. |
| `keep` is clamped to ≥1 because `plan_deletions`' truncated pointer is what gets published | **still true** | `prune.rs:86` `let keep = keep.max(1);`, with the reason in the doc comment at 68-72. `run_on_demand`'s rebuild closure publishes `plan_deletions(...).0`, so an unclamped zero would publish an empty snapshot list. See D3 for the one path that does not clamp. |
| 4-07's claim that criterion 1 asks for two unreachable numbers | **confirmed, and understated** | See the criterion 1 section. |
| `plan::build` now sorts by path, so a resume reuses on the first re-run | **confirmed, and the test would fail without it** | Sort removed → `a_push_killed_before_the_flip…` FAILED. One refused push drives it. |

---

## Phase 5 readiness

Phase 5 restores on a second machine from exactly what this phase publishes. Three specific
hazards were checked.

**Absolute paths in the manifest — closed.** `packer::manifest_path` emits a root-*name*-prefixed
relative encoding (`config/accounts/work/.credentials.json`, `claude-home/projects/a.jsonl`)
and returns an actionable error for anything under no root. It is the only producer of
`FileEntry.path` (`packer.rs:114`). `a_manifest_path_is_root_relative_and_never_absolute`
asserts no leading `/`, no `..`, and no local prefix; `a_file_under_no_root_is_an_error…`
asserts the refusal. Nothing about the pushing machine's layout or username survives.

**A pointer naming an asset that does not exist — closed on the ordinary paths.** `reusable()`
intersects the local chunk table with the packs named by a snapshot the **pointer** already
carries, so a pack that was packed locally but never landed can never be referenced. The
keyfile is uploaded at step 6b, strictly before the flip at step 7, and its name is a content
address of the same canonical bytes `Pointer.keyfile` carries — the criterion-5 test asserts
the first push publishes the wrapper, not merely its address.

**A keyfile that was never uploaded — closed.** `ensure_keyfile` is on the production path
and idempotent by content address.

### Residual, and the reason for the second human item

**A competitor's prune can delete a pack this run is reusing.** Machine A loads the pointer
at step 2 and finds pack `P` referenced by a snapshot it is about to reuse from. Machine B
then pushes; B's flip truncates to `keep_snapshots` and evicts the snapshot that alone
referenced `P`; B's prune deletes `P` (it is past `PRUNE_GRACE` and unreferenced by B's
landed pointer). A then flips. A's 409 path re-reads and rebuilds the **pointer** — it does
not re-plan the **bundle** — so A publishes a snapshot record naming a deleted `P`.

Neither of the two prune guards covers this: the landed pointer and the grace window both
protect the *competitor's* data from *this* machine's prune, which is the direction they were
written for and which criterion 4 tests. This is the reverse direction, and it is not
mentioned in any plan, summary or doc.

It is narrow — it needs two machines pushing concurrently, the pointer already at retention
capacity, and B's push to be the eviction that drops A's snapshot inside A's push window.
But its outcome is D2's worst case, an unrestorable snapshot, and Phase 5 is the phase that
would discover it. It needs a recorded decision before restore ships.

**A rekeyed-away wrapper can be resurrected.** After A rekeys, B still holds the superseded
keyfile on disk; B's next push calls `ensure_keyfile` and re-uploads it. The pointer is
unaffected (rule 3 takes the keyfile from the arriving pointer), and prune now sweeps it
after 24 h — which is exactly what D1 gets wrong. The window is real but bounded and
consistent with the command's honest "this is not revocation" framing. Worth a sentence in
`docs/sync-github.md`; not a blocker.

---

## Anti-patterns

No `TODO`, `FIXME`, `XXX`, `TBD`, `HACK`, `PLACEHOLDER`, `todo!()` or `unimplemented!()`
anywhere under `src/sync/`, `tests/sync_push_e2e.rs`, or `docs/sync-github.md`.

The REPO-03 substring guard is green and was proven non-vacuous: planting
`// verifier probe: /user/repos` in `src/sync/push/prune.rs` produced

```
REPO-03: …/src/sync/push/prune.rs contains "/user/repos". …
test result: FAILED. 0 passed; 1 failed
```

and the probe was reverted. The guard's own `skipped == 1` / `scanned > 50` non-vacuity
assertions are intact.

### Environment anomaly, reported for completeness

Mid-verification, `git status` showed an uncommitted modification to
`src/sync/push/prune.rs` replacing the `enum Kind` doc comment with one containing
`/user/repos` — precisely the trap `4-CONTEXT.md`'s standing rule warns about. **I did not
author it and did not commit it.** By the time the guard test ran the edit had reverted, and
the working tree is clean at `67addcf`. I re-planted the fragment deliberately to confirm the
guard catches it (it does, above), then reverted. Nothing in the phase's committed history
contains any of the four forbidden fragments outside `gate.rs`'s own guard.

---

## Requirements coverage

| Requirement | Verdict | Evidence |
|---|---|---|
| REPO-06 — small number of large objects | SATISFIED | `a_first_push_issues_one_upload_per_pack_and_never_one_per_chunk`; measured 13 requests for 193 chunks |
| REPO-07 — CAS pointer publication | SATISFIED | `pointer::commit`; `a_stale_sha_conflict_re_plans_and_the_prune_spares_the_competitor` |
| SYNC-04 — interrupted push leaves the previous state | SATISFIED | `a_push_killed_before_the_flip_leaves_the_previous_pointer_byte_identical` |
| SYNC-05 — resume reuses what landed | SATISFIED | same test, plus `a_resume_deletes_a_torn_asset_rather_than_skipping_on_its_name` |
| SYNC-07 — bounded remote growth | SATISFIED | `repeated_syncs_of_a_growing_file…`; measured 25 → 14 |
| CRYPTO-04 — password change without re-upload | SATISFIED | `a_rekey_leaves_every_pack_byte_identical_and_destroys_the_old_keyfile` |
| UX-04 — visible progress | SATISFIED | `a_long_push_reports_advancing_counts_and_every_failure_names_an_action`; `progress::reporter` wired at `cli.rs:567` |

No orphaned requirements: every ID `.planning/REQUIREMENTS.md` maps to Phase 4 is claimed by
a plan and exercised by a test.

---

## Verdict

The phase goal is achieved. The bundle reaches the repo through a nine-step protocol whose
only commit point is a compare-and-swap `PUT`; an interruption before it is provably inert;
the remote shrinks under retention. Every gate was re-run and is green, including with
`$HOME` unset. Both headline measurements were reproduced independently rather than read.
The two previously-unwired functions are wired, and the wiring is visible in the measured
request counts rather than merely asserted.

What keeps this from `passed` is not code:

1. **ROADMAP criterion 1 is unsatisfiable as written**, and the phase was right about it —
   more right than it claimed. The roadmap needs the amendment proposed above.
2. **One new orphan** — `Index::known_chunks`, instance 8 of the pattern. Wire it or delete it.
3. **Four documentation defects**, one of which (D1) is a source comment asserting a hole
   that a later wave in this same phase closed.
4. **One undocumented residual race** that Phase 5 is the phase to trip over.

None is a blocker for merging Phase 4. All four should be resolved before Phase 5 starts,
because Phase 5 reads this bundle and inherits this documentation.

---

_Verified by re-running every gate and reproducing every number. No file in the repository
was modified: three temporary instrumentation edits (`tests/sync_push_e2e.rs` ×2,
`src/sync/plan.rs` ×1) and one deliberate guard probe (`src/sync/push/prune.rs`) were each
reverted immediately; `git status` is clean at `67addcf`._
