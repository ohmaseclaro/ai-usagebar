---
phase: 05-pull-and-restore
plan: 02
type: execute
wave: 2
depends_on: ["5-01"]
files_modified:
  - src/sync/restore/fetch.rs
autonomous: true
requirements: [SAFE-05]
must_haves:
  truths:
    - "Every hop of the chain — pointer → keyfile → root → manifest → index → pack → blob → plaintext — is authenticated before its output is used, and a failure at any hop yields zero bytes of plaintext to the caller."
    - "`manifest_chunks` and the index object's chunk list are each read under an explicit ceiling, because both are id lists a reader consumes **before** the container that authenticates them is fully verified."
    - "A pack whose downloaded bytes do not hash to the asset name it was fetched under is refused before its header is opened."
    - "A manifest entry naming a chunk id absent from the index object is a refusal that names the id, not a partially restored tree."
    - "Each pack is downloaded at most once per restore, and the total downloaded is bounded by a ceiling derived from `PACK_MAX`."
    - "`anchor::accept` is called against the **root's** sealed `counter` and `repo_id`, never the plaintext pointer's, and its `repo_id`-mismatch arm is not routed around under `allow_rollback`."
    - "Whole packs are fetched. `Range:` is named as the optimisation it would be if CAL-1 were ever run positive, and is not implemented on an unmeasured assumption."
    - "No test opens a socket to a host that is not the injected `Endpoints` base."
  artifacts:
    - src/sync/restore/fetch.rs — `resolve`, the bounded chain, the pack cache, and the four read ceilings
  key_links:
    - "`push::pointer::load` is reused, not reimplemented — it already probes `format` before deserializing and refuses a foreign `repo_id`"
    - "`Root::open` binds `repo_id` into the AAD, so a repo swap fails the Poly1305 tag before anything is parsed; nothing here re-checks it by string comparison and calls that security"
    - "`chunk::reassemble` and `chunk::open_chunk` already re-derive the id from the plaintext; this module supplies the ids, it does not re-verify by hand"
    - "`download_asset`'s 64 MiB cap sits above `pack::PACK_MAX` of 48 MiB, so no streaming verb and no `reqwest` `stream` feature is needed"
---

<objective>
The verified chain. Turn a pointer into an opened `Root`, a reassembled `Manifest`, an
`IndexObject`, and a `PackSource` that can hand out any chunk the manifest names — with a bound on
every list a hostile remote controls, and nothing partially trusted on the way.

Purpose: this is where a tampered bundle either stops or becomes local writes. Every ceiling here
exists because the value it bounds is attacker-chosen.

Output: `src/sync/restore/fetch.rs`, filled.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/05-pull-and-restore/5-CONTEXT.md
@.planning/phases/05-pull-and-restore/5-01-SUMMARY.md
@CLAUDE.md
@docs/sync-format.md
@src/sync/model.rs
@src/sync/chunk.rs
@src/sync/pack.rs
@src/sync/anchor.rs
@src/sync/crypto.rs
@src/sync/push/mod.rs
@src/sync/push/pointer.rs
@src/sync/github/write.rs
@src/sync/restore/mod.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Pointer to root — the bootstrap, and the anchor gate at the only place it can be right</name>
  <files>src/sync/restore/fetch.rs</files>
  <behavior>
    - A pointer with an empty `snapshots` list is a distinct error saying nothing has been pushed yet, not an empty successful restore.
    - The newest snapshot is selected by the **sealed** `counter` inside each opened root, not by the pointer's list order — a reordered list is inert.
    - A keyfile asset absent from the release is an error naming the asset; a keyfile that will not open under the supplied passphrase is the existing wrong-password error, unchanged and unelaborated.
    - `Root::open` under a `repo_id` other than the caller's own fails, and the message does not distinguish a wrong repo from a wrong key.
    - `anchor::accept` is called with the root's `repo_id` and `counter`; a lower counter is refused and names `--allow-rollback`; a mismatched `repo_id` is refused **even with** `allow_rollback` set.
    - A pointer carrying more `SnapshotRecord`s than `MAX_SNAPSHOTS_IN_POINTER` is refused naming the ceiling, before any asset is fetched.
  </behavior>
  <action>
