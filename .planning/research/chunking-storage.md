# Chunking + Content-Addressed Storage for Encrypted GitHub Sync

**Researched:** 2026-08-19
**Scope:** chunking algorithm, small-file packing, repo layout, incremental detection, pruning/GC, Rust crates, restic/borg prior art
**Overall confidence:** HIGH on the measurable facts (GitHub limits, crate versions, restic/borg internals), HIGH on the append-only analysis (it is a proof, not a benchmark), MEDIUM on the compression ratios and per-sync growth numbers (estimated from JSONL characteristics, not measured on this machine)

---

## TL;DR — the one recommended design

| Decision | Value |
|---|---|
| Chunker | **Fixed-size 256 KiB, offset-aligned, with an explicit unsealed tail chunk.** Not CDC. |
| Chunk ID | **Keyed BLAKE3** (`new_keyed`) — a MAC of plaintext, not a plaintext hash |
| Small files | Files < 256 KiB become one whole-file blob; blobs are packed |
| Pack file | Target **32 MiB**, hard cap **48 MiB** (stays under GitHub's 50 MiB warning), immutable, content-addressed, sharded `packs/ab/<64hex>.pack` |
| Store | The git repo as a **dumb immutable blob host** — a single-commit orphan branch, force-pushed each sync. Version history lives in our snapshot objects, never in git commits. |
| Change detection | borg's rule: `(size, mtime_ns, ctime_ns, inode)` match ⇒ unchanged. Append verified by re-hashing the **last sealed chunk only** (256 KiB read). |
| Local index | SQLite via `rusqlite` — **already a dependency**, zero new deps |
| Transcripts | Opt-in **and bounded** (default 90 days / 1 GiB cap). 4 GB unbounded does not fit GitHub and needs a second code path; do not build it. |
| New crates | `blake3`, `chacha20poly1305`, `argon2`, `zstd`. **No** `fastcdc`, **no** `git2`/`gix`. |

**Worked example — append 200 KB to a 50 MB transcript: ~90 KB uploaded.** (Derivation in §8.) Naive whole-file re-encrypt would upload ~10 MB. That is a **~110x** reduction, and it needs no content-defined chunking at all.

---

## 1. Chunking algorithm — CDC vs fixed-size

### 1.1 The claim to test

CDC (FastCDC / Rabin / Gear) exists to solve exactly one problem: **boundary shift**. If you chunk at fixed offsets `0, N, 2N, …` and someone *inserts* or *deletes* k bytes at offset p, every byte after p moves by k, so every chunk after p gets a new content hash even though the data is unchanged. CDC picks boundaries from a rolling hash of the content itself, so boundaries re-synchronise a few hundred bytes after the edit and the tail of the file re-dedups.

### 1.2 Appends do not shift boundaries — a proof, not a heuristic

Let a file have length `L`, chunked at fixed size `N` into chunks `C_i = bytes[iN, (i+1)N)`.

Append `k` bytes: the new file is `bytes[0..L] ++ new[0..k]`. By definition of append, **no byte at an offset < L changes value or offset.** Therefore for every `i` with `(i+1)N ≤ L`, `C_i` is byte-identical before and after, and its content hash is unchanged.

The only chunks affected are:
- the chunk straddling `L` (the partial tail), and
- newly created chunks past `L`.

**Boundary shift is caused by offset displacement. A pure append displaces nothing.** For append-only files, fixed-size chunking achieves *identical* dedup to CDC, while being simpler, faster, seek-aligned, and deterministic without a rolling hash.

This is the single most important finding in this document: **the boundary-shift problem does not apply to this workload, so the CDC dependency does not need to exist.**

### 1.3 The workload actually has three mutation shapes, and CDC helps with none of them

| Payload | Size | Mutation shape | Does CDC help? |
|---|---|---|---|
| Chat transcripts (`.jsonl`) | 4.0 GB, 4110 files | **Append** — new lines appended | No — §1.2 |
| Chat session indexes | 78 MB, many small JSON | **Whole-file rewrite** (serialize the whole doc) | No — the whole file is new content; nothing is dedupable |
| Claude Desktop OAuth profiles | 24 MB (LevelDB/SQLite/Local Storage) | **In-place page overwrite** (SQLite pages, LevelDB `.log` appends; `.ldb` files are write-once) | No — an in-place same-offset overwrite touches exactly the chunks it lands in. No displacement. |
| config | 13 MB | Whole-file rewrite | No |
| routines | 20 KB | Whole-file rewrite | No |

The pattern CDC was designed for — *insert/delete in the middle of a large file, shifting the remainder* — occurs in **none** of the five categories. A generic backup tool cannot know that; this project can.

### 1.4 The residual risk, and how to bound it cheaply

The one thing that would break fixed-size is a **mid-file rewrite or truncate-from-front** of a large transcript (e.g. a future tool compacts a `.jsonl` in place rather than starting a new session file). With fixed-size, the cost is re-uploading that one file from the edit point — bounded, one-off, and rare. With CDC you would save perhaps 80% of that one file, once.

Two cheap insurances, both recommended:

1. **Verify appendness before trusting it.** When a file grew, re-read and re-hash the *last sealed chunk* (256 KiB) and compare to the cached ID. Match ⇒ genuine append, hash only the new bytes. Mismatch ⇒ full re-chunk. Cost: one 256 KiB read per changed large file. This is the calibration knob — it makes the append assumption checked rather than assumed.
2. **Put the chunker in the manifest.** Store `"chunker": "fixed-256k"` in each snapshot. Switching to `"fastcdc-v2020-1m"` later is a new snapshot format value, not a restore-breaking change. One string field, and the door stays open.

Add a counter for "bytes re-uploaded due to appendness check failure". If that counter is ever non-trivial in the field, adopt FastCDC. Until then it is a dependency buying nothing.

### 1.5 If you nonetheless choose CDC (parameters for the record)

Use FastCDC v2020 with normalized chunking, via `fastcdc` 4.0.1, `min 256 KiB / avg 1 MiB / max 4 MiB`. Crate hard bounds: `MINIMUM_MIN=64`, `MINIMUM_MAX=1 MiB`, `AVERAGE_MIN=256`, `AVERAGE_MAX=4 MiB`, `MAXIMUM_MIN=1024`, `MAXIMUM_MAX=16 MiB`. **Pin `>= 4.0.1`**: 4.0.0 replaced a rounded `log2` with floored `ilog2` and silently changed cut points for non-power-of-two average sizes; 4.0.1 restored 3.2.1-compatible cut points. Also note 4.0.0 downgraded the size-parameter bounds checks from `assert!` to `debug_assert!`, so invalid parameters no longer panic in release builds — validate them yourself.

FastCDC is ~10x faster than open-source Rabin and ~3x faster than plain Gear at comparable dedup ratios, so if CDC is needed, FastCDC is the correct CDC. It is just not needed here.

### 1.6 Why 256 KiB and not restic's 1 MiB / borg's 2 MiB

restic and borg pick ~1–2 MiB averages because each chunk carries per-chunk index and (for restic) per-blob crypto overhead across a network with per-request latency. We pack (see §2), so the per-chunk network cost is zero — the cost is only index bytes.

The chunk size directly sets the **tail waste**: each sync re-uploads the current unsealed tail of every changed file, on average `N/2` bytes.

| N | Tail waste per changed file | Chunks for 4 GB | Index size (72 B/entry) |
|---|---|---|---|
| 2 MiB | ~1 MiB | 2,048 | 147 KB |
| 1 MiB | ~512 KiB | 4,096 | 295 KB |
| **256 KiB** | **~128 KiB** | **16,384** | **1.2 MB** |
| 64 KiB | ~32 KiB | 65,536 | 4.7 MB |

256 KiB is the knee: tail waste drops 4–8x versus restic's sizing, and the index is still ~1 MB (trivially small, and it is incremental — see §3). Below 256 KiB the index grows faster than the tail waste shrinks, and zstd's ratio starts to degrade on small buffers.

Per-chunk crypto overhead at 256 KiB: 24-byte nonce + 16-byte tag = 40 B on 262,144 B = **0.015%**. Negligible.

---

## 2. Small-file handling — pack objects

4110 transcript files plus several thousand small session-index JSON files. One git object per file is wrong for three reasons:

1. **GitHub caps directory width at 3,000 entries** and directory depth at 50. A flat `blobs/` directory of 4,000+ files is already over the limit.
2. Each git object costs a round-trip's worth of protocol overhead on clone and a filesystem inode on checkout.
3. Git's "recommended maximum size for a single object is 1MB" guidance means thousands of small objects is the *good* case for git, but a full clone still walks all of them.

**Design (restic's pack format, simplified):**

```
<blob0 ciphertext><blob1 ciphertext>…<blobN ciphertext><encrypted header><u32 LE header length>
```

The header lists, per blob: type (data / tree), chunk ID, ciphertext length, plaintext length. Encrypting the header means the pack file alone reveals only its size and blob count boundaries after decryption of the header — the same tradeoff restic makes.

- **Any file < 256 KiB → one blob.** No chunking, no per-file chunk list overhead.
- **Files ≥ 256 KiB → fixed-size chunks**, each a blob.
- Blobs from the same file go into the same pack, **in file order**, so a single-file restore reads contiguous bytes from one or two packs (pack locality).
- Packs are **sealed and immutable** once written. A chunk already in a pack is never re-packed except by an explicit prune-repack (§5).

**Pack size: target 32 MiB, seal when the next blob would exceed 48 MiB.**

Rationale:
- GitHub warns at 50 MiB and hard-blocks at 100 MiB per file. 48 MiB keeps every push warning-free.
- 32 MiB sits inside restic's proven 4–128 MiB range (restic's own default is 16 MiB, with the rule-of-thumb `sqrt(repo_size_GB) * 4 MiB` — for a 1 GiB repo that gives 4 MiB, but restic optimises for object-store request cost; git clone cost favours fewer, larger files).
- 4 GB of transcripts ⇒ ~128 packs. The 115 MB default payload ⇒ ~4 packs. Both far under the 3,000-entry directory limit even before sharding.
- Shard anyway: `packs/<first-2-hex>/<64-hex>.pack`. One line, and it makes the layout safe for any future growth.

**The win, concretely:** the 78 MB of session indexes goes from ~4,000 git objects (over the directory-width limit, thousands of inodes) to **~3 pack files**.

---

## 3. Repo layout

```
config.json                      plaintext: format version, chunker id + params, KDF params + salt
HEAD.json                        plaintext: latest snapshot id, index generation, repo format version
keys/<id>.json                   password-wrapped master key (Argon2id → XChaCha20-Poly1305 wrap)
packs/<ab>/<64-hex>.pack         encrypted, immutable, ≤48 MiB
index/<64-hex>.idx               encrypted: chunk-id → (pack-id, offset, ciphertext len, plaintext len)
                                 + a `supersedes` list of index ids this one replaces
snapshots/<unix-ts>-<id>.json    encrypted: file tree → per-file ordered chunk-id list + metadata
.gitattributes                   `* -delta -diff -merge binary`
```

Everything except `config.json` and `HEAD.json` is **immutable and content-addressed**, so every sync is a pure set of *additions*. Nothing is ever modified in place, which is what makes "each sync adds mostly-new objects" fall out for free.

**Encrypt the snapshot and index objects.** File paths leak account UUIDs, session IDs and possibly chat titles; the chunk→pack map leaks the file size distribution. Only the format version and the KDF salt are safe in plaintext.

**`.gitattributes` with `-delta`** tells git not to burn CPU attempting delta compression on high-entropy ciphertext, where it is guaranteed to fail. Cheap, real win on both push and clone.

### 3.1 Is git even the right store? — No, and here is how to use it anyway

Git as a *version control system* is actively wrong here:

- Delta compression cannot work on ciphertext (that is the premise of this whole feature).
- Git history is append-only and cannot forget, so N syncs of a 115 MB payload would trend toward N × churn forever.
- We already do our own content addressing, dedup, and snapshot history. Git's object model would be a redundant second copy of that.

Git as a *dumb, authenticated, free, replicated blob host with a well-supported partial-fetch protocol* is excellent.

**So: one orphan branch, exactly one commit, force-pushed every sync.**

```
git checkout --orphan sync         # first time
# every sync:
git add -A && git commit --amend -m "sync <ts>" && git push --force origin sync
```

Consequences:
- **Repo tree size ≈ live data size**, not the sum of every sync. This is the thing that makes "light" achievable.
- Version history is provided entirely by our `snapshots/` objects. Git commits carry none.
- A restoring machine's `git clone --depth 1` downloads *only live objects*, regardless of anything left over server-side.

### 3.2 Restore fetches only what it needs

```
git clone --filter=blob:none --depth 1 --sparse --branch sync <url> <dir>
```

Partial clone (`--filter=blob:none`) plus sparse checkout is git's native "fetch only what you need" and is fully supported by GitHub. Sequence:

1. Clone with `--filter=blob:none --depth 1 --sparse --no-checkout` — downloads the tree, no blobs.
2. `git sparse-checkout set HEAD.json config.json index keys` — fetches only the small control objects (a few MB).
3. Decrypt the snapshot, resolve the needed chunk IDs to pack IDs via the index.
4. `git sparse-checkout add packs/ab/<id>.pack …` for exactly the packs required.

A single-file restore therefore downloads one or two 32 MiB packs, not the whole repo.

**Do not add `git2` or `gix`.** `git2` pulls in libgit2 (a C dependency, at odds with the AUR source-build hermeticity note in CLAUDE.md); `gix` is a very large dependency tree. Shell out to the `git` binary via `tokio::process`, which this project already uses. A user syncing to a GitHub repo has git.

### 3.3 The 2 GB push limit is a real constraint

GitHub's documented repository limits: **on-disk size 10 GB**, **push size enforced at 2 GB**, single-object recommended max 1 MB "enforced at 100MB", directory width 3,000, directory depth 50.

An initial sync of a 4 GB payload **cannot be one push**. Batch it: stage and push packs in ≤1 GiB groups (safety margin), and write `index/`, `snapshots/` and `HEAD.json` **last**, in that order.

Push ordering gives the right failure mode: packs are unreferenced until the index lands, and the index is unreferenced until `HEAD.json` lands. **A crashed sync leaves orphan packs — garbage, never corruption.** The next prune sweeps them.

### 3.4 Sizing verdict on the 4 GB transcripts

4 GB compresses to roughly 800 MB–1 GB (chat JSONL is highly redundant; see §8), which lands right on GitHub's "ideally less than 1 GB" line and needs a multi-push initial sync. Unbounded, it is a bad fit; bounded, it fits.

**Recommendation: keep the git-repo path as the only store, and make the transcript opt-in bounded** — default `--transcripts=90d` with a hard `sync.max_repo_bytes` budget (default 1 GiB) that the sync refuses to exceed. This matches PROJECT.md's own "Opt-in and bounded only" and keeps the implementation to one code path.

**Documented escape hatch, not built now:** GitHub **Release assets** allow up to 2 GiB per file with no stated total-size limit, live outside the git object store (so deletion actually reclaims, and they never inflate the repo), and are uploaded via `uploads.github.com`. If someone genuinely needs unbounded 4 GB, packs move to release assets and the repo keeps only `config`/`HEAD`/`index`/`snapshots` (a few MB). That is the right second implementation — but it is a whole second transport, so it should be driven by a real user, not speculation.

---

## 4. Incremental detection

### 4.1 The rule (borg's, proven at scale)

A file is **unchanged** iff all of `size`, `mtime_ns`, `ctime_ns`, and `inode` match the cached values. borg defaults to `ctime,size,inode` precisely because **ctime cannot be set from user space**, making it a safe change detector for both metadata and contents, whereas mtime can be forged.

Do **not** rehash unchanged files. A no-op sync must be `stat()` over the file set — for 4110 files that is tens of milliseconds — plus one small SQLite read. This is the "a no-op sync should cost near-nothing" constraint from PROJECT.md.

### 4.2 The append fast path

For a file that changed and whose `size` grew:

```
if new_size >= cached.sealed_chunks * CHUNK:
    reread chunk[sealed_chunks - 1]           # 256 KiB
    if its keyed-BLAKE3 id == cached id:
        # genuine append: hash only bytes from sealed_chunks*CHUNK onward
    else:
        full re-chunk
else:
    full re-chunk                              # truncation
```

I/O per sync ≈ `new_bytes + 256 KiB × changed_files`. For 20 changed transcripts that is ~5 MB of reads for the whole 4 GB set.

### 4.3 Local index format

Use `rusqlite` 0.40 with `bundled` — **already a dependency of this project** for reading Cursor's `state.vscdb`. Zero new dependencies, and the bundled/static-link property that keeps the AUR source build hermetic is already established precedent.

```sql
CREATE TABLE file (
  path          TEXT PRIMARY KEY,
  size          INTEGER NOT NULL,
  mtime_ns      INTEGER NOT NULL,
  ctime_ns      INTEGER NOT NULL,
  inode         INTEGER NOT NULL,
  sealed_chunks INTEGER NOT NULL,   -- count of full CHUNK-sized chunks
  chunk_ids     BLOB    NOT NULL,   -- concatenated 32-byte keyed-BLAKE3 ids, in order
  seen_gen      INTEGER NOT NULL
);
CREATE TABLE chunk (
  id       BLOB PRIMARY KEY,        -- 32 bytes
  pack     BLOB NOT NULL,           -- 32 bytes
  offset   INTEGER NOT NULL,
  clen     INTEGER NOT NULL,        -- ciphertext length
  plen     INTEGER NOT NULL,        -- plaintext length
  seen_gen INTEGER NOT NULL
);
CREATE TABLE meta (k TEXT PRIMARY KEY, v BLOB);   -- repo url, last snapshot id, index generation
```

`seen_gen` is borg's cache-age mechanic: bump a generation counter each sync, stamp every entry touched, and evict entries not seen in the last N generations (borg's `BORG_FILES_CACHE_TTL`) so a deleted file's entry does not live forever.

