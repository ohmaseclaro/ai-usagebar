# Roadmap: ai-usagebar — Encrypted GitHub Sync

**Created:** 2026-08-19
**Milestone goal:** Carry ai-usagebar state — settings, Claude account credentials, routines,
chat indexes — between machines through a **private GitHub repo**, encrypted end-to-end with a
password only the user knows, syncing only what changed.

**Granularity:** standard (no `.planning/config.json`; defaults apply) · **Phase IDs:** sequential
**Coverage:** 37/37 v1 requirements mapped, no orphans, no duplicates.

The design is **already decided** — see `.planning/research/SUMMARY.md`. This roadmap sequences it;
it does not re-open it.

---

## Sequencing rationale

Five constraints drove the split, and each one is load-bearing:

1. **The pure offline core comes first.** Phases 1–2 add no network call and read no real `$HOME`.
   The bundle format can be proven against an adversary before any credential exists.

2. **Transport is separable.** Phases 1–2 ship and test with no GitHub token at all.
3. **The safety gates land strictly *before* the first push.** Phase 3 (private-repo verification,
   no-repo-creation, token storage) completes with zero bytes uploaded; Phase 4 is the first write.

4. **Restore is its own phase.** A broken restore is worse than no restore, so pull never rides
   along with push.

5. **Surfaces come last.** The CLI is the contract; the TUI and menu bar wrap a working CLI.

The four **unverified assumptions** from `SUMMARY.md` §Unverified are scheduled, not assumed —
each sits in the phase whose decision it gates, with a named fallback so none can block:

| # | Measurement | Scheduled | Gates | Fallback if unanswered |
|---|---|---|---|---|
| CAL-1 | `Range:` on **private**-repo release assets after the 302 | Phase 1 | Pack size, partial restore | Assume no → pack at 32 MiB |
| CAL-2 | Does Claude Desktop LevelDB compaction rewrite the 24 MB profile wholesale? | Phase 2 | Whether that category dominates daily cost | Assume yes → bound it like transcripts |
| CAL-3 | Argon2id m=1 GiB/t=3/p=1 on the slowest supported target (aarch64 Linux) | Phase 1 | Shipped KDF default, `--kdf-memory` floor | Keep 1 GiB, refuse actionably on low-RAM |
| CAL-4 | Real compressed size of the 115 MB default bundle | Phase 2 | The number SCOPE-03 shows the user | Show measured-at-runtime, quote nothing |

---

## Milestone-wide invariants

These apply to **every** phase and are not restated per phase. A phase that violates one is not done.

- **Hermetic tests.** No `#[test]` reads or writes a real `$HOME`/`$XDG` path, the Keychain, or the
  network. Every new module gets the injected-path seam the repo already uses (`Cache::at`,
  `creds::read_from`, `Endpoints`). The AUR `check()` runs `cargo test` during `makepkg`.

- **Live checks are `#[ignore]`d** in `tests/live.rs` — including all four calibrations above.
- **The widget always exits 0.** Any sync failure surfaces as the fallback `⚠` JSON.
- **Writes are atomic** — `tempfile` + `persist()`, created in the destination directory, explicit
  mode 0600. Never `/tmp`.

- **New deps are pure-Rust or vendored/`cc`-built.** No system `-dev` package may enter the AUR
  source build. Run `cargo machete` every phase.

- **No secrets in tracked files, argv, env, or logs.** Ever.
- **`.planning/` never reaches an upstream PR** — use `/gsd-pr-branch`.

---

## Phases

- [ ] **Phase 1: Encrypted Bundle Core** - Key hierarchy, chunker, pack, snapshot — offline, adversary-tested
- [ ] **Phase 2: Bundle Scope, Local Index, Dry-Run Planning** - What would be sent, and cheap no-op syncs
- [ ] **Phase 3: GitHub Auth and the Private-Repo Gate** - Paired and verified private, before any byte moves
- [ ] **Phase 4: Push — Packs, Atomic Flip, GC, Rekey** - The bundle reaches the repo, atomically and boundedly
- [ ] **Phase 5: Pull and Restore** - A second machine reproduces the state, reversibly
- [x] **Phase 6: Surfaces and Ship** - TUI + macOS menu bar, exit-0 invariant, release (completed 2026-08-20)

---

## Phase Details

### Phase 1: Encrypted Bundle Core

**Goal**: The bundle format — key hierarchy, chunker, pack, snapshot — exists and survives an
attacker who controls the remote, proven entirely by hermetic tests with no network, no `$HOME`,
and no GitHub credentials.

**Depends on**: Nothing (first phase)

**Requirements**: CRYPTO-01, CRYPTO-02, CRYPTO-03, CRYPTO-05, CRYPTO-06, CRYPTO-07

**Security-sensitive**: **yes** — this is the crypto. Security audit required.

**Scope — in**:

- `src/sync/crypto.rs` — Argon2id (m=1 GiB, t=3, **p=1**) → KEK → unwrap a random 32-byte master
  key → BLAKE3 `derive_key` subkeys (`chunk_key` / `name_key` / `root_key`) under versioned,
  hardcoded context strings. Keyfile JSON carries `{format, kdf{algo,version,m_kib,t,p,salt}, nonce,
  wrapped_master_key}`; the canonical serialization of `format` + `kdf` is bound as **AAD**, so a
  parameter downgrade fails to unwrap.

