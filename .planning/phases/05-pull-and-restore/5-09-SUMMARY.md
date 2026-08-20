---
phase: 05-pull-and-restore
plan: 09
subsystem: sync/restore
tags: [security-remediation, case-folding, hostile-input, structural-guard]
status: complete
requires: [5-01, 5-03, 5-04, 5-06, 5-SECURITY]
provides: [sync::FixedName, display::sanitize_untrusted_line, display::sanitize_untrusted_path]
affects: [src/sync/scope.rs, src/sync/restore/merge.rs, src/sync/restore/layout.rs, src/sync/restore/write.rs, src/error.rs]
findings_closed: [F-1, F-2, F-3, F-4, NEW-1, NEW-2]
threats_closed: [T-5-23, T-5-60, T-5-02, T-5-50]
negative_controls_run: 7
metrics:
  duration: one session
  completed: 2026-08-20
---

# Phase 5 Plan 09: Security Remediation Summary

Closed all three blocking findings of the Phase 5 audit, plus F-4 and both
unnamed findings. Three atomic commits, each independently green.

---

## Signature changes — read this first

Two, both `pub`/`pub(crate)` and both compile-checked across the crate:

| Symbol | Was | Is |
|---|---|---|
| `sync::guard::production_code` | `fn(&str) -> &str` | `fn(&str) -> String` |
| `error::AppError::Io`'s `Display` | `io error at {path}: {source}` via `Path::Display` | same text, path rendered through `display::sanitize_untrusted_path` |

`production_code`'s return type changed because the result is no longer a
contiguous slice of the input — comments are removed before the test-module
marker is looked for. Both call sites (`passphrase.rs`, and `github/gate.rs`'s
two hand-rolled splits, now routed through the helper) were updated; the only
caller-visible consequence is binding the `String` before `.lines()`.

`AppError::Io`'s rendered *text* is unchanged for every ordinary path — the
sanitizer only removes control characters and bidi overrides. No test needed
adjusting.

New public API: `display::sanitize_untrusted_line` and
`display::sanitize_untrusted_path`. New crate-internal type: `sync::FixedName`,
with `sync::scope::CREDENTIAL_FILE` moved out of `restore/merge.rs`.

---

## F-1 — CRITICAL — the credential gate defeated by one capital letter

`merge::credential_bearing` compared a basename to `".credentials.json"`
byte-exactly. The audit's attack, confirmed reproducible on this machine
before any code was written (`echo STALE > .Credentials.json` beside
`.credentials.json` left one file, still lowercase-named, containing `STALE`):
a manifest naming `.Credentials.json` passed `accept_for_write` and
`from_manifest_path`, `symlink_metadata` found the **live** credential because
the kernel folds case, and the classifier said "not a credential" — so
`decide` skipped the gate arm, returned `Overwrite`, `write::apply`'s tripwire
had no variant to fire on and the CLI filtered for one that no longer existed.
`--force` alone reverted a live OAuth token.

**Fixed as a class, not as an instance.** See the shared fix below.

### The test that had to cross a seam

The audit's diagnosis of *why* no test caught it was the specification for the
new one. `merge.rs:566` called `decide()` with the `credential` bool hardcoded
true — proving the arm works, never that it is *reached*. `merge.rs:687`
classified eight fixtures all spelled one way. Each correct alone; nothing ran
from a **manifest string** to a **disposition**, which is the only place the
defect lived.

So `a_credential_reaches_the_second_consent_however_the_manifest_spells_it`
goes through `merge::plan` end to end, four spellings, with the audit's own
flags (`force=true, force_credentials=false`). It seeds at the manifest's
spelling, so it proves the classification crosses the seam on a case-sensitive
volume too — the assertion is platform-independent, not "passes on the
maintainer's Mac".

The audit's PoC A3 was *also* run verbatim on this real case-insensitive
volume — live file lowercase, manifest capitalised, asserting
`canonicalize(dest) == canonicalize(live)` to prove the premise still holds —
and confirmed the disposition is now `NeedsCredentialConfirm`. Removed
afterwards; the committed test is the platform-independent one.

## F-2 — HIGH — D4's exclusion list folded the same way

`scope::is_excluded` admitted `Bridge-State.json`, `Ant-Device-Registry.json`,
`Backups/` and `Local-Agent-Mode-Sessions/` — the device-identity state D4
exists to keep off the machine — with no `--force` and no consent of any kind.

## The shared fix: a type, so the next one does not compile

