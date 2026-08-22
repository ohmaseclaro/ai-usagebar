---
phase: 06-surfaces-and-ship
plan: 04
subsystem: tui/settings
tags: [tui, settings, sync, toml-edit, focus, scroll, call-sites]
status: complete
requires:
  - "config::SyncCategory::{ALL, label} — the canonical order and the one spelling of each token"
  - "config::SyncConfig::{categories, includes} — the single source of truth for what sync carries"
  - "tui::settings::save_to_path — the overlay's one writer (atomic write + chmod 600)"
  - "tui::settings::save_to_config_default — the only thing that signals Waybar; unchanged"
  - "sync::index::{default_path, Index::at, Index::last_sync} — read in the binary, never in the overlay"
provides:
  - "tui::settings::Focus::SyncCategory(usize) — NEW variant, between Key(i) and Save"
  - "tui::settings::SettingsState::from_config_with_sync — NEW seam carrying last-sync"
  - "tui::settings::SettingsState::{sync_categories, sync_last_sync, sync_dirty} — NEW public fields"
  - "`[sync] categories` written from inside the existing save_to_path"
affects:
  - "6-05 — the README's 'sync nothing' wording; the exact emitted TOML is quoted below"
tech-stack:
  added: []
  patterns:
    - "the new writer lives inside the existing save_to_path, so chmod 600 and the Waybar signal are inherited rather than re-implemented"
    - "the constructor stays pure and the caller owns the only read that touches a file — which is what keeps every settings test hermetic"
    - "a dirty flag gates the write, the same discipline the primary selector and the key fields already follow"
    - "the render records the focused body line while building it, rather than deriving it from a second layout table that would drift"
key-files:
  created: []
  modified:
    - src/tui/settings.rs
    - src/bin/ai-usagebar-tui.rs
decisions:
  - "sync_dirty added beyond the plan: an untouched save must not turn 'never chose' into a persisted choice"
  - "src/bin/ai-usagebar-tui.rs modified instead of src/tui/view.rs — view.rs already flexed, and the bin is where from_config_with_sync gets its only production call site"
  - "the overlay body now scrolls to the focused row; without it the nine new lines pushed Save off an 80x24 window"
  - "'last sync: never' in English, matching render_status in src/sync/report.rs, not the plan's Portuguese draft wording"
  - "no sync action (push/pull/rekey) in the overlay at all — a modal that cannot carry the CLI's two confirmations must not offer an easier path to the same operation"
metrics:
  duration: ~70 min
  completed: 2026-08-20
---

# Phase 6 Plan 04: The TUI Sync section — Summary

The Settings overlay grew a Sync section: one focusable row per `SyncCategory`,
an on/off marker, the last-sync time, and a pointer at `ai-usagebar sync status`
for the counts it deliberately does not compute. Space or Enter toggles a row;
saving writes `[sync] categories` through the overlay's **one existing writer**,
so the `chmod 600` and the post-save Waybar signal come along unchanged.

---

## What the plan asked for, and what shipped

| Plan artifact | Shipped |
|---|---|
| `Focus::SyncCategory(usize)` threaded through `next`/`prev` | yes, unchanged |
| sync fields on `SettingsState` | yes, **plus `sync_dirty`** (see below) |
| `[sync] categories` inside the existing `save_to_path` | yes, unchanged |
| the Sync block in `render` | yes |
| a height change in `src/tui/view.rs` | **not needed** — see "view.rs already flexed" |

### The final `Focus` cycle order

```
Primary → Key(0) … Key(9) → SyncCategory(0) … SyncCategory(4) → Save → Primary
```

`next` and `prev` were both updated; a test walks every sync row and asserts
`f.next().prev() == f` and `f.prev().next() == f` on each, so no row is a trap
in either direction.

### The signature

```rust
/// Pure: config in, state out. No filesystem, no clock, no `$HOME`.
pub fn from_config(cfg: &Config) -> Self;

/// Same, plus the last-sync instant the caller already had.
pub fn from_config_with_sync(cfg: &Config, last_sync: Option<DateTime<Utc>>) -> Self;
```

`from_config` delegates with `None`, so it opens no file and every existing
caller — including the `settings --json` bridge and `settings apply` — is
byte-for-byte unaffected.

