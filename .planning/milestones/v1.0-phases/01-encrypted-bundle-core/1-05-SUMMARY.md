---
phase: 01-encrypted-bundle-core
plan: 05
subsystem: sync
tags: [passphrase, crockford-base32, argon2-adjacent, rollback, monotonic-counter, zeroize, tofu]

# Dependency graph
requires: [1-01]
provides:
  - "src/sync/passphrase.rs — generate-by-default passphrases, the 12-character floor, and the only two non-TTY password input paths"
  - "NO_RECOVERY and OFFLINE_ATTACK_NOTE — the plain-language consequence and threat text every password-setting surface prints"
  - "src/sync/anchor.rs — the monotonic rollback anchor, persisted mode-0600 into an injected path"
  - "anchor::accept — the pure (local anchor, remote claim) → rollback decision 1-06 drives adversarially"
affects: [1-06, 1-08, phase-2, phase-3, phase-4]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Password input paths are an enumerable set (reader, mode-0600 file, TTY prompt owned elsewhere), enforced by a source-scanning test that was negative-checked"
    - "The rollback decision is a pure function of its two inputs, so every branch is tested with no filesystem"
    - "Mode-0600 is asserted on an overwrite as well as on creation — a state file rewritten every fetch can lose its mode on the second write"

key-files:
  created: []
  modified:
    - src/sync/passphrase.rs
    - src/sync/anchor.rs

key-decisions:
  - "generate() emits exactly 100 bits, not the ~94 the plan estimated — 13 CSPRNG bytes are 104 bits and the 20 characters consume the first 100 five at a time, so every character is an unbiased draw and the figure is exact"
  - "read_from_file stats the opened handle, not the path, so the file inspected is the file read (no TOCTOU window between the mode check and the open)"
  - "read_from_file delegates its newline handling to read_line rather than duplicating the strip, so a password ending in a space behaves identically from a pipe and from a file"
  - "write_to reuses crate::cache::atomic_write instead of a fresh NamedTempFile::new_in — same tempfile-in-destination-dir + persist mechanism, but the project's one atomic-write helper rather than a second implementation of it"
  - "The env/argv gate scans only the shipped lines (everything before the first #[cfg(test)]), which lets the test name its own needles and use env!(\"CARGO_MANIFEST_DIR\") without self-triggering"
  - "The offline-attack note is jargon-tested: a test fails if it contains 'entropy', 'Argon2', 'KDF', 'bits of', or 'brute-force', because a user who understands 'they can guess forever, offline' chooses better than one told a rule"
  - "A repo_id mismatch errors even when allow_rollback is set — allow-rollback is an escape for an older snapshot of the same bundle, never for a counter borrowed from a different one"

patterns-established:
  - "Negative-checked invariant tests, continuing 1-01's precedent: the environment gate was verified to go red before being trusted"
  - "Consequence text lives as a public const next to the logic that enforces it, so no surface can enforce the floor without having the explanation to hand"

requirements-completed: [CRYPTO-05 (rollback half), CRYPTO-06]

