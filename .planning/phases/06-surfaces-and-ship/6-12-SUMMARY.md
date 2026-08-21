---
phase: 6
plan: 12
subsystem: sync
tags: [ux, progress, terminal, restore, passphrase]
requires:
  - 4-03 (the push progress reporter this extends rather than duplicates)
  - 6-06 (report::Style, and the env read kept outside src/sync/)
provides:
  - "sync pull: the key derivation, the pack download and the write all report"
  - "an ETA on the pack download, from a measured rate and an injected clock"
  - "a flushed, unterminated password prompt marker at the one read all four asks share"
affects:
  - src/sync/push/progress.rs
  - src/sync/restore/fetch.rs
  - src/sync/restore/write.rs
  - src/sync/restore/mod.rs
  - src/sync/passphrase.rs
  - src/sync/github/setup.rs
  - src/sync/cli.rs
tech-stack:
  added: []
  patterns:
    - "one reporter, parametrised by a Stage (verb + noun), rather than a second vocabulary"
    - "Clock as an injected enum (Monotonic | Fixed) so an ETA is asserted exactly and no test reads a clock"
    - "an ETA flag per stage, off by default, on only where remaining work and rate share a unit"
    - "prompt marker at the shared reader, with the stream supplied by the caller"
key-files:
  created: []
  modified:
    - src/sync/push/progress.rs
    - src/sync/restore/fetch.rs
    - src/sync/restore/write.rs
    - src/sync/restore/mod.rs
    - src/sync/restore/merge.rs
    - src/sync/passphrase.rs
    - src/sync/github/setup.rs
    - src/sync/cli.rs
    - tests/sync_restore_e2e.rs
decisions:
  - "Extended push::progress rather than writing a restore reporter: the shapes did not differ, only the verb and the noun did."
  - "The download bar's byte total is the release listing's, not plan.bytes_to_fetch — the latter is computed after the download and counts a different quantity."
  - "Per pack, not per byte: download_asset buffers a whole asset, so there is no hook inside a 32 MiB pack and none is faked."
  - "ETA on the download only. Upload would change output 6-06 pinned; write is dominated by per-file syscalls, so a bytes-based rate would describe neither the 400 small items nor the one 2 GiB one."
  - "The prompt marker lives in passphrase::read_line and its stream is the caller's, because sync pull prompts on stderr and sync setup on stdout."
metrics:
  duration: ~2h
  completed: 2026-08-21
status: complete
---

# Phase 6 Plan 12: `sync pull` says what it is doing, and the password prompt looks like one — Summary

A restore of 2.1 GiB used to run under a terminal with nothing on it; now the
key derivation, each downloaded pack and each written item report, with an ETA
on the one stretch that has a measurable rate. Separately, the password read
now draws a flushed `password: ` so a blocking read stops reading as a hang.

## Signature changes

Stated first.

| Symbol | Change |
|---|---|
| `push::progress::Stage` | **new** `{ verb, noun, eta }`, with `UPLOAD` / `DOWNLOAD` / `WRITE` |
| `push::progress::Progress::stage(Stage, usize, u64)` | **new**, defaulted to `start` — every existing impl keeps compiling |
| `push::progress::render` | `(done, total, bd, bt)` → **`(Stage, done, total, bd, bt)`** |
| `push::progress::render_styled` | same, `Stage` prepended |
| `push::progress::Clock` | **new** `Monotonic(Instant) \| Fixed(Duration)` |
| `push::progress::{eta, human_left, render_left}` | **new**, pure |
| `Terminal::clocked` / `Plain::clocked` | **new** injection seams |
| `restore::run(ctx)` | → **`run(ctx, &mut dyn Progress)`** |
| `restore::fetch::resolve(ctx, anchor)` | → **`resolve(ctx, anchor, &mut dyn Progress)`** |
| `restore::write::apply(ctx, plan, packs)` | → **`apply(ctx, plan, packs, &mut dyn Progress)`** |
| `passphrase::read_line(r)` | → **`read_line(r, Option<&mut dyn Write>)`** |
| `passphrase::MARKER` | **new** `"password: "` |
| `github::setup::TtyPrompt::new(style)` | → **`new(style, interactive)`** |
| `cli::sync_password_from(r)` | → **`sync_password_from(r, Option<&mut dyn Write>)`** |
| `cli::PullIo` | **new field** `progress: &'a mut dyn Progress` |