- `src/sync/chunk.rs` — fixed **256 KiB** offset-aligned chunks plus an explicit tail;
  `u32 true_len` prefix and tail padding to the next power of two; **zstd level 3 before encrypt**;
  `id = blake3::keyed_hash(name_key, plaintext)`; `XChaCha20-Poly1305` with
  `nonce = derive_key(CTX_NONCE, id)[..24]` and `aad = id`; `open_chunk` re-checks
  `chunk_id(pt) == id`.

- `src/sync/pack.rs` — `<blob ciphertexts><encrypted header><u32 LE header len>`, content-addressed
  pack names, sharded `packs/<ab>/<64hex>.pack` layout.

- `src/sync/model.rs` — snapshot root (fresh **random** 24-byte nonce; plaintext
  `{format, counter, created_at, repo_id, manifest_id}`), manifest (`path, mode, true_len,
  [chunk ids]`, itself a sealed chunk), index object with a `supersedes` list, and the literal
  `"chunker": "fixed-256k"` so the chunker can change later without breaking restore.

- Passphrase handling — **generate by default** (20 chars of Crockford base32 from `getrandom`,
  ~94 bits), hard-reject supplied passwords under 12 chars, warn below 20, explain the offline
  attack in plain language, state that there is no recovery. Password arrives via TTY prompt,
  stdin, or a mode-0600 file — **never** `--password` and **never** an env var.

- Rollback anchor — a local monotonic `counter` high-water mark in the *config* dir (not the
  wipeable cache), mode 0600; first contact is TOFU and is documented as a residual gap.

- Zeroization — `Zeroizing` on every derived key; the `Vec<u8>` returned by the AEAD is explicitly
  `.zeroize()`d; no `Debug` derive on anything holding key material. No `mlock` (documented as
  deliberate, not overlooked).

- `Cargo.toml` — `blake3` 1.8, `chacha20poly1305` 0.11, `argon2` 0.5.3 (`default-features = false`,
  no `password-hash`), `zstd` 0.13, `zeroize` 1.9, `getrandom` 0.4.

- `KdfParams` passed as an argument everywhere — that *is* the cheap-KDF test seam
  (`{m_kib: 8, t: 1, p: 1}` in tests).

**Scope — out**: any filesystem access outside a caller-supplied directory; any network call
(the one exception is CAL-1, an `#[ignore]`d probe, not production code); `sync` CLI subcommands;
category selection; the local SQLite index; token handling.

**Calibration scheduled here** (both gate a Phase 1 format constant):

- **CAL-1** — probe `Range:` support on a private-repo release asset after the 302 to signed
  storage. One-off manual `curl` plus an `#[ignore]`d test against a throwaway private repo. If
  Range works, larger packs are strictly better; if not, pack size must track restore granularity.
  **Fallback: assume no, pack at 32 MiB.** This must not block the phase.

- **CAL-3** — measure Argon2id m=1 GiB/t=3/p=1 on aarch64 Linux. Sets the shipped default and the
  `--kdf-memory` floor. A 1 GB-RAM box must get an actionable refusal, never an OOM.

**Deliverables**: the four modules above; RFC 9106's Argon2id vector as a guard on the crate;
pinned known-answer vectors for `(password, salt, params) → kek` and `(mk, plaintext) → (id, ct)`;
the five adversarial tests; `docs/sync-format.md` recording the on-disk format, both calibration
numbers, and the accepted metadata leakage (total size, sync timing, per-sync change volume).

**Plans:** 8 plans across 5 waves (1 / 2 / 2 / 2 / 1)

Plans:

- [ ] 1-01-PLAN.md — wave 1 — crate wiring, module tree, and the complete key hierarchy (tracer)
- [ ] 1-02-PLAN.md — wave 2 — fixed 256 KiB chunker: keyed plaintext id, frame, zstd, padding, seal
- [ ] 1-05-PLAN.md — wave 2 — passphrase generation and strength gate, plus the rollback anchor
- [ ] 1-03-PLAN.md — wave 3 — pack format: blobs, sealed header, trailer id, sharded name
- [ ] 1-04-PLAN.md — wave 3 — snapshot root, manifest, and index object, each with a read ceiling
- [ ] 1-06-PLAN.md — wave 4 — the nine adversarial tests, each asserting zero plaintext
- [ ] 1-08-PLAN.md — wave 4 — CAL-3 and CAL-1 calibrations, and `docs/sync-format.md`
- [ ] 1-07-PLAN.md — wave 5 — pinned known-answer vectors, after the last plan that may edit `src/`

**Success Criteria** (what must be TRUE):

1. A multi-megabyte fixture round-trips byte-exactly through chunk → zstd → seal → pack → unpack →
   open → reassemble, and sealing the same bytes twice produces **identical** ciphertext.

2. Each of: wrong password, downgraded KDF params, a chunk served under another chunk's id, a
   single flipped ciphertext bit, and a truncated manifest — fails with one distinct "cannot
   decrypt" error and yields **zero** bytes of plaintext.

