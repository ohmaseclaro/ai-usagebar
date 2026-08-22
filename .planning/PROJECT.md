# ai-usagebar

## What This Is

A Rust CLI + status-bar widget that shows how much of your AI plan quota is left across
every provider you use — Anthropic/Claude, OpenAI/Codex, Cursor, Z.AI, OpenRouter,
DeepSeek, Kimi, Grok, Antigravity, Kiro, MiniMax and more — in one place. It ships as a
Waybar module, a native TUI, a macOS menu-bar app, a GNOME extension, a KDE Plasma 6
plasmoid and an Omarchy plugin. It also manages **multiple Claude accounts**: showing
which account the Claude Desktop app and the `claude` CLI are signed into, and switching
between them while carrying local chat history and routines along.

## Core Value

Answer "how much quota do I have left, and on which account?" instantly and correctly —
without the user opening a browser, and without ever mis-reporting one account's usage as
another's.

## Current Milestone: v1.1 Staged upstream PRs

**Goal:** Decompose the ~51,000-line encrypted-sync change into small, independently
reviewable, independently mergeable PRs, and close the blocking findings from
akitaonrails's review of PR #113 before the first one ships.

**Target features:**
- Bounded KDF: mandatory memory, time and parallelism ceilings enforced **before** any
  Argon2id work, on every path that opens a keyfile (restore, join, open). The
  available-memory preflight this originally promised is **deleted rather than finished**
  — its macOS arm read total installed memory, a constant, so it could never fire, and it
  named a `--kdf-memory` flag that does not exist. See KDF-05 and `research/KDF-BOUNDS.md`.
- Portable archiving: `tar` resolved per-platform and its stderr sanitized through the
  project's usual terminal-control and sensitive-diagnostic path
- Three staged PRs in the maintainer's own order: format + bounded KDF/keyfile core,
  then local archive/restore, then remote transport and lifecycle
- An explicit ownership split: the core goes upstream; credential backup and remote
  transport live in a companion repository, which is the boundary the maintainer named

**Key context:** the work is complete and green on macOS and Linux (1866 tests) on
`milestone/encrypted-sync`; upstream `main` has moved to v1.4.0, so every PR rebases onto
it. `.planning/` must never reach an upstream PR. The fork stays the daily driver
throughout — it has found four defects no audit did.

## Requirements

### Validated

<!-- Shipped and confirmed valuable. -->

- ✓ Multi-vendor quota + time-to-reset in a status bar, TUI, and menu bar — v0.x–v1.1
- ✓ `ai-usagebar usage` — quota and reset for every configured entry in one command — v0.21.0
- ✓ Claude account switching (Desktop + CLI) with history and routine migration — v0.20.0
- ✓ Capture a Claude Desktop account (`account add <label> --desktop`) — v0.20.0
- ✓ Desktop-account usage with **no `claude` CLI login**, by reading the Desktop app's own
  encrypted OAuth token (Chromium safeStorage) — v0.21.0
- ✓ Deleted routines/chats are confirmed on switch instead of silently resurrected — v0.21.0
- ✓ A renamed routine's title converges to one value across accounts — v1.0.x
- ✓ A CLI+Desktop label collision is sourced from Desktop only, ending the refresh-token
  rotation war and silent usage misattribution — v1.1.0
- ✓ Encrypted GitHub sync: a second Mac pulls the bundle and opens Claude Desktop
  already signed in on all four accounts, with Cursor live too — confirmed in the
  field 2026-08-21 — v1.0 (fork only, not upstream)

### Active

<!-- Current milestone: encrypted GitHub sync. -->

- [ ] Push local state to a **private** GitHub repo, encrypted with a user-set password
- [ ] Pull that state on another machine and continue where the user left off
- [ ] Per-category opt-out (defaults to syncing everything except bulk transcripts)
- [ ] Incremental sync — re-upload only what actually changed
- [ ] Refuse to push credential-bearing bundles to a public repo

### Out of Scope