Path: `~/.cache/ai-usagebar/sync/index.sqlite3`, mode 0600 (it contains file paths, which leak account UUIDs).

**Hermeticity (CLAUDE.md invariant):** the constructor must be `Index::at(&Path)` with the real-path resolver as a thin non-test wrapper, exactly like `Cache::at` / `creds::read_from`. No test may touch the real cache dir.

The local index is a **cache, not a source of truth** — it must be reconstructible from the remote index objects, so a lost or corrupt SQLite file degrades to a slow sync, never to data loss. Provide `sync --rebuild-index` and `sync --force-rehash`.

---

## 5. Pruning and GC — and git's memory problem

### 5.1 The danger, stated plainly

**Git never forgets.** Any blob that was ever in a commit that is still reachable stays in the repository forever. If sync used a normal branch with a commit per sync, then after 100 syncs the repo would contain every superseded tail chunk and every pruned pack, and no amount of local `prune` would shrink what a new machine has to clone. That directly defeats the "light" goal.

**This is why §3.1's single-commit orphan branch is not a stylistic choice — it is load-bearing.** With exactly one commit, the reachable object set is precisely the live object set, so:

- a fresh `git clone --depth 1` downloads only live data, and
- `prune` genuinely shrinks what every future clone costs.

### 5.2 Prune algorithm (restic's, simplified)

