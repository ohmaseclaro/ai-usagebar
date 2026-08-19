# Phase 2 Context — Bundle Scope, Local Index, Dry-Run Planning

**Decisions locked by the orchestrator** on the user's instruction to "decide all for me using
the same mindset the codebase already uses, and focusing on my usage use cases". Grounded in
this machine's measured state, not hypotheticals. Downstream planners and executors honour
these; do not re-open them.

## The user's actual setup (measured, not assumed)

- **Four Claude Desktop accounts** — `gmail`, `hotmail`, `struct`, `toptal` — all
  Desktop-sourced. There are currently **zero** `[[anthropic.accounts]]` CLI entries; the
  CLI-vs-Desktop collision bug was fixed in v1.1.0 by preferring Desktop.
- Cursor (Ultra) is the other active vendor.
- `~/.claude-acc/profiles/` = 24 MB across the four profiles.
- Chat session indexes = 78 MB (~1300 `local_*.json`).
- Transcripts = 4.0 GB, 4110 `.jsonl`.
- Two routines exist (`daily-skill-update`, a standup report).
- Primary machine is macOS; the menu bar runs as a LaunchAgent.

## Locked decisions

### D1 — Exact category → path mapping

| Category | Included | Default |
|---|---|---|
| `config` | `~/.config/ai-usagebar/config.toml`, and `accounts/*/.credentials.json` if any exist | on |
| `credentials` | `~/.claude-acc/profiles/*/` — `meta.json`, `config-tokenCache`, `config-tokenCacheV2`, `desktop-state/` | on |
| `routines` | `~/.claude/scheduled-tasks/**`, plus each account's `scheduled-tasks.json` registry | on |
| `chat_index` | `claude-code-sessions/<account>/<org>/local_*.json` | on |
| `transcripts` | `~/.claude/projects/**/*.jsonl` | **off** |

### D2 — Hard exclusions, never synced under any setting

These are not a size optimisation; each is *wrong* to carry to another machine.

- **`bridge-state.json`** — volatile remote-control session id. This project already learned
  that restoring a stale one breaks `/remote-control` with a `session_url` crash. It is
  deleted on every account switch and must never be synced or restored.
- **`ant-device-registry.json`** — browser-extension device pairing, authorised server-side
  per account. It cannot be made valid on another machine; carrying it invites confusion.
- **Caches** — `~/Library/Caches/ai-usagebar/**`, `.stale`, `.last_error`, `.fetch.lock`.
  Entirely regenerable from a single fetch; syncing them wastes bytes and can restore a stale
  quota reading that looks authoritative.
- **`~/.claude-acc/backups/`, `prelogin-backup/`, `hidden/`** — local rollback state whose
  meaning is machine-specific.
- **Cowork / `local-agent-mode-sessions/`** — the transcript path embeds the owning account
  UUID plus an unreconstructable `ou-` suffix, so a copy renders as an empty chat. Already
  documented as unmigratable.
- Any lock or temp file (`*.lock`, `*.tmp`, `*-journal`).

### D3 — Transcript bounding, when the user opts in

Two bounds, both applied, newest-first: **`transcript_days = 30`** and
**`transcript_max_bytes = 2 GiB`**. Whichever binds first wins. Rationale: this user works
daily and the archive is 4.0 GB, so 30 days is a meaningful working window that still lands
an order of magnitude under the raw total; the byte budget is the backstop that keeps a
heavy month from silently ballooning. Both are `config.toml` values, not constants.

Selection is per *file* by mtime, never a partial file — a truncated JSONL is worse than an
absent one.

### D4 — Dry-run output shape

`--dry-run` prints, per category: file count, raw bytes, and — once the index exists —
**bytes that would actually upload** (new chunks only). The last number is the one that
matters for the user's "fast, light" goal, and is what makes SYNC-02's near-zero no-op
visible rather than merely claimed. Totals at the end, plus the resulting snapshot size.

### D5 — Local index

`rusqlite` (already a dependency, bundled — no new AUR burden), one DB at
`~/.cache/ai-usagebar/sync/index.sqlite3`, mode 0600.

Change detection is **`(path, size, mtime_ns, inode)`** first; a match short-circuits without
re-hashing. A mismatch re-chunks the file. The cache is a *hint* and is always safe to
delete — a missing or corrupt index must degrade to a full re-scan, never to a wrong answer.
This mirrors how the project already treats its vendor caches.

The index lives under `~/.cache`, **not** `~/.config`, precisely so it is never itself
synced.

### D6 — Category selection lives in config and is itself synced

`[sync] categories = ["config", "credentials", "routines", "chat_index"]` in `config.toml`.
Because `config` is a synced category, a second machine inherits the same selection — which is
SCOPE-05, and it means a change of selection propagates rather than silently differing per
machine.

## Constraints inherited from the codebase

- Tests hermetic: every root injected (`Paths::at`-style seams). No test reads a real `$HOME`,
  the real profile store, or the network. The AUR `check()` runs `cargo test`.
- Scanning must not follow symlinks out of the tree — the existing `context` scanner already
  guards this and has a test for it; do the same here.
- Writes atomic (tempfile + persist), as `cache::atomic_write` already does.
- Never log a credential path's *contents*; paths and byte counts only.

## Calibrations owed by this phase

- **CAL-2** — does Claude Desktop's LevelDB compaction rewrite the 24 MB profile wholesale each
  session? Measure by hashing `desktop-state/` chunk-by-chunk across two app restarts. If it
  does, that category dominates daily sync cost and the user should be told so in `sync status`.
  *Fallback if unmeasurable:* report the category's churn in `sync status` from real index data
  after a week, rather than blocking.
- **CAL-4** — the real compressed size of the default bundle. Measure the actual zstd ratio on
  this machine's true payload rather than quoting the 115 MB raw figure.

---

## Deferred from Phase 1 security audit (NEW-3) — object-type separator in the chunk AAD

Every object sealed through `chunk::seal_chunk` — data chunk, manifest chunk, index chunk, pack
header — uses one key with `aad = its own chunk_id` and **no object-type domain separator**.
Serde ignores unknown fields, so an `IndexObject` structurally deserializes as a `PackHeader`.

Confirmed to dead-end today: bounds checks and per-blob tags stop it, so it is a confused read
that errors, not a compromise. Deliberately **not** fixed in Phase 1 — a type byte changes ~10
signatures and ~60 call sites across four modules and two test binaries, moves every ciphertext
pin, and would rewrite §3–§5 of a format stabilised days earlier. A rushed AAD change on a
just-stabilised format is worse than a documented known issue.

**The trigger that forces it:** introducing a *new kind of object* sealed under `chunk_key`. If
this phase adds one, the separator lands first. Recorded at `Keys::seal`'s safety contract and
in `docs/sync-format.md` §3.

Also carried forward from `1-09`: `manifest_chunks` is unbounded, which is safe only because it
sits inside authenticated plaintext. **If this phase grows any path that reads an id list before
its container authenticates, that path needs its own bound.**
