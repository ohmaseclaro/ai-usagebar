# Requirements: ai-usagebar — Encrypted GitHub Sync

**Defined:** 2026-08-17
**Core Value:** Answer "how much quota do I have left, and on which account?" instantly and
correctly — without the user opening a browser, and without ever mis-reporting one
account's usage as another's.

**Milestone goal:** Let a user carry their ai-usagebar state — settings, Claude account
credentials, routines, chat indexes — between machines through a **private GitHub repo**,
encrypted end-to-end with a password only they know, syncing only what changed.

## v1 Requirements

### Bundle scope (what gets synced)

- [ ] **SCOPE-01**: The bundle covers, as independently toggleable categories: app config,
      Claude Desktop account credentials/profiles, routines/scheduled tasks, chat session
      indexes, and (opt-in) chat transcripts.

- [ ] **SCOPE-02**: Every category except chat transcripts is enabled by default; the user
      can uncheck any of them.

- [ ] **SCOPE-03**: Chat transcripts are **off** by default and, when enabled, are bounded
      (by age and/or total size) with the resulting bundle size shown before the first push.

- [ ] **SCOPE-04**: The user can preview exactly what a push would include — per category,
      file count and byte size — without pushing (`--dry-run`).

- [ ] **SCOPE-05**: Category selection persists in `config.toml` and is itself part of the
      synced config, so a second machine inherits the same choices.

### Encryption

- [ ] **CRYPTO-01**: The whole bundle is encrypted client-side with a key derived from a
      user-set password; the remote never receives plaintext or the password.

- [ ] **CRYPTO-02**: Password-derived keys use a memory-hard KDF with parameters stored
      alongside the data, so parameters can be raised in future versions without breaking
      existing bundles.

- [ ] **CRYPTO-03**: A wrong password fails cleanly and unambiguously — never partial or
      garbage output.

- [ ] **CRYPTO-04**: The user can change the sync password without re-uploading the entire
      bundle.

- [ ] **CRYPTO-05**: Tampering with, reordering, truncating, or rolling back remote data is
      detected on pull and refuses to restore.

- [ ] **CRYPTO-06**: Password strength is enforced at set time, with the offline-attack risk
      explained in plain language.

- [ ] **CRYPTO-07**: Key material is zeroized after use and never appears in process
      arguments, environment variables, logs, or error messages.

### Safety gates

- [ ] **SAFE-01**: A push whose bundle contains credentials is **refused** unless the target
      repo is verified private, checked immediately before every push.

- [ ] **SAFE-02**: If a previously-private target repo is found to be public, the push aborts
      and the user is told to rotate the affected credentials.

- [x] **SAFE-03**: Restoring never silently overwrites local credentials that are newer than
      the remote copy; the user is told what would change first.

- [x] **SAFE-04**: A local backup is taken before the first restore writes anything, and the
      command to roll it back is printed.

- [x] **SAFE-05**: No plaintext of any synced file is ever written to a temporary path that
      outlives the operation.

### Sync mechanics

- [ ] **SYNC-01**: Only data that actually changed since the last successful sync is
      uploaded.

- [ ] **SYNC-02**: A sync with no local changes completes near-instantly and uploads nothing.
- [ ] **SYNC-03**: Appending to a large file uploads roughly the appended bytes, not the whole
      file.

- [ ] **SYNC-04**: An interrupted push leaves the remote in its previous consistent state —
      never a half-written snapshot a pull could read.

- [ ] **SYNC-05**: A resumed push after interruption reuses what already uploaded.
- [x] **SYNC-06**: When two machines have both changed the same item, the most recent wins
      per item and the user is told what was overwritten.

- [ ] **SYNC-07**: Remote storage does not grow without bound; superseded data can be pruned.

### GitHub integration

- [ ] **REPO-01**: The user supplies a GitHub token scoped to the **single** sync repo, with
      no permission to create or administer repositories.