1. **Retention policy** → the set of live snapshots. Default: keep the last 10 snapshots plus one per month for 6 months.
2. **Mark:** decrypt live snapshots, union their chunk-ID lists → the live chunk set.
3. **Sweep:** for each pack, compute `live_bytes / pack_bytes`.
   - 0% live → delete the pack.
   - < 50% live → **repack**: read the live chunks out, write them into a fresh pack, delete the old one. (restic's `--repack-small`/threshold behaviour; borg's `compact --threshold`.)
   - ≥ 50% live → leave it.
4. **Rewrite the index**, with `supersedes` naming the index objects it replaces, then delete the superseded ones.
5. **Rebuild the single orphan commit** containing only live objects and force-push.

Repacking is the only operation that ever rewrites a pack. Everything else is add-or-delete.

### 5.3 Garbage sources (there are only two)

1. **Superseded tail chunks.** Each sync of an actively-growing file orphans the previous tail (avg 128 KiB). 20 active transcripts × 30 daily syncs ≈ **77 MB/month of tail garbage**, before compression. This is the ongoing cost of the tail-chunk design and the main reason prune must exist.
2. **Pruned snapshots' unique chunks.**

Note what is *not* garbage: a normal sync of unchanged data produces zero garbage, because chunks are content-addressed and immutable.

