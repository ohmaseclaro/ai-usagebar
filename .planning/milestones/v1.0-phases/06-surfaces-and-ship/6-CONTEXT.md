# Phase 6 Context — Surfaces and Ship

**Decisions locked by the orchestrator.** This phase is deliberately routine: the CLI already
works by now, and these are additive surfaces over it.

## Locked decisions

### D1 — Surfaces call the CLI; they do not reimplement sync

The macOS menu bar shells out to `ai-usagebar sync …` and parses its JSON, exactly as it
already does for `account status --json` and `usage --json`. No sync logic in Swift, no second
implementation of the crypto or the transport. This is the project's standing "frontend adapters
stay thin" invariant, and it is what keeps one code path under test.

### D2 — Non-interactive by construction

A menu-bar subprocess cannot prompt. Anything needing a password or a confirmation is either
(a) handled by a already-unlocked in-process key for the session, or (b) refused with a clear
"run this in a terminal" message — never a silent hang waiting on stdin. The account-switch
deletion prompt already established this pattern: no terminal ⇒ take the safe branch and say so.

### D3 — The exit-0 invariant is the widget's, not sync's

`ai-usagebar sync …` returns non-zero on failure — scripts depend on that. The **widget** must
still exit 0 with a fallback payload no matter what sync did, because Waybar hides modules that
don't. These are not in tension; they are different binaries' contracts, and a test should pin
the widget side.

### D4 — Sync state is surfaced, not just actions

The menu bar shows last-sync time and whether local changes are pending, because a backup nobody
can see the staleness of is a backup nobody trusts. A stale or failed sync is visible without
opening a terminal.

### D5 — GNOME, KDE and Omarchy are explicitly out of scope

Each is an independent frontend with its own contract test suite. Adding sync to all of them
triples the surface for no additional proof that the feature works. The Rust CLI is the
portable path for those users; revisit once the macOS surface has shipped and settled. Recorded
as a deliberate exclusion so a later reader does not read it as an oversight.

## Constraints inherited from the codebase

- Swift changes go with their harness tests (`macos/ai-usagebar-tests.swift`), pure functions
  only — the existing pattern.
- `make test` must stay green, including the desktop and plugin contract suites.
- No new dependency in the Swift binary; it is a single file built with `swiftc -O
  -parse-as-library`.
- Release checklist in `CLAUDE.md` applies if this phase cuts a version — both `Cargo.toml` and
  the Omarchy `manifest.json` versions, CHANGELOG, both PKGBUILDs, and **both `.SRCINFO`s
  regenerated before tagging** (v0.17.0 never shipped for exactly that omission).