Fill the first half of `resolve` in `src/sync/restore/fetch.rs`, following the numbered chain in
`restore/mod.rs`'s doc comment.

Load the pointer through `push::pointer::load`, which already probes `format` against the build
ceiling and refuses a `repo_id` that is not the caller's. Do not add a second `format` check or a
second `repo_id` string comparison — a duplicated check that later diverges is worse than one
check in one place, and `push/pointer.rs` is the owner.

Bound the pointer's `snapshots` list with `const MAX_SNAPSHOTS_IN_POINTER: usize` set from the
`keep_snapshots` default plus generous headroom for the monthly retention tail, and refuse above
it naming the number. The pointer is plaintext and unauthenticated; every list inside it is a
remote-chosen length and needs a ceiling before it is walked.

Resolve the release id through `write::ensure_release` against the frozen `RELEASE_TAG` — but note
in a comment that on the read side a missing release is "nothing pushed yet", never a reason to
create one, and take the read-only branch if `ensure_release` would create. If its shape makes
that awkward, call `Client::get_json` on the tag directly rather than reaching for a verb whose
404 arm creates. Restore must be structurally incapable of writing to the remote.

`list_assets` once, and keep the result: the keyfile and every pack are looked up in that one
listing by name, so a restore issues one listing, not one per object.

Download the keyfile asset named by `Pointer.keyfile`, parse it as `crypto::Keyfile`, and `open`
it with the passphrase the CLI supplied. Nothing about the failure message changes here — the
existing wrong-password error is deliberately indistinguishable from a tampered keyfile, and
elaborating it would build the oracle Phase 1 refused to build.

For each `SnapshotRecord`, fetch its root and `Root::open` it under the caller's own `repo_id`.
The `repo_id` is bound into the root's AAD, so a repo swap fails the Poly1305 tag before any field
is parsed — say that in a comment so nobody later "hardens" it with a string compare and thinks
they added something. Select the snapshot with the highest sealed `counter`. Ordering of the
pointer's list is not trusted for selection.

Call `anchor::accept(local_anchor, &root.repo_id, root.counter, ctx.opts.allow_rollback)` with the
values from the **opened** root. Do not persist anything: `accept` decides, `restore::mod`'s
step 7 persists, and only after this whole function returns `Ok`. Repeat the reason in a comment,
because the ordering is the mitigation for a permanent lockout an attacker with repo write access
could otherwise trigger at will.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore::fetch</automated>
  </verify>
  <done>`cargo test --lib sync::restore::fetch` is green for the bootstrap half. A restore issues exactly one `list_assets`. Snapshot selection is by sealed counter. `anchor::accept` receives the root's values, and the `repo_id`-mismatch case is refused under `allow_rollback`. No code path in the module can create a release or send a request body. The pointer's snapshot list has a ceiling.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Manifest, index, and packs — four ceilings and a cache</name>
  <files>src/sync/restore/fetch.rs</files>
  <behavior>
    - A root whose `manifest_chunks` exceeds `MAX_MANIFEST_CHUNKS` is refused naming the ceiling and the observed length, before a single chunk is fetched.
    - The same for the index object's chunk list against `MAX_INDEX_CHUNKS`, and for the sum of pack sizes against `MAX_RESTORE_BYTES`.
    - A manifest chunk whose ciphertext is served under another chunk's id fails, because `chunk::open_chunk` re-derives the id — asserted here rather than assumed from Phase 1.
    - A truncated manifest — the last chunk withheld — fails to reassemble and yields no partial `Manifest`.
    - A pack whose downloaded bytes do not content-address to the id in its asset name is refused before `pack::read_header` is called.
    - A manifest entry naming a chunk id the index object does not resolve is an error naming the id.
    - Two manifest entries sharing a chunk cause exactly one download of the pack holding it, asserted as a mock hit count.
    - A `PackSource` handed the same chunk id twice returns identical plaintext both times and downloads nothing the second time.
  </behavior>
  <action>