- [ ] **REPO-02**: The GitHub token is stored with the same protection as existing
      credentials (macOS Keychain / mode-0600 file), never in a tracked file.

- [ ] **REPO-03**: The tool **never creates a repository**. The user points it at a private
      repo they already own; a missing repo is an actionable error, not an auto-fix.
      *(Research finding: withholding `Administration: write` makes the app structurally
      incapable of creating a public repo — a stronger guarantee than SAFE-01's runtime
      check, which remains as defence in depth.)*

- [ ] **REPO-04**: An existing token from the environment or the user's git/gh credential
      helper can be reused so setup needs no new secret.

- [ ] **REPO-05**: Network, auth, and rate-limit failures produce an actionable message and a
      non-zero exit — never a silent partial success.

- [ ] **REPO-06**: Bulk data is uploaded as a small number of large objects, never one
      request per chunk, staying inside GitHub's content-creation limits (80/min, 500/hour).

- [ ] **REPO-07**: The snapshot pointer is published with a compare-and-swap precondition, so
      two machines pushing concurrently cannot interleave into a corrupt state.

### Commands and surfaces

- [x] **UX-01**: `ai-usagebar sync push` and `ai-usagebar sync pull` (or equivalent) perform
      the two directions, with `--dry-run` on both.

- [ ] **UX-02**: `ai-usagebar sync status` reports what is configured, when the last sync ran,
      and what would change now.

- [ ] **UX-03**: First-time setup is guided end to end: choose repo, set password, choose
      categories, confirm the size, push.

- [ ] **UX-04**: Progress is visible for a long first push (bytes/objects, not a frozen
      terminal).

- [x] **UX-05**: The macOS menu bar exposes sync state and can trigger a push/pull, reusing
      the existing non-interactive-subprocess conventions.

- [x] **UX-06**: The widget's exit-0 invariant holds — a sync failure never takes the status
      bar down.

## v2 Requirements

Deferred. Tracked, not in this roadmap.

### Automation

- **AUTO-01**: Scheduled/background sync on an interval
- **AUTO-02**: Sync-on-change triggered by config or credential writes

### Portability

- **PORT-01**: Non-GitHub remotes (generic git, S3-compatible object storage)
- **PORT-02**: Export/import a bundle as a single local file, no remote at all

### Recovery

- **REC-01**: Browse and restore an *older* snapshot, not just the latest
- **REC-02**: Selective restore of a single category or account

## v1.1 Requirements — Staged upstream PRs

**Defined:** 2026-08-21, from akitaonrails's closing review of PR #113.

Not new features. This milestone decomposes work that already exists and closes the
findings that made it unmergeable. Two decisions taken up front shape every requirement
below: the **core goes upstream and credential backup plus remote transport live in a
companion repository**, and the **fork stays the daily driver** throughout — it is what
found four defects no audit did.

### Bounded KDF (his finding 2 and 3 — the blocker)

- [x] **KDF-01**: Every path that opens a keyfile — restore, join, and open — enforces a
      ceiling on Argon2id memory, time **and** parallelism **before** any allocation or
      work begins. Today only `m` is checked, at 4 GiB, and `t` and `p` have no ceiling at
      all, so a hostile keyfile chooses the cost the victim pays before anything
      authenticates it.

- [x] **KDF-02**: The ceilings are `m ≤ 2 GiB`, `t ≤ 16`, `p ≤ 16`, each justified in
      writing against a published recommendation rather than chosen for looking large.
      2 GiB is RFC 9106 §4's first recommended option and the most any standard asks for;
      4 GiB was never a bound, since it is a guaranteed OOM on the 4 GB aarch64 class this
      project ships for.

- [x] **KDF-03**: `p` is documented as a **tamper signal, not a cost bound** — the vendored
      `argon2` 0.5.3 has no `parallel` feature, so parallelism multiplies neither memory nor
      work. A ceiling that implies otherwise is the same defect being fixed.