### The exact TOML for the empty-selection case — 6-05 must quote this

Every category off, saved over a file with **no** `[sync]` section:

```toml
# my config
[zai]
enabled = true

[ui]
primary = "anthropic"

[sync]
categories = []
```

Saved over a file that **already had** a `[sync]` section, the array is
rewritten in place and the neighbouring keys keep their position:

```toml
[sync]
categories = []
keep_snapshots = 3
```

`categories = []` reloads through `Config::load_from` as
`sync.categories.is_empty()` and `includes(_) == false` for all five — "sync
nothing", not the default set. A **missing** key still means "the default";
the two are different statements and the writer keeps them apart.

---

## Deviations from plan

### 1. `src/bin/ai-usagebar-tui.rs` modified instead of `src/tui/view.rs`

**[Rule 2 — missing critical functionality.]** The plan said to add
`from_config_with_sync(cfg, last_sync)` and "let `from_config` delegate with
`None`, **so no existing call site changes**". Followed literally, that
guarantees `from_config_with_sync` has **zero production call sites** and the
last-sync line renders `never` forever — precisely the dead-code failure mode
this milestone was told to stop shipping.

`src/bin/ai-usagebar-tui.rs:345` is the only place in the codebase that
constructs a `SettingsState` for the overlay (the `s` keypress). It is not in
this plan's `files_modified`, and it is not owned by any sibling plan — 6-01
owns `src/sync/{cli,report}.rs` and `macos/`, 6-03 owns
`src/widget/run.rs` and `src/config.rs`. The change is 20 lines:

```rust
/// When sync last completed, for the Settings overlay's Sync section.
///
/// Read here rather than inside the overlay so `SettingsState::from_config`
/// stays pure and every settings test stays hermetic. The index is opened only
/// when it already exists — pressing `s` must not create a sync index for a
/// user who has never synced. A failed open reads as `never`, which is exactly
/// what `ai-usagebar sync status` prints for the same state.
fn last_sync() -> Option<chrono::DateTime<chrono::Utc>> {
    let path = ai_usagebar::sync::index::default_path().ok()?;
    if !path.exists() {
        return None;
    }
    ai_usagebar::sync::index::Index::at(&path)
        .ok()
        .and_then(|index| index.last_sync())
}
```

The `path.exists()` guard matters: `Index::at` **creates** the database
(`create_dir_all` + `create_private` + `execute_batch(SCHEMA)`). Without the
guard, opening a settings dialog would create a sync index for a user who has
never synced.

### 2. `sync_dirty` added — an untouched save must not invent a choice

Not in the plan. Without it, opening Settings to paste one API key and pressing
Ctrl-S would write `[sync] categories = ["config","credentials","routines","chat_index"]`
into a config that never had a `[sync]` section — silently converting "never
chose" into "chose the defaults", which is the exact distinction **T-6-22**
exists to protect. The flag is the file's own established discipline: the
primary selector already refuses to write a disabled primary as a side effect,
and `update_key` already returns early on `!input.dirty`.

Pinned by `an_untouched_save_never_invents_a_sync_section`.

### 3. `render` scrolls — a regression I introduced and then fixed

**[Rule 1 — bug.]** The Sync block adds nine lines. Measured against a
`TestBackend`, the Save row needed a **30-row** terminal to stay on screen
(it needed ~22 before). Terminal.app's default window is 80x24: a user there
would have seen a "Sync" header with nothing under it, while Tab still moved
focus into rows they could not see — toggling what leaves the machine blind.

Two changes: the modal takes 96% of the frame height rather than 88%, and the
body `Paragraph` scrolls just far enough to keep the focused control visible.
`a_short_terminal_scrolls_to_the_focused_row_instead_of_hiding_it` drives every
sync row, Save, and Primary at 80x24.

### 4. Wording and one fixture

- **"last sync: never"**, in English, matching `render_status` in
  `src/sync/report.rs`. The plan drafted `"última sync: nunca"`; the rest of the
  overlay ("Primary vendor", "API keys", "Save") is English, and the CLI already
  prints `last sync: never` for the same state.
- The plan's "unrelated `[ui]` section stays byte-identical" test uses
  **`[context]`** instead. `[ui]` is *not* unrelated — the overlay owns
  `ui.primary` and rewrites it on every save. Asserting on a section the overlay
  genuinely has no key in is the honest version of that claim.