3. A snapshot whose `counter` is below the local high-water mark is refused unless
   `--allow-rollback` is passed.

4. A supplied password under 12 characters is refused; the generated-passphrase path is the default
   and prints the no-recovery warning; no test or code path accepts a password from argv or env.

5. `cargo test` passes with `$HOME` unset and the network unavailable, inside the AUR `check()`
   time budget.

6. `cargo clippy --all-targets -- -D warnings` and `cargo machete` are clean with the six new
   crates, and the AUR source build still needs no system `-dev` package.

**Plans**: TBD

---

### Phase 2: Bundle Scope, Local Index, Dry-Run Planning

**Goal**: The user can see exactly what a push would send — per category, file count and bytes —
without pushing, and a sync with nothing changed costs a `stat()` sweep.

**Depends on**: Phase 1

**Requirements**: SCOPE-01, SCOPE-02, SCOPE-03, SCOPE-04, SCOPE-05, SYNC-01, SYNC-02, SYNC-03,
UX-02

**Security-sensitive**: **yes** — this phase decides *which credential files leave the machine*,
and the local index holds account UUIDs. Security audit required.

**Scope — in**:

- Category enumeration and collectors for the five categories: app config, Claude Desktop
  credentials/profiles, routines/scheduled tasks, chat session indexes, and (opt-in) chat
  transcripts. Every collector takes an injected root; none calls a real `$HOME` resolver.

- `[sync]` section in `config.toml` via `toml_edit`. Defaults: **everything on except transcripts**.
  The section is itself inside the config category, so a second machine inherits the choices.

- Transcript bounding — default 90 days plus a hard `max_bundle_bytes` budget the sync refuses to
  exceed; the resulting size is shown before the first push.

- `src/sync/index.rs` — `rusqlite` (already a dependency), `Index::at(&Path)`, the `file` / `chunk` /
  `meta` tables, `seen_gen` cache-age eviction, mode 0600. The index is a **cache, not a source of
  truth**: it must be reconstructible from the remote index objects.

- Change detection — borg's rule: unchanged iff `(size, mtime_ns, ctime_ns, inode)` all match.
  Unchanged files are never re-hashed.

- Append fast path — when a file grew, re-read and re-hash **only the last sealed chunk**; match ⇒
  hash from that offset onward; mismatch or shrink ⇒ full re-chunk. Ship a counter for "bytes
  re-uploaded because the append check failed" — this is the knob that says whether the
  no-CDC decision held in the field.

- Plan builder — produces `(new chunk ids, pack layout, per-category file/byte totals)`. This object
  is exactly what Phase 4 uploads; nothing in Phase 2 transmits it.

- CLI — `ai-usagebar sync status` and `ai-usagebar sync push --dry-run`.

**Scope — out**: any network call; token handling; the private-repo gate; writing packs anywhere
remote; restore; the merge/conflict model (Phase 5).

**Calibration scheduled here**:

- **CAL-2** — `stat` the Claude Desktop profile files across several sessions to determine whether
  LevelDB compaction rewrites the 24 MB profile wholesale. If it does, that category dominates
  daily sync cost and its default needs revisiting before Phase 4 ships a push.

- **CAL-4** — run Phase 1's chunker over the real 115 MB default bundle and record the actual
  compressed size. The ~33 MB in the research is an estimate; this is the number SCOPE-03 shows.

**Deliverables**: the collectors, the `[sync]` config schema, the SQLite index with its migration,
the change-detection + append fast path, the plan builder, `sync status`, `sync push --dry-run`,
and both calibration numbers written into `docs/sync-calibration.md`, linked from
`docs/sync-format.md`.

**Success Criteria** (what must be TRUE):

1. `sync status` on a seeded temp tree lists every category with file count and byte size, shows
   transcripts as off, and reports last-sync as "never".

2. `sync push --dry-run` prints per-category totals plus "would upload N bytes across M packs", and
   creates no file anywhere outside the injected temp roots.

3. Re-planning an unchanged tree returns an empty plan and opens **zero** file bodies — asserted by
   a read counter, not by timing.

4. Appending 200 KB to a 50 MB fixture produces a plan of ~2 chunks / under 400 KiB; **truncating**
   the same fixture falls back to a full re-chunk instead of producing a wrong plan.

5. Toggling a category off changes the next plan, and the toggle itself is in the bundle set, so it
   survives a round trip.

6. Every test injects its roots; `cargo test` passes with `$HOME` unset, and the index file is
   created mode 0600.

**Plans**: 7 plans, 4 waves (max 3 concurrent). Plans 2-06 and 2-07 need Phase 1 merged; the
rest build against Phase 1's contract only.

- [ ] 2-01-PLAN.md — wave 1 — `[sync]` config, roots seam, bounded symlink-safe walker with the D2 exclusions, `sync status` end-to-end for the config category
- [ ] 2-02-PLAN.md — wave 2 — credentials, routines and chat_index collectors (D1)
- [ ] 2-03-PLAN.md — wave 2 — the rusqlite local index (D5), including corrupt-degrades-to-rescan
- [ ] 2-04-PLAN.md — wave 2 — opt-in transcripts and their 30-day / 2 GiB bounds (D3)
- [ ] 2-05-PLAN.md — wave 3 — change detection, the append fast path, and the plan builder
- [ ] 2-06-PLAN.md — wave 3 — CAL-2 and CAL-4 measurements *(needs Phase 1)*
- [ ] 2-07-PLAN.md — wave 4 — `sync push --dry-run` in D4's shape, wired to Phase 1's chunker *(needs Phase 1)*

