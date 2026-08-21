---
phase: 06-surfaces-and-ship
plan: 04
type: execute
wave: 1
depends_on: []
files_modified:
  - src/tui/settings.rs
  - src/tui/view.rs
autonomous: true
requirements: [UX-05]
must_haves:
  truths:
    - "The Settings overlay has a Sync section listing every category with an on/off state and the last-sync time (D-04)."
    - "Toggling a category and saving writes `[sync] categories` through `toml_edit`, preserving every comment and unrelated key in the file."
    - "The saved file is mode 0600 on Unix and a running Waybar is signalled, because the save goes through the overlay's existing save path rather than a second one."
    - "Opening the overlay costs an index read, not a filesystem walk — a status panel that stat-walked 4 GB of transcripts on every keypress would freeze the TUI."
    - "Turning transcripts on is visibly flagged as the expensive, opt-in choice, so the toggle that can add gigabytes is not the one that looks like every other row."
    - "Tab / BackTab reach every new row and wrap exactly as before; no existing key binding changes meaning."
    - "Every test drives `save_to_path` with a temp path and `SettingsState` built from an explicit `Config`; none reads a real `$HOME`."
  artifacts:
    - "`Focus::SyncCategory(usize)` in src/tui/settings.rs, threaded through `next`/`prev`"
    - "Sync fields on `SettingsState` — the per-category flags and the last-sync instant"
    - "`[sync] categories` writing inside the existing `save_to_path`"
    - "The Sync block in `render` (src/tui/settings.rs) and whatever height the overlay's caller in src/tui/view.rs must give it"
  key_links:
    - "`save_to_config_default` already chmods and calls `waybar::request_refresh` — routing the new keys through `save_to_path` is what gets both for free; a second writer would silently lose them"
    - "`Focus::next`/`prev` is the only place tab order lives; adding a variant without updating both wraps the user into a row they cannot leave"
    - "`SyncConfig::categories` is the single source of truth for what sync carries; the overlay must round-trip that exact `Vec<SyncCategory>`, not a parallel bool set"
    - "This plan touches no file under `src/sync/` — 6-01 owns `src/sync/report.rs` in the same wave"
---

<objective>
The TUI half of the surfaces: a Sync section inside the Settings overlay showing what sync
carries, letting the user toggle a category, and saying when it last ran.

It reuses the overlay wholesale — the same `Focus` walk, the same `toml_edit` writer, the
same `chmod 600`, the same post-save Waybar signal. A second settings surface with its own
save path is how a project ends up with two ways to write one file and one of them wrong.

Implements **D-04** (sync state is surfaced, not just actions) on the TUI, and keeps
**D-01** intact: this section reads configuration and the local index. It never encrypts,
uploads, or fetches anything.

Purpose: make the category choice — which credentials leave the machine — editable somewhere
other than a hand-edited TOML file.
Output: the Sync section of the Settings overlay.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/06-surfaces-and-ship/6-CONTEXT.md
@CLAUDE.md
@src/tui/settings.rs
@src/tui/view.rs
@src/config.rs
@src/sync/index.rs
</context>

<source_audit>
This plan covers the ROADMAP's TUI deliverable and the TUI half of **UX-05**. The phase-wide
audit is in 6-01. `6-CONTEXT.md` numbers its decisions `D1`…`D5`, cited here as `D-01`…`D-05`.

| Source | Item | Covered by |
|---|---|---|
| ROADMAP | "TUI — a sync panel/section showing status, category toggles, and last-sync, reusing `src/tui/settings.rs`'s `toml_edit`-backed conventions and its post-save waybar signal" | this plan |
| ROADMAP | Success criterion 3: the TUI sync panel toggles a category, the change lands in `config.toml` at mode 0600, and waybar is signalled exactly as the Settings overlay already does | this plan |
| REQ | UX-05 — the requirement names the macOS menu bar; the ROADMAP's phase scope adds the TUI section alongside it, so both surfaces trace here and to 6-01/6-02 | this plan (TUI), 6-01 + 6-02 (menu bar) |
| CONTEXT | D-04 sync state is surfaced, not just actions | this plan, 6-01 |
| CONTEXT | D-01 surfaces call the CLI; no sync logic in the surface | this plan (reads config + index only) |

**Deliberate scope limit, not a gap:** the section shows toggles and last-sync, not per-category
file counts and byte totals. Those come from a filesystem walk that `sync status` already
performs and prints; running it synchronously on overlay open would block the render loop on a
transcript tree measured in gigabytes. The section names `ai-usagebar sync status` as where the
numbers live. If a future phase wants them inline, they arrive off-thread, which is a different
design than this overlay has today.
</source_audit>

<tasks>