coverage:
  - id: D1
    description: "A generated passphrase is 20 Crockford base32 characters, differs on every call, and every alphabet character is reachable"
    requirement: CRYPTO-06
    verification:
      - kind: unit
        ref: "cargo test --lib sync::passphrase — a_generated_passphrase_is_twenty_crockford_characters_and_never_repeats, every_alphabet_character_is_reachable"
        status: pass
    human_judgment: false
  - id: D2
    description: "Below 12 characters is refused; 12–19 warns and points at a six-word diceware phrase; 20+ is strong; length is counted in characters, not UTF-8 bytes"
    requirement: CRYPTO-06
    verification:
      - kind: unit
        ref: "cargo test --lib sync::passphrase — a_password_below_the_floor_is_refused, a_password_between_the_floor_and_the_recommendation_warns_about_diceware, twenty_characters_is_strong, length_is_measured_in_characters_not_utf8_bytes"
        status: pass
    human_judgment: false
  - id: D3
    description: "The no-recovery consequence is stated without hedging, and the offline-attack risk is explained in plain language rather than as a rule"
    requirement: CRYPTO-06
    verification:
      - kind: unit
        ref: "cargo test --lib sync::passphrase — the_no_recovery_text_states_the_consequence_without_hedging, the_offline_attack_note_explains_the_risk_in_plain_language"
        status: pass
    human_judgment: false
  - id: D4
    description: "A password survives a trailing space; only one newline (and a CR pair) is stripped; a second line is never merged in"
    requirement: CRYPTO-06
    verification:
      - kind: unit
        ref: "cargo test --lib sync::passphrase — reading_a_line_strips_one_newline_and_nothing_else"
        status: pass
    human_judgment: false
  - id: D5
    description: "A mode-0644 password file is refused and the error never echoes the password; a mode-0600 one is read"
    requirement: CRYPTO-07
    verification:
      - kind: unit
        ref: "cargo test --lib sync::passphrase — a_group_readable_password_file_is_refused_and_a_private_one_is_read"
        status: pass
    human_judgment: false
  - id: D6
    description: "No shipped line in either module takes a password from argv or the process environment"
    requirement: CRYPTO-07
    verification:
      - kind: unit
        ref: "cargo test --lib sync::passphrase — no_password_input_path_reads_the_process_environment (negative-checked: adding a std::env::var read to passphrase.rs makes it fail with a pointed message)"
        status: pass
    human_judgment: false
  - id: D7
    description: "A counter below the local high-water mark is refused unless rollback is explicitly allowed, and the refusal names the escape"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::anchor — a_lower_counter_is_refused_and_the_message_names_the_escape, a_lower_counter_is_accepted_when_rollback_is_explicitly_allowed, a_higher_or_equal_counter_is_accepted"
        status: pass
    human_judgment: false
  - id: D8
    description: "First contact has no anchor and is accepted as trust-on-first-use, documented as an inherent residual gap"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::anchor — first_contact_has_no_anchor_and_is_trusted"
        status: pass
    human_judgment: false
  - id: D9
    description: "A counter from a different bundle is never compared, at any counter value and even under allow_rollback"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::anchor — a_mismatched_repo_id_is_refused_at_any_counter"
        status: pass
    human_judgment: false
  - id: D10
    description: "The anchor round-trips through an injected path at mode 0600, an absent file reads as first contact, and a corrupt one errors rather than resetting the high-water mark"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::anchor — write_then_read_round_trips_and_the_file_is_owner_only, an_absent_anchor_is_first_contact_not_an_error, a_corrupt_anchor_errors_instead_of_resetting_the_high_water_mark, overwriting_an_existing_anchor_keeps_it_owner_only"
        status: pass
    human_judgment: false

# Metrics
duration: 25min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 05: Passphrase Policy and the Rollback Anchor Summary

**Two small modules either side of the crypto: how a password is chosen and how it enters the process, and how a replayed-but-authentic old snapshot is refused.**

## Performance

- **Duration:** ~25 min
- **Tasks:** 2/2
- **Files modified:** 2 (both were doc-only stubs from 1-01)
- **Test suite:** 20 new tests (11 passphrase + 9 anchor); the whole `sync::` tree is 38 tests in 0.02 s

## Accomplishments

- **Generate-by-default is real, not advice.** `generate()` draws 13 bytes from the OS CSPRNG and emits 20 Crockford base32 characters — an unbiased five-bit slice per character, no wordlist to ship, no modulo bias. A test draws 200 passphrases and asserts all 32 alphabet characters are reachable, which is what catches an off-by-one in the shift that would silently halve the keyspace.
- **The floor is explained, not just enforced.** `check()` refuses below 12 characters and warns below 20, and `OFFLINE_ATTACK_NOTE` says in words a user can act on that anyone holding a copy of the repository can guess forever with nothing on the other end to lock them out. A test *fails* if that text drifts into jargon (`entropy`, `Argon2`, `KDF`, `bits of`, `brute-force`); a companion test fails if `NO_RECOVERY` acquires a hedge (`may be`, `might`, `usually`, …).
- **Password input paths are an enumerable set, and the set is enforced.** A reader, a mode-0600 file, and a TTY prompt owned by the calling surface. `no_password_input_path_reads_the_process_environment` scans the shipped lines of *both* modules from `CARGO_MANIFEST_DIR` and was negative-checked — appending `std::env::var("PW")` to `passphrase.rs` makes it fail naming the needle and the sanctioned alternatives.
- **The rollback decision is pure and exhaustively covered.** `accept(local, remote_repo_id, remote_counter, allow_rollback)` takes the local anchor as an argument and touches nothing else. All five branches (first contact, equal, higher, lower, mismatched bundle) are tested with no filesystem at all; only the persistence tests need a `TempDir`.
- **A corrupt anchor is an error, never a reset.** `read_from` returns `Ok(None)` for a genuinely absent file and errors for a present-but-unparseable one, because treating corruption as first contact is exactly the free rollback the module exists to prevent.

