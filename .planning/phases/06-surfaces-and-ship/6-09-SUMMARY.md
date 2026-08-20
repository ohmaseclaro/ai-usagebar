---
phase: 6
plan: 9
subsystem: sync/restore
tags: [security, credentials, macos, safe-storage, restore]
status: complete
requires: [sync/restore/merge, safe_storage]
provides: [Disposition::ForeignSafeStorage, safe_storage::looks_like_value, safe_storage::Key]
affects: [src/safe_storage.rs, src/sync/restore/merge.rs, src/sync/restore/mod.rs, src/sync/restore/report.rs]
---

# Phase 6 Plan 9: A Token Cache That Will Not Decrypt Here Is Never Written

Restore now refuses a Claude Desktop token cache sealed by another Mac's login
Keychain, decided by attempting the decryption with this machine's key.

## Signature changes

**One public enum gained a variant.** `Disposition::ForeignSafeStorage`
(`src/sync/restore/mod.rs`). It is a unit variant, `writes()` returns `false`
for it, and both exhaustive matches over `Disposition` — `report::facing` and
`report::mtimes` — were extended. Nothing else in the crate matches
exhaustively on it (`cli.rs` and `write.rs` use `matches!`).

Two additions to `src/safe_storage.rs`, both new, neither a change:
`pub type Key = [u8; KEY_LEN]` and `pub fn looks_like_value(&str) -> bool`.
Existing signatures are byte-identical — `decrypt`/`encrypt`/`derive_key`/
`macos_key` were not touched.

Two private functions in `merge.rs`: `plan_with_safe_key` (the seam) and
`foreign_safe_storage` (the decision). `merge::plan`'s own public signature is
unchanged.

## What a user loses and keeps when a token cache is refused

**Keeps:** the Claude Desktop login they already have on this Mac, byte for
byte — this is the whole point, and it is asserted on the file, not just on the
disposition. Keeps `.credentials.json`, so Claude Code the CLI works on the
second machine exactly as before. Keeps transcripts, routines and the chat
index, none of which this change comes near. Keeps the blob itself in the
bundle: pushing is unchanged, and restored to the *same* machine after a disk
loss it opens and works — which is precisely what the decrypt test detects.

**Loses:** nothing that was ever usable. The refused blob is AES ciphertext
under a key that does not exist on this machine; writing it would have left
Claude Desktop a token cache it cannot read, which is worse than finding none.
The cost is one sign-in, and the report says so in those words:

```
  >> NOT RESTORED — 1 item(s) in the snapshot were refused:
     desktop-profiles/work/config-tokenCacheV2
       REFUSED — this Claude Desktop session is locked to the Mac that saved it
       and cannot be read here; sign in to Claude Desktop on this Mac
```

## How the decision is made

By attempting the decryption, not by recording provenance. The local
`Claude Safe Storage` key either opens the incoming blob or it does not, and
that *is* the question. No manifest field, no `MANIFEST_VERSION` bump, no new
metadata to keep in sync — and nothing a bundle could forge.

In `merge::decide_entry`, after the path resolves and **before the destination
is stat'ed**, so the refusal holds whether or not a local token cache is there
to lose. `dest` stays `None`, matching the two existing refusals: an entry that
will never be written has structurally nowhere to be written.

`foreign_safe_storage` returns `true` when all of:
- `true_len` is non-zero and ≤ 64 KiB (a token cache is a few hundred bytes of
  base64; the ceiling also keeps a 50 MB transcript from being decrypted into
  memory to answer a question its size already answers, and, being under
  `CHUNK_SIZE`, makes "one chunk" true rather than assumed);
- the content is valid UTF-8 starting with `djEw` — base64 of Chromium's `v10`
  marker, pinned to `PREFIX` by its own test so the two cannot drift;
- and this machine's key does not open it. **No local key at all is the same
  answer** for the same reason.

**Only the boolean escapes.** The decrypted plaintext is dropped through
`Zeroizing` and neither it, the key, nor any fragment of either reaches a
return value, a log line, a report or an error message.

macOS-only: the `#[cfg(not(target_os = "macos"))]` arm is `false` and the only
macOS-gated symbol used anywhere is `safe_storage::macos_key`, called solely
inside `#[cfg(target_os = "macos")] fn local_safe_key`.