## Before / after — one real screen

Captured by driving the shipped `Terminal` and `Plain` writers through
`Terminal::to(writer)` with `Clock::Fixed`, not typed by hand. (The ETA shrinks
across these lines because the fixed clock holds elapsed at 64 s; a real run's
elapsed grows with it.)

**Before**, a 2.1 GiB restore, from the user's own session:

```
$ ai-usagebar sync pull --apply
The sync password for this bundle. It is echoed — this build has no hidden-input dependency.
▏
```

…and then, after the password, nothing at all until the report — through the
~1.5 s key derivation, through ~880 MiB of packs, through 431 writes.

**After**, terminal (one line rewritten in place; shown expanded):

```
deriving the sync key (Argon2id)…
[░░░░░░░░░░░░░░░░░░░░░░░░]   0% downloading 0/19 packs — 0 B of 880.0 MiB
[░░░░░░░░░░░░░░░░░░░░░░░░]   3% downloading 1/19 packs — 32.0 MiB of 880.0 MiB — ~28m 16s left
[█░░░░░░░░░░░░░░░░░░░░░░░]   7% downloading 2/19 packs — 64.0 MiB of 880.0 MiB — ~13m 36s left
[██░░░░░░░░░░░░░░░░░░░░░░]  10% downloading 3/19 packs — 96.0 MiB of 880.0 MiB — ~8m 42s left
[███░░░░░░░░░░░░░░░░░░░░░]  14% downloading 4/19 packs — 128.0 MiB of 880.0 MiB — ~6m 16s left
```

**After**, piped (`Plain`, no `\r`, no escape byte, one line per pack):

```
deriving the sync key (Argon2id)…
downloading 0/19 packs — 0 B of 880.0 MiB
downloading 1/19 packs — 32.0 MiB of 880.0 MiB — ~28m 16s left
downloading 2/19 packs — 64.0 MiB of 880.0 MiB — ~13m 36s left
writing 0/431 items — 0 B of 2.1 GiB
writing 1/431 items — 700.0 MiB of 2.1 GiB
writing 1/431 items — 700.0 MiB of 2.1 GiB — done
```

**After**, the password prompt:

```
The sync password for this bundle. It is echoed — this build has no hidden-input dependency.
password: ▏
```

## The one shape that genuinely differed, and what was done about it

The brief asked that a real difference be named rather than duplicated quietly.
There is one, and it is **the download's byte total**.

`RestorePlan::packs_needed` and `RestorePlan::bytes_to_fetch` exist, but neither
can drive the bar:

- They are produced by `merge::to_fetch`, which runs in `restore::run` **step 3**
  — *after* `fetch::resolve` in step 2 has already downloaded everything. A total
  that arrives after the work is not a progress total.
- `bytes_to_fetch` sums the index's per-chunk `clen`, i.e. sealed chunk payload.
  The bar measures **pack asset bytes**, which carry the pack header and framing
  on top. They are different quantities and would disagree by a few percent.

What the bar uses instead is the figure that *does* exist before the first
request: the release listing's `asset.size`, which `fetch_packs` already sums
into `round_bytes` to check its own two ceilings. Per-pack progress reports what
actually arrived (`bytes.len()`), not what the remote declared — a bar that
reaches 100% on a remote-chosen number is reporting the remote.

Everything else reused `push::progress` verbatim: the counters, the `\r`
rewrite, the visible-width padding, `NO_COLOR`, the `Plain`-when-piped shape and
the `reporter(is_terminal, style)` choice. `UPLOAD` reproduces push's line byte
for byte, which the existing push tests still pin.

## Honesty of the ETA

Enabled on `DOWNLOAD` only, and refused in all three cases where the arithmetic
would invent a number:

- no bytes moved yet — no rate to divide;
- nothing left — no remainder;
- under a second elapsed — a rate measured over a fraction of a second and
  multiplied by 880 MiB is noise wearing a number's clothes.

