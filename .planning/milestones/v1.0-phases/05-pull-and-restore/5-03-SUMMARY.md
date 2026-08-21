---
phase: 05-pull-and-restore
plan: 03
subsystem: sync/restore
tags: [restore, merge, dispositions, credentials, safe-03, sync-06, d7]
status: complete
requires:
  - "sync::restore::{Disposition, ItemPlan, RestorePlan, RestoreCtx, RestoreOptions, Resolved, PackSource} (5-01) — unchanged"
  - "sync::restore::layout::{from_manifest_path, accept_for_write} (5-01)"
  - "sync::crypto::Keys::chunk_id + sync::CHUNK_SIZE — the push side's addressing, reused verbatim"
  - "sync::model::{FileEntry, IndexObject, IndexEntry, Root}"
provides:
  - "sync::restore::merge::plan — every manifest entry becomes exactly one ItemPlan"
  - "the disposition table below, which report.rs (5-06) renders and cli.rs (5-07) maps flags onto"
  - "the credential-classification rule the second consent hangs on"
affects:
  - "5-04 (write.rs) — must stamp restored mtimes to RestorePlan::created_at"
  - "5-06 (report.rs) — RejectedPath now also carries local-destination refusals"
  - "5-07 (cli.rs) — --force-credentials is an addition to --force, never a substitute"
tech-stack:
  added: []
  patterns:
    - "digest before timestamp: identity short-circuits both clocks and both consents"
    - "the pure core takes stat facts as arguments; the impure shell does one symlink_metadata and one hash"
    - "a refusal stays in the plan with dest: None rather than being dropped"
key-files:
  created: []
  modified:
    - src/sync/restore/merge.rs
decisions:
  - "a non-regular-file destination (symlink, directory, socket, device) is RejectedPath, not a SkipLocalNewer-shaped skip — RejectedPath is the only variant carrying a reason, and no consent promotes it"
  - "force_credentials requires force alongside it; force_credentials alone still skips"
  - "LocalFacts::chunk_ids is Option, so an unreadable file can never compare equal to a zero-chunk manifest entry"
  - "packs_needed / bytes_to_fetch are rolled from the index by writable item, replacing 5-01's already-downloaded counts"
metrics:
  duration: ~55 min
  completed: 2026-08-19
---

# Phase 5 Plan 03: The per-item decision — Summary

`merge::plan` turns an opened manifest into a `RestorePlan` in which every entry
carries a disposition and a reason, including the ones it refuses. Identity is
hashed off the disk and decided before either machine's clock is consulted; a
conflict defaults to a skip; a locally-newer credential needs a second consent
that `--force` alone does not grant.

## SIGNATURE CHANGES: none

`merge::plan(&RestoreCtx, &Resolved) -> Result<RestorePlan>` is exactly 5-01's
frozen signature. `Disposition`, `ItemPlan`, `RestorePlan`, `RestoreOptions`,
`RestoreCtx`, `Resolved` and `PackSource` are untouched — no variant added, no
field added, no field's meaning narrowed. Nothing outside
`src/sync/restore/merge.rs` was modified. The four sibling plans in this wave
build against the same text they started from.

Two *behavioural* notes for siblings, neither of which changes a type:

1. **`Disposition::RejectedPath(String)` now also carries a refusal of the local
   destination**, not only of the manifest path — see the decision below. It
   still means "this entry will not be written, and here is why", still carries
   `dest: None`, and `writes()` is still false for it. 5-06's `"rejected"` label
   stays correct.
2. **`packs_needed` / `bytes_to_fetch` changed meaning** from 5-01's tracer
   values (the packs `fetch::resolve` had already downloaded) to what a real run
   *would* fetch. No test depended on the old values.

---

## THE DISPOSITION TABLE

`decide` is pure. Read top to bottom; the first matching row wins.