## Known limitation: a dry run cannot answer the question

`fetch::resolve` downloads file content only under `apply`, so a dry run has no
data packs and `packs.chunk()` fails. `foreign_safe_storage` returns `false`
there — a dry run reports a token cache's ordinary disposition, and the run
that would actually write is the run that refuses. The gate guards the write,
and the write is where the loss would happen. A test pins this rather than
leaving it to be discovered. Making the dry run exact would mean downloading
the credentials category's data packs during a dry run, which is a change to
`fetch`'s contract for a report line; deferred deliberately.

## Report changes (two sites, for the sibling branches)

`src/sync/restore/report.rs` — the minimum a new disposition needs, no more:

- **lines 630–636**: one new `facing()` arm, `Disposition::ForeignSafeStorage`
  → `Kind::Refusal` with its verb. No existing arm was edited.
- **line 661**: `| Disposition::ForeignSafeStorage` added to the `None` list in
  `mtimes()`. One line, appended to an existing chain.

Nothing else in that file was touched — not the colour handling, not the
`--apply` wording, not `refusals_block`, which already renders the new variant
because it filters on `Kind::Refusal`.

## Tests

Seven added (six in `merge.rs`, one in `safe_storage.rs`). All hermetic:
`TempDir`, no real `$HOME`/`$XDG`, no network, no wall clock. **No test reads
the real login Keychain** — the key is injected through `plan_with_safe_key`,
which is why that seam exists.

| Test | Asserts |
|------|---------|
| `a_blob_this_machines_key_opens_is_restored` | Same key → restored, and the bytes on disk match |
| `a_blob_from_another_machine_is_refused_and_the_live_session_survives` | Refused, `!writes()`, named in `render_plan`, **and the pre-existing local file is byte-identical after a full `write::apply`** |
| `a_blob_is_refused_when_this_machine_has_no_key_at_all` | Absent locally + no key → still not written |
| `a_dot_credentials_json_restores_in_the_same_run_that_refuses_a_token_cache` | The carve-out is narrow |
| `a_file_at_the_same_path_shape_that_is_not_a_safe_storage_value_is_untouched_by_the_gate` | The marker decides, not the path |
| `without_the_data_packs_the_gate_stays_out_of_the_way` | The dry-run limitation, pinned |
| `the_base64_marker_is_the_v10_prefix_and_cannot_drift_from_it` | `djEw` == base64(`PREFIX`) |

## New call sites

Every function added has at least one production caller — none is zero:

| Added | Production call site |
|-------|---------------------|
| `safe_storage::looks_like_value` | `merge::foreign_safe_storage` |
| `safe_storage::Key` (type) | `merge::{local_safe_key, plan_with_safe_key, decide_entry, foreign_safe_storage}` |
| `merge::local_safe_key` | `merge::plan` |
| `merge::plan_with_safe_key` | `merge::plan` |
| `merge::foreign_safe_storage` | `merge::decide_entry` |
| `Disposition::ForeignSafeStorage` | produced by `merge::decide_entry`; consumed by `report::facing`, `report::mtimes`, `Disposition::writes` |

## Verification

- `cargo test` — **1737 passed, 0 failed** (baseline 1730 + 7 new).
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo fmt --check` — clean.
- `make test` — GNOME, KDE and Omarchy contract suites pass.
- `Cargo.toml` / `Cargo.lock` byte-identical; no dependency added.
- No Swift touched, so `./macos/run-tests.sh` was not run.
- **Linux:** no cross-compilation target was installed (adding one was out of
  scope). Verified instead by flipping every `target_os = "macos"` predicate in
  `src/` to a value that never matches and running `cargo check --all-targets`:
  the only two errors were pre-existing macOS-gated modules unrelated to this
  change (`sync::github::keychain` and its `clear`, imported by `tests/live.rs`).
  Nothing in `safe_storage.rs`, `merge.rs`, `mod.rs` or `report.rs` failed.

## Deviations from plan

None. One process incident: a `git checkout -- src` run to undo the Linux probe
discarded the whole uncommitted change, which was then reapplied identically and
re-verified before committing. Nothing shipped differs because of it.

## Self-Check: PASSED