### 5.4 The honest caveat about server-side reclamation

Force-pushing makes objects unreachable on GitHub's side, but GitHub's server-side `gc` runs on its own schedule and is not user-triggerable, so the repo's *billed/on-disk* size can lag the live size for a while. Three points:

- It does **not** affect clone size or bandwidth for the restoring machine — unreachable objects are not sent.
- It does **not** grow without bound, because garbage is generated only by tails and prunes, not by ordinary syncs.
- If it ever matters (approaching the 10 GB on-disk limit), the reliable reclaim is `sync --recreate-remote`: delete the private repo via the API and re-push a fresh single commit containing only live packs. That is two API calls and one push — ~30 lines — and it is the only mechanism that is guaranteed to reclaim. Ship it as a documented maintenance command, not as part of the normal path.

Do **not** attempt shallow-history tricks, `filter-branch`, or BFG-style rewrites. With one commit there is no history to rewrite; that is the entire point.

---

## 6. Rust crates

| Crate | Version | Role | Status / notes |
|---|---|---|---|
| `blake3` | **1.8.6** (2026-08-05) | Chunk IDs, keyed | Very actively maintained (165M downloads). Use `Hasher::new_keyed(&id_key)` — see §6.1. ~3 GB/s single-threaded; the whole 4 GB set hashes in seconds. Pure Rust. |
| `chacha20poly1305` | **0.11.0** (2026-08-05) | Per-chunk AEAD | RustCrypto. Use **XChaCha20-Poly1305** (192-bit random nonce ⇒ no nonce-counter state to manage across machines). Pure Rust, no system libs → AUR-safe. Project already uses RustCrypto (`aes` 0.9, `sha2` 0.11) so the version line is consistent. |
| `argon2` | **0.5.3** (2026-04-21) | Password → master key | RustCrypto, pure Rust. Argon2id, `m=64 MiB, t=3, p=4`. Stronger than restic's scrypt(N=65536,r=8,p=1) for a user-chosen password. Store params + 16-byte salt in `config.json`. |
| `zstd` | **0.13.3** (2025-02-20) | Per-chunk compression | Bindings to C zstd via `zstd-sys`, which **vendors and `cc`-builds the C source by default** — same hermeticity story as `rusqlite`'s `bundled`, so the AUR source build is safe. Level 3. |
| `rusqlite` | **0.40.2** — *already present* | Local index | Zero new dependency. |
| `serde` / `serde_json` | already present | Snapshot + index encoding | JSON, zstd-compressed before encryption. Do not add `postcard`/`rmp-serde` — the index is ~1 MB as JSON and compresses well. |
| `reqwest` 0.12, `tokio`, `base64`, `directories`, `tempfile`, `fs2` | already present | GitHub API, async I/O, atomic writes, locking | Reuse. |
| **`fastcdc`** | 4.0.1 | — | **Not recommended** (§1). If adopted: pin `>= 4.0.1` (4.0.0 silently changed cut points for non-power-of-two averages), and validate size params yourself (4.0.0 downgraded bounds checks to `debug_assert!`). Maintained — last commit 2026-06-24, MIT. |
| **`git2`** 0.21 / **`gix`** 0.86 | — | — | **Not recommended.** libgit2 is a C dependency at odds with AUR hermeticity; `gix` is a very large tree. Shell out to `git` via `tokio::process`. |

