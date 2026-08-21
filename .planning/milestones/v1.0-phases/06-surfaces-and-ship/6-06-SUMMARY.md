---
phase: 6
plan: "06"
subsystem: sync
tags: [cli, ux, ansi, terminal, no-color, progress]
requires:
  - "6-02 (report.rs — the status/dry-run model and its renderers)"
  - "6-03 (setup.rs — the five-step guided flow)"
  - "4-03 (progress.rs — the Progress trait and its two writers)"
provides:
  - "sync::report::Style — the whole crate's ANSI palette, injected never sniffed"
  - "sync::report::render_status_styled / render_dry_run_styled"
  - "sync::report::reflow — wrap-at-a-space, styled runs only"
  - "sync::push::progress::Progress::phase — the local phases before the upload"
  - "sync::push::progress::bar / render_styled — the upload bar"
  - "display::color_enabled — the one NO_COLOR read, outside the guarded subtree"
affects:
  - "src/sync/cli.rs (wiring only)"
  - "src/sync/push/mod.rs (two phase calls)"
tech-stack:
  added: []
  patterns:
    - "Zero new dependencies. Four raw SGR codes, the way src/widget/pretty.rs already writes them."
    - "The plain path is byte-identical by construction: Style::sgr returns its argument untouched when off."
    - "Injected facts, never ambient reads — IsTerminal and NO_COLOR are resolved in sync::cli and passed down."
key-files:
  created: []
  modified:
    - src/sync/report.rs
    - src/sync/push/progress.rs
    - src/sync/github/setup.rs
    - src/sync/cli.rs
    - src/display.rs
    - src/sync/push/mod.rs
decisions:
  - "Colour, computed column widths and wrapping are all terminal-only. A pipe emits today's bytes."
  - "4-bit SGR (2/1/31/32/1;36), never 24-bit: the palette borrows the user's terminal theme."
  - "NO_COLOR disables the bar too, not only the colour."
  - "The packing phase gets transitions with real figures, not a bar — a bar needs callbacks in plan::build (21 call sites)."
metrics:
  duration: ~2h
  completed: 2026-08-20
status: complete
---

# Phase 6 Plan 06: Making `ai-usagebar sync` readable — Summary

Colour, a table whose columns line up, five step headers that read as five
steps, and a progress bar — with the plain (piped) rendering byte-identical to
what shipped, because `sync status --json` feeds the macOS menu bar and the Node
contract suites parse the text form.

## Before / after — one real screen

The user's own `sync status`, run on their machine. **This is the same command
in the same terminal**; the only difference is that the second one knows it is
talking to a tty.

**Before** — one weight of grey, three unlabelled columns of numbers:

```
  config           1 files       309 B
  credentials    107 files    23.8 MiB
  routines         9 files    15.0 KiB
  chat_index    1537 files    76.0 MiB
  transcripts   2146 files     2.0 GiB

  total         3800 files     2.1 GiB

  last sync: never
  index:     /Users/…/Library/Caches/ai-usagebar/sync/index.sqlite3

  repo:      ohmaseclaro/ai-usagebar-sync
  visible:   private
  token:     present (Keychain)
  verified:  2026-08-20T14:17:59.369711+00:00
```

**After** — `‹dim›` is `ESC[2m`, `‹bold›` is `ESC[1m`, `‹green›` is `ESC[32m`:

```
‹dim›                    files       raw‹/›          ← a header, which status never had
  ‹dim›config‹/›          1 ‹dim›files‹/›  ‹dim›   309 B‹/›
  ‹dim›credentials‹/›   107 ‹dim›files‹/›  ‹dim›23.8 MiB‹/›
  ‹dim›routines‹/›        9 ‹dim›files‹/›  ‹dim›15.0 KiB‹/›
  ‹dim›chat_index‹/›   1537 ‹dim›files‹/›  ‹dim›76.0 MiB‹/›
  ‹dim›transcripts‹/›  2150 ‹dim›files‹/›  ‹dim› 2.0 GiB‹/›

  ‹dim›total‹/›        ‹bold›3804‹/› ‹dim›files‹/›  ‹bold› 2.1 GiB‹/›   ← the figures that decide

  ‹dim›last sync: ‹/›never
  ‹dim›index:     ‹/›/Users/…/Library/Caches/ai-usagebar/sync/index.sqlite3

  ‹dim›repo:      ‹/›ohmaseclaro/ai-usagebar-sync
  ‹dim›visible:   ‹/›‹green›private‹/›              ← the gate's precondition, and the only green
  ‹dim›token:     ‹/›present (Keychain)
  ‹dim›verified:  ‹/›2026-08-20T14:17:59.369711+00:00
```

