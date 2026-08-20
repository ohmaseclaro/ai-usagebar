# Phase 5 — Security Audit

**Verdict:** `OPEN_THREATS`
**ASVS level:** 2, with L3 depth on the write path, the credential gate and the anchor. `block_on: high` (inherited — no Phase 5 plan carries a `<config>` block).
**Register:** 63 threats across eight plans. **Closed 59 · Open 4** (3 blocking, 1 non-blocking) · **2 findings the threat models never named.**

**threats_open: 3**

Audited adversarially from the threat model and the on-disk format, **not** from the
test suite. That instruction paid for the fourth time running: the headline finding
is invisible to all 1,559 lib tests, to the 60 integration assertions, and to the six
negative controls the verifier ran — because every one of them spells the filename
the honest way.

Ten negative controls were run in this worktree, each injected, watched, and reverted.
Nine went red. **One guard stayed green on a real violation.**

---

## Threat model audited against

The bundle sits in a hosted GitHub repo and carries **live OAuth credentials**. An
attacker may obtain the whole thing and grind the password offline with no rate limit;
an attacker with repo write access may tamper, reorder, truncate, substitute or roll
back any object; the host sees every ciphertext, size and access pattern. Phase 5 is
the first code that takes bytes from that hostile remote and writes them to a second
machine's disk, over the top of live credentials.

---

## F-1 — BLOCKER (critical) — the credential gate is defeated by one capital letter

**`merge.rs:269-275` (`credential_bearing`), `merge.rs:250-255` (the gate arm).**

`credential_bearing` decides whether an item gets D2's second consent. It asks two
questions, and both are **byte-exact string comparisons**:

```rust
category == SyncCategory::Credentials
    || manifest_path.rsplit('/').next().is_some_and(|name| name == CREDENTIAL_FILE)
```

`CREDENTIAL_FILE` is `".credentials.json"`. The comparison is case-sensitive. **The
filesystem this project's primary users run is not.** macOS ships APFS
case-insensitive by default; HFS+ was too.

I confirmed the platform behaviour directly on the audit machine before writing a line
of Rust: `echo STALE > .Credentials.json` in a directory holding `.credentials.json`
left **one** file, still named `.credentials.json`, now containing `STALE`.

### The attack

An attacker with repo write access changes one byte in the manifest:
`config/accounts/work/.credentials.json` → `config/accounts/work/.Credentials.json`.

1. `layout::accept_for_write` — passes; `.Credentials.json` is on no exclusion list.
2. `layout::from_manifest_path` — passes; it is a perfectly ordinary `Component::Normal`.
3. `merge::local_at` — `symlink_metadata` on `<config>/accounts/work/.Credentials.json`
   **finds the live credential**, because the kernel folds the case. Reports its real
   mtime, which is newer than the replayed snapshot's.
4. `merge::decide` — local is newer, `opts.force` is set, and
   `credential_bearing(...)` now returns **false**, so the
   `credential && !opts.force_credentials` arm is skipped entirely.
5. Disposition: **`Overwrite`**, not `NeedsCredentialConfirm`.

`Overwrite` is a writing disposition. `write::apply`'s `NeedsCredentialConfirm` tripwire
(`write.rs:111-118`) never fires because the item is not carrying that variant. The CLI's
`confirm_credentials` is never invoked because `cli.rs:1014` filters for a
`NeedsCredentialConfirm` that no longer exists. A stale OAuth token lands on the live
one and the machine is signed out of the account the tool exists to report on.

**`--force` alone is sufficient. `--force-credentials` is never requested.** That is
precisely the property the phase context (D2) and threats T-5-23 / T-5-60 exist to
guarantee, and the property `docs/configuration.md` promises the user.

### Proof

Injected into `merge.rs`'s own test module, run, reverted. All four passed:

| PoC | Asserts | Result |
|---|---|---|
| A1 | `credential_bearing("config/accounts/work/.Credentials.json", …)` is **false** | pass |
| A2 | `accept_for_write` admits `Bridge-State.json`, `Ant-Device-Registry.json`, `Backups/old.tar.gz`, `Local-Agent-Mode-Sessions/x.json` | pass |
| A3 | end-to-end `merge::plan` with `force=true, force_credentials=false` yields `Disposition::Overwrite`, and `canonicalize(dest) == canonicalize(live)` proves it is the same file | pass |
| A4 | a manifest component carrying `ESC [2J` survives the layout gate into `ItemPlan.manifest_path` | pass |