### 6.1 Chunk IDs must be keyed

Use `blake3::Hasher::new_keyed(&id_key)` where `id_key` is derived from the master key, **not** a bare `blake3::hash`.

With unkeyed IDs, anyone with read access to the repo (GitHub itself, a compromised token, a collaborator) can test whether a *guessed* plaintext chunk is present — a confirmation-of-file attack. restic uses plain SHA-256 of plaintext and accepts this; borg uses HMAC-SHA256 with a secret `id_key` and does not. **Copy borg.** Keyed BLAKE3 is the same call with the same cost.

### 6.2 Order of operations per chunk

```
plaintext → zstd(level 3) → XChaCha20-Poly1305(key, random 192-bit nonce) → append to pack
chunk_id  = BLAKE3_keyed(id_key, plaintext)
```

Compress-then-encrypt leaks approximate compressibility and exact ciphertext length. Both restic and borg accept this tradeoff; for chat transcripts it is acceptable and it is what makes the 4–5x size reduction possible. Say so explicitly in the design doc so it is a decision, not an accident.

---

## 7. How restic and borg actually do it

### restic

- **Chunker:** Rabin fingerprints over a 64-byte sliding window. "Files smaller than 512 KiB are not split, Blobs are of 512 KiB to 8 MiB in size", aiming for **1 MiB average**. A random irreducible polynomial is chosen per repository and stored in `config`, specifically to make watermark attacks harder.
- **Packs:** blobs appended, then an *encrypted header* listing each blob (type, plaintext hash, encrypted length, and uncompressed size for compressed blobs), then a 4-byte little-endian header length. Blob types: data / tree / compressed-data / compressed-tree. Default pack size **16 MiB**, tunable 4–128 MiB via `--pack-size` / `RESTIC_PACK_SIZE`; restic scales it with repo size, roughly `sqrt(repo_size_GB) * 4 MiB`.
- **Index:** separate index objects mapping blobs to `(pack, offset, length)`, kept **below 8 MiB each**, with a `supersedes` list naming the index files a repack replaced.
- **Snapshots:** `snapshot → tree hash`; trees are JSON with a `nodes` array; file nodes carry an ordered list of content hashes.
- **Crypto:** AES-256-CTR + Poly1305-AES, layout `IV || CIPHERTEXT || MAC`, 32 bytes overhead; password → key via scrypt (N=65536, r=8, p=1).
- **Prune:** the only operation that removes data; takes an exclusive lock; a pack must be removed from the referencing index *before* deletion.