| # | Local | Digest | Local mtime vs `Root::created_at` | `force` | credential | `force_credentials` | → |
|---|---|---|---|---|---|---|---|
| 1 | absent | — | — | any | any | any | `Create` |
| 2 | present | **equal** | any | any | any | any | `SkipIdentical` |
| 3 | present | differs | `<=` (equal is **not** newer) | any | any | any | `Update` |
| 4 | present | differs | `>` | no | any | any | `SkipLocalNewer { local_mtime, remote_mtime }` |
| 5 | present | differs | `>` | yes | no | any | `Overwrite { local_mtime, remote_mtime }` |
| 6 | present | differs | `>` | yes | **yes** | no | `NeedsCredentialConfirm { … }` |
| 7 | present | differs | `>` | yes | **yes** | yes | `Overwrite { … }` |

And two decided before `decide` is reached, in `decide_entry`, both with
`dest: None`:

| Condition | → |
|---|---|
| `layout::accept_for_write` refuses the **manifest** path (D4: `bridge-state.json`, `local-agent-mode-sessions/**`, caches, locks) | `ExcludedByPolicy` |
| `layout::from_manifest_path` refuses the manifest path (absolute, `..`, backslash, drive letter, unknown root, …) | `RejectedPath(why)` |
| the destination exists and is **not a regular file**, or cannot be stat'd, or has no readable mtime | `RejectedPath(why)` |

`Disposition::writes()` is `Create | Update | Overwrite` and nothing else, so
rows 2, 4, 6 and all three refusals reach neither `backup::take`'s target list
nor `write::apply`.

### The reading order is the security property

**Digest before timestamp, always** (D7, T-5-25). Row 2 short-circuits both
clocks *and* both consents. A timestamp check running first would turn every
already-restored file into a `SkipLocalNewer`, and a resumed interrupted restore
would report two hundred conflicts it does not have.

**Row 4 before row 5** (SAFE-03, D2). The default for a conflict is a *skip with
a report*, not an overwrite. Both timestamps travel in the variant so 5-06 can
say which is which and the user can re-run with `--force`.

**Row 6 before row 7** (D2, T-5-23). `force` alone never overwrites a
locally-newer credential.

---

## THE CREDENTIAL-CLASSIFICATION RULE

`report.rs` and `cli.rs` both depend on this. An entry is credential-bearing iff:

```rust
category == SyncCategory::Credentials          // the whole desktop-profiles/** store
    || file_name == ".credentials.json"        // whatever root it arrives under
```

The file-name half is not redundant. `scope` collects
`config/accounts/<name>/.credentials.json` under `SyncCategory::Config`
alongside `config.toml`, and `claude-home/.credentials.json` would land under
`Routines` — a category-only rule would give both of them the ordinary
`Overwrite` arm, which is precisely the live-OAuth-token revert D2 exists to
prevent. `desktop-profiles/**` is credential-bearing wholesale because the token
caches beside `meta.json` are.

The classification is checked **only** on rows 6/7, i.e. only when the item is
both locally newer and already under `force`. A credential that is not locally
newer is an ordinary `Update` (table row 3): the second consent guards the
*loss*, not the category, and demanding a confirmation for every credential in a
fresh restore is how a gate gets reflexively passed.

`--force-credentials` is an **addition** to `--force`, never a substitute:
`force_credentials` without `force` still yields row 4. 5-07 should reflect that
in clap (`--force-credentials` requires `--force`).

---

## WHAT THE TIMESTAMP COMPARISON ASSUMES

`model::FileEntry` carries `path`, `mode`, `true_len` and the chunk ids — **no
mtime** — so the remote side of every comparison is `Root::created_at`, one
value for the whole snapshot. Adding a per-file mtime is a `MANIFEST_VERSION`
bump to an already-shipping wire format for a refinement nothing yet needs; the
named upgrade path is a per-file `mtime_ns` under `MANIFEST_VERSION` 3, read in
preference to `created_at` when present.

**Plan 5-04 owes this module one thing:** stamp every restored file's mtime to
`RestorePlan::created_at`. That is what makes the comparison exact rather than
merely conservative — a restored-then-untouched file then compares *equal*
(table row 3, not row 4) and the next pull updates it cleanly instead of
reporting a phantom conflict.