---

### Phase 3: GitHub Auth and the Private-Repo Gate

**Goal**: The tool is paired with a private GitHub repo the user already owns, holds a token that is
*structurally incapable* of creating a repo, and refuses to proceed against anything not verifiably
private — all with zero bytes uploaded.

**Depends on**: Phase 2

**Requirements**: REPO-01, REPO-02, REPO-03, REPO-04, REPO-05, SAFE-01, SAFE-02, UX-03

**Security-sensitive**: **yes** — token storage and the gate that keeps credentials off a public
repo. Security audit required.

**Scope — in**:

- `src/sync/github/` with an injectable `Endpoints { api_base, uploads_base }` seam. **Both** hosts,
  because `uploads.github.com` is a separate host and hard-coding it makes Phase 4 untestable.
  Tests point both at one `mockito::Server`.

- The visibility gate — `GET /repos/{owner}/{repo}` asserting **all** of: `private == true` **and**
  `visibility == "private"` (reject `"internal"`), `owner.login` matches config **and** `owner.id`
  matches the id recorded at first pairing (defeats delete-and-resquat), `archived == false`,
  `fork == false`. A 404 is a hard abort with the "create it first" message — never a reason to
  create one.

- **No repo creation, structurally.** There is no `POST /user/repos` call in the crate, and the
  documented token omits `Administration: write`. The runtime gate is defence in depth on top.

- Token storage — macOS Keychain via **Security.framework for writes** (so the token never enters
  argv), mode-0600 file under XDG on Linux; both following `src/anthropic/keychain.rs`'s existing
  convention. **Never** `config.toml`.

- Token discovery, behind one injected provider fn: `$GITHUB_TOKEN`/`$GH_TOKEN` →
  `git credential fill` → `gh auth token`. No test spawns a process or touches a real helper.

- Pairing record `{repo_id, owner_id, private, checked_at}` in the config dir, mode 0600, plus the
  drift check that reports "your backup repo is now public" rather than a generic error.

- Failure taxonomy — 401 (bad token: clear it, re-auth) vs 403/429 (rate limit or missing
  permission: back off via `retry-after` → `x-ratelimit-reset` → jittered exponential, min 60 s) vs
  404 (abort). All non-zero exit, all actionable. Time-dependent logic takes `now: DateTime<Utc>`.