The audit asked for one normalising comparison rather than three patches and a
fourth bug later. A per-call-site `to_lowercase()` is exactly the shape that
produced F-4: a lesson that reaches the call sites that existed and none that
came after.

`sync::FixedName` is a newtype over `&'static str` with **no** `PartialEq<str>`,
**no** `Deref<Target = str>` and **no** accessor handing back a comparable
`&str`. `name == CREDENTIAL_FILE` and `EXCLUDED_NAMES.contains(&name)` do not
compile. The only way to ask is `matches` / `is_prefix_of` / `is_suffix_of`,
and they fold. **A future byte-exact comparison is a build failure, not a test
failure, and not a shipped hole.** Verified as a negative control.

Folding is `str::to_lowercase` (full Unicode, so U+212A KELVIN SIGN → `k` —
which `backups` and `.lock` both need) plus an explicit U+017F LONG S → `s`,
the only other code point that folds onto a single ASCII character. ASCII
candidates take an allocation-free `eq_ignore_ascii_case` path, so the
200,000-entry walk does not slow down.

Routed through it: `scope::is_excluded`'s four rules, `scope::collect`'s
credential retain filter, and `merge::credential_bearing`. `CREDENTIAL_FILE`
now has one home in `scope`, shared by the collector and the restore gate —
two copies of that literal is how one of them ends up case-sensitive.

**Deliberately left byte-exact:** `layout::ROOT_PREFIXES`. A prefix that does
not match is a *refusal*, so folding there would only admit more bundles, and
the push side emits exactly one spelling. A comment says so, because it is the
kind of thing a future reader would otherwise "fix". Likewise
`merge::category_of`'s sub-path checks: category is report grouping only, and
an unknown root prefix is refused by `from_manifest_path` before it matters.

The widening is safe in both directions, as the audit noted: for `is_excluded`
more excluded, for `credential_bearing` more items asking the second consent.

## F-3 — HIGH — an attacker-chosen string printed with `{}`

`write.rs:155` was the one output site in the phase that used `{}` on a
verbatim remote string — no `{:?}`, no `sanitize_untrusted_field` — landing on
stderr right after the report the user had just consented to.

Fixed at both halves *and* at the root:

- The line is now a testable `stopped_line`, sanitising the manifest path and
  the rendered error alike.
- **`AppError::Io`'s own `Display` renders its path through the sanitizer.**
  That is the root-cause half: this error reaches stderr from `cli.rs:52`,
  `:365`, `:383` and every `?` on the restore path, and its `PathBuf` is built
  component-by-component out of the same manifest string. One escape in the one
  `Display` they all go through beats an escape in each print site that
  happened to remember. The audit raised this as "consider"; it is one line and
  restore is unlikely to be the last code to render an attacker-influenced path.
- `display::sanitize_untrusted_line` is the shared rule;
  `restore::report::safe` is now a delegate rather than a second copy.

## F-4 — MEDIUM — a structural guard blind to 397 lines

`guard::production_code` split on the first *textual* `#[cfg(test)]`, which in
`pairing.rs` is prose at line 76 — hiding five production functions from the
T-5-66 guard, which the audit proved by injecting an env read there and
watching it pass.

Fixed **structurally rather than with a smarter marker search**, as asked. A
line-anchored marker keeps the same shape and waits for the next doc comment.
Removing comments *before* looking for the marker makes it unambiguous by
construction: prose is no longer part of the text being searched, so prose
cannot truncate code. Both other hand-rolled splits (`github/gate.rs` ×2) now
route through the helper, so no guard in the crate still carries the flaw.

Two regression tests: one on a synthetic source with the marker in a doc
comment, one naming `pairing.rs`'s five real functions — the file the blind
spot was actually in, checked by name rather than in the abstract.

---

## The two unnamed findings

**NEW-1 (closed).** `from_manifest_path` bounded eight shapes and zero sizes.
Now 1024 bytes total, 32 components, 255 bytes per name — far above anything
the push side emits, far below `PATH_MAX` once a root is prepended. The length
check runs **first and does not echo its input**, which is what makes every
other refusal message bounded too: a 32 MiB entry cannot become a 32 MiB error
string on its way to a terminal. That closes F-3's delivery mechanism at the
source rather than trusting every print site.