<task type="tracer" tdd="true">
  <name>Task 1: The Sync rows — state, focus walk, and the toggle</name>
  <precondition>The API this task names — `SyncCategory::ALL`, `SyncCategory::label`, `SyncConfig::categories`, `SyncConfig::includes` — exists in `src/config.rs` today, but `SyncConfig` may have grown fields in Phases 2–5 (a repo, transcript bounds). Read it first, confirm those four, and round-trip the real `categories` vector rather than a shape invented here.</precondition>
  <files>src/tui/settings.rs</files>
  <behavior>
    - `SettingsState::from_config` over a default `Config` yields one sync row per `SyncCategory::ALL` entry, in that order, with transcripts off and the other four on.
    - `Focus::next` from the last key row lands on `SyncCategory(0)`; from the last sync row it lands on `Save`; `prev` reverses both, so the walk stays a closed cycle.
    - Space or Enter on a focused sync row flips exactly that row and nothing else.
    - A `Left`/`Right` on a sync row does not flip it — those keys mean "cycle a choice" on the Primary row, and reusing them for a boolean would make a mis-aimed arrow change what leaves the machine.
    - Ctrl-C on a sync row still returns `Action::Quit`, and Esc still returns `Action::Close`, unchanged.
    - A config whose `categories` list is empty yields every row off and is representable — "sync nothing" is a legal state and must not be silently rewritten to the default.
  </behavior>
  <action>
Add `Focus::SyncCategory(usize)` between `Key(usize)` and `Save`, and update both `next` and
`prev`. That enum is the only place tab order lives; a variant added to one and not the other
traps the cursor in a row it cannot leave, which in a modal overlay reads as a hang.

Add to `SettingsState`: `sync_categories: Vec<(SyncCategory, bool)>` in `SyncCategory::ALL`
order, and `sync_last_sync: Option<DateTime<Utc>>`.

Build the flags in `from_config` from `SyncConfig::includes`. Take the last-sync value as a
parameter rather than reading the index inside `from_config` — that constructor is pure today
and several tests build it from a bare `Config`. Add `from_config_with_sync(cfg, last_sync)`
and let `from_config` delegate with `None`, so no existing call site changes.

Handle the toggle in the `Focus::SyncCategory(i)` arm of `handle_key`, accepting Space and
Enter only. Keep the existing modifier-chord rejection above it — the overlay swallows every
key while open, so an unhandled chord must stay a no-op rather than flip a category.

The credentials row deserves a word in the rendered label but not a confirmation dialog: it
is on by default, it is the point of the feature, and nothing leaves the machine until a push
runs behind the private-repo gate. The row that needs flagging is transcripts, which is the
one that turns a 30 MB bundle into a multi-gigabyte one; give it a suffix naming it as the
opt-in, expensive choice.
  </action>
  <verify>
    <automated>cargo test --lib tui::settings</automated>
  </verify>
  <done>Every category has a focusable row, the focus cycle is closed in both directions, Space and Enter toggle, arrows and stray chords do not, and an empty category list round-trips as "all off".</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Saving — one writer, keeping comments, mode, and the Waybar signal</name>
  <files>src/tui/settings.rs</files>
  <behavior>
    - Saving over a file containing a commented, hand-written sync section preserves every comment and every key the overlay does not own.
    - Toggling transcripts on and saving produces a `categories` array containing all five labels; toggling it back off and saving produces the four.
    - Turning every category off writes an empty array, and reloading that file yields a config that syncs nothing — not the default set.
    - Saving over a file with no sync section at all creates one, and saving over a file with an unrelated `[ui]` section leaves it byte-identical.
    - Re-saving without changing anything leaves the file byte-identical, so the overlay is not a source of spurious diffs.
    - On Unix the saved file is mode 0600.
  </behavior>
  <action>
Extend the existing `save_to_path` — do not add a second writer. It already reads the original
text, edits it through `toml_edit`, writes atomically, and chmods; `save_to_config_default`
wraps it with the Waybar signal. Both come free by editing in place, and both would be lost by
a parallel path, which is exactly the drift D-01 warns about at the frontend boundary.

Write `[sync] categories` as an array of the labels `SyncCategory` already emits — the same
strings the config parser reads, taken from the enum, never re-spelled as literals in the
writer. An empty selection writes an empty array and must not be elided; a missing key means
"default", and the user asking for nothing is a different statement than the user not having
chosen.

Reuse the file's existing round-trip test helpers. The mode assertion goes behind `cfg(unix)`
like `PERMS_NOTE` already is — Windows has no such step, and the crate builds there.
  </action>
  <verify>
    <automated>cargo test --lib tui::settings</automated>
  </verify>
  <done>Toggles round-trip through a real file, comments and unrelated sections survive, an empty selection is preserved as empty, an unchanged save is a no-op diff, and the file is 0600 on Unix.</done>