## Task Commits

1. **Task 1: Passphrase generation, strength gate, and safe input paths** — `b5dc626` (feat)
2. **Task 2: The rollback anchor** — `c5128e8` (feat)

## Public surface

`src/sync/passphrase.rs`:

```rust
pub const MIN_CHARS: usize = 12;
pub const RECOMMENDED_CHARS: usize = 20;

/// Printed by every surface that sets a password, before it is set.
pub const NO_RECOVERY: &str;
/// Printed alongside NO_RECOVERY when the user supplies their own password.
pub const OFFLINE_ATTACK_NOTE: &str;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strength { Rejected(&'static str), Weak(&'static str), Strong }

pub fn generate() -> Result<Zeroizing<String>>;          // 20 chars, exactly 100 bits
pub fn check(pw: &str) -> Strength;                      // characters, not bytes
pub fn read_line(r: impl BufRead) -> Result<Zeroizing<String>>;
pub fn read_from_file(path: &Path) -> Result<Zeroizing<String>>;   // refuses any group/other bit
```

`src/sync/anchor.rs` — **the signature 1-06 drives the rollback adversarial case through**:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor { pub repo_id: String, pub counter: u64 }

/// Pure. `None` = first contact = trust-on-first-use (documented residual gap).
/// repo_id mismatch → always Err. remote_counter < local.counter → Err unless
/// allow_rollback; the message names `--allow-rollback`.
pub fn accept(
    local: Option<&Anchor>,
    remote_repo_id: &str,
    remote_counter: u64,
    allow_rollback: bool,
) -> Result<()>;

pub fn read_from(path: &Path) -> Result<Option<Anchor>>;   // Ok(None) only if absent
pub fn write_to(path: &Path, anchor: &Anchor) -> Result<()>;  // atomic, mode 0600
```

Neither module resolves a path. The thin wrapper naming the real anchor location belongs to whichever phase owns the config directory.

## Decisions Made

- **`generate()` is exactly 100 bits, not the ~94 the plan estimated.** 13 bytes is 104 bits; the 20 characters consume the first 100 of them five at a time, so each character is a uniform draw from a 32-character alphabet and the total is exact. The doc comment states 100 rather than repeating the plan's conservative figure — a number in a doc comment is a claim, and this one is checkable.
- **`read_from_file` stats the opened handle, not the path.** Checking the mode on the path and then opening it leaves a window in which the file inspected is not the file read. `File::open` first, then `file.metadata()`, closes it.
- **`read_from_file` delegates the newline strip to `read_line`.** One implementation of "strip a single trailing newline and nothing else", so a password ending in a space behaves identically whether it arrived on a pipe or from a file. `&[u8]` already implements `BufRead`; no adapter was needed.
- **`write_to` reuses `crate::cache::atomic_write`.** It is already `tempfile` in the destination directory plus `persist` — the exact mechanism the plan specifies and the invariant its `key_links` names — so a second implementation of it would be a second thing to keep correct. The explicit `set_permissions(0o600)` afterwards is the belt-and-braces the plan asks for, and matches `src/kiro/fetch.rs::write_persisted_oauth` and the Settings overlay.
- **The env/argv gate scans only shipped lines.** It splits each source at the first `#[cfg(test)]` and scans what precedes it. That lets the test spell out its own needles (`"env::var"`, `"clap"`, …) and use `env!("CARGO_MANIFEST_DIR")` to locate the sources without self-triggering, and it scopes the invariant correctly: the rule is about how a password reaches *production* code.
- **A `repo_id` mismatch errors even under `allow_rollback`.** The escape exists for a user who wants an older snapshot of the same bundle; a counter borrowed from a different bundle is not an older snapshot, it is a reset of the high-water mark by renaming.
- **`Anchor` derives `Debug`.** It holds a public repository identifier and a counter — no key material — so the manual-`Debug` rule (D5) does not apply. Nothing in either module's error text can carry a password: `Strength` holds only `&'static str` constants, and the two consequence texts are compile-time literals.

## Deviations from Plan

None in behaviour. Three implementation choices worth naming, all recorded above:

1. **`write_to` calls `crate::cache::atomic_write` rather than constructing `tempfile::NamedTempFile::new_in` inline.** Same mechanism, same guarantee, one implementation in the codebase instead of two.
2. **The generated passphrase is documented as 100 bits, where the plan says "roughly 94".** Arithmetic, not a design change.
3. **The env/argv gate skips the test module wholesale rather than relying only on the `//` prefix rule.** The `//` skip is still implemented and still applies to prose in the shipped half; the extra split is what lets the test name its own needles.