**The assumption, stated:** the two timestamps come from two different machines'
clocks, so this assumes only that they agree to within a snapshot's age. NTP
makes that true and a few seconds of drift does not break it. Both directions of
a wrong guess are recoverable rather than destructive:

- **This clock runs fast** (or the pusher's runs slow) → an unchanged local file
  looks newer → row 4, skipped and named in the report. Costs one re-run with
  `--force`.
- **This clock runs slow** → a locally-changed file looks older → row 3, updated.
  This is the only direction that loses data, and it is why D3's backup is taken
  before the first byte even when nothing looked like a conflict, and why every
  overwritten item is named in the outcome. Recovery is the `tar -xzf` line
  `BackupRecord::rollback_command` prints.

Digest-first is what keeps drift cheap: a file that did not change is decided at
row 2 before either clock is read, so only genuinely diverged files can be
misjudged at all.

---

## HOW LOCAL FACTS ARE GATHERED

- **One stat call, `fs::symlink_metadata`** — the only stat in the file, verified
  by the plan's own grep (`fs::metadata(` count outside comments: 0). A link
  planted at a destination is *seen* as a link (T-5-22).
- **Identity is hashed off the disk** with `Keys::chunk_id` in `CHUNK_SIZE`
  buffers, exactly as `plan::build` does on the push side, so an untouched file
  is recognised across machines. **The local SQLite index is never consulted**:
  it is a cache keyed on the *push* side's stat tuple, and one stale row would
  declare a file the user has since edited identical and skip it (T-5-24). A
  test seeds two same-length, different-content files to pin this.
- **A length mismatch short-circuits the read.** A file whose size differs from
  the manifest's `true_len` cannot share its chunk ids; learning that from the
  stat beats reading a 50 MB transcript to reach the same answer.
- **The hash buffer is `Zeroizing`.** It holds the plaintext of, among other
  things, a live OAuth token.
- **`LocalFacts::chunk_ids` is `Option<Vec<ChunkId>>`, not `Vec<ChunkId>`.** A
  zero-byte manifest entry has no chunks either, so an unreadable file
  represented as an empty list would compare *equal* to it and be skipped as
  "identical" without a byte ever having been read. `None` means "identity not
  established" and falls through to the timestamp rule, which is the correct
  semantics for both the unreadable and the length-mismatch cases.

## RESTORE PLANS NO DELETION

Stated in the module doc as a decision, not left as an omission: a local file
the manifest does not mention is left exactly as it is, and a test asserts it
never enters the plan even under `--force --force-credentials`. A snapshot is
what one machine had, not an assertion about what every machine should have.
The `synced.json` baseline `claude_desktop::merge` uses to tell a deletion from
"never had it" is the right machinery for a future selective restore (REC-02,
deferred to v2); reaching for it here would build a second reconciliation model
for a case v1 does not have.

---

## Deviations from Plan

### [Rule 1 — correctness] A non-regular-file destination is `RejectedPath`, not a "`SkipLocalNewer`-shaped refusal"

The plan asked for a symlink at a destination to be a "`SkipLocalNewer`-shaped
refusal **with its own message**". `SkipLocalNewer { local_mtime, remote_mtime }`
is frozen and carries no message, and — worse — it is the one disposition
`--force` *promotes to `Overwrite`*. A symlink reported as `SkipLocalNewer`
would be written through by the user's obvious next command, which is exactly
T-5-22 (critical). It would also render as "skip (local is newer)", a false
reason.

`RejectedPath(String)` is the only variant that carries a reason, `dest: None`
is its frozen contract, and **no consent path promotes it** — a test asserts the
refusal holds under `force`, under `force + force_credentials`, and that the
symlink's target is untouched. The same guard covers a directory, a socket, or a
device node where a regular file is expected: one rule — *the destination is a
regular file or nothing at all* — rather than a symlink special case plus a
directory special case.

This also resolves the plan's directory-becomes-`Update` clause, which would
have hard-failed `write::apply` mid-restore *after* the backup was taken,
instead of reporting the obstruction and restoring everything else (D6).

### [Rule 1 — correctness] The io error of an unreadable file is not carried

The plan asked for an unreadable local file to be "`Update` with the io error
recorded". `Disposition::Update` and `ItemPlan` are both frozen and carry no
free text, and T-5-26 keeps disposition payloads to paths, ids and timestamps.
The behaviour that mattered holds — no panic, no silent skip: an unreadable
*regular* file yields `chunk_ids: None`, which is never identical, and falls
through to the timestamp rule (`Update` when not locally newer, exactly as the
plan says; `SkipLocalNewer` when it is, which is SAFE-03 applying uniformly
rather than a chmod-000 hole in it). The error itself is dropped. A stat failure
or an unreadable mtime — where nothing at all can be established — is
`RejectedPath` and does carry its message.

### [Rule 3 — blocking] mtime is converted via `Metadata::modified`, not `scope::push_path`

The plan asked for the mtime to come "through the crate's existing `mtime_ns`
handling in `scope::push_path`". `scope::stats` is private and `push_path`
returns nothing — it appends a `scope::FileEntry` to a `CategoryScan`, and this
plan may modify no other source file. `md.modified()` reads the same `st_mtim`
`scope::stats` reads on unix and is what its own non-unix arm already uses, and
`DateTime::<Utc>::from(SystemTime)` is one conversion rather than a second
hand-rolled epoch split.

### `force_credentials` requires `force`

The plan's behaviour list says "`force_credentials` set turns that into
`Overwrite`" without saying whether `force` must also be present. Implemented as
**both required** — the conservative reading of "a second explicit confirmation"
and of "`force` alone does not grant it" — with a test naming the choice. The
alternative (either flag suffices) is a strictly wider write door on the one
class of file this phase is most afraid of losing.

### `packs_needed` / `bytes_to_fetch` now count what a run would fetch

5-01's tracer set them from `PackSource::{packs, bytes}` — what `fetch::resolve`
had already downloaded. They are now rolled from `resolved.index`: each writable
item's chunk ids resolved to their distinct packs, then the sealed `clen` of
every index entry in those packs. Built through one `HashMap` pass rather than
`IndexObject::resolve` per chunk, which that method's own doc asks of a caller
resolving thousands of ids.

---

## Known Stubs

None. `merge.rs` has no remaining stub; the four dispositions 5-01 listed
against this plan (`SkipIdentical`, `SkipLocalNewer`, `Overwrite`,
`NeedsCredentialConfirm`) are all live.

## Threat Flags

None. No new network endpoint, auth path, or trust boundary — this plan removed
one write path (symlink follow-through) and added no reads outside the roots
`layout` already gates.

## Verification

```
cargo test                                    1409 lib passed, 0 failed   (baseline 1388, +21)
                                              1450 total passed, 0 failed (baseline 1429, +21)
cargo test --lib sync::restore::merge         22 passed, 0 failed
HOME= cargo test --lib sync::restore          44 passed, 0 failed
cargo clippy --all-targets -- -D warnings     clean
cargo fmt --check                             clean
grep -v '^\s*//' merge.rs | grep -c 'fs::metadata('   0
git diff Cargo.toml Cargo.lock                empty — no new crates
```

Tests are hermetic: every destination is a `TempDir`, every timestamp is the
fixed `SNAPSHOT` constant passed in as `Root::created_at`, local mtimes are
stamped with `fs::FileTimes` rather than read from the wall clock, and the
`Client` points at `http://127.0.0.1:1` and is never called. The unreadable-file
test checks its own premise and returns early if it is running as root.

## Self-Check: PASSED

- `src/sync/restore/merge.rs` — present, modified, the only source file touched.
- Commits `4368faa` (RED, test) and `9959ded` (GREEN, feat) — both on `gsd/5-03`.
- `git status --short` after the feat commit: only this SUMMARY untracked.