</task>

<task type="auto">
  <name>Task 3: Rendering the section</name>
  <files>src/tui/settings.rs, src/tui/view.rs</files>
  <action>
Add the Sync block to `render`, below the key rows and above Save, following the block layout
and theme colours the overlay already uses. Per row: the category label, an on/off marker in
the same visual language as the rest of the overlay, and the focus highlight driven by
`state.focus`.

Above the rows, one line for last-sync — "última sync: nunca" when absent — and one dim line
naming `ai-usagebar sync status` as where per-category file counts and byte totals live. Say
that plainly rather than leaving a blank space where numbers look like they should be; a user
who cannot see counts and is not told where they are assumes the feature is broken.

The overlay's height is computed by its caller; grow whatever constant `src/tui/view.rs` uses
so five extra rows plus two lines fit, and keep the overlay clamped to the terminal so a short
window degrades to a scrolled or truncated view rather than a panic. If the current layout
already flexes, change nothing there and say so in the summary.

Keep the render a pure function of `SettingsState` and `Theme`, as it is today. No filesystem
read, no clock read, no index open inside `render` — the last-sync value arrives on the state,
which is what makes the wording testable without a disk.
  </action>
  <verify>
    <automated>cargo test --lib -- tui::settings tui::view</automated>
  </verify>
  <done>The section renders with five toggle rows, a last-sync line, and the pointer to `sync status`; the overlay fits its computed area; `render` reads nothing outside its arguments.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| TUI keystroke → `[sync] categories` | A keypress decides which credential files are eligible to leave the machine |
| overlay → `config.toml` | A rewrite of a file that also holds inline API keys |
| `config.toml` → overlay | Existing on-disk content, possibly hand-edited or restored from another machine, is parsed and re-emitted |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-6-20 | Tampering | a mis-aimed keypress enabling a category | high | mitigate | Only Space and Enter on a focused sync row toggle; arrows are reserved for the Primary selector and modifier chords stay no-ops, so no single stray key changes what sync carries |
| T-6-21 | Information disclosure | rewriting a file holding inline API keys | critical | mitigate | The write goes through the existing `save_to_path`, which preserves untouched keys via `toml_edit` and re-applies mode 0600; no second writer is introduced and no key is re-serialized from memory unless the user edited it |
| T-6-22 | Tampering | an empty selection silently reverting to the default set | medium | mitigate | An empty `categories` array is written explicitly and round-tripped in a test; "sync nothing" stays distinguishable from "never chose" |
| T-6-23 | Denial of service | opening the overlay walking a multi-gigabyte transcript tree | medium | mitigate | The section reads the index for last-sync and the config for flags; no scan runs on open, and counts are delegated to `sync status` by name |
| T-6-24 | Information disclosure | a hand-written comment holding a secret being relocated by the rewrite | low | mitigate | `toml_edit` preserves comment position; a fixture with a commented sync section asserts byte-level survival of everything the overlay does not own |
| T-6-SC | Tampering | dependency surface | low | accept | Zero new crates — `toml_edit`, `chrono`, and `ratatui` are already declared. `Cargo.toml` is not in this plan's `files_modified` |
</threat_model>

<verification>
- `cargo test --lib -- tui::settings tui::view` is green. Multi-filter uses the
  `cargo test --lib -- a b` form; `cargo test --lib a b` takes one positional only.
- No test constructs `Config::load`, `default_config_path`, `save_to_config_default`, or
  `index::default_path`; every save test passes an explicit `&Path` under a `TempDir`.
- `git diff --stat` touches nothing under `src/sync/`, `macos/`, `gnome-extension/`,
  `kde-plasmoid/`, or `omarchy/` (D-05), and does not touch `Cargo.toml`.
- Drive it once by hand: open the TUI, press `s`, tab to a sync row, toggle, Ctrl-S, and
  confirm the file changed and Waybar refreshed.
</verification>

<success_criteria>
The Settings overlay has a Sync section. Toggling a category and saving lands in
`config.toml` at mode 0600 with comments intact and Waybar signalled, through the overlay's
one existing save path. Last-sync is visible; per-category counts are pointed at, not faked.
</success_criteria>

<output>
Create `.planning/phases/06-surfaces-and-ship/6-04-SUMMARY.md` when done.

Record the final `Focus` cycle order, the signature of `from_config_with_sync`, and whether
`src/tui/view.rs` needed a height change or already flexed. Record the exact TOML the writer
emits for the empty-selection case — 6-05 documents the "sync nothing" state in the README and
must quote it correctly.
</output>