- **Public repos for credential-bearing bundles** — an encrypted blob in a public repo is
  an unlimited offline attack on the user's password, permanently archived by forks and
  third-party mirrors. Private only.
- **Chat transcripts in the default sync** — 4.0 GB / 4110 files on a real machine, far past
  GitHub's ~1 GB repo guidance. Opt-in and bounded only.
- **A hosted sync service** — the project has no backend and should not grow one; the user's
  own GitHub repo is the store.
- **Migrating Cowork (agent-mode) sessions** — their transcript path embeds the owning
  account UUID plus an unreconstructable suffix, so a copy renders as an empty chat.
- **Syncing to non-GitHub remotes (S3, Dropbox, generic git)** — deferred; GitHub first.

## Context

- **Upstream fork.** Canonical repo is `akitaonrails/ai-usagebar`; work here lands via PRs
  from the `ohmaseclaro` fork. `.planning/` must not reach an upstream PR — use
  `/gsd-pr-branch` to filter it out.
- **Existing crypto experience.** The project already decrypts Chromium/Electron
  `safeStorage` (AES-128-CBC + PBKDF2-HMAC-SHA1, `src/safe_storage.rs`) to read Claude
  Desktop's OAuth token, with hermetic tests and pinned compatibility vectors. That module
  is a precedent for how crypto is structured and tested here, but its primitives are
  Chromium's and are **not** what a password-protected backup should use.
- **Credential handling is already careful.** Per-account credentials live in mode-0600
  files or `CLAUDE_CONFIG_DIR`-scoped macOS Keychain items; rotation hazards between two
  stores of the same account are a known, previously-shipped bug class.
- **What the user actually wants synced** (measured): config 13 MB, Claude Desktop OAuth
  profiles 24 MB, chat session indexes 78 MB, routines 20 KB — about 115 MB — plus an
  optional 4.0 GB of chat transcripts.
- **Reconciliation discipline exists.** Routines and chats already merge newest-wins with a
  `synced.json` baseline that distinguishes a deletion from "never had it". Sync conflict
  handling should reuse that thinking rather than invent a second model.

## Constraints

- **Tech stack**: Rust 1.88, edition 2024. Reuse existing deps where possible — `reqwest`
  0.12 (rustls), `serde`, `tokio`, `base64`, RustCrypto crates already present.
- **Security**: Never commit a real credential. Never place secrets in process arguments or
  environment. Credential-bearing bundles are private-repo-only, enforced before every push.
- **Testing**: Tests must be hermetic — a `#[test]` may never read or write a real
  `$HOME`/`$XDG` path, the Keychain, or the network. Live tests stay `#[ignore]`d. The AUR
  `check()` runs `cargo test` during `makepkg`.
- **Packaging**: New dependencies must not break the AUR source build (no system libs that
  need `-dev` packages; static/bundled only, as with `rusqlite`).
- **Compatibility**: The widget must always exit 0 (Waybar hides modules that don't), and
  cache writes stay atomic.
- **Performance**: The user's explicit ask — "fast, light". Sync must be incremental; a
  no-op sync should cost near-nothing.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Default sync excludes 4 GB transcripts; opt-in and bounded | GitHub recommends <1 GB repos; 4 GB of append-only JSONL would dominate every sync | — Pending |
| Private repos only for credential-bearing bundles | A public encrypted blob is an unlimited offline password attack, permanently archived | — Pending |
| Content-addressed **encrypted chunks**, not whole-file encryption | Ciphertext defeats git delta compression: a 1-line append to a 50 MB file would re-upload 50 MB every sync | — Pending |
| Conflicts resolve last-write-wins per item, with a report | Matches the per-item reconciliation already used for routines and chats; keeps sync non-interactive | — Pending |
| Desktop token is the single source for a CLI+Desktop label collision | Two stores refreshing one rotating token invalidate each other and can silently misattribute usage | ✓ Good (v1.1.0) |

---
*Last updated: 2026-08-17 after starting the encrypted-GitHub-sync milestone*

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state