## Issues Encountered

One self-inflicted bad test on the first draft — an assertion comparing two independently generated passphrases for equality, which is exactly what `generate()` must *not* satisfy. Caught before the first run and replaced with the assertion that actually matters: a generated passphrase clears the gate it recommends.

## Verification

All commands run in the worktree at `.claude/worktrees/1-05`.

| Check | Result |
|---|---|
| `cargo test --lib sync::passphrase` | 11 passed, 0 failed |
| `cargo test --lib sync::anchor` | 9 passed, 0 failed |
| `env -u HOME cargo test --lib sync::` | 38 passed, 0 failed, 0.02 s — hermetic with `$HOME` unset |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt -- --check` | clean |
| Environment-gate negative check | appending `pub fn bad() -> Option<String> { std::env::var("PW").ok() }` to `passphrase.rs` makes the test fail as intended |
| 1-01's containment gate | still green — neither module imports `argon2` or `chacha20poly1305` |

Every filesystem test uses `tempfile::TempDir`. No test in this plan reads a real `$HOME`/`$XDG` path, the network, the Keychain, or the clock; the only path resolved from the environment is `CARGO_MANIFEST_DIR`, and only to locate the crate's own sources. No Argon2 call exists in either module, so the cheap-KDF seam is not needed here — the whole 38-test `sync::` tree still runs in 0.02 s.

## Known Stubs

None from this plan. `src/sync/{chunk,pack,model}.rs` remain 1-01 stubs owned by 1-02, 1-03 and 1-04.

## Threat Flags

**T-05-04 (first-contact TOFU) is accepted, not mitigated — carry it into 1-08's residual-risk section.** A machine with no anchor has nothing to compare against, so an attacker who already controls the remote at the moment of the very first fetch can serve an old snapshot and it will be believed. Every fetch afterwards is protected. This is documented in `accept`'s doc comment in the same terms; closing it would require the user to carry a counter out of band, which is a different trade than this design makes.

The remaining register entries are mitigated as planned: T-05-01 by generate-by-default plus the explained floor, T-05-02 by the enumerated-and-enforced input paths, T-05-03 by the monotonic counter and the corrupt-anchor refusal, T-05-05 by the mode-0600 atomic write.

Two notes for whichever phase wires these up:

- **An attacker with local write access can still delete the anchor**, which downgrades the next fetch to first contact. That is the same residual as T-05-04 and is why the module docs insist the anchor lives in the config directory, never the wipeable cache.
- **`accept` decides; it does not persist.** Advancing the high-water mark after a successful fetch is the caller's job, and it must happen only after the snapshot verifies — advancing first would let a failed fetch of a forged high counter lock the user out of their own real bundle.

## User Setup Required

None — both modules are pure and offline.

## Next Phase Readiness

**Ready.** 1-06 has the exact `accept` signature above and can drive its rollback adversarial case straight through it with no filesystem. 1-08 has the TOFU gap and the two caller obligations above for its residual-risk write-up. Any surface that sets a password has `generate`, `check`, `NO_RECOVERY` and `OFFLINE_ATTACK_NOTE`, and is expected to print both texts at the point the password is set.

## Self-Check: PASSED

Both files exist and are non-stub (338 and 243 lines); both task commits (`b5dc626`, `c5128e8`) exist in git on `gsd/1-05`; the working tree is clean apart from this summary.