- [x] **KDF-04**: Every ceiling has a test proving an oversized parameter is refused before
      allocation, and the refusal names a remedy that exists.

- [x] **KDF-05**: `check_memory_budget` and `available_memory_kib` are **deleted, not
      wired up**. The macOS arm reads `hw.memsize` — total installed memory, a constant —
      so it structurally cannot fail; there is no Windows arm; and it names a flag that
      does not exist. A preflight that cannot fire is worse than none, because the docs
      claim it protects the user.

- [x] **KDF-06**: The imaginary `--kdf-memory` flag is resolved — either implemented or
      struck from all six places that assume it, including the live refusal text
      (`crypto.rs:788`) and the doc line asserting that refusal ships
      (`docs/sync-format.md:591`). Found while verifying his finding; it is the same class.

### Portability (his finding 1)

- [x] **PORTAB-01**: `tar` is resolved per platform and spelled absolutely on both, so a
      writable working directory cannot hijack it. *(a9f3ee4)*

- [x] **PORTAB-02**: The archive module's platform-independent decisions — argv, member
      order, the archive's name and root — are tested without spawning a child, so they
      run on Windows. Only the two assertions that genuinely need a process are gated.
      *(3b20ef7 — a9f3ee4 fixed production and left five tests failing.)*

- [x] **PORTAB-03**: Structural guards read a CRLF checkout the same as an LF one.
      *(bc1f8e2)*

- [ ] **PORTAB-04**: All four CI jobs, Windows included, are green on the exact head of
      every staged PR before it is opened. Nothing above counts as closed until observed.

### Diagnostics (his finding 4)

- [x] **DIAG-01**: `tar`'s stderr goes through the project's own untrusted-text path —
      control bytes dropped, newlines collapsed, length capped — because a restore's
      members are paths a hostile manifest chose. *(98fb0bb)*

- [ ] **DIAG-02**: The same treatment is offered upstream for the three pre-existing sites
      with the identical defect (`claude_desktop/app.rs:231`, `anthropic/keychain.rs:142`
      and `:233`), as a separate small PR rather than folded into ours.

### The split (his finding 5)

- [ ] **SPLIT-01**: Three PRs in his order: format and bounded KDF/keyfile core, then local
      archive/restore, then remote transport and lifecycle.

- [ ] **SPLIT-02**: No PR exceeds what one reviewer can hold. Measured today the grouping
      is 4.4k / 12.6k / 19.1k lines, so the third is split further before it is opened.

- [ ] **SPLIT-03**: Every PR rebases onto upstream `main` at v1.4.0 or later, and
      `.planning/` never appears in one.

- [ ] **SPLIT-04**: Each PR states that its dependency additions were already in the tree —
      the milestone held a zero-new-crate rule — rather than leaving a reviewer to verify
      24 lines of `Cargo.toml` by hand.

### Ownership (his closing note)

- [ ] **OWN-01**: Credential backup and remote transport live in a companion repository.
      The upstream core carries the format, the bounded KDF, and the local archive.

- [ ] **OWN-02**: An independent crypto/security review is obtained before credential
      backup is enabled anywhere it is offered to other users.

## Out of Scope

| Feature | Reason |
|---------|--------|
| Public repos for credential-bearing bundles | An encrypted blob in a public repo is an unlimited offline attack on the password, permanently archived by forks/mirrors |
| Chat transcripts in the default bundle | 4.0 GB / 4110 files measured; far past GitHub's ~1 GB repo guidance |
| A hosted sync service or backend | The project has no server and should not grow one; the user's own repo is the store |
| Password recovery / escrow | Zero-knowledge by design — a recoverable password defeats the threat model |
| Migrating Cowork (agent-mode) sessions | Their transcript path embeds the owning account UUID plus an unreconstructable suffix |
| Real-time multi-machine collaboration | Sync is snapshot-based; last-write-wins per item is the agreed model |
| Creating the GitHub repo for the user | Withholding repo-creation permission is what makes a public-repo push structurally impossible; auto-create would require the very permission we refuse |
| Git LFS | 10 GiB/month free bandwidth ≈ 87 restores, then LFS is disabled account-wide for the rest of the month |
| Storing bundle data as git objects/commits | AEAD ciphertext gets zero delta compression, git retains deleted blobs permanently, and the 10 GB repo cap arrives within ~2 years of weekly syncs |
| `keyring`/`secret-service` for the token | zbus needs a live D-Bus session and fails over SSH — exactly the headless restore case; the project's existing Keychain + mode-0600 convention already covers both platforms |