Fill the second half of `resolve`.

Four ceilings, each a `const` at the top of the file with a comment naming what it bounds and why
that value is attacker-chosen: `MAX_MANIFEST_CHUNKS`, `MAX_INDEX_CHUNKS`, `MAX_PACKS_PER_RESTORE`,
`MAX_RESTORE_BYTES`. Derive each from something real rather than picking a round number — the
manifest ceiling from `docs/sync-format.md`'s recorded manifest sizing (a ~1600-file manifest is
2 chunks, ~5700 files is 5) with an order of magnitude of headroom; the byte ceiling from
`MAX_PACKS_PER_RESTORE` times `pack::PACK_MAX`. Each refusal names the ceiling and the observed
value, so a user who legitimately outgrows one gets a number to raise rather than a mystery.

The `manifest_chunks` ceiling is the one that matters most, and its comment should say why:
`Root.manifest_chunks` is an unbounded `Vec<ChunkId>` that is safe inside Phase 1's authenticated
plaintext, but a reader consumes it to decide **how many fetches to issue** — a decision made
from a list before the objects it names have authenticated anything. That is the exact shape the
Phase 1 handoff flagged as living in restore.

Fetch the manifest's chunks through the `PackSource`, then `Manifest::open`. Fetch the index
object's chunks through `Pointer`'s `RemoteIndexEntry` bootstrap — those entries carry `pack`,
`offset`, `clen`, and `true_len`, which is exactly what a reader holding only the pointer needs —
then `IndexObject::open`.

`PackSource` is the pack cache and the only downloader. Keyed by pack `ChunkId`, it downloads a
whole pack through `write::download_asset` under `push::pack_asset_name`, verifies
`crypto::content_address(&bytes)` equals the id it asked for **before** calling
`pack::read_header`, then holds the bytes and the parsed header. A chunk lookup resolves through
the `IndexObject` (falling back to the pointer's `RemoteIndexEntry` list for the index object's
own chunks, which is the bootstrap), locates the `PackEntry`, and returns
`pack::open_blob`'s `Zeroizing<Vec<u8>>`. Do not cache plaintext: caching the ciphertext is what
avoids the re-download, and holding decrypted bytes longer than the write that consumes them is
the opposite of what SAFE-05 is about.

Whole packs, not ranges. Write the reason in the module doc: CAL-1 — whether private-repo release
assets honour `Range:` after the 302 to signed storage — was scheduled in Phase 1 and again in
Phase 3 plan 3-06 and was **not** run, so `PACK_TARGET` stands at 32 MiB and a restore fetches
each pack it needs in full. Name the optimisation explicitly so a future measurement has somewhere
to land: if `Range:` is confirmed, `PackSource` gains a byte-range fetch keyed on the `PackEntry`'s
`offset` and `clen`, and everything else in this file is unchanged. Do not implement it on an
assumption; the pessimistic path is correct either way and the optimistic one is wrong if the
measurement comes back negative.

Every test seeds its remote by calling the push side's `packer::build` and serving the result
through `mockito`, then mutates one byte, withholds one chunk, or renames one asset to produce the
adversarial cases. A hand-written fixture would pass while the real pair is broken.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore::fetch</automated>
  </verify>
  <done>`cargo test --lib sync::restore::fetch` is green. All four ceilings exist, are derived from a named quantity, and each has a refusal test that names the observed value. Tampered pack, wrong-id chunk, truncated manifest, and missing chunk each refuse and return no plaintext. A shared pack downloads once. Nothing decrypted is cached beyond the call that produces it. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| plaintext pointer → process | Unauthenticated container; its lists are remote-chosen lengths |
