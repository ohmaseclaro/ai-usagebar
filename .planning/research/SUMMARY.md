# Research Summary — Encrypted GitHub Sync

**Synthesized:** 2026-08-17 from `encryption.md`, `chunking-storage.md`, `github-transport.md`.
This is the reconciled, decided design. Where the three passes disagreed, the resolution and
its reason are recorded here — the source docs keep the full argument.

## The design in one paragraph

The user names a **private GitHub repo they already own**. A password runs through
**Argon2id** to unwrap a random 32-byte master key held in a keyfile; **BLAKE3** splits that
master into `chunk_key` / `name_key` / `root_key`. Local files are cut into **fixed 256 KiB
chunks**, each addressed by `blake3::keyed_hash(name_key, plaintext)` and sealed with
**XChaCha20-Poly1305** under a nonce *derived from that address*, with the address bound as
AAD. Chunks are batched into **packs uploaded as GitHub Release assets**; the snapshot root
is published through the **Contents API with a `sha` precondition**, giving one
compare-and-swap linearization point. Re-sync uploads only packs containing new chunks.

## Decided parameters

| Choice | Value | Source |
|---|---|---|
| KDF | Argon2id, m=1 GiB, t=3, **p=1** | encryption.md §1 |
| AEAD | XChaCha20-Poly1305, deterministic nonce from chunk id, id as AAD | encryption.md §2 |
| Key hierarchy | password → KEK → wrapped random master → BLAKE3 `derive_key` subkeys | encryption.md §3 |
| Chunk id | `blake3::keyed_hash(name_key, plaintext)` — keyed, not plain | both crypto + chunking |
| Chunking | **fixed 256 KiB** + explicit unsealed tail | chunking-storage.md |
| Compression | zstd before encryption | chunking-storage.md |
| Object store | **Release assets**, packs ≤1.9 GiB | github-transport.md + user decision |
| Snapshot pointer | Contents API `PUT` with `sha` precondition (CAS) | github-transport.md |
| Auth | fine-grained PAT, `Contents: read/write` + `Metadata: read`, one repo | github-transport.md |
| Local index | `rusqlite` (already a dependency) | chunking-storage.md |

**New crates:** `blake3` 1.8.6, `chacha20poly1305` 0.11.0, `argon2` 0.5.3, `zstd` 0.13.3,
`zeroize` 1.9.0, `getrandom` 0.4.3. All pure-Rust/vendored — the AUR source build stays
hermetic. `reqwest` 0.12 needs its `stream` feature added. Reuses existing `rusqlite`,
`serde`, `tokio`, `base64`.

**Rejected:** `fastcdc` (buys nothing — see below), `git2`/`gix` (C dependency / push
unimplemented), `keyring`/`secret-service` (needs live D-Bus, fails over SSH), Git LFS
(10 GiB/month bandwidth ≈ 87 restores, then disabled account-wide), `rand`, `secrecy`,
per-chunk HKDF subkeys, Ed25519 root signatures.

## Conflicts between the research passes, and how they resolved

**1. Object store — Release assets vs git objects.** `github-transport.md` argued AEAD
ciphertext gets zero delta gain, git never forgets a deleted blob, and the undocumented
**10 GB repo cap** arrives within ~2 years of weekly syncs. `chunking-storage.md` countered
with a single orphan branch force-pushed each sync, making the reachable set equal the live
set — but conceded GitHub's `gc` is not user-triggerable, so unreachable objects keep
counting toward billed size with no user control.
→ **Resolved to Release assets** (user decision). Asset deletion actually reclaims space, it
absorbs the optional 4 GB transcripts in the same transport (2 GiB/asset, no stated total
cap), and it sidesteps both the **2 GB push cap** and the 10 GB repo cap. Also removes the
requirement that a `git` binary be installed.

**2. Chunk size — 256 KiB vs 1 MiB.** `encryption.md` suggested 1 MiB (restic/borg sizing);
`chunking-storage.md` argued that sizing exists to amortise per-chunk *network round-trips*,
which packing already eliminates, leaving tail waste (avg N/2 re-uploaded per changed file)
as the only cost driver.
→ **Resolved to 256 KiB.** The reasoning is specific to our packed transport, and it wins the
worked example outright.

**3. Syncing credentials at all.** `encryption.md` recommends *not* syncing OAuth credentials
by default, since re-authenticating on a new machine takes ten seconds.
→ **Overridden — credentials sync by default.** Carrying credentials between machines is the
user's stated purpose for the milestone. The risk is bounded instead by the private-repo-only
gate (SAFE-01/REPO-03) and by making the credentials category clearly visible and
uncheckable at setup. Recorded so the tradeoff is deliberate, not accidental.

## Findings that changed the plan

- **Content-defined chunking is unnecessary — proven, not assumed.** An append changes no
  byte's value and no byte's offset below the old length, so every fully-contained fixed
  chunk keeps its hash. CDC exists to resynchronise after *displacement*; appends displace
  nothing. All five payload categories are append, whole-file rewrite, or same-offset
  overwrite — none has the insert-in-the-middle shape CDC addresses. `fastcdc` dropped.
- **Never create the repo.** Requiring a pre-existing private repo lets the token omit
  `Administration: write`, making the app *structurally incapable* of creating a public repo.
  Stronger than any runtime check, which stays as defence in depth.
- **Per-chunk upload is impossible at any transport.** GitHub's content-creation secondary
  limit is **80/min, 500/hour**; a 5,000-chunk bundle would take ~10 hours. Packed: 3 requests.
- **The 3,000-entries-per-directory limit** alone rules out one-object-per-file for the
  ~4,000 session-index JSONs, before any performance argument.
- **Deterministic nonces are load-bearing**, not a micro-optimisation: a random nonce makes
  every re-upload a fresh object even when the plaintext is identical, defeating dedup.
- **Argon2id p=1 beats p=4.** `argon2` 0.5.3 has no threading, so p>1 costs the defender ~10%
  (measured 1582 ms vs 1429 ms at m=1 GiB) while handing a parallel attacker free speedup.
- **Encrypted chunk *sizes* leak.** arXiv:2504.02095 fingerprints restic/Borg/Duplicacy from
  ciphertext sizes alone; keyed chunk ids plus padding are the mitigations.
- **LFS free tier is 10 GiB storage + 10 GiB bandwidth/month** — an earlier "1 GB" figure used
  while scoping was stale. It does not change the rejection.

## Residual risks carried into the roadmap

| Risk | Handling |
|---|---|
| Password change is not revocation if old keyfiles survive | Release-asset deletion genuinely removes the old wrapped key — verify this is done on rekey |
| Rollback / first-contact TOFU | Local monotonic snapshot counter + refuse non-fast-forward; CAS on the pointer gives the linearization point |
| Stranded packs after prune | Explicit prune path; asset deletion reclaims, unlike git gc |
| Append assumption could be violated | Re-hash the last sealed chunk before taking the fast path; mismatch falls back to full re-chunk |
| Format evolution | `"chunker": "fixed-256k"` and KDF params recorded in the snapshot, so both can change without breaking restore |

## Unverified — must be measured during implementation

1. Whether **private-repo release assets honour `Range:`** after the 302 to signed storage —
   decides pack sizing and partial restore.
2. Whether **Claude Desktop's LevelDB compaction rewrites the 24 MB profile wholesale** each
   session — if so, that category dominates daily sync cost.
3. Real Argon2id timings on a slow Linux target (figures above are M3 Max).
4. Actual compressed size of the 115 MB default bundle (estimates are from data shape).

---
*Sources: `.planning/research/{encryption,chunking-storage,github-transport}.md`*