**Copy:** the pack format (blobs + encrypted trailing header + u32 LE length), the separate index objects with `supersedes`, the snapshot→tree→content-hash-list shape, and the index-before-pack deletion ordering.

### borg

- **Chunker:** buzhash, default `--chunker-params=buzhash,19,23,21,4095` = min `2^19` = 512 KiB, max `2^23` = 8 MiB, target `2^21` ≈ 2 MiB, 4095-byte window. Chunker params are a first-class user tunable (`10,23,16,4095` for fine-grained dedup at much higher resource cost).
- **Chunk IDs:** HMAC-SHA256 with a **secret `id_key`** — the anti-confirmation-attack property restic lacks.
- **Files cache:** default mode `ctime,size,inode`; a file is unchanged iff those match. ctime is preferred because it cannot be set from user space. Entries carry an `age` reset to 0 when seen and incremented otherwise; entries unseen for `BORG_FILES_CACHE_TTL` backups are evicted.
- **Repository:** an append-only segment log, segments capped at **4 GiB** (offsets are 32-bit in the repo index). `borg compact --threshold` reclaims space from segments below a liveness ratio.

**Copy:** keyed chunk IDs, the `(ctime, size, inode)` change-detection rule, the cache-age eviction mechanic, and the `compact --threshold` liveness-ratio repack policy.