- `ai-usagebar sync init` — guided end to end: choose repo → set password → choose categories →
  confirm the size (Phase 2's number) → "ready to push".

- Docs — the exact fine-grained PAT recipe: "Only select repositories" → the one repo,
  `Contents: Read and write` + `Metadata: Read`, and nothing else.

**Scope — out**: uploading anything; the release and asset APIs; the snapshot-pointer flip; GC;
restore. OAuth Device Flow is deferred (a `repo`-scoped token grants write on every repo the user
owns — the opposite of this phase's point).

**Deliverables**: the GitHub client with its `Endpoints` seam, the gate, token read/write on both
platforms, the discovery chain, the pairing record and drift check, the error taxonomy, `sync init`,
the PAT documentation, and mockito coverage of every refusal path.

**Success Criteria** (what must be TRUE):

1. `sync init` against a mock repo reporting `private: false`, `visibility: "internal"`,
   `archived: true`, `fork: true`, or 404 refuses in **each** case with a distinct actionable message
   and a non-zero exit — and never offers to create the repo.

2. A repo that reads private at pairing and public on re-check produces the SAFE-02 incident
   message, naming the credentials to rotate and stating plainly that published bytes cannot be
   un-published.

3. `sync init` completes on a mock private repo: repo chosen, password set, categories confirmed
   with a byte total, token stored — and the token appears in neither `config.toml`, nor
   `/proc/*/cmdline`, nor any log line at any verbosity.

4. With no token configured, the discovery chain is tried in order, each step exercised through the
   injected provider; no test spawns a subprocess.

5. 401, 403-with-`retry-after`, 429, and a connection reset each produce a distinct message and a
   non-zero exit; none is reported as success.

6. `grep` over the crate finds no call to `POST /user/repos` and no request for
   `Administration` permission.

**Plans:** 7 plans across 3 waves (2 / 3 / 2). Wave 1's tracer freezes every cross-file
signature, so the three wave-2 plans each own whole files and compile in isolation.

Plans:

- [ ] 3-01-PLAN.md — wave 1 — the `src/sync/github/` module tree, frozen seams, and config → token → request → gate → CLI end to end (tracer)
- [ ] 3-05-PLAN.md — wave 1 — the fine-grained PAT recipe, `[sync] repo`, and the README entry point
- [ ] 3-02-PLAN.md — wave 2 — D-02's token chain: the macOS Keychain half, the `gh` half, and the mode-0600 write path
- [ ] 3-03-PLAN.md — wave 2 — the failure taxonomy and header-derived backoff, six outcomes with six actionable messages
- [ ] 3-04-PLAN.md — wave 2 — the full gate assertion set, the pairing record, drift, and the SAFE-02 incident
- [ ] 3-06-PLAN.md — wave 3 — CAL-1, run or explicitly declined, with `PACK_TARGET` reconciled *(checkpoint; must not block)*
- [ ] 3-07-PLAN.md — wave 3 — guided `sync setup` end to end, and `sync status` learning about the repo

**Note:** `3-CONTEXT.md` D-05 names the guided command `sync setup`; the scope list above says
`sync init`. CONTEXT is the locked artifact, so it ships as `sync setup`.

**REPO-06 / REPO-07:** foundation only in this phase — `Endpoints.uploads_base` and the frozen
`Conflict` error arm exist so Phase 4's upload and CAS paths are testable. Both requirements
stay assigned to Phase 4, where they are observable; D-05 forbids an upload here.

---

### Phase 4: Push — Packs, Atomic Flip, GC, Rekey

**Goal**: The user's encrypted bundle reaches their private repo; an interrupted push never leaves a
snapshot a pull could read; and the remote does not grow without bound.

**Depends on**: Phase 3

**Requirements**: REPO-06, REPO-07, SYNC-04, SYNC-05, SYNC-07, CRYPTO-04, UX-04

**Security-sensitive**: **yes** — this is the first phase that moves credential-bearing ciphertext
off the machine, and it owns rekey. Security audit required.

**Scope — in**:

- Pack upload — `POST https://uploads.github.com/repos/{o}/{r}/releases/{id}/assets?name=<pack>`
  with a **streamed** body (add the `stream` feature to `reqwest` 0.12 so a large pack is not
  buffered in RAM), concurrency capped at **4**, backoff per Phase 3's taxonomy. **Never one request
  per chunk** — 80/min and 500/hour content-creation limits make that structurally impossible.

- The nine-step protocol: plan → **gate** → draft release → resume scan → upload → **re-gate** →
  publish → `PUT /repos/{o}/{r}/contents/<pointer>` with the `sha` precondition → GC. Steps 1–7 are
  invisible to any reader; the CAS `PUT` is the single linearization point.

- Resume — skip assets matching name **and** size **and** `state == "uploaded"`; `DELETE` any asset
  whose `state != "uploaded"` before retrying (GitHub creates the asset record before the body
  finishes, so a torn upload leaves a zombie). The `state` transitions are MEDIUM-confidence in the
  research — verify with an `#[ignore]`d live test.

- CAS conflict — a 409 on the pointer `PUT` re-reads and re-plans. **Never blind-overwrite**: the
  other machine's newer pointer may reference assets this run is about to GC.

- GC / prune (SYNC-07) — runs **only after a successful flip**, deletes only assets unreferenced by
  the current pointer. Retention: keep the last 10 snapshots plus one per month for 6 months.
  Repack packs below 50% liveness. Superseded tail chunks are the main garbage source (~77 MB/month
  at 20 active transcripts), so prune is a correctness requirement, not a nicety.

- `sync rekey` (CRYPTO-04) — new salt → new KEK → rewrap the **same** master key → upload the new
  keyfile → **delete the old keyfile asset** (the research flags this explicitly: asset deletion is
  what makes it real). Print honestly that this is *not* revocation if the repo was ever cloned.

- Progress (UX-04) — advancing bytes/objects for a long first push.
- Push ordering — packs, then index, then the pointer. A crashed push leaves orphan packs, which are
  garbage, never corruption.

**Scope — out**: pull, restore, merge, the TUI and menu bar, `--recreate-remote` (documented
maintenance escape hatch, not built now).

**Deliverables**: the upload client, the nine-step orchestrator, resume, CAS handling, prune/repack,
`sync rekey`, progress reporting, mockito coverage of the whole protocol, and the `#[ignore]`d live
tests for asset `state` transitions.

**Success Criteria** (what must be TRUE):

1. A first push against a mockito server issues **one upload request per pack**, never one per
   chunk; a bundle of ~5,000 chunks completes in under 10 HTTP requests total.

2. Killing the process after the uploads but before the flip leaves the previous pointer intact and
   readable, and the pointer never references a pack that is not fully uploaded.

3. Re-running the killed push re-uploads only the packs that are missing or in a non-`uploaded`
   state; already-uploaded packs are skipped by name+size+state.

4. A stale-`sha` 409 on the pointer `PUT` triggers a re-read and re-plan, and no asset referenced by
   the competing pointer is deleted.

5. `sync rekey` under a new password unwraps the same master key, rewrites **only** the keyfile
   (bundle bytes untouched), and the old keyfile asset is gone from the release afterwards.

6. After repeated syncs of a growing file, prune removes the superseded tail packs and the asset
   list shrinks — remote size tracks live data, not cumulative history.

7. A long push prints advancing byte/object counts rather than a frozen terminal, and every failure
   path exits non-zero with an actionable message.

**Plans:** 7 plans across 4 waves (1 / 4 / 1 / 1). Wave 1's tracer creates every file and freezes
every cross-module type in `push/mod.rs`, so the four wave-2 plans each own whole files, share
none, and compile in isolation. 4-05 is wave 3 because its delete pass calls `Index::forget_chunks`,
which 4-02 adds — a sibling call, not a sibling dependency, and the distinction is what Phase 1
got wrong.

Plans:

- [ ] 4-01-PLAN.md — wave 1 — the six write verbs, the remote layout, and one file to the repo end to end via the CAS flip (tracer)
- [ ] 4-02-PLAN.md — wave 2 — `SyncPlan` to packs, manifest, index object and root, plus the `chunk` table's first writer
- [ ] 4-03-PLAN.md — wave 2 — the resume scan, four-at-a-time uploads, digest verification, and progress
- [ ] 4-04-PLAN.md — wave 2 — the bounded, merging compare-and-swap on the pointer
- [ ] 4-06-PLAN.md — wave 2 — `sync rekey`: gate, rewrap, publish, then verifiably destroy the old wrapper
- [ ] 4-05-PLAN.md — wave 3 — retention and GC: the landed pointer plus a grace window, and the warning that never fails a push
- [ ] 4-07-PLAN.md — wave 4 — the seven success criteria as seven named integration tests, and the user-facing docs

**Two reconciliations with the scope list above, both recorded in `4-01-PLAN.md`'s source audit.**
The **draft release** (steps 3 and 7) is dropped: a draft release has no git tag, so
`GET /releases/tags/{tag}` cannot find it and the resume scan SYNC-05 requires would need a full
release listing plus name matching — machinery bought to briefly hide ciphertext that the
re-gate's incident path already deletes. Atomicity comes from the flip, not from draft state. The
**`stream` feature** is not added to `reqwest`: CAL-1 was not run, so the pack ceiling stays where
it is and `PackWriter::finish` already returns a `Vec<u8>`. Note which constant governs —
`pack::should_seal` compares against **`PACK_MAX` = 48 MiB** and never reads `PACK_TARGET`, which
is advisory. The trigger is named: raising `PACK_MAX` past ~256 MiB means adding `stream`, writing
packs to a tempfile, **and** re-checking `pack.rs`'s single-chunk header ceiling at the same time,
because that ceiling is a function of `PACK_MAX`.

---

### Phase 5: Pull and Restore

**Goal**: A second machine reproduces the user's state from the remote — and a restore that would
clobber something newer says so first, backs it up, and can be undone.

**Depends on**: Phase 4

**Requirements**: SAFE-03, SAFE-04, SAFE-05, SYNC-06, UX-01

**Security-sensitive**: **yes** — this is the only phase that writes decrypted credentials to disk,
and it owns rollback detection. Security audit required.

**Scope — in**:

- `sync pull` and `sync pull --dry-run` — fetch the pointer → decrypt the root → verify `counter` ≥
  the local high-water mark → resolve chunk ids through the index → fetch only the packs needed
  (Range requests if CAL-1 said yes, whole packs otherwise) → verify the AAD binding at every hop
  (`root → manifest_id → manifest → chunk ids → chunks`) → reassemble.

- Pre-restore backup (SAFE-04) — everything that would be written is copied first, and the exact
  rollback command is printed. The backup exists **before** the first write, not after.

- Per-item merge (SYNC-06) — last-write-wins per item, reusing the existing `synced.json` baseline
  thinking that already distinguishes a deletion from "never had it" for routines and chats. Do not
  invent a second reconciliation model. Report what was overwritten.

- Newer-local protection (SAFE-03) — local credential files newer than the remote copy are listed
  before anything is written; the user is told what would change first.

- No plaintext temp files (SAFE-05) — every decrypted output goes through
  `NamedTempFile::new_in(dest_dir)` + `persist()` + an **explicit** `chmod 600` (persist keeps the
  mode, but set it anyway, as the Settings overlay already does). Never `/tmp` — it is
  world-readable, often a different filesystem so `persist` degrades to a copy leaving a plaintext
  original, and may be tmpfs that survives in swap. A failed restore deletes its partial outputs.

- Index recovery — `--rebuild-index` and `--force-rehash`, so a lost or corrupt local SQLite index
  degrades to a slow sync, never to data loss.

- UX-01 completes here: both directions exist with `--dry-run` on both.

**Scope — out**: browsing or restoring an *older* snapshot (v2: REC-01); selective per-category or
per-account restore (v2: REC-02); the TUI and menu bar.

**Deliverables**: the pull orchestrator, pack fetch (Range or whole), chain verification, the
pre-restore backup and its rollback command, the per-item merge and its report, the newer-local
gate, the plaintext-safe write path, `--rebuild-index` / `--force-rehash`, and a full
push→wipe→pull round-trip test against mockito.

**Success Criteria** (what must be TRUE):

1. `sync pull` into an empty injected home reproduces the pushed tree byte-for-byte, with every
   credential file at mode 0600.

2. `sync pull --dry-run` lists every file that would be created, overwritten, or skipped, and writes
   nothing.

3. A local credential file newer than its remote copy is reported before any write and is not
   silently overwritten.

4. The pre-restore backup exists before the first byte is written, and the printed rollback command
   restores the prior state exactly.

5. A pull of a rolled-back snapshot (lower `counter`), a tampered pack, or a manifest referencing a
   missing chunk refuses to restore and writes **zero** files.

6. Two machines that edited the same routine converge on the newer one, and the overwritten value is
   named in the report.

7. Killing the process mid-restore leaves no plaintext outside the destination directory and no
   half-written credential file.

**Plans:** 8/8 plans executed
`src/sync/restore/` files, **fills** `layout.rs` because four wave-2 plans call it, and freezes
every cross-module type in `restore/mod.rs` — so the five wave-2 plans each own exactly one
whole file, share none, and compile in isolation.

Plans:

- [x] 5-01-PLAN.md — wave 1 — pointer → keyfile → root → manifest → pack → one file written at 0600, dry-run by default (tracer, D1/D4/D5, SAFE-05)
- [x] 5-02-PLAN.md — wave 2 — the verified chain with a ceiling on every remote-chosen list, and whole-pack fetch
- [x] 5-03-PLAN.md — wave 2 — the eight dispositions: digest before timestamp, and the credential arm `--force` cannot open (SAFE-03, SYNC-06, D2/D7)
- [x] 5-04-PLAN.md — wave 2 — the write path: tempfile in the destination's own directory, 0600 before content, nothing left behind (SAFE-05)
- [x] 5-05-PLAN.md — wave 2 — the pre-restore archive and a rollback command proven by running it (SAFE-04, D3)
- [x] 5-06-PLAN.md — wave 2 — one gate, a second for credentials, and a summary that names what was lost (D1/D6)
- [x] 5-07-PLAN.md — wave 3 — `sync pull` wired in safety order, plus `--rebuild-index` / `--force-rehash` (UX-01)
- [x] 5-08-PLAN.md — wave 4 — the push→pull round trip, the six refusals, and the two-machine docs

**One reconciliation, recorded in `5-01-PLAN.md`'s source audit.** Phase 4's `4-02` builds the
manifest from `FilePlan.path`, which is an **absolute local path** — unresolvable on a second
machine, and exactly what D5 orders restore to reject. Plan 5-01 owns the fix: a root-prefixed
relative encoding in `src/sync/restore/layout.rs`, emitted by a one-expression change to
`src/sync/push/packer.rs` and consumed by restore. It is the phase's one edit to a Phase 4 file.

**CAL-1 is still unmeasured** after Phases 1, 3, and 4. Plan 5-02 ships whole-pack fetch on the
pessimistic assumption — correct either way — and 5-08 leaves an `#[ignore]`d probe so the
measurement has somewhere to land. It does not block the phase.

---

### Phase 6: Surfaces and Ship

**Goal**: Sync is reachable from the TUI and the macOS menu bar, a sync failure can never take the
status bar down, and the release ships through the full checklist.

**Depends on**: Phase 5

**Requirements**: UX-05, UX-06

**Security-sensitive**: no (the sensitive paths were audited in Phases 1–5; this phase wraps them)

**UI hint**: yes

**Scope — in**:

- TUI — a sync panel/section showing status, category toggles, and last-sync, reusing
  `src/tui/settings.rs`'s `toml_edit`-backed conventions and its post-save waybar signal.

- macOS menu bar (`macos/`) — sync state plus push/pull triggers, following the existing
  non-interactive-subprocess conventions. A sync that needs a password it cannot prompt for must
  report "run `ai-usagebar sync push` in a terminal" rather than hanging on a TTY that isn't there.

- Widget path — any sync error surfaces as the fallback `⚠` JSON with **exit 0**, via
  `widget::run::fallback`.

- Docs — README sync section, the PAT recipe, and the honest limits: no password recovery, password
  change is not revocation, the accepted metadata leakage, and GitHub's AUP §9 excessive-bandwidth
  clause.

- Release checklist per `CLAUDE.md`: `Cargo.toml` + root `manifest.json` versions matched, CHANGELOG
  section, both PKGBUILDs bumped, **both `.SRCINFO`s regenerated before tagging**, then `make test` +
  `cargo clippy --all-targets -- -D warnings` + `cargo machete` + `omarchy plugin validate .`.

**Scope — out**: GNOME extension, KDE plasmoid, and Omarchy panel sync surfaces. Deliberately
deferred — the CLI plus the menu bar cover the milestone, and each of those three is an independent
frontend contract suite. Recorded as a decision, not an oversight.

**Deliverables**: the TUI sync panel, the menu-bar integration, the widget fallback path with a test
that injects a failing transport, the README/docs updates, and a tagged release that passes the full
gate.

**Success Criteria** (what must be TRUE):

1. With sync configured and the transport failing, the widget exits 0 and renders the fallback `⚠`
   JSON — asserted by a test that injects the failure, not by manual observation.

2. The macOS menu bar shows last-sync state and can trigger a push and a pull; a sync needing a
   password it cannot prompt for reports that clearly instead of hanging.

3. The TUI sync panel toggles a category, the change lands in `config.toml` at mode 0600, and waybar
   is signalled exactly as the Settings overlay already does.

4. `make test`, `cargo clippy --all-targets -- -D warnings`, `cargo machete`, and
   `omarchy plugin validate .` are all clean.

5. The README documents the fine-grained PAT recipe and states plainly that there is no password
   recovery and that changing the password is not revocation.

**Plans:** 5/5 plans complete
`sync status --json` key set, so the wave-2 menu-bar plan parses a contract it never has to
re-derive. No two plans in a wave share a file: 6-01 owns the Swift pair plus the sync CLI,
6-03 owns the widget, 6-04 owns the TUI.

Plans:

- [x] 6-01-PLAN.md — wave 1 — `sync status --json` end to end into a menu-bar state row (tracer, D-01/D-03/D-04)
- [x] 6-03-PLAN.md — wave 1 — the widget exit-0 gate: injected sync-shaped failures, plus a structural unreachability test (UX-06, D-03)
- [x] 6-04-PLAN.md — wave 1 — the TUI Sync section: category toggles and last-sync through the overlay's one `toml_edit` save path (D-04)
- [x] 6-02-PLAN.md — wave 2 — menu-bar push/pull triggers and the CLI's non-interactive refusal (UX-05, D-01/D-02)
- [x] 6-05-PLAN.md — wave 3 — README limits, the release checklist, and both `.SRCINFO`s regenerated before tagging *(checkpoint; the tag is a human action)*

**Note:** `sync status` ships text-only today. 6-01 adds `--json` as the surfaces' contract;
that is Phase 6 work, not an assumption about Phases 3–5. Where a plan needs a field Phases
3–5 may or may not have shipped — a `repo` key, a non-interactive flag — it states it as a
`<precondition>` and adopts the existing spelling rather than inventing a second one.

---

## Requirement Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| SCOPE-01 | Phase 2 | Pending |
| SCOPE-02 | Phase 2 | Pending |
| SCOPE-03 | Phase 2 | Pending |
| SCOPE-04 | Phase 2 | Pending |
| SCOPE-05 | Phase 2 | Pending |
| CRYPTO-01 | Phase 1 | Pending |
| CRYPTO-02 | Phase 1 | Pending |
| CRYPTO-03 | Phase 1 | Pending |
| CRYPTO-04 | Phase 4 | Pending |
| CRYPTO-05 | Phase 1 | Pending |
| CRYPTO-06 | Phase 1 | Pending |
| CRYPTO-07 | Phase 1 | Pending |
| SAFE-01 | Phase 3 | Pending |
| SAFE-02 | Phase 3 | Pending |
| SAFE-03 | Phase 5 | Pending |
| SAFE-04 | Phase 5 | Pending |
| SAFE-05 | Phase 5 | Pending |
| SYNC-01 | Phase 2 | Pending |
| SYNC-02 | Phase 2 | Pending |
| SYNC-03 | Phase 2 | Pending |
| SYNC-04 | Phase 4 | Pending |
| SYNC-05 | Phase 4 | Pending |
| SYNC-06 | Phase 5 | Pending |
| SYNC-07 | Phase 4 | Pending |
| REPO-01 | Phase 3 | Pending |
| REPO-02 | Phase 3 | Pending |
| REPO-03 | Phase 3 | Pending |
| REPO-04 | Phase 3 | Pending |
| REPO-05 | Phase 3 | Pending |
| REPO-06 | Phase 4 | Pending |
| REPO-07 | Phase 4 | Pending |
| UX-01 | Phase 5 | Pending |
| UX-02 | Phase 2 | Pending |
| UX-03 | Phase 3 | Pending |
| UX-04 | Phase 4 | Pending |
| UX-05 | Phase 6 | Pending |
| UX-06 | Phase 6 | Pending |

**Coverage: 37/37 v1 requirements mapped. No orphans, no duplicates.**

Assignment notes where a requirement could have gone elsewhere:

- **CRYPTO-04** (change password without re-uploading) — the rewrap primitive is Phase 1, but the
  observable requirement includes deleting the old keyfile from the remote, which needs Phase 4.

- **SAFE-05** (no surviving plaintext temp file) — Phase 1's seal path is pure and in-memory by
  construction, so the only place plaintext actually reaches disk is restore. Assigned to Phase 5,
  where it is observable.

- **UX-01** (push and pull, both with `--dry-run`) — assigned to Phase 5, the first phase in which
  *both* directions exist. Phase 4 delivers the push half.

- **UX-02** (`sync status`) — assigned to Phase 2, the first phase that can deliver it. Phase 3
  extends the same command with repo/visibility/drift lines; that is scope, not a second mapping.

- **SYNC-03** (append uploads roughly the appended bytes) — the property is produced by Phase 2's
  chunk-delta planner and is verifiable there against a fixture; Phase 4 only transmits the result.

---

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Encrypted Bundle Core | 0/8 | Not started | - |
| 2. Bundle Scope, Local Index, Dry-Run Planning | 0/7 | Not started | - |
| 3. GitHub Auth and the Private-Repo Gate | 0/7 | Planned | - |
| 4. Push — Packs, Atomic Flip, GC, Rekey | 0/7 | Planned | - |
| 5. Pull and Restore | 8/8 | In Progress|  |
| 6. Surfaces and Ship | 5/5 | Complete   | 2026-08-20 |

**Security audits required:** Phases 1, 2, 3, 4, 5.

---
*Roadmap created: 2026-08-19*