| authenticated plaintext → fetch decisions | `manifest_chunks` and the index chunk list are consumed to decide how many fetches to issue |
| release asset bytes → pack parser | Attacker-supplied bytes reaching a length-prefixed header parser |
| remote counter → local anchor | The value that decides whether this machine can read its own bundle again |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-5-10 | Denial of service | `Root.manifest_chunks` | high | mitigate | `MAX_MANIFEST_CHUNKS`, checked before the first fetch is issued, derived from the recorded manifest sizing with headroom and naming the observed value on refusal |
| T-5-11 | Denial of service | the index object's chunk list and the pack set | high | mitigate | `MAX_INDEX_CHUNKS`, `MAX_PACKS_PER_RESTORE`, and `MAX_RESTORE_BYTES` derived from `pack::PACK_MAX`, each refusing with its number |
| T-5-12 | Denial of service | the pointer's `snapshots` list | medium | mitigate | `MAX_SNAPSHOTS_IN_POINTER`, checked before any asset fetch |
| T-5-13 | Tampering | a pack served under another pack's asset name | critical | mitigate | `crypto::content_address` of the downloaded bytes is compared to the requested id **before** `pack::read_header` parses a length prefix |
| T-5-14 | Tampering | a chunk served under another chunk's id | critical | mitigate | `chunk::open_chunk` re-derives the id from the plaintext and the id is the AEAD's AAD; asserted here rather than inherited on trust |
| T-5-15 | Spoofing | a snapshot selected by pointer list order | high | mitigate | Selection is by the **sealed** `counter` inside each opened root, so reordering the plaintext list changes nothing |
| T-5-16 | Denial of service | a forged high counter persisted before verification | critical | mitigate | This module calls `anchor::accept` and persists nothing; persistence is `restore::run` step 7, after `resolve` returns `Ok` |
| T-5-17 | Spoofing | a counter borrowed from another bundle by renaming | high | mitigate | `accept` errors on a `repo_id` mismatch at any counter and under `allow_rollback`; `Root::open` binds `repo_id` into the AAD so the tag fails first |
| T-5-18 | Information disclosure | an error distinguishing a wrong password from a tampered keyfile | medium | mitigate | The existing single "cannot decrypt" error is reused verbatim; no new arm is added |
| T-5-19 | Information disclosure | decrypted bytes cached longer than needed | high | mitigate | `PackSource` caches ciphertext only; plaintext is `Zeroizing` and returned to the caller that consumes it |
| T-5-20 | Elevation of privilege | restore writing to the remote | high | mitigate | No request-body call site exists in this file; a missing release is "nothing pushed yet", never a create |
| T-5-SC | Tampering | npm/pip/cargo installs | high | mitigate | No new crates; `cargo machete` runs in the phase gate |
</threat_model>

<verification>
- `cargo test --lib sync::restore::fetch` green.
- `grep -vn '^\s*//' src/sync/restore/fetch.rs | grep -c 'upload_asset\|put_contents\|delete_asset'` is 0.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` clean.
- `HOME= cargo test --lib sync::restore::fetch` passes.
</verification>

<success_criteria>
1. Every hop authenticates before its output is used; a failure at any hop yields zero plaintext.
2. Four ceilings exist, each derived from a named quantity, each refusing with its number.
3. A tampered pack, a misfiled chunk, a truncated manifest, and a missing chunk each refuse.
4. Each pack downloads once; nothing decrypted outlives its consumer.
5. Whole-pack fetch ships; `Range:` is documented as the CAL-1-gated optimisation and not implemented.
</success_criteria>

<output>
Create `.planning/phases/05-pull-and-restore/5-02-SUMMARY.md` when done, recording the four
ceiling constants with the quantity each was derived from.
</output>