**Do not copy:** restic's random Rabin polynomial (irrelevant without CDC), borg's segment log (git is our object store), and either project's ~1–2 MiB chunk average (they price per-chunk network cost; packing removes it — see §1.6).

---

## 8. Numbers: expected growth and the worked example

Assumed compression: chat JSONL and session-index JSON are highly redundant (repeated keys, role markers, UUIDs, boilerplate). zstd level 3 on this shape typically lands **4–5x**. LevelDB/SQLite profile data is more mixed, ~2x. *(MEDIUM confidence — these are estimates from the data shape, not measured on this machine. Measure before publishing a number to users.)*

### Default payload (115 MB, no transcripts)

| | Raw | In repo |
|---|---|---|
| config | 13 MB | ~3 MB |
| Desktop OAuth profiles | 24 MB | ~12 MB |
| Chat session indexes | 78 MB | ~18 MB |
| routines | 20 KB | ~5 KB |
| **Initial repo** | 115 MB | **~33 MB** |

Typical daily sync: config mostly static; assume ~5% of the session indexes churn (4 MB raw → ~1 MB) plus a few profile pages. **~1–3 MB per daily sync.** Thirty days ≈ +30–90 MB before pruning; retention of 10 snapshots reclaims most of it.

### Opt-in transcripts, bounded to 90 days

4 GB raw → **~800 MB–1 GB** in packs (~128 packs at 32 MiB, minus compression ⇒ ~30 packs). Initial push must be split into ≤1 GiB batches (§3.3). Steady state: ~20 active transcripts/day gaining ~2 MB of new text plus ~128 KiB of tail waste each ⇒ ~2.5 MB raw ⇒ **~600 KB/day**.

### The worked example: append 200 KB to a 50 MB transcript

`CHUNK = 256 KiB = 262,144 B`.

| | Before | After |
|---|---|---|
| File length | 50,000,000 B | 50,200,000 B |
| Sealed chunks | ⌊50,000,000 / 262,144⌋ = **190** (49,807,360 B) | ⌊50,200,000 / 262,144⌋ = **191** (50,069,504 B) |
| Tail | 192,640 B | 130,496 B |

Work performed:
- `stat()` says size/mtime/ctime changed → re-read chunk 189 (256 KiB) → keyed-BLAKE3 matches cache ⇒ **confirmed append**.
- Hash and chunk only from offset 49,807,360 → yields 1 newly-sealed chunk + 1 new tail.

