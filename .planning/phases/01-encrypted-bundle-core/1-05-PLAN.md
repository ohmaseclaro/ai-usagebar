---
phase: 01-encrypted-bundle-core
plan: 05
type: execute
wave: 2
depends_on: [1-01]
files_modified:
  - src/sync/passphrase.rs
  - src/sync/anchor.rs
autonomous: true
requirements: [CRYPTO-05, CRYPTO-06, CRYPTO-07]
must_haves:
  truths:
    - "A generated passphrase is 20 Crockford base32 characters and differs on every call."
    - "A supplied password under 12 characters is refused; one under 20 is accepted with a warning."
    - "The no-recovery consequence is stated in plain language at the point the password is set."
    - "A snapshot whose counter is below the local high-water mark is refused unless rollback is explicitly allowed."
    - "First contact has no anchor and is accepted as trust-on-first-use, which is documented, not silent."
  artifacts:
    - src/sync/passphrase.rs with generate, strength checking, and the non-argv non-environment input paths
    - src/sync/anchor.rs with the injected-path monotonic counter and its pure accept decision
  key_links:
    - "Anchor::write_to uses tempfile in the destination directory plus an explicit mode 0600 — the project's existing atomic-write invariant"
    - "accept() is pure and takes the local anchor as an argument, so rollback refusal is testable without a filesystem"
---

<objective>
Two small modules that sit either side of the crypto: how a password is chosen and read, and how a
rolled-back snapshot is refused.

Password strength matters here more than in a login form. The threat model is an attacker holding
the complete repository and mounting an unlimited offline attack, so at roughly 10^5 guesses per
second a century of cracking covers about 2^55 candidates. Generating the passphrase by default is
the control that makes that irrelevant; a length floor is the fallback when the user insists on
their own.

Rollback is the one attack the AEAD cannot answer, because re-serving an old root is a replay of
genuinely authentic data. Only a local monotonic anchor detects it.

Implements **D-05 (D5)** secret hygiene and **D-01 (D1)** — every path in both modules is injected.

Purpose: CRYPTO-06 and the rollback half of CRYPTO-05.
Output: `src/sync/passphrase.rs`, `src/sync/anchor.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/01-encrypted-bundle-core/1-CONTEXT.md
@.planning/research/encryption.md
@.planning/phases/01-encrypted-bundle-core/1-01-SUMMARY.md
@src/sync/crypto.rs
@src/cache.rs
@CLAUDE.md
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Passphrase generation, strength gate, and safe input paths</name>
  <files>src/sync/passphrase.rs</files>
  <behavior>
    - `generate()` returns 20 characters, all drawn from the Crockford base32 alphabet, and two calls differ.
    - `check("short")` and any input below 12 characters is `Rejected`.
    - A 15-character input is `Weak` and its message suggests a six-word diceware phrase.
    - A 20-character input is `Strong`.
    - Reading from a reader yields the line with its trailing newline stripped and no interior trimming, so a password ending in a space survives.
    - `read_from_file` on a mode-0644 file is refused; on a mode-0600 file it succeeds.
    - The no-recovery text states that a lost password means lost data, with no hedging.
  </behavior>
  <action>
Implement `src/sync/passphrase.rs`.

`pub fn generate() -> Result<Zeroizing<String>>` — draw 13 bytes with `getrandom::fill` and emit 20
characters from the Crockford base32 alphabet `0123456789ABCDEFGHJKMNPQRSTVWXYZ` by slicing five bits
at a time. That is roughly 94 bits, needs no embedded wordlist, and is about three lines. Generating
by default is the recommended path; a user-supplied password is the exception.

`pub enum Strength { Rejected(&'static str), Weak(&'static str), Strong }` and
`pub fn check(pw: &str) -> Strength` — reject below 12 characters, warn below 20, otherwise strong.
Measure in characters, not bytes, so a passphrase with non-ASCII is not judged by its UTF-8 length.
The `Weak` message should point at a six-word EFF diceware phrase, which is 77.5 bits and is the
right thing to suggest.

`pub const NO_RECOVERY: &str` — plain language, no hedging: there is no recovery, no reset, and no
escrow; a lost password means the bundle is permanently unreadable, and that is the property the
whole design exists to provide. Any surface that sets a password prints this.

`pub const OFFLINE_ATTACK_NOTE: &str` — one short paragraph explaining, without jargon, that anyone
who obtains the repository can guess at the password forever on their own hardware, which is why the
length floor exists (CRYPTO-06 requires the risk explained in plain language, not merely enforced).

Input paths, and only these three: `pub fn read_line(r: impl std::io::BufRead) -> Result<Zeroizing<String>>`
for stdin or a piped source; `pub fn read_from_file(path: &Path) -> Result<Zeroizing<String>>` which
first stats the file and refuses any mode with group or other bits set, then reads and wraps the
contents; and a TTY prompt, which belongs to whichever surface owns the terminal and is therefore
out of scope for this module. Strip only a single trailing newline or carriage-return pair — a
password may legitimately end in a space.

There is deliberately no function that takes a password from a command-line argument or from the
process environment. Both are readable by any local user through `/proc`, environment values leak
into crash dumps, and the project already forbids credentials in process arguments.

Add a `#[test]` named `no_password_input_path_reads_the_process_environment` that reads
`src/sync/passphrase.rs` and `src/sync/anchor.rs` and asserts no *code* line — lines whose trimmed
form begins with `//` are skipped before matching, so documentation prose can discuss the rule
freely — performs an environment lookup or a `clap` argument read for a password.
<!-- planner-discipline-allow: env::var -->