Piped, it is the first block again — verified byte-for-byte against a build of
`main` (`diff` of `sync status` and of `sync status --json`: identical).

## The four reported problems

### 1. No colour anywhere

`sync::report::Style` — one `bool` and five methods, no crate, following
`src/widget/pretty.rs`'s existing habit of writing `"\x1b[2m"` by hand.

| method | code | reserved for |
|---|---|---|
| `dim` | `2` | labels, units, prose, the figures that are context |
| `bold` | `1` | the numbers that decide — bytes to send, files at risk |
| `head` | `1;36` | a step marker, and the generated passphrase |
| `bad` | `31` | a refusal. Nothing else is red. |
| `good` | `32` | something that actually succeeded. Nothing else is green. |

Four-bit SGR rather than 24-bit on purpose: the palette borrows the user's own
terminal theme instead of overriding it, which is what keeps the screen calm.
`the_palette_is_four_attributes_and_never_a_literal_colour` asserts the whole
set, so a fifth colour fails a test rather than a review.

**Untrusted text.** `Style::sgr` runs its payload through
`display::sanitize_untrusted_field` before wrapping it, so a manifest path or a
GitHub error message cannot terminate the sequence it is inside and start its
own. Applied once, in the one place, rather than at each call site that
remembered — `untrusted_text_cannot_close_the_sequence_it_is_styled_inside`
feeds it an OSC-52 clipboard write and a bidi override and checks that only this
module's own (balanced) escapes survive.

**`NO_COLOR`** is honoured, and is read in `src/display.rs` rather than beside
the palette: `passphrase.rs`'s structural guard walks the whole of `src/sync/`
refusing `std::env` in production code, because every password input path lives
there. `display::color_enabled(is_terminal)` makes the read;
`color_enabled_with(is_terminal, no_color_set)` is the injected form the tests
drive, so no test mutates a process-wide variable.

### 2. The five-column table did not line up

`{:>5}` for a file count overflows at six digits and shoves every column right
of it out of line; `{:>10}` does the same for `1023.9 MiB`; and `would send` is
ten characters in a field that was never measured against it. `Widths::of` now
measures every figure that will actually be drawn — the category rows, the
totals row, the exclusion line, **and the header labels** — when styled, and
returns the old hard-coded set when plain.

`2141 files 1.7 GiB left out by the age and size bounds` is 82 columns once
padded into the table, and there is no arrangement of those columns that makes
it fit at 80. It stopped pretending to be a table row: it is now a dim sub-note
indented under transcripts, short enough to fit whole. Everything else long
(`note`, the `--dry-run` sentences, the `sync setup` prose) goes through
`reflow`, which breaks **only at a space** — a word longer than the budget
overruns visibly rather than being cut.

`no_styled_line_runs_past_the_terminal_or_breaks_a_word` checks every line of
two reports against 80 columns and then checks that every word of the plain
rendering survives the wrapped one intact.

### 3. `sync setup` read as one wall

Narration only — **no step moved, no `SetupPrompt` method changed, no control
flow touched.** See "For the join path" below.

- The five markers wear the only accent in the flow, and are the only thing that
  does: `1/5`…`5/5` in `1;36`, body dim.
- **The no-recovery warning is the only bold prose on the screen.** Of everything
  setup says, it is the one sentence whose cost is unrecoverable, and it used to
  read at exactly the weight of everything around it.
- **The generated passphrase carries the accent**, on the value rather than the
  label — it is the one thing the user has to act on.
- Refusals from the strength floor are red; `warning:` and `first contact:` are
  bold; everything else is dim.

`the_steps_and_the_two_lines_that_matter_carry_the_only_weight` counts the
accent exactly six times (five markers + the passphrase) and asserts every
sequence is closed. `the_unstyled_flow_says_the_same_words_the_styled_one_does`
pins the unstyled wording — and deliberately never prints the narration on
failure, because it holds the generated passphrase.