A3 is the whole attack, executed. The `canonicalize` equality is what turns "a
suspicious name" into "the live credential".

### Why no existing test sees it

`merge.rs:566` (`force_alone_never_overwrites_a_locally_newer_credential`) calls
`decide()` directly with the `credential` **bool hardcoded to `CREDENTIAL` (true)**. It
proves the arm works when it is reached; it cannot prove it is reached.
`merge.rs:687` (`the_credential_arm_covers_the_profile_store_and_every_dot_credentials_json`)
is the test that classifies — and every one of its eight fixtures is spelled in exactly
one case. The two tests are each correct and the seam between them is unguarded.

---

## F-2 — BLOCKER (high) — D4's machine-bound exclusion list folds the same way

**`layout.rs:170-172` (`accept_for_write`) → `scope.rs:127-145` (`is_excluded`), lines 133 and 142.**

Same root cause, second consequence, and it needs no `--force` at all.

`is_excluded` compares components against `EXCLUDED_DIRS` and the basename against
`EXCLUDED_NAMES` with `contains(&name)` — byte-exact. So `config/Bridge-State.json`,
`desktop-profiles/work/Ant-Device-Registry.json`, `desktop-data/Backups/old.tar.gz`
and `claude-home/Local-Agent-Mode-Sessions/x.json` are all `accept_for_write == true`
(PoC A2), resolve through the layout gate cleanly, and — on a case-insensitive volume —
**land on the very files D4 exists to keep off this machine**.

D4's stated purpose is that "a bundle produced by a future or modified client must not be
able to talk this side into writing them". A modified client only has to hold down Shift.
`bridge-state.json` and `ant-device-registry.json` are device-identity state; restoring
another machine's copy is the cross-device-registry corruption the milestone's own context
calls out as an already-shipped bug family.

The `accept_for_write` doc comment reasons carefully about *which* path to check (the
manifest path, not the resolved destination) and is right about that. The case question
is never asked.

### Scope note — this is a restore-side finding only

`scope::is_excluded` is shared with the push-side collector, but there it is applied to
names read from `read_dir`, i.e. the filesystem's own spelling, so the collector is
unaffected. The defect is confined to the path where the string is **attacker-supplied**,
which is Phase 5's boundary and no earlier phase's.

---

## F-3 — BLOCKER (high) — the failure line prints an attacker-chosen string raw

**`write.rs:155`.**

```rust
eprintln!("sync: restore stopped at {}: {why}", item.manifest_path);
```

`{}` is `Display`. `item.manifest_path` is a verbatim manifest string from the hostile
remote, and `{why}` is an `AppError` whose `Io` variant renders as
`io error at {path}: {source}` (`error.rs:16`) — `PathBuf`'s `Display`, also unescaped,
also carrying attacker-chosen components.

Every other rendering site in the phase is correct and goes through
`display::sanitize_untrusted_field`: `report.rs:266, 373, 419, 499` via the `safe()`
helper at `report.rs:719`, and `cli.rs:1034`. The sibling error messages in this very
function (`write.rs:116, 130, 142, 183, 220`) use `{:?}`, whose `Debug for str` escapes
control characters. **Line 155 is the one that uses `{}`.** T-5-50's mitigation is real;
it just does not cover all of its output sites.

### Why it is reliably reachable, not incidental

`layout::from_manifest_path` imposes **no bound on path length or component count** (the
file contains exactly one `len()` call, the Windows drive-letter check). So the attacker
does not have to wait for a disk to fill: a manifest entry of a few thousand components
makes `ensure_dir`/`tempfile_in` fail with `ENAMETOOLONG` on demand, driving `write_one`
to `Err` and line 155 to fire with an attacker-authored payload on the same run.

PoC A4 confirms the ESC survives the layout gate into `ItemPlan.manifest_path`. The
payload lands on stderr immediately after the restore report the user has just read and
consented to — so cursor-repositioning or screen-clearing sequences rewrite the record of
what was about to happen, and OSC 52 reaches the clipboard. That is T-5-50's threat
("a terminal that interprets it is a terminal an attacker is scripting") on a path the
mitigation missed.

---

## F-4 — OPEN, non-blocking (medium) — a structural guard is blind to 397 lines

**`sync/mod.rs:250-252` (`guard::production_code`), consumed by `passphrase.rs:449-476`.**

```rust
pub(crate) fn production_code(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap_or_default()
}
```