---

## view.rs already flexed — no change was needed

`src/tui/view.rs:36` calls `settings::render(f, f.area(), s, &app.theme)` — the
full frame, every frame. The overlay sizes itself with
`centered_rect(74, %, area)`, which is percentage-based. There is no height
constant in `view.rs` to grow, and the file is **unmodified**. The one sizing
change (88 → 96) lives inside `settings.rs` where `centered_rect` is called.

---

## Consent: what this surface deliberately does not offer

The overlay ships **no sync action** — no push, no pull, no rekey, no restore,
no passphrase field. It reads configuration and one already-computed timestamp,
and it writes one TOML key. This is deliberate, not an omission:

- **`sync pull` is a dry run by default** — the *absence* of `--apply`, not a
  flag anything checks. A button cannot express "the absence of a flag" without
  reimplementing the CLI's semantics in a second place (D-01).
- **`sync pull --apply` carries two separate confirmations** (5-06/5-07):
  `confirm_apply` renders the whole plan and then asks, and `confirm_credentials`
  is a *second*, separate consent that `--force` alone does not answer —
  `--force-credentials` also requires `--force`. A modal overlay cannot carry
  both of those faithfully; approximating them would put an **easier path to an
  irreversible operation** in the TUI than exists in the CLI. That is a
  regression even if every line of it works, so the actions are left out.
- **`sync rekey` is not revocation** and a locally-newer item is skipped and
  named; a symlinked destination is `RejectedPath` and **no flag promotes it**.
  None of that is restated here, because this surface makes no claim about
  restore behaviour at all.

**No passphrase is collected, displayed, or written.** Nothing in this plan
touches `src/sync/passphrase.rs`, and `config.toml` gains exactly one key:
`[sync] categories`, an array of five known tokens. Toggling a category is
reversible and moves no bytes — nothing leaves the machine until a `sync push`
runs behind the private-repo gate, which is where the consent for *that*
already lives.

The `[sync] keep_snapshots` rule (`0` refused at config load, clamped to ≥1 by
prune) is untouched: the overlay neither reads nor writes that key, so a
hand-set value survives a save byte-for-byte — pinned by
`save_preserves_a_hand_written_commented_sync_section`, which round-trips
`keep_snapshots = 3`.

---

## Production call sites of everything added — none are zero

| Added | Kind | Production call sites |
|---|---|---|
| `Focus::SyncCategory(usize)` | pub variant | `Focus::next` ×2, `Focus::prev` ×3, `handle_key` (the toggle arm), the hint-footer match, `render` (scroll target), `sync_lines` (focus highlight) — **7** |
| `SettingsState::from_config_with_sync` | pub fn | `src/bin/ai-usagebar-tui.rs:362` (the `s` keypress) and `from_config` — **2** |
| `SettingsState::sync_categories` | pub field | `from_config_with_sync`, `toggle_sync_category`, `update_sync_categories`, `sync_lines` — **4** |
| `SettingsState::sync_last_sync` | pub field | `from_config_with_sync`, `last_sync_text` — **2** |
| `SettingsState::sync_dirty` | pub field | `toggle_sync_category`, `update_sync_categories` — **2** |
| `toggle_sync_category` | private fn | `handle_key` — **1** |
| `update_sync_categories` | private fn | `save_to_path` — **1** |
| `sync_lines` | private fn | `render` — **1** |
| `last_sync_text` / `sync_note` / `sync_row` | private fns | `sync_lines` / `sync_row` / `sync_lines` — **1** each |
| `SYNC_PREAMBLE_LINES` | private const | `render`, `sync_lines` (debug_assert) — **2** |
| `last_sync()` (binary) | private fn | the `s` keypress arm — **1** |

**Zero-call-site items: none.** The user-visible path is end to end: press `s`
→ `last_sync()` reads the index if it exists → `from_config_with_sync` builds
the rows → `render`/`sync_lines` draw them → Space/Enter flips one →
`handle_key` → Ctrl-S → `save_to_config_default` → `save_to_path` →
`update_sync_categories` → atomic write → `chmod 600` → `waybar::request_refresh`.

---

## Threat register — dispositions as implemented