Every returned password lives in `Zeroizing<String>`. Never format one into an error; a failure says
what could not be read, never what was read.

Write the `<behavior>` assertions inline. `read_from_file` tests use `tempfile::TempDir` with an
explicitly set mode — never a real path.
  </action>
  <verify>
    <automated>cargo test --lib sync::passphrase</automated>
  </verify>
  <done>Every `<behavior>` line passes, including the mode-0644 refusal and the environment-lookup gate.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: The rollback anchor</name>
  <files>src/sync/anchor.rs</files>
  <behavior>
    - `accept(None, 7, false)` is `Ok` — first contact is trust-on-first-use.
    - `accept(Some(counter 7), 8, false)` is `Ok`; `accept(Some(counter 7), 7, false)` is `Ok` (a re-read of the same snapshot).
    - `accept(Some(counter 7), 6, false)` errors and the message names the allow-rollback escape.
    - `accept(Some(counter 7), 6, true)` is `Ok`.
    - An anchor whose `repo_id` differs from the remote's errors regardless of counter.
    - `write_to` then `read_from` in a temp dir round-trips, and the written file's mode is exactly 0600.
    - `read_from` on an absent path returns `Ok(None)`, not an error.
    - `read_from` on a corrupt file errors rather than silently resetting the high-water mark to zero.
  </behavior>
  <action>
Implement `src/sync/anchor.rs`.

`pub struct Anchor { pub repo_id: String, pub counter: u64 }`, serde as JSON.

`pub fn accept(local: Option<&Anchor>, remote_repo_id: &str, remote_counter: u64, allow_rollback:
bool) -> Result<()>` — the pure decision, and the part worth testing. `None` is first contact and is
accepted; document in the doc comment that this is trust-on-first-use and is an inherent residual
gap, not a bug, because a brand-new machine has nothing to compare against. A mismatched `repo_id`
is always an error. A lower counter is an error unless `allow_rollback`, and the message must name
the escape so a user who genuinely wants an older snapshot knows the word to type.

`pub fn read_from(path: &Path) -> Result<Option<Anchor>>` and `pub fn write_to(path: &Path, a:
&Anchor) -> Result<()>`. `read_from` returns `Ok(None)` only for a genuinely absent file; a present
but unparseable file is an error, because silently treating corruption as first contact would turn a
damaged anchor into a free rollback. `write_to` uses `tempfile::NamedTempFile::new_in` on the
destination's parent directory followed by `persist`, then sets mode 0600 explicitly rather than
relying on the temp file's inherited mode — the same belt-and-braces the Settings overlay already
applies to `config.toml`.

Both take a `&Path`. A thin non-test wrapper resolving the real location may exist, but it belongs in
whichever phase owns the config directory; this module never resolves a path itself, exactly as
`Cache::at` exists so nothing has to call `Cache::for_vendor` in a test. Document in the module doc
comment that the anchor belongs in the config directory and not the cache directory: the cache is
wipeable, and a wiped rollback anchor is a free rollback.

Write the `<behavior>` assertions with `tempfile::TempDir`.
  </action>
  <verify>
    <automated>cargo test --lib sync::anchor</automated>
  </verify>
  <done>All `<behavior>` lines pass. The anchor file is written atomically at mode 0600 into an injected directory.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| user → password | The single secret protecting every synced credential |
| local filesystem → anchor | An attacker with local write access can delete or lower the high-water mark |
| remote root counter → `accept` | Fully attacker-chosen |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-05-01 | Elevation of privilege | weak user-chosen password | critical | mitigate | Generate by default at ~94 bits; hard floor at 12 characters; the offline-attack risk stated in plain language at set time |
| T-05-02 | Information disclosure | password reaching argv or the environment | critical | mitigate | Only three input paths exist — a reader, a mode-0600 file, and a TTY prompt owned elsewhere — enforced by a test that scans this module's code lines |
| T-05-03 | Tampering | snapshot rollback | high | mitigate | Local monotonic counter, refused when lower unless explicitly allowed; a corrupt anchor errors rather than resetting to zero |
| T-05-04 | Spoofing | first-contact rollback (no anchor yet) | medium | accept | Trust-on-first-use is inherent to a machine with no prior state; documented as a residual gap in 1-08 rather than papered over |
| T-05-05 | Information disclosure | anchor file readable by other local users | medium | mitigate | Written atomically at an explicit mode 0600 into the config directory, never the wipeable cache |
</threat_model>

<verification>
- `cargo test --lib sync::passphrase` and `cargo test --lib sync::anchor` both pass with `$HOME` unset.
- Every filesystem test uses `tempfile::TempDir`; no real configuration or cache path is touched.
</verification>

<success_criteria>
1. A supplied password under 12 characters is refused, and the generated path is the default.
2. The no-recovery consequence and the offline-attack explanation are both present in plain language.
3. No code path in either module accepts a password from an argument or the environment, proven by a test.
4. A snapshot whose counter is below the high-water mark is refused unless rollback is explicitly allowed.
5. The anchor is written atomically at mode 0600 into an injected directory.
</success_criteria>

<output>
Create `.planning/phases/01-encrypted-bundle-core/1-05-SUMMARY.md` when done. Record the `accept`
signature — 1-06 drives the rollback adversarial case through it.
</output>