It splits on the **first textual occurrence** of the marker. In
`src/sync/github/pairing.rs` the first occurrence is at **line 76, inside a doc comment**:

> `/// The only production wrapper in this module, and nothing under `#[cfg(test)]` calls it.`

So `production_code` truncates that file at line 75 and the T-5-66 guard
(`no_password_input_path_reads_the_process_environment` — "only a reader, a mode-0600
file, and a TTY prompt are sanctioned password input paths") never sees lines 76-472,
which contain five production functions: `default_path`, `read_from`, `write_to`,
`check_drift`, `went_public_incident`.

**NC-J:** injected `std::env::var("SYNC_PASSWORD")` into `pairing.rs:79`, inside
`default_path`. The guard **passed** — `1 passed; 0 failed`. Reverted.

This is exactly the defect `github/mod.rs:281-307`'s own doc comment says was learned and
fixed — *"the marker it skipped to also occurs inside a doc comment in `pairing.rs`, which
silently truncated that file's scanned region to its first 76 lines. A guard that stops
looking where it happens to find a string is not a guard."* That lesson was applied to the
`github/mod.rs` guard and **not** to the shared helper 4-08 extracted, so the same file
is still the same blind spot for a different guard.

Non-blocking, and deliberately so: **the control itself holds.** A full grep of
`src/sync/` finds no passphrase env read anywhere, including the unscanned region — the
only `std::env::var` is `token.rs:92` for the GitHub token, which is the guard's one
sanctioned exemption. What is missing is the regression net, not the property. Exactly one
file is affected; a scan for phantom markers across all 35 files under `src/sync/` found
no others.

---

## Threat register — verification by disposition

Every threat carries disposition `mitigate`; verification is a grep for the declared
mitigation in the cited file, at L2 depth (does it address the vector, at the right
boundary) with L3 tracing on the write path, the credential gate, the anchor and the backup.

### Closed (59)

| ID | Category | Sev | Evidence |
|---|---|---|---|
| T-5-01 | Tampering | critical | `layout.rs:87-148` — empty/NUL/absolute/backslash/drive/no-root/unknown-root/nothing-beneath, then `.`/`..`/empty component and a `Component::Normal` single-component assert per part, then `starts_with(root)`. Built component-wise, never `root.join(rest)`. No `canonicalize`. Root prefixes are matched by **exact equality** on the first component (`layout.rs:114-118`), so no prefix-of-another-root confusion; `Path::starts_with` is component-wise, so `/home/bo` is not an ancestor of `/home/bob` |
| T-5-03 | DoS | critical | `mod.rs:400-408` — anchor written only when `applied.failed_at.is_none()`, from `resolved.root.counter` (sealed), after steps 2-6 all returned `Ok` |
| T-5-04 | Spoofing | high | `anchor.rs:111-118` — `repo_id` mismatch arm precedes the counter arm and ignores `allow_rollback` |
| T-5-05 | InfoDisc | critical | `write.rs:188-202` — `tempfile_in(dir)`, 0600 before any byte. NC-A red |
| T-5-06 | Tampering | high | `packer::manifest_path` errors rather than emitting absolute; `layout.rs:93-97` refuses one regardless |
| T-5-07 | InfoDisc | high | No error in `restore/` interpolates a file body; errors carry a path + io source. (The *escape* leg of path rendering is F-3, under T-5-50) |
| T-5-08 | Repudiation | medium | `merge.rs:190,194,203` → `RejectedPath`/`ExcludedByPolicy` kept in the plan; `report.rs:373,419` render them |
| T-5-10 | DoS | high | `fetch.rs:87,251` — `MAX_MANIFEST_CHUNKS` before `fetch_packs` at 289 |
| T-5-11 | DoS | high | `fetch.rs:97,105,113` + checks at `244,262,278` and cumulative re-checks at `434,441` |
| T-5-12 | DoS | medium | `fetch.rs:70,150` before any asset fetch |
| T-5-13 | Tampering | critical | `mod.rs:274` — `content_address(&bytes) != id` refuses **before** `pack::read_header` at 280 |
| T-5-14 | Tampering | critical | `pack::open_blob` → `open_chunk(keys, &entry.id, …)`; id is the AAD and is re-derived from plaintext |
| T-5-15 | Spoofing | high | `fetch.rs:216-219` — `(root.counter, root.created_at, &record.root)`; both leading terms come out of the **opened** root, so plaintext list order decides nothing. A hostile pointer cannot forge `created_at` (sealed) and can only *omit* records — withholding, which is indistinguishable from a host not serving data and is bounded by the anchor |
| T-5-16 | DoS | critical | `fetch.rs:237-242` calls `accept` and persists nothing; persistence is `mod.rs:401` |
| T-5-17 | Spoofing | high | `fetch.rs:205` passes `ctx.repo_id` (local pairing record) into `Root::open`, binding it to the AAD — tag fails before a field is parsed |
| T-5-18 | InfoDisc | medium | `fetch.rs:179-186` — one "cannot be opened" arm; no new discriminating error |
| T-5-19 | InfoDisc | high | `PackSource.packs` holds sealed pack bytes; `chunk()` returns `Zeroizing<Vec<u8>>` (`mod.rs:318-324`) |
| T-5-20 | EoP | high | Four read verbs only in `fetch.rs`; two **constants** imported from `github::write`, no verb. NC-E red |
| T-5-21 | Tampering | high | As T-5-08; refusal counts render rather than vanish |
| T-5-22 | Tampering | critical | `merge.rs:289` — `symlink_metadata` is the only stat call; `merge.rs:323` refuses a link by name. `RejectedPath` returns `writes() == false` (`mod.rs:140-145`), and no option promotes it — `--force` reaches only `SkipLocalNewer`. **Ancestor symlink assessed:** `write.rs:73-82` follows one deliberately; the reasoning holds — the bundle cannot create a symlink (nothing in `restore/` calls `symlink`), and an attacker able to plant one in `$HOME` already has the write access a restore would give them. `persist` is `rename(2)`, which replaces a link at the destination rather than writing through it, so the plan→apply TOCTOU is closed by construction |
| T-5-24 | Tampering | high | `merge.rs:340-365` hashes the file on disk with `Keys::chunk_id`; no index read on the restore side |
| T-5-25 | DoS | medium | `merge.rs:233` — digest compared before either clock is read |
| T-5-26 | InfoDisc | high | No `Disposition` variant carries bytes (`mod.rs:104-136`) |
| T-5-30 | InfoDisc | critical | `write.rs:188-190`; source guard forbids `temp_dir`/`/tmp`/`into_temp_path`. NC-A red |
| T-5-31 | InfoDisc | critical | `write.rs:196-202` (chmod) strictly precedes `207-212` (write); `persist` keeps the mode |
| T-5-32 | InfoDisc | critical | `ItemPlan` (`mod.rs:153-161`) carries **no** `mode` field — structurally unreadable, not merely unread |
| T-5-33 | InfoDisc | high | `write.rs:186` `ensure_dir` (0700 via `DirBuilderExt`) precedes `188` `tempfile_in` |
| T-5-34 | Tampering | critical | The destination is reached only by `tmp.persist(dest)` at `write.rs:237`; no `File::create(dest)` anywhere |
| T-5-35 | InfoDisc | high | Every error path drops the `NamedTempFile`; no `into_temp_path().keep()` (enforced by the same guard) |
| T-5-36 | Tampering | medium | `write.rs:137-144` re-runs `from_manifest_path` over the whole plan before the first byte; disagreement is a hard `Err`. (The policy half is NEW-2, non-blocking) |
| T-5-37 | DoS | medium | `write.rs:207-212` — one chunk in memory at a time |
| T-5-40 | InfoDisc | critical | `backup.rs:90` (dir 0700) precedes `101` (`tar` creates), `122` chmods the archive 0600 |
| T-5-41 | Tampering | high | `backup.rs:107` — `--` precedes `args(&members)`; members are `strip_prefix`ed local paths, never manifest strings. `Command` is exec-based, no shell |
| T-5-42 | EoP | high | `backup.rs:43` `const TAR = "/usr/bin/tar"`; `take_with` is `pub(crate)` and the CLI reaches only `take` |
| T-5-43 | Tampering | high | `backup.rs:152-159` — allowlist quoting, `'\''` closure for embedded quotes |
| T-5-44 | Repudiation | critical | The round-trip test executes the rendered command through `/bin/sh` and compares contents **and** modes. Mode preservation verified by reasoning too: `tar` stores 0600, and non-root extraction applies umask, which only *clears* bits — a 0600 credential cannot widen to 0644 |
| T-5-45 | DoS | critical | `take` returns `Err`; `mod.rs:393` propagates with `?` before `write::apply` at 396. The only other exit is `Ok(None)` with nothing on disk to preserve |
| T-5-46 | InfoDisc | medium | `mod.rs:387-392` — targets filtered by `disposition.writes()` |
| T-5-51 | Repudiation | high | `report::render_outcome` lists every overwritten path; the attention block is never truncated |
| T-5-52 | EoP | critical | `report.rs:250` reads `force_credentials` only; a source-scanning test asserts `assume_yes` is absent from the function body. NC-G red |
| T-5-53 | Tampering | high | `report.rs:65` `MAX_ATTENTION_ITEMS` is separate from and larger than the per-category budget, and its blocks lead the report |
| T-5-54 | DoS | medium | `report.rs:50` per-category budget with "and N more" |
| T-5-55 | Repudiation | high | EOF returns `false` and names the flag; `cli.rs:1017` hands the gate `io::empty()` when there is no TTY |
| T-5-56 | InfoDisc | high | `render_*` emit paths, counts, byte totals, timestamps only |
| T-5-61 | Repudiation | high | The gate is in `cli.rs` **before** `restore::run(apply: true)` at `1011`; `backup::take` is inside `run` at step 5, so a decline reaches neither |
| T-5-62 | Spoofing | high | `allow_rollback` is passed to `accept` (`fetch.rs:241`), whose `repo_id` arm errors first; the CLI adds no bypass |
| T-5-63 | Tampering | high | Restore consults no index; `merge` hashes disk |
| T-5-64 | DoS | medium | `index::reset_at` removes the file rather than issuing SQL against it |
| T-5-65 | Tampering | medium | No `--force-rehash` on `Pull` and no such field on `RestoreOptions`; `rehashing` suppresses reads only |
| T-5-66 | InfoDisc | critical | No `--password` flag, no env fallback. Independently verified by grep over all of `src/sync/`: the only `std::env::var` is `token.rs:92` (GitHub token, the guard's sanctioned exemption). NC-C red. **The control holds; its guard has a hole — see F-4** |
| T-5-67 | Repudiation | high | Every error path returns 1 with one stderr line; `cli.rs:1020` returns `i32::from(outcome.failed_at.is_some())`, so a partial restore is not a success |
| T-5-70 | Repudiation | high | `Machine::restored` walks root B; called from six tests |
| T-5-71 | InfoDisc | critical | `plaintext_anywhere_under` + the `write.rs` source guard. NC-A red |
| T-5-72 | DoS | critical | `assert_anchor_frozen` byte-compares the anchor from six refusal tests |
| T-5-73 | Tampering | high | Every adversarial case is one mutation of a bundle `push::run` produced |
| T-5-74 | Spoofing | high | `bob_roots` leaf names all differ from `alice_roots` |
| T-5-75 | Repudiation | medium | `docs/sync-format.md` §9 records first-contact TOFU and the never-deletes decision |
| T-5-76 | Tampering | high | CAL-1/CAL-5 `#[ignore]`d in `tests/live.rs`; the rollback test skips without `/bin/sh` or `/usr/bin/tar` |
| T-5-SC ×8 | Tampering | high | `git diff 5db4db6..HEAD -- Cargo.toml Cargo.lock` is **empty**. No new crates in any of the eight plans |

### Open — blocking (severity ≥ `high`)

| ID | Category | Sev | Mitigation expected | Where it is absent |
|---|---|---|---|---|
| T-5-23 | Tampering | critical | "Credential-bearing items that are locally newer require `force_credentials`" | `merge.rs:274` — classification is case-sensitive; `.Credentials.json` is not credential-bearing. **F-1**, PoC A3 |
| T-5-60 | EoP | critical | "`--force` does not answer it; only `--force-credentials` or an explicit typed answer does" | Gate ordering is correct (`cli.rs:1014-1046`), but the item never reaches it — same root cause. **F-1** |
| T-5-02 | EoP | high | "`accept_for_write` refuses anything `scope::is_excluded` refuses" | `scope.rs:133,142` — byte-exact list membership; case variants admitted. **F-2**, PoC A2 |
| T-5-50 | Tampering | high | "Paths are rendered through an escaping helper … before printing" | `write.rs:155` prints `manifest_path` with `{}`, and `{why}` renders `AppError::Io`'s `PathBuf` unescaped. **F-3**, PoC A4 |

*T-5-23 and T-5-60 are one defect counted once. `threats_open = 3` (F-1, F-2, F-3).*

### Open — non-blocking (below `block_on: high`)

| ID | Category | Sev | Note |
|---|---|---|---|
| — | — | medium | **F-4**, the `production_code` blind spot. Filed against no threat ID: it is a defect in the *guard* for T-5-66, not in T-5-66's control, which holds |

---

## Findings the threat models never named

### NEW-1 (medium, non-blocking) — no length bound at the hostile-input boundary

`layout::from_manifest_path` bounds eight *shapes* and zero *sizes*. There is no cap on
total path length or component count. A manifest is bounded only by
`MAX_MANIFEST_CHUNKS × CHUNK_SIZE` = 32 MiB, which is room for a great many
thousand-component paths. Two consequences: `ensure_dir`'s `recursive(true)` will happily
create deep directory chains inside the roots before the kernel refuses, and — the reason
this is filed rather than shrugged at — it hands the attacker a **deterministic trigger**
for the `ENAMETOOLONG` failure that F-3 needs. On its own it is litter inside a root the
user already consented to write; combined with F-3 it is the delivery mechanism.

### NEW-2 (low, non-blocking) — the write boundary re-checks the path rule but not the policy rule

T-5-36 re-runs `layout::from_manifest_path` for the whole plan at `write.rs:137` and calls
a disagreement a hard error. `layout::accept_for_write` — D4 — is re-run **nowhere**; its
only call site is `merge.rs:189`. The two halves of the layout gate therefore have
different defence-in-depth postures. No live hole: the plan is an in-process `Vec` between
the two points and nothing mutates it. Recorded because `write.rs`'s module doc claims the
preflight is "defence in depth against a plan mutated between planning and applying", and
a plan mutated that way could still carry an `ExcludedByPolicy` path promoted to `Update`.

### Informational — the signed-URL excerpt

`http::from_transport` (`http.rs:218-231`) interpolates `excerpt(&e.to_string())`, and
`reqwest::Error`'s `Display` includes the request URL — which, on the
`follow_unauthenticated` hop (`github/write.rs:528-552`), is the signed storage URL. Not
filed as a finding: `excerpt` caps at 200 characters and runs `sanitize_untrusted_field`
first, so a GitHub asset URL is truncated well before its signature parameter; the
capability is short-lived; and it grants only ciphertext. Inherited Phase 3/4 code, no
Phase 5 threat ID. Noted so it is a decision rather than an oversight.

---

## Negative controls — ten run, nine red, one green

Every injection was made in `src/`, the named test run, and the edit reverted with
`git checkout`. The worktree is clean and `cargo test --lib` is **1559 passed, 0 failed**
at the end of this report.

| # | Injection | Guard | Result |
|---|---|---|---|
| NC-A | `std::env::temp_dir()` into `write_one` | `nothing_in_this_module_reaches_for_a_shared_temporary_directory` | **RED** |
| NC-B | `backup::take` moved after `write::apply` in `run` | `the_archive_is_taken_before_the_first_byte_is_written` | **RED** — at **lib** level. Phase 5's `ordering_guard` (`mod.rs:422-450`) closes verification warning W2, which recorded the whole lib suite staying green on this swap |
| NC-C | `std::env::var("SYNC_PASSWORD")` into `fetch::resolve` | `no_password_input_path_reads_the_process_environment` | **RED** — confirms `rs_files_in` does recurse into `restore/` |
| NC-D | `use chacha20poly1305::…` into `restore/backup.rs` | `only_the_crypto_module_imports_the_cryptographic_crates` | **RED** — confirms recursion into `restore/` |
| NC-E | `.json(&())` into `github/http.rs` | `every_request_body_in_this_directory_lives_in_write_rs` | **RED** |
| NC-F | `const _AUDIT: &str = "/user/repos"` into `restore/fetch.rs` | `no_repository_creating_endpoint_is_reachable_from_the_crate` | **RED** — confirms `rs_files(src/)` covers new files |
| NC-G | `if opts.assume_yes { return Ok(true) }` into `confirm_credentials` | `assume_yes_alone_leaves_the_credential_gate_refusing` | **RED** |
| NC-H | a third `pub fn confirm_third` with a `read_line` | `there_are_exactly_two_gates_and_no_third_prompt` | **RED** |
| NC-I | `_ => None` arm over `Disposition` | `no_match_over_a_disposition_falls_through_a_wildcard` | **RED** |
| NC-J | `std::env::var("SYNC_PASSWORD")` into `pairing.rs:79` | `no_password_input_path_reads_the_process_environment` | **GREEN — BLIND.** F-4 |

### The 4-08 recursive-walk claim, checked rather than accepted

`sync::guard::rs_files` (`mod.rs:234-238`) does genuinely recurse, and NC-C, NC-D and NC-F
each prove it reaches a file the old hand-maintained lists would have had to name —
including `src/sync/restore/`, which did not exist when the lists were written. The shape
is sound. **What it scans is not the same as what its consumers scan**: `production_code`
truncates one file to 16% of its length (F-4).

One shape note, not a finding: `github/mod.rs:337` uses a flat `read_dir` rather than
`guard::rs_files`, so a future `src/sync/github/<subdir>/` would be missed. That directory
is flat today, so the guard is currently complete.

---

## Controls verified sound beyond the register

The ordering in `restore::run` is a genuine structural property, not a convention: the
dry-run early return at `mod.rs:373` means the write half is **unreachable**, not merely
unvisited, and both gates live above `restore::run(apply: true)` in the CLI rather than
inside it.

Claim 3 of this audit's brief was checked independently rather than read from `5-02-SUMMARY`.
`RemoteIndexEntry`'s `offset`, `clen` and `true_len` have **zero production readers** —
an exhaustive scan of every `.offset`/`.clen`/`.true_len` access in non-test code across
`src/sync/` returns only `pack.rs` (from the **sealed** header, bounds-checked by
`entries_within` and by `pack.get(offset..end)` with `checked_add`), `packer.rs` (push
side), `merge.rs:388` (`clen` from the **authenticated** `IndexObject`, summed for a
display estimate, never used to index), `merge.rs:341` and `write.rs:216` (`true_len`
from the authenticated `Manifest`, as a truncation check). `fetch.rs` reads only `.pack`
and `.id` off the pointer, both content addresses. Bounds precede allocation at every
ceiling, and `check_version` (`mod.rs:120-128`) is `found <= ceiling` — at-or-below, never
equality.

The anchor precondition Phase 1 handed forward is honoured: `push::anchor_path`
(`push/mod.rs:482-486`) keys the file on the locally-configured `RepoRef` from
`config.toml`, never on the remote's claimed `repo_id`, and `cli.rs:959` is the only
construction site on the restore path — the same helper the three publish paths use, so
there is one implementation rather than two that can disagree.

Secret hygiene on this phase's own surfaces is clean: `RestoreCtx` deliberately derives no
`Debug` (`mod.rs:79-80`), the local-hash buffer in `merge.rs:348` is `Zeroizing`,
`PackSource::chunk` returns `Zeroizing`, the backup archive's name is a timestamp, and no
passphrase, token or keyfile byte reaches a log line, a process argument or the archive.

---

## What a fix would have to touch (not applied — auditors do not edit)

F-1 and F-2 are one root cause: **security predicates compare bytes while the OS compares
folded case.** A per-call-site `to_lowercase()` would be three patches and a fourth bug
later; the durable shape is one normalising comparison used by `credential_bearing`,
`is_excluded` and anything else that classifies an attacker-supplied name. Note that
`is_excluded` is shared with the push collector, so the change wants to be *widening*
(more things excluded) — safe in that direction for both callers.

The stricter alternative, worth considering because it removes the class rather than the
instance: have `from_manifest_path` refuse any component that is not already in its
normalised form, so the bundle has exactly one spelling for every file and a folded name
is a refusal that shows up in the report as tampering. That also closes NEW-1's sibling
concerns if a length bound is added in the same place.

F-3 is one line: route `write.rs:155` through `display::sanitize_untrusted_field`, as
`report.rs` and `cli.rs` already do — and consider whether `AppError::Io`'s `Display`
should escape its `PathBuf` for every consumer, since restore is unlikely to be the last
code to render an attacker-influenced path through it.

F-4 is one line: make `production_code` split on a line-anchored `#[cfg(test)]` rather
than the first textual match.

---

_Audited at `4085ab9` on branch `audit/5-security`, worktree `.claude/worktrees/5-audit`._
_Ten negative controls injected and reverted; four proof-of-concept tests written, run, and reverted._
_`git status --porcelain` empty; `cargo test --lib` 1559 passed, 0 failed._
_No implementation file was modified. The reported `// TEMP: sort disabled for verification` residue was **not** present in either tree — the string exists only as quoted text inside `4-SECURITY.md:439`._