| Threat ID | Disposition | Where it lives |
|---|---|---|
| T-6-20 mis-aimed keypress enabling a category | mitigated | `toggle_sync_category` accepts only `Char(' ')` and `Enter`; Left/Right fall through to nothing and the pre-existing modifier-chord swallow sits above it. Tests: `arrows_never_flip_a_sync_row`, `a_modifier_chord_on_a_sync_row_is_a_no_op` |
| T-6-21 rewriting a file holding inline API keys | mitigated | the write is `update_sync_categories` **inside** `save_to_path`; no second writer, no key re-serialized. `the_sync_write_inherits_the_overlays_chmod` pins mode 0600 on the sync path specifically |
| T-6-22 empty selection reverting to the default set | mitigated | `categories = []` written explicitly and never elided; `every_category_off_writes_an_empty_array_that_reloads_as_syncing_nothing` reloads it through `Config::load_from`. `sync_dirty` keeps "never chose" distinct |
| T-6-23 overlay walking a multi-gigabyte transcript tree | mitigated | `sync_lines` is a pure function of `(state, theme)`; no filesystem, clock, or index read on the render path. The one index read is in the binary, once per overlay open, guarded by `path.exists()` |
| T-6-24 a hand-written comment being relocated | mitigated | `save_preserves_a_hand_written_commented_sync_section` asserts a leading comment, an interior comment, and a trailing `# two weeks was too much` all survive verbatim |
| T-6-SC dependency surface | accepted | zero new crates; `Cargo.toml` and `Cargo.lock` untouched (`git diff --name-only` returns two source files) |

---

## Verification

| Gate | Baseline | Result |
|---|---|---|
| `cargo test --lib` | 1512 passed / 0 failed | **1539 passed / 0 failed** (+27) |
| `cargo test` (total) | 1572 passed / 0 failed | **1599 passed / 0 failed** (+27) |
| `cargo clippy --all-targets -- -D warnings` | clean | **clean** (0 warnings) |
| `cargo fmt --check` | clean | **clean** |
| `make test` (incl. GNOME / KDE / Omarchy contract suites) | green | **green** — `marker logic tests passed`, `plasmoid logic tests passed`, `Omarchy model tests passed` |

`git diff --name-only bfb49ef..HEAD` → `src/bin/ai-usagebar-tui.rs`,
`src/tui/settings.rs`. Nothing under `src/sync/`, `macos/`, `gnome-extension/`,
`kde-plasmoid/`, `omarchy/` (D-05). `Cargo.toml` and `Cargo.lock` unchanged.

**Hermeticity.** Every new test builds its state from `Config::default()` or
`Config::load_from(<TempDir path>)`, saves through `save_to_path(&state, &path)`
with a `TempDir` path from the existing `temp_config` helper, and themes with
`Theme::default()` (a pure constant table — no Omarchy file, no `$XDG`). No new
test calls `Config::load`, `default_config_path`, `save_to_config_default`, or
`index::default_path`. The one pre-existing `default_config_path()` reference in
the test module (`settings_save_uses_the_same_config_path_as_load`, line 1712)
is untouched and is CLAUDE.md's explicit carve-out: a test asserting the path
*resolver itself* agrees with the loader.

---

## Deferred — live UAT, not skipped work

The plan's fourth verification item is manual and cannot run headlessly:

> Drive it once by hand: open the TUI, press `s`, tab to a sync row, toggle,
> Ctrl-S, and confirm the file changed and Waybar refreshed.

The file-change half is covered automatically (`save_to_path` round-trip tests).
The **Waybar refresh** half is not: `waybar::request_refresh` is only reached
through `save_to_config_default`, which resolves the real config path and may
not be called from a hermetic test. It is unchanged by this plan — the sync
write was added *inside* `save_to_path`, which `save_to_config_default` already
wraps — but "unchanged code on an untested path" is a claim, not a proof.
Recorded as an unrun verify.

---

## Self-Check: PASSED

- `src/tui/settings.rs` — FOUND (modified)
- `src/bin/ai-usagebar-tui.rs` — FOUND (modified)
- `.planning/phases/06-surfaces-and-ship/6-04-SUMMARY.md` — FOUND
- commits `7d207cb`, `efa0dd1`, `7bbdeb2`, `34fab39`, `364d07a`, `313b346` — all present in `git log`