Bytes uploaded:

| Item | Raw | After zstd (~4.5x) |
|---|---|---|
| 1 newly-sealed chunk | 262,144 B | ~58 KB |
| 1 new tail chunk | 130,496 B | ~29 KB |
| AEAD overhead (2 × 40 B) | — | 80 B |
| Index delta + snapshot delta | — | ~2 KB |
| **Total** | 392,640 B | **≈ 90 KB** |

Disk read: 256 KiB (verification) + 392 KiB (new content) ≈ **648 KiB**, out of a 50 MB file.

Garbage created: the superseded 192,640 B tail (~43 KB compressed), reclaimed at the next prune.

**Comparison:**

| Approach | Uploaded for a 200 KB append |
|---|---|
| Whole-file encrypt (the current problem) | ~10 MB (50 MB compressed) |
| **Fixed 256 KiB chunks + tail** | **~90 KB** |
| FastCDC avg 1 MiB | ~110 KB (one ~1 MiB re-cut chunk, compressed) — *worse than fixed here*, because a larger average chunk means a larger re-uploaded boundary chunk |

That last row is the point: for this workload, the simpler algorithm is also the faster one.

---

## 9. Open questions to resolve before implementing

- **Measure the real compression ratio** on a sample of this machine's transcripts and session indexes before quoting sizes to users. Every size number in §8 is an estimate.
- **Count the session-index files.** "78 MB of many small JSON files" — if the count is >10,000, revisit whether whole-file blobs need a sub-index.
- **Confirm the Claude Desktop profile mutation shape.** §1.3 assumes LevelDB `.ldb` files are write-once and SQLite writes are page-aligned in place. If either is wrong (e.g. a compaction rewrites 24 MB every session), that category becomes whole-file churn and dominates the daily sync cost — which changes the transcript-vs-default cost balance, though not the chunker choice.
- **Decide the transcript bound** (90 days? size cap? both?) — this is the single knob that determines whether the git-repo path suffices or a Releases transport eventually becomes necessary.

---

## Sources

**Chunking**
- Xia et al., *FastCDC: a Fast and Efficient Content-Defined Chunking Approach for Data Deduplication*, USENIX ATC '16 — https://www.usenix.org/conference/atc16/technical-sessions/presentation/xia (slides: https://www.usenix.org/sites/default/files/conference/protected-files/atc16_slides_xia.pdf)
- `fastcdc-rs` — https://github.com/nlfiedler/fastcdc-rs · https://crates.io/crates/fastcdc · changelog for the 4.0.0 cut-point regression and 4.0.1 fix

**restic**
- Design document — https://github.com/restic/restic/blob/master/doc/design.rst
- Tuning backup parameters (pack size, `RESTIC_PACK_SIZE`) — https://restic.readthedocs.io/en/v0.16.0/047_tuning_backup_parameters.html
- Dynamic pack sizing PR — https://github.com/restic/restic/pull/3367

**borgbackup**
- Data structures and file formats — https://borgbackup.readthedocs.io/en/stable/internals/data-structures.html
- Additional notes (chunker params) — https://borgbackup.readthedocs.io/en/stable/usage/notes.html
- FAQ (files cache / change detection) — https://borgbackup.readthedocs.io/en/stable/faq.html

**GitHub limits**
- About large files on GitHub (50 MiB warning, 100 MiB block, 25 MiB browser upload, <1 GB ideal / <5 GB strongly recommended) — https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github
- Repository limits (10 GB on-disk, 2 GB push, 1 MB recommended object / 100 MB enforced, 3,000 directory width, 50 depth, 5,000 branches) — https://docs.github.com/en/repositories/creating-and-managing-repositories/repository-limits
- Git LFS billing — **10 GiB storage + 10 GiB bandwidth/month on Free and Pro** (the old 1 GB free tier is superseded; data packs discontinued in favour of metered billing) — https://docs.github.com/en/billing/managing-billing-for-git-large-file-storage/about-billing-for-git-large-file-storage
- About releases (2 GiB per asset, no stated total limit) — https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases

**Crates** (versions verified against the crates.io API on 2026-08-19)
- blake3 1.8.6 · chacha20poly1305 0.11.0 · argon2 0.5.3 · zstd 0.13.3 · rusqlite 0.40.2 · fastcdc 4.0.1 · git2 0.21.0 · gix 0.86.0