## Traceability

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
| SAFE-03 | Phase 5 | Complete |
| SAFE-04 | Phase 5 | Complete |
| SAFE-05 | Phase 5 | Complete |
| SYNC-01 | Phase 2 | Pending |
| SYNC-02 | Phase 2 | Pending |
| SYNC-03 | Phase 2 | Pending |
| SYNC-04 | Phase 4 | Pending |
| SYNC-05 | Phase 4 | Pending |
| SYNC-06 | Phase 5 | Complete |
| SYNC-07 | Phase 4 | Pending |
| REPO-01 | Phase 3 | Pending |
| REPO-02 | Phase 3 | Pending |
| REPO-03 | Phase 3 | Pending |
| REPO-04 | Phase 3 | Pending |
| REPO-05 | Phase 3 | Pending |
| REPO-06 | Phase 4 | Pending |
| REPO-07 | Phase 4 | Pending |
| UX-01 | Phase 5 | Complete |
| UX-02 | Phase 2 | Pending |
| UX-03 | Phase 3 | Pending |
| UX-04 | Phase 4 | Pending |
| UX-05 | Phase 6 | Complete |
| UX-06 | Phase 6 | Complete |

**Coverage:**

- v1 requirements: 37 total
- Mapped to phases: 37 ✓
- Unmapped: 0

*(The earlier "33 total" count was a miscount: SCOPE 5 + CRYPTO 7 + SAFE 5 + SYNC 7 + REPO 7 + UX 6 = 37.)*

## Traceability — v1.1 (Staged upstream PRs)

| Requirement | Phase | Status |
|-------------|-------|--------|
| KDF-01 | Phase 7 | Pending |
| KDF-02 | Phase 7 | Pending |
| KDF-03 | Phase 7 | Pending |
| KDF-04 | Phase 7 | Pending |
| KDF-05 | Phase 7 | Pending |
| KDF-06 | Phase 7 | Pending (roadmap D-1: strike, not implement) |
| PORTAB-01 | Phase 8 | Code complete (a9f3ee4) · unobserved |
| PORTAB-02 | Phase 8 | Code complete (3b20ef7) · unobserved |
| PORTAB-03 | Phase 8 | Code complete (bc1f8e2) · unobserved |
| PORTAB-04 | Phase 8 | Pending |
| DIAG-01 | Phase 10 | Code complete (98fb0bb) · not yet upstream |
| DIAG-02 | Phase 8 | Pending |
| SPLIT-01 | Phase 9 | Pending |
| SPLIT-02 | Phase 10 | Pending |
| SPLIT-03 | Phase 8 | Pending |
| SPLIT-04 | Phase 9 | Pending |
| OWN-01 | Phase 11 | Pending |
| OWN-02 | Phase 11 | Pending |

**Coverage:**

- v1.1 requirements: 18 total (KDF 6 + PORTAB 4 + DIAG 2 + SPLIT 4 + OWN 2)
- Mapped to phases: 18 ✓
- Unmapped: 0

The four `[x]` requirements above stay in this table: PORTAB-01/02/03 are committed but
**unobserved** — PORTAB-04 gates them on a Windows job that has never run on this fork — and
DIAG-01's code is closed on the fork but has not reached upstream.

---
*Requirements defined: 2026-08-17 · Traceability filled during roadmap creation: 2026-08-19*