`WRITE` has measured bytes and declines anyway: a write's cost is dominated by
the per-file syscalls, and 400 small items plus one 2 GiB one give an average
that describes neither. `UPLOAD` declines because changing push's line would
move output 6-06 already pinned under a real pty, for a stretch nobody reported
waiting blind through. Both are one flag from having one.

## Constraints held

- **Piped output unchanged.** `sync status --json` never reaches either change:
  its `local_keyfile(_, false)` refuses on a terminal before any password read,
  and piped it passes `marker: None`. Progress is stderr-only on every path.
- **`Plain` for pipes.** The reporter is chosen from `stderr().is_terminal()`,
  read at the CLI and injected — `pull_with_parts` takes it through `PullIo`
  beside `gate`, so the tested seam still needs no terminal.
- **`NO_COLOR` and `report::Style`.** The ETA clause goes through the same
  `Style` as the bar; `no_color_reaches_the_eta_clause_as_well_as_the_bar` pins
  that the styled writer emits not one escape byte under it. No `std::env` was
  added anywhere under `src/sync/` — `no_password_input_path_reads_the_process_environment`
  walks the whole subtree and still passes.
- **No name on a line.** Pack names are content addresses; item names are
  attacker-chosen manifest paths. Neither is rendered — the reporter takes the
  name and drops it, and
  `an_attacker_chosen_manifest_path_never_reaches_a_progress_line` drives a path
  carrying `\x1b[2J`, a BEL and a fake `github_pat_` through both writers and
  asserts none of it lands. Nothing therefore needed
  `sanitize_untrusted_field`; had a name been shown it would have.
- **A dying pull leaves a usable terminal.** `restore::run` is a wrapper that
  calls `finish` on **both** arms, so every `?` below it still ends the line. One
  rewriting line, no alternate screen, no cursor hiding — asserted.
- **No crate added.** `Cargo.toml` and `Cargo.lock` are byte-identical
  (`git diff --stat` on both: empty).

## The password marker

Placed in `passphrase::read_line` — the one door `sync pull`, `sync push`'s
terminal ask, `sync setup`'s two asks and `sync rekey`'s two asks all go
through — so the next caller cannot forget it. It is not optional to *answer*,
only to be `None`: the parameter has no default, so a new password reader has to
say which stream its prompt went to or that it has none.

The stream is the caller's on purpose. `sync pull` prints its prompt sentence to
**stderr** and `sync setup`/`sync rekey` print theirs to **stdout**; a marker
hard-coded to one of them would split half of every prompt into the redirected
file. `TtyPrompt` gained an `interactive` flag separate from `style`, because
`NO_COLOR` at a real keyboard must not take the marker with it.

## Production call sites of everything added

Enumerated as asked. Two have none:

| Added | Production call sites |
|---|---|
| `Stage`, `UPLOAD` | `Counters::default()` (via `Stage::default`), `render`, `render_styled` |
| `DOWNLOAD` | `restore::fetch::fetch_packs` |
| `WRITE` | `restore::write::apply` |
| `Progress::stage` | `fetch_packs`, `write::apply` |
| `Clock`, `Clock::default` | `Counters::default` |
| `Clock::elapsed` / `Clock::restart` | `Counters::left` / `Counters::start` |
| `eta` | `Counters::left` |
| `human_left` | `render_left` |
| `render_left` | `Counters::line`, `Terminal::line` |
| `Counters::begin` | `Terminal::stage`, `Plain::stage` |
| `Terminal::end_line` | `Terminal::stage`, `Terminal::finish` |
| `passphrase::MARKER` | `passphrase::read_line` |
| `TtyPrompt::marker` | `TtyPrompt::passphrase`, `TtyPrompt::existing_passphrase` |
| **`Terminal::clocked`** | **none — test-only injection seam** |
| **`Plain::clocked`** | **none — test-only injection seam** |

The two `clocked` seams are deliberate and are the project's convention
(`Cache::at` beside `Cache::for_vendor`): the brief asked for the clock to be
injected rather than read, and production takes `Clock::default()`. They exist
so `an_eta_is_refused_wherever_there_is_no_rate_to_divide` and the two writer
tests assert an exact `~30s left` instead of a fuzzy one. They are reported here
because a `pub fn` with no production caller is worth naming either way.