### 4. The push had a silent minute

`Progress` gained `phase(label, files, bytes)`, **defaulted to a no-op** so
`Silent` and the recording doubles in `tests/sync_push_e2e.rs` compile
untouched. `push::run` calls it twice, around the two local reads that are the
whole of the wait on a first push:

```
reading what changed…
sealing into packs 3803 files — 2.1 GiB
[████████░░░░░░░░░░░░░░░░]  33% uploading 1/3 assets — 40.0 MiB of 120.0 MiB
```

The bar is a single `\r`-rewritten line, as `Terminal` already did — no
alternate screen, no cursor hiding, so a push that dies leaves the terminal
usable. Rewrites are padded to the widest line seen so far rather than using
`\x1b[K`: `NO_COLOR` takes this writer too and must emit **no** escape, and it
also fixes a latent bug where `1023.9 KiB` → `1.0 MiB` left a stray tail
(`a_shrinking_line_leaves_no_tail_of_the_one_before_it`).

**Honest ceiling, marked `ponytail:` in the code.** The phases are transitions
with real figures, not a moving bar. A moving bar needs a per-file callback
inside `plan::build` (21 call sites, generic over its hasher) and inside
`packer::build`; `phase`'s signature already carries the totals such a bar would
need. On a first push this turns one dead line into three that arrive at real
boundaries; the gaps between them are still gaps.

## Deviations from plan

**1. [Rule 3 — blocking] `src/display.rs`, outside the four listed files.**
The brief allowed this explicitly ("put the read wherever the guard allows").
`passphrase.rs::no_password_input_path_reads_the_process_environment` walks all
of `src/sync/` and fails on `std::env`, so the `NO_COLOR` read cannot live in
`report.rs`. Two functions, six lines, plus one test.

**2. [Rule 3 — blocking] `src/sync/push/mod.rs`, three lines.**
Problem 4 cannot be met from the four listed files: `packer::build` and
`plan::build_with_keys` are both called from inside `push::run`, and nothing in
`cli.rs` can see between them. The change is two `progress.phase(...)` calls and
a comment; no signature changed, because `phase` is defaulted.

**3. `#[allow(clippy::too_many_arguments)]` on `cli::status_with`.** It went
from seven arguments to eight. Every one is a fact the real world supplies that
a test has to fake; a struct would move the same eight one line up.

## For the join path

You asked to be told exactly what changed in `setup.rs`. Nothing structural —
but two things are worth knowing before you rebase:

- **`SetupPrompt` gained one defaulted method, `fn style(&self) -> Style`.** A
  seam of the same kind as `kdf()` and `store_token()`, and the only way to get
  a palette into `run` without changing `run`'s signature. `TtyPrompt` overrides
  it; every existing double takes the default (`Style::PLAIN`) and therefore
  asserts against the unstyled wording it always did. `TtyPrompt` is now
  `TtyPrompt::new(style)` — two call sites, both in `cli.rs`.
- **`let style = prompt.style();` is the first statement in `run`**, above the
  preconditions. Nothing below branches on it except for weight.
- **The generated-passphrase emphasis is applied in place, at the existing
  `prompt.say(...)` line.** It assumes nothing about whether that line runs: if
  your join path skips generation, the styled line disappears with it and
  nothing else has to change. The only test that reads it
  (`the_steps_and_the_two_lines_that_matter_carry_the_only_weight`) drives the
  fresh-bundle path, so it will need the same branch you give step 3.
- Step 4's `render_dry_run` call became `render_dry_run_styled(…, style)` — same
  report, same figures, same position.
- The step-1 category question, step 4's confirmation and step 5's ordering are
  untouched.

Two small local helpers were added at module scope, `step(style, marker, text)`
and `under(style, text)`. Both are pure formatters that know nothing about which
step they are in or what comes next.

## Production call sites of everything added

The brief asked for this explicitly. Every item added, and where production
calls it:

| item | production call sites |
|---|---|
| `display::color_enabled` | `cli::style_of` |
| `display::color_enabled_with` | `display::color_enabled` |
| `report::Style` + `PLAIN` | `SetupPrompt::style` default, `Terminal::to`, `Plain::phase` |
| `Style::color` | `cli::style_of` |
| `Style::is_on` | `report::sentence`/`sentence_with`/`note`/`excluded_note`/`Widths::of`, `progress::render_styled`/`render_phase` |
| `Style::sgr` | the five palette methods |
| `Style::dim` | 20+ sites across `report.rs`, `setup.rs`, `progress.rs`, `cli.rs` |
| `Style::bold` | `row_bold`, `sentence_with`, `note`, `render_styled`, `render_phase`, `setup::run` |
| `Style::head` | `setup::step`, `setup::run`'s passphrase line |
| `Style::bad` | `render_repo`, `note`, `cli::refuse`, `setup::run`'s strength refusal |
| `Style::good` | `render_repo`, `setup::run`'s step 2 |
| `report::reflow` | `sentence`, `note`, `setup::step`/`under`/`run` |
| `report::wrap` | `reflow`, `sentence_with`, `excluded_note` |
| `report::render_status_styled` | `cli::status_with` |
| `report::render_dry_run_styled` | `cli::dry_run`, `setup::run` step 4 |
| `report::field` | `render_status_styled`, `render_repo` |
| `report::sentence` / `sentence_with` | `render_dry_run_styled`, `rebuilt_note` |
| `report::Widths::of`, `row`, `row_bold`, `pad_left` | `report::table` |
| `report::Weight` | `note` |
| `progress::Progress::phase` | `push::run` (×2) |
| `progress::bar` | `render_styled` |
| `progress::render_styled` | `Terminal::line` |
| `progress::render_phase` | `Terminal::phase`, `Plain::phase` |
| `progress::Terminal::styled` | `progress::reporter` |
| `progress::visible_width` | `Terminal::rewrite` |
| `setup::SetupPrompt::style` | `setup::run` |
| `setup::TtyPrompt::new` | `cli::setup`, `cli::rekey` |
| `setup::step` / `under` | `setup::run` |
| `cli::style_of` / `stdout_style` / `stderr_style` | `cli::status`, `cli::dry_run`, `cli::setup`, `cli::rekey`, `cli::push_with_parts`, `cli::refuse`, `cli::open_keyfile` |

**Zero-call-site count: zero.** One item was removed for exactly this reason
during the work: `Style::COLOR` existed as a companion to `Style::PLAIN`, was
used by ten tests and by nothing in production, and is gone — the tests say
`Style::color(true)`. The comment where it used to be says why there is no
constant there.

Two pre-existing public functions, `render_status` and `render_dry_run`, now
have no production caller: `cli.rs` and `setup.rs` both pass a `Style` (which
may be `PLAIN`). They are kept deliberately — they are the executable definition
of the plain contract, and `styling_changes_nothing_at_all_when_it_is_off`
asserts the styled renderers agree with them byte for byte. Deleting them would
churn ~40 existing assertions and remove the thing the byte-identity guarantee
is stated against.

## Verification

```
cargo test                                  1680 lib + 61 integration, 0 failing
cargo clippy --all-targets -- -D warnings   clean
cargo fmt --check                           clean
make test                                   + GNOME, KDE and Omarchy JS suites pass
```

Baseline before this plan was 1664 lib tests (the brief's 1724 did not match the
tree); +16 added here — 8 in `report.rs`, 5 in `progress.rs`, 2 in `setup.rs`, 1
in `display.rs`. `Cargo.toml` and `Cargo.lock` are unchanged.

Also checked by hand, against a build of `main`:

- `ai-usagebar sync status` piped — `diff` identical.
- `ai-usagebar sync status --json` — `diff` identical.
- The same commands under a pty (`script -q /dev/null`) — styled.
- `NO_COLOR=1` under a pty — zero escape bytes in the output.
- `sync push --dry-run` under a pty with a wrong password — the refusal renders
  bold-headed and dim-bodied, and wraps at a space.

## Self-Check: PASSED

- `src/sync/report.rs`, `src/sync/push/progress.rs`, `src/sync/github/setup.rs`,
  `src/sync/cli.rs`, `src/display.rs`, `src/sync/push/mod.rs` — all present and
  modified.
- Commit `3aa75e0` — present on `gsd/6-06`.