**NEW-2 (closed, rather than justified away).** T-5-36 re-ran the *path* half
of the layout gate at the write boundary; `accept_for_write` — the *policy*
half — was re-run nowhere. There is no live hole today: the plan is an
in-process `Vec` between the two points and nothing mutates it. Closed anyway,
because `write.rs`'s module doc claims the preflight is "defence in depth
against a plan mutated between planning and applying", and a plan mutated that
way could carry an `ExcludedByPolicy` path promoted to a writing disposition.
The cost is one list comparison per item. **It was cheaper to close than to
keep explaining** — leaving it would have meant two halves of one gate with
different postures and a module doc that overclaims.

---

## Negative controls — seven run, seven red or non-compiling

Every injection made in `src/`, the named test run, the edit reverted.

| # | Injection | Guard | Result |
|---|---|---|---|
| 1 | `credential_bearing` back to byte-exact | `a_credential_reaches_the_second_consent_…` + the classification test | **RED ×2** |
| 2 | `is_excluded` back to byte-exact | `every_machine_bound_name_is_excluded_in_every_spelling_…` + `machine_bound_state_is_refused_however_the_bundle_capitalises_it` | **RED ×2** |
| 3 | the failure line back to raw `{}` | `the_failure_line_escapes_the_manifest_path_and_the_error_alike` | **RED** |
| 4 | `std::env::var("SYNC_PASSWORD")` at `pairing.rs:79` — **the audit's NC-J, the one that stayed green** | `no_password_input_path_reads_the_process_environment` | **RED** |
| 5 | all three size bounds disabled | `every_hostile_spelling_is_refused_with_its_own_message` | **RED** |
| 6 | the policy re-check disabled at the write boundary | `a_machine_bound_path_promoted_to_a_write_is_refused_before_the_first_byte` | **RED** |
| 7 | `EXCLUDED_NAMES.contains(&name)` reintroduced | the type system | **DOES NOT COMPILE** |

`git status --porcelain` is empty at the end of this report — checked
explicitly, and no `.ncbak` or `false &&` residue remains in `src/`.

---

## Gates

| Gate | Result |
|---|---|
| `cargo test` | **pass** — 1573 lib + 60 integration, 0 failed, 16 ignored (the live/calibration set) |
| `cargo clippy --all-targets -- -D warnings` | **pass** — clean |
| `cargo fmt --check` | **pass** — clean |
| `make test` | **pass** — incl. GNOME, KDE and Omarchy JS contract suites |
| `./macos/run-tests.sh` | **pass** — 239 assertions |
| `git diff f0d7a4c..HEAD -- Cargo.toml Cargo.lock` | **empty** — no new crates |

Baseline was 1562 lib / ~1622 total. **+11 lib tests**, 0 failing: 4 in
`sync/mod.rs` (the type and the guard), 3 in `scope.rs`, 1 in `merge.rs`,
1 in `layout.rs`, 2 in `restore/write.rs`.

All tests remain hermetic per CLAUDE.md — every new test uses `SyncRoots::at`
or a `TempDir`, none reads a real `$HOME`, an env var, or the wall clock.

---

## Commits

| Commit | Findings |
|---|---|
| `e97ee77` | F-4 — `production_code` drops comments before it looks for the marker |
| `7fcde9d` | F-1, F-2, NEW-1 — `FixedName`, and size bounds at the hostile-input boundary |
| `1c95517` | F-3, NEW-2 — the failure line, `AppError::Io`'s `Display`, the policy re-check |

Ordered so each compiles and is green on its own; `mod.rs` carries both the
type and the guard fix, so the split is by defect rather than by file.

---

## Known stubs

None. No stub, skipped test, or unrun `<verify>` was introduced.

## Ceiling worth knowing about

`FixedName` closes **case folding**, which is the reported defect. Windows'
*other* name canonicalisations — trailing dots and spaces, 8.3 short names,
`:` alternate data streams — are a separate class it does not close. The sync
feature's users are macOS and Linux, and the ADS/trailing-dot variants do not
reach a live credential's main stream. Recorded in the type's own doc comment
with the upgrade path (normalisation at `layout`'s boundary, not more cases in
`fold`) so it is a decision rather than an oversight.

## Still outstanding for the phase

The three `human_needed` items in `5-VERIFICATION.md` are untouched by this
plan and still stand — chiefly the real two-machine flow against a real private
repo. **That is the run this remediation matters for:** the user restoring onto
a second MacBook is precisely the case-insensitive volume F-1 and F-2 were
exploitable on.

## Self-Check: PASSED

Every file and every commit hash claimed above was verified present at the end
of this plan. `git diff --diff-filter=D f0d7a4c..HEAD` is empty — nothing was
deleted. Working tree clean.