## Tests

11 new, all hermetic — `TempDir`, mockito, injected clock, injected writer, no
network and no wall clock.

| Test | Where | What it pins |
|---|---|---|
| `the_three_stages_differ_by_a_verb_and_a_noun_and_nothing_else` | `push::progress` | one vocabulary; `Stage::default() == UPLOAD` |
| `a_restores_stages_each_end_their_own_line_and_the_download_carries_an_eta` | `push::progress` | three kept lines, exact `~30s left`, no alt screen, ends on a fresh line |
| `an_eta_is_refused_wherever_there_is_no_rate_to_divide` | `push::progress` | the three refusals + `human_left` shapes |
| `a_piped_restore_gets_the_plain_shape_and_no_escape_bytes` | `push::progress` | no `\r`, no escape, no bar |
| `no_color_reaches_the_eta_clause_as_well_as_the_bar` | `push::progress` | `NO_COLOR` reaches the clause 6-12 added |
| `an_attacker_chosen_manifest_path_never_reaches_a_progress_line` | `push::progress` | T-4-25 for the write stage, both writers |
| `every_slow_stretch_of_a_restore_reports_and_the_key_derivation_reports_first` | `restore` | the whole call sequence through a real mockito restore; derive first, download before write, one `done` per announced item, `finish` last |
| `a_dry_run_narrates_only_what_it_actually_does` | `restore` | no write stage a dry run will never enter |
| `an_interactive_read_draws_an_unterminated_marker_and_a_piped_one_draws_nothing` | `passphrase` | marker present, flushed, no trailing newline; piped draws nothing |
| `nothing_after_the_marker_carries_the_password` | `passphrase` | the prompt stream holds the marker and nothing else |
| `a_terminal_gets_the_prompt_marker_on_the_same_stream_and_a_pipe_gets_nothing` | `sync::cli` | the marker is on the stream `ECHOED_PROMPT` used |

`the_archive_is_taken_before_the_first_byte_is_written` — the SAFE-04 ordering
guard — was moved from reading `run`'s source to reading `restore`'s, because
the seven steps moved into `restore` under the new `finish`-on-both-arms
wrapper. It still asserts the same order over the same two markers.

## Verification

| Gate | Result |
|---|---|
| `cargo test` (macOS) | **1748 lib passing, 0 failing** (baseline 1737 + 11 new) |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `make test` | green, including the GNOME, KDE and Omarchy contract suites |
| Linux `clippy --all-targets -D warnings` (`rust:1.88`, `linux/amd64`) | clean |
| Linux `cargo test --lib` | **1739 passing, 1 failing** — the pre-existing one |

The Linux count is nine below the macOS one because `anthropic::keychain` and
`safe_storage` are `#[cfg(target_os = "macos")]` and never compile there. All
eleven tests this plan added were run and passed on Linux, checked by name
rather than by count. `supergrok::acp::tests::missing_binary_has_a_clear_non_secret_error`
fails on Linux on untouched `main`; it is pre-existing and not this plan's.

The gate was run against a **committed** tree: both commits were made before
any of these, and `git status` was clean but for this summary.

## Deviations from plan

**[Rule 3 — blocking] The plan's stated source for the download total does not
exist at download time.** The brief said "the plan already knows `packs_needed`
and `bytes_to_fetch`". It does — but only in step 3, after step 2 has done the
downloading, and it counts chunk `clen` rather than pack asset bytes. Rather
than reporting a total the code does not have when it needs it, the bar uses
`fetch_packs`'s own `round_bytes`, which is already computed from the release
listing before the first request in order to enforce two ceilings. Documented on
`fetch_packs` and above.

**[Rule 3 — blocking] Two commits, not one per concern more finely.** The two
defects share `src/sync/cli.rs`, and any split finer than the two shipped would
have produced an intermediate commit that does not compile (`cli.rs` calling a
2-argument `read_line` against a 1-argument `passphrase.rs`, or the reverse).
The `cli.rs` diff was split by hunk so each commit builds and tests green on its
own — 1745 passing at `11d64dd`, 1748 at `a247235`.

## Known stubs

None.
