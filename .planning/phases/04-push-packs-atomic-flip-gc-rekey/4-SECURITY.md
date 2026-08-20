# Phase 4 — Security Audit

**Verdict:** `OPEN_THREATS`
64/67 declared register entries closed; **2 declared threats open** (one `high` `accept` whose
named backstop is absent, one `critical` `mitigate` whose property is false system-wide), plus
**1 new blocking finding** the register never named, and 5 non-blocking findings.

**ASVS 2**, L3 depth on the deletion path, the capability chain, and the flip.
`block_on: high`. **threats_open: 3.**

Audited from the threat model and the format. **The test suite was not treated as proof** —
NEW-1, F-2, F-3, F-4 and both halves of F-5 pass every test in the phase. Every structural guard
was negative-controlled by hand: the violation injected, the guard run, the tree reverted. Two
guards passed on a real injected violation.

---

## Threat model audited against

The bundle sits in a hosted GitHub repo and carries live OAuth credentials. An attacker may
obtain the whole thing and grind the password offline with no rate limit; an attacker with repo
write access may tamper, reorder, truncate, substitute or roll back any object; the host sees
every ciphertext, size and access pattern. Phase 4 adds the first code that sends bytes to a
network and the first that can delete remote data.

---

## NEW-1 — BLOCKER (high) — the snapshot counter is computed before the race and never recomputed after it

`push/mod.rs:288-290` · `push/packer.rs:367-384` · `push/mod.rs:323-327` · `push/mod.rs:336-368`
· `push/pointer.rs:157-159` · `anchor.rs:93-94`

**No attacker.** Two machines, which is the entire point of the milestone.

`push::run` reads the pointer at step 2, and `packer::build` at step 3 derives the new snapshot's
counter from it — `next_counter` opens every root the pointer carries, takes the maximum, adds
one. That counter is then **sealed inside `bundle.root`** and `record` is built from it once
(`mod.rs:323`). The `rebuild` closure captures `record` by move and `clone()`s it on every
invocation.

`pointer::commit` re-invokes `rebuild` on a 409 — but `rebuild` rebuilds the *list*, not the
*root*. The packer does not run again. So:

1. A and B both `load` a pointer whose highest counter is 6. Both compute 7. Both seal a root at
   counter 7.
2. A flips first. B's `PUT` gets a 409.
3. B re-`load`s, `rebuild(winner)` carries A's record forward (rule 1, correctly) and appends B's
   own. Both are now in the published pointer, **both claiming counter 7**.
4. `next_counter` uses `.max()`, so the collision is permanent in the bundle's history.

Rule 1's dedup is `snapshots.retain(|existing| existing.root != record.root)` — it compares root
*bytes*, which differ between A and B, so it cannot see this.

**Why it is not cosmetic.** The counter is the format's only ordering field. `push/mod.rs:184-186`
states the design outright: "a reader selects by the `counter` inside each sealed root, not by
position". With two roots at 7 that selection is ambiguous. Worse, Phase 1's anchor is keyed on
exactly this value and `anchor::accept` treats a counter *equal* to the anchor as "a re-read of
the snapshot already seen, which is the ordinary steady state" — so once a machine restores A's
counter-7 snapshot, B's genuinely distinct counter-7 snapshot reads as already-seen. A backup is
silently dropped by the control built to protect backups.

**Unregistered.** T-4-28, T-4-29 and T-4-30 all reason about the record *list*; none reaches the
value sealed inside the root. `4-04-SUMMARY.md`'s Threat Flags section says "None."

**Suggested remediation:** recompute the counter against the pointer that actually arrived. That
means re-sealing the root inside the conflict path — either by moving the counter derivation into
the `rebuild` closure and having it re-seal, or by having `commit` return the losing outcome so
the orchestrator can re-run `packer::build`'s step 4 against `winner`. A cheaper containment is
to make `commit` refuse to append a record whose counter is not strictly greater than every
counter already in `arriving`, and error rather than publish — a collision then costs a re-run
instead of a corrupt history.

---

## NEW-2 (T-4-04) — BLOCKER (high) — the push path never consults the anchor, so a rolled-back pointer is laundered and then pruned into permanence

`push/mod.rs:284-285` · `push/packer.rs:367-384` · `push/prune.rs:94-108` ·
`grep -rn anchor src/sync/push/` → three doc-comment mentions, **zero reads**

T-4-04 is dispositioned `accept` (high), and the accept is justified in the register by naming a
specific control: *"a dropped entry is a rollback caught by the local anchor's counter"*. The same
sentence is repeated in `push/mod.rs:183`. **The anchor is not read anywhere on the push path.**
Phase 1's `1-SECURITY.md` F-3 called the anchor "the only defence against replay of *authentic
old snapshots* — the one attack that authenticates perfectly", which is precisely this input.

**The attack.** An attacker with repo write — squarely in the declared model — replaces
`sync/pointer.json` with an authentic *older* copy of itself. Nothing errors: every root in it is
genuine, so `Root::open` succeeds in `next_counter`, and `repo_id` matches, so `pointer::load`'s
check passes. Then:

1. The next honest push carries all of those old records forward (rebuild rule 1), appends its
   own, and flips — **republishing the rollback as the current, legitimately-written pointer**,
   with a fresh valid `sha`.
2. `prune::run` is handed that pointer as `landed` and computes liveness as the union over its
   surviving records (`prune.rs:94-98`). Every pack referenced only by the snapshots the rollback
   dropped is now unreferenced.
3. Those packs are older than 24 hours, so `PRUNE_GRACE` does not cover them. They are deleted.

The rollback stops being reversible. The victim's own machine performs the destruction and the
command exits 0 reporting a successful push.

An attacker with repo write could delete those assets directly, so raw capability is not
increased — but this converts a *reversible* tamper into an *irreversible* one, executed by the
victim, and it does so past a control the design believes is standing. An `accept` whose written
justification names a control that is absent on the path is not a closed threat.

**Suggested remediation:** read the anchor in `push::run` between step 2 and step 3 and refuse a
pointer whose highest openable counter is below the local high-water mark, with the same
`allow_rollback` escape `anchor::accept` already defines. The counter is already being computed
there by `next_counter`; the comparison is the missing half.

---

## NEW-3 (T-4-45) — BLOCKER (high) — `ensure_keyfile` republishes a superseded keyfile, so a rekey's deletion is undone by the next push from any other machine

`push/upload.rs:146-152` (the code's own "Known sharp edge") · `push/upload.rs:153-177` ·
`push/mod.rs:320` · `push/rekey.rs:183-215` · `docs/sync-github.md:204-208`

T-4-45 is `critical` / `mitigate`: *"The confirming re-list is mandatory and its failure is an
error naming the asset — D5's 'verifiably delete', not 'attempt to delete'."* The re-list is
present and correct at `rekey.rs:203-213`. The **property it exists to establish is false.**

**No attacker.** Two machines:

1. Machine A runs `sync rekey`. Uploads keyfile-NEW, flips the pointer to it, deletes keyfile-OLD,
   re-lists and confirms it is gone. Reports success, truthfully, at that instant.
2. Machine B — which has not rekeyed and whose local keyfile is still the old wrapper — runs an
   ordinary `sync push`. Rebuild rule 3 correctly carries keyfile-NEW into the pointer. Then step
   6b calls `upload::ensure_keyfile`, which publishes **whatever keyfile is on this machine's
   disk** — keyfile-OLD — straight back onto the release.

The old wrapper is live again, readable by anyone who can read the repository, openable with the
old password. `prune` classifies it as an orphan keyfile, but `PRUNE_GRACE` retains anything
younger than 24 hours and B's next push re-uploads it, resetting `created_at`. It is never
collected.

`rekey::destroy` compounds it: it deletes exactly one name, `previous.keyfile`
(`rekey.rs:194-196`). Any *other* keyfile asset already on the release survives a rekey untouched
and unmentioned.

The code names this — `upload.rs:146-152` says "the old wrapper comes back as an orphan asset
prune never collects" and hands the problem to "whoever wires the rekey path". Nobody wired it.
4-CONTEXT.md's D5 is unambiguous that the deletion "must actually happen and be verified, not
merely attempted", and `docs/sync-github.md:204-208` tells the user only that a copy *someone
already took* survives — not that the tool itself republishes one.

**Suggested remediation:** `ensure_keyfile` must not upload a keyfile the arriving pointer does
not name. When `ctx.previous` carries a `keyfile` field and it differs from this machine's local
content address, that machine is stale: refuse and tell the user to re-run with the new password,
rather than silently resurrecting the wrapper the rekey destroyed. The first-push case
(`previous == None`) is the only one that legitimately publishes from local state.

---

## F-4 — OPEN, non-blocking (medium) — the went-public incident cleanup selects assets by comparing GitHub's clock to the local clock, and reports complete cleanup when it deletes nothing

`push/mod.rs:442-470`, specifically `:446` and `:461-463` · `cli.rs:61` · `push/upload.rs:84-122`
· `tests/sync_push_e2e.rs:499`

The ordering the prompt asked about **does hold**: the re-gate is at `mod.rs:304`,
`ensure_keyfile` at `:320`, the flip at `:370`. The wrapped master key is not uploaded before the
re-gate, and `went_public_mid_push` therefore never needs to delete it. That is correct.

The cleanup itself is not. It deletes the intersection of "a name this run's bundle produced" and
`a.created_at >= ctx.now`:

```rust
.filter(|a| ours.contains(&a.name) && a.created_at >= ctx.now)
```

`ctx.now` is **this machine's** clock, captured once at process start (`cli.rs:61`,
`Utc::now()`). `a.created_at` is **the remote's**. Two ways it fails:

- **No attacker.** If the local clock runs ahead of GitHub's by more than the elapsed upload time,
  the filter matches nothing. On an incremental push — pack a small delta, gate, upload — that
  window is seconds. The user then reads: *"All 0 asset(s) this run uploaded were deleted."*
- **With the declared attacker.** `created_at` is host-supplied data. A hostile remote simply
  backdates it and the incident cleanup is a guaranteed no-op that reports success.

The information needed is already in hand and thrown away: `upload::run` knows exactly which packs
it sent (`pending`, `upload.rs:96-116`) and returns only `(usize, usize, u64)`. The timestamp
comparison is reconstructing, from untrusted data, something the process observed directly.

The `created_at` leg is not gratuitous — `ours` is `bundle.packs`, which includes packs this run
*skipped* because an earlier run had already uploaded them, and deleting one of those is exactly
what the leg prevents. The fix is to carry the uploaded set out of `upload::run`, not to drop the
leg.

Medium rather than high: the exposed bytes are encrypted packs, and the message still names the
three real remediation steps (make private, rotate the token, rekey). The defect is that it
asserts a cleanup that did not happen — the same "documentation asserts behaviour that does not
exist" class Phase 1 flagged twice, now expressed as a runtime message.

**Why no test caught it:** the fake remote plants every upload at exactly `NOW`
(`tests/sync_push_e2e.rs:499`), i.e. zero skew — the single value that cannot expose this.

---

## F-5 — OPEN, non-blocking (medium) — two inherited structural guards do not reach the directories Phase 4 created

Both proven by injection, both reverted.

**(a) The crypto-import invariant is blind to 11,500 lines.**
`crypto.rs:1295-1297` walks `read_dir("src/sync")` **non-recursively**. I added
`use chacha20poly1305::XChaCha20Poly1305;` to `src/sync/push/packer.rs` and ran
`only_the_crypto_module_imports_the_cryptographic_crates`:

```
test sync::crypto::tests::only_the_crypto_module_imports_the_cryptographic_crates ... ok
```

`src/sync/push/` (5,600 lines) and `src/sync/github/` (5,900 lines) are entirely invisible to the
invariant whose stated value is "what lets a security auditor read one file instead of six". It is
correct today — verified, no such import exists — but it is a claim, not a guard.

**(b) The passphrase-input guard's file list was not extended to the file that holds two live
passwords.** `passphrase.rs:431-435` lists `passphrase.rs`, `anchor.rs`, `github/setup.rs`. I
added `std::env::var("SYNC_PW")` to `src/sync/push/rekey.rs` and ran
`no_password_input_path_reads_the_process_environment`:

```
test sync::passphrase::tests::no_password_input_path_reads_the_process_environment ... ok
```

This is **Phase 3's F-8 recurring verbatim, one phase later**. Phase 3 found the list had not been
extended to `setup.rs` when setup grew a passphrase surface; the remediation added `setup.rs` to
the list rather than making the list unnecessary. Phase 4 added `push/rekey.rs` — the file
T-4-47 (critical) is about — and the list was not extended again. T-4-47 is correct today
(`grep -rn "std::env|env::var|var_os" src/sync/push/` → nothing) but rests on the same kind of
claim that F-3 and this finding are made of.

**Suggested remediation for both:** walk recursively. Both guards already have a `collect_rs`
helper available in-crate (`github/mod.rs:422-431`, `gate.rs:775-784`) that does exactly this.

---

## F-6 — OPEN, non-blocking (low) — `Pushing` proves *when* a check happened, never *what* it was about

`gate.rs:228` · `write.rs:17-31`

The capability chain is otherwise exactly as advertised, and I enumerated it rather than taking
the doc's word:

- **Write verbs, all four taking `&Pushing`:** `ensure_release` (`write.rs:256`), `upload_asset`
  (`:362`), `delete_asset` (`:445`), `put_contents` (`:616`).
- **Read verbs, correctly permitless:** `list_assets`, `download_asset`, `get_contents`,
  `get_json`.
- **`Pushing` mint sites: exactly one** — `Ok(Pushing(()))` at `gate.rs:244`, inside
  `PushClearance::spend`. Pinned by `freshness_is_the_only_exit_from_a_clearance`.
- **`PushClearance` mint sites: exactly one** — `gate.rs:383`, the tail of `assert_pushable`.
- **Callers minting a permit: exactly three** — `push::run` twice via `gate_now` (`mod.rs:280`,
  `:304`), `prune::run_on_demand` (`prune.rs:216`), `rekey::run` (`rekey.rs:97`). All four calls
  route through the single `gate_now` (`mod.rs:404-414`), so the pre-upload and pre-flip checks
  cannot drift apart.

**There is no path to a write without one.** Phase 3's F-3 carry-forward is properly closed.

What is *not* closed is the subject. `Pushing` is a unit struct with no repo identity, so a permit
minted against repo A type-checks against a `put_contents` to repo B. `write.rs:17-22` claims a
call reaching that file "proves a visibility check that was fresh within `MAX_CLEARANCE_AGE`" —
true about time, silent about subject. It holds today only because `gate_now` and every verb read
the same `ctx.repo`. One `repo_id` field would make it structural, matching the pattern `Root`'s
AAD already uses for exactly this reason (Phase 1 F-3).

---

## F-7 — OPEN, non-blocking (low) — `plan_deletions`' keyfile exclusion is defined entirely by attacker-controlled data

`prune.rs:104` · `prune.rs:78-84` · `push/mod.rs:354-360`

```rust
Some(Kind::Keyfile) => a.name != kept.keyfile,
```

`kept.keyfile` comes from the pointer — untrusted plaintext, carried forward from the arriving
remote by rebuild rule 3. T-4-36 calls deleting the keyfile "the single worst thing this function
could do", and its exclusion set is sourced from the one input the module is told to distrust.
A pointer naming a keyfile that does not exist makes the honest machine sweep the real one as an
orphan.

The machine holds trustworthy local knowledge it declines to use: `PushCtx.keyfile_asset`
(`mod.rs:240`) is this machine's own keyfile content address. Adding it to the exclusion set is
one clause. Bounded to low because an attacker with repo write can delete the asset directly; the
finding is that the mitigation is keyed on the wrong source.

---

## Answers to the seven questions put to this audit

1. **Deletion.** `plan_deletions` is sound against the inputs it was designed for. `created_at` in
   the **future** yields a negative `now - created_at`, which is not `> grace`, so the asset is
   retained — fails safe. `created_at` **absent** fails deserialization outright: the field
   deliberately carries no `#[serde(default)]` (`write.rs:116-120`), so `list_assets` errors and
   prune degrades to a warning — fails safe. **Malformed** likewise. The `landed`-pointer rule and
   the grace floor both hold and are genuinely independent. The two real gaps are NEW-2 (a
   *rolled-back* landed pointer makes prune the executioner) and F-7 (the keyfile exclusion is
   attacker-defined).
2. **The gate.** Enumerated in full under F-6: one mint site, four write verbs, three entry
   points, no path to a write without a permit. The `ensure_keyfile`-after-re-gate ordering
   **does** hold (`mod.rs:304` then `:320` then `:370`). The incident cleanup is complete for the
   packs and correctly needs no keyfile arm — but its *selection predicate* is broken (F-4). The
   release object created at step 4 is not deleted by the incident path; its note is a fixed
   non-identifying string (`write.rs:656`), so that is acceptable. Cross-repo permits are
   structurally possible but unreachable today (F-6).
3. **The flip.** `commit` cannot drop another machine's snapshot: rule 1 carries `arriving`
   forward, the retry re-invokes `rebuild` against the *winner*, and the `retain` on `record.root`
   makes a lost-response replay idempotent. It cannot resurrect a pruned snapshot **of its own
   accord** — but it will faithfully republish one handed to it (NEW-2). What it *does* get wrong
   is the counter (NEW-1).
4. **Unauthenticated trust boundary.** `MAX_POINTER_BYTES` is enforced by `send_capped` before
   deserialization, and `read_body_capped` (`vendor.rs:79-101`) checks `Content-Length` before
   allocating **and** accumulates per chunk with `saturating_sub` — no `with_capacity` anywhere on
   this path. Version handling is at-or-below via `check_version` (`sync/mod.rs:116-124`), probed
   through `VersionProbe` *before* the full deserialize so a future pointer is refused by version
   rather than by a missing-field complaint. `repo_id` is compared against a locally-sourced value,
   never against itself (`pointer.rs:68`). Phase 1's carry-forward — an id list read before its
   container authenticates needs its own bound — is satisfied *transitively*: `SnapshotRecord.packs`
   and `RemoteIndexEntry` are unbounded in count but the whole document is capped at 1 MiB, and
   Phase 4 never allocates against `offset`/`clen`/`true_len` (it only echoes them). **Phase 5 will
   slice packs at those offsets and must bound them itself** — carried forward below.
5. **Rekey.** The order (upload → flip → local write → delete → confirm) is correct and each step's
   interruption leaves an openable bundle. `Keyfile::rewrap` is called, not approximated. Failure
   to confirm the deletion is an `Err` with the asset named (`rekey.rs:218-226`), not swallowed —
   T-4-45's *mechanism* is right. Its *property* is not: NEW-3. "Not revocation" is stated in the
   CLI on the way in and out (`cli.rs:643`, `:672`), in `docs/sync-github.md:204-208`, and in
   `docs/sync-format.md:743-747`.
6. **Secret hygiene.** `PushCtx` carries no `Debug` and says why (`mod.rs:223-225`). Nothing under
   `src/sync/push/` prints; progress goes to stderr only (`progress.rs:47,107,157`), outcome to
   stdout — T-4-27 closed. Remote text reaches the terminal only through `http::message_of` →
   `excerpt` → `sanitize_untrusted_field`, truncated to 200 bytes and delimited
   (`http.rs:328-350`) — Phase 3's F-4 stayed fixed. The signed storage URL is never logged; the
   second hop builds an anonymous client with no default headers (`write.rs:523-548`), asserted by
   `no_third_http_client_is_built_under_src_sync`, which I confirmed goes red. Passwords reach
   `rekey::run` only as `Zeroizing` arguments from a TTY prompt; `grep` over `src/sync/push/`
   finds no `std::env`, `env::var` or `var_os`. T-4-01, T-4-10, T-4-25, T-4-47, T-4-48 closed —
   but the *guard* protecting T-4-47 does not cover the file (F-5b).
7. **The guards.** Ten negative controls run by hand. Results below.

---

## Guard negative controls — every one run, injected and reverted

| Guard | Injection | Result |
|---|---|---|
| `no_repository_creating_endpoint_is_reachable_from_the_crate` | `/user/repos` in a **doc comment** in `push/prune.rs` — the exact case 4-01's guard failed on | **RED** ✅ |
| `every_request_body_in_this_directory_lives_in_write_rs` | `.delete(` in `github/gate.rs` production code | **RED** ✅ |
| `no_third_http_client_is_built_under_src_sync` | `reqwest::Client::builder()` in `push/upload.rs` | **RED** ✅ |
| `nothing_in_this_file_issues_a_delete_request` | `delete_asset` literal in `push/pointer.rs` | **RED** ✅ |
| `the_push_path_seals_only_the_formats_four_object_kinds` | a fifth `.seal(ctx.keys)?` call site in `push/prune.rs` | **RED** ✅ |
| `both_pack_size_constants_are_pinned…` | `PACK_MAX` 48 MiB → 96 MiB | **RED** ✅ |
| `no_password_input_path_reads_the_process_environment` | `std::env::var` in `github/setup.rs` (a listed file) | **RED** ✅ |
| `no_password_input_path_reads_the_process_environment` | `std::env::var` in `push/rekey.rs` (**not** listed) | **GREEN** ❌ F-5b |
| `only_the_crypto_module_imports_the_cryptographic_crates` | `use chacha20poly1305::…` in `push/packer.rs` | **GREEN** ❌ F-5a |
| `the_cli_says_plainly_that_a_password_change_is_not_revocation` | removed the **pre-prompt** warning (`cli.rs:643`) | **GREEN** ❌ F-8 |
| `the_cli_says_plainly_that_a_password_change_is_not_revocation` | removed the **post-success** note (`cli.rs:672`) | **RED** ✅ |

The 4-01 REPO-03 defect the prompt named is genuinely fixed: the guard now excludes only `file!()`,
asserts `skipped == 1` and `scanned > 50`, and I confirmed it fires on a fragment inside a doc
comment in a file two directories away. `every_request_body_…` and `no_third_http_client_…` both
learned the `#[cfg(test)]`-in-a-doc-comment lesson correctly — the first assembles its needles at
runtime and skips nothing, the second splits on the full `\n#[cfg(test)]\nmod tests` header and
asserts `split_files > 5` for non-vacuity.

Working tree verified clean (`git status --porcelain` empty) and all eleven guards re-run green
after the last revert.

## F-8 — informational — three guards prove less than their prose claims

- **`the_cli_says_plainly_that_a_password_change_is_not_revocation`** (`rekey.rs:893`) matches the
  lowercase string `"not revocation"`. Only the post-success `println!` (`cli.rs:672`) satisfies
  it; the pre-prompt warning at `cli.rs:643` says "NOT revocation" in caps and is **unpinned** —
  deleting it leaves the guard green, proven above. The warning shown *before* the user commits to
  the operation is the one that changes a decision.
- **`nothing_in_this_file_issues_a_delete_request`** (`pointer.rs:773`) is real, but its doc claims
  "a losing race costs one extra round trip, never remote data" — and pointer.rs's actual
  destructive capability is `put_contents` publishing a pointer that drops records, not
  `delete_asset`. The guard proves the narrow property and the doc claims the broad one.
- **`list_assets`' page cap** (`write.rs:88`) bounds *pages* at 10, not assets: a hostile page may
  carry far more than `ASSETS_PER_PAGE`, so the real ceiling is 10 × (2 MiB ÷ ~150 B) ≈ 140k
  assets, not the 1,000 the constant's derivation names. Memory-bounded, so T-4-06 stands; the
  comment overstates the tightness.

---

## Controls confirmed sound

The `landed`-pointer rule and `PRUNE_GRACE` are genuinely independent halves and both are real
(`prune.rs:99-108`, `push/mod.rs:89`). D2's mandatory order is structural, not a step: truncation
happens *inside the pointer being written* (`mod.rs:350-352`), so the record is dropped by the
flip and `prune::run` is only reachable after `commit` returns. `asset_kind` is a strict
allow-list — `pack-<hex>.bin.bak` is not a pack — so an unrecognised object is never collected.
The zero-snapshot case returns explicitly rather than falling out of an empty union
(`prune.rs:90-92`), and `keep.max(1)` guards a zero arriving by any route. `forget_chunks` sees
confirmed deletions only, and the failure direction costs a re-upload.

`reusable` (`packer.rs:216-226`) is the sharpest control in the phase: a chunk is reusable only
when its pack is named by a snapshot the **pointer** already carries — the remote's own evidence
that the pack landed and has not been pruned — so a crashed push's stale index rows cannot produce
a snapshot referencing a pack no remote ever saw. `referenced_packs` is the union over
`index_object.entries` *and* `index_chunks`, so the manifest's and the index object's own packs are
both named (T-4-13 closed). The manifest carries the root-prefixed relative encoding
(`packer.rs:391-397`), and a file under no root is an error rather than an absolute path — 4-CONTEXT's
Phase-5 defect is fixed at the source as instructed.

`with_retry` refuses `Unauthorized`, `Forbidden`, `NotFound` and `Conflict`, leaving
`pointer::commit`'s bounded single retry as the only path past a 409 (T-4-08, T-4-29, T-4-30).
`put_contents` maps its 422 to `Conflict` at the one call site that knows it omitted the `sha`,
and nowhere else. The `sha` precondition is `skip_serializing_if` rather than a null. The
bearer-token redirect leak is closed twice — the same-origin policy refuses the hop, and the
second request is issued from a client with no default headers built inside `follow_unauthenticated`.
Upload verification recomputes `content_address` over what came back and fails `run` before the
orchestrator can reach the flip. `decide` requires name **and** size **and** the uploaded state.
The concurrency window is hand-polled rather than spawned, and the module doc explains honestly
that this is because `Pushing` is not `Clone` — the borrow of the permit survives, which is the
right reason. `PACK_MAX`/`PACK_TARGET` are both pinned with `PACK_MAX` named as governing, and
the pin's non-vacuity leg re-checks `should_seal` against it.

---

## Accepted residual risks, each with its backstop stated

- **T-4-SC** (×7, low) — zero new crates. Verified: `Cargo.toml` and `Cargo.lock` unchanged across
  all seven plans.
- **T-4-46** (high, accept) — an old password still opens a leaked keyfile copy. Inherent to a
  rewrap, mitigated by disclosure in three places. **Note that NEW-3 turns this accepted risk into
  a mitigated-threat failure**: the exposure is no longer limited to a copy someone took, because
  the tool republishes one.
- **`download_asset`'s second hop follows a host-named `Location` with no scheme or origin check**
  (`write.rs:495-506`, `:523-548`). Deliberate and correct for T-4-01 — the token is stripped —
  but an attacker able to control `api.github.com` responses can direct an unauthenticated GET at
  an arbitrary URL and see up to 200 sanitized bytes of the reply through `http::classify`.
  Requires controlling TLS for `api.github.com`, which is Phase 3's F-4 threshold. Residual.

---

## Process flags

- **A leftover injected violation was sitting in the working tree at audit start.**
  `src/sync/plan.rs:234` read `let _ = category_start; // TEMP: sort disabled for verification`,
  replacing `plan.file_plans[category_start..].sort_by(|a, b| a.path.cmp(&b.path))`. It reverted
  on its own moments later — a concurrent process mid-negative-control — so it is not a defect in
  the phase. Recording it because that sort is what keeps chunk addresses independent of
  filesystem enumeration order, and a negative control left in place is how one ships.
- **No `<config>` block in any of the seven plans.** `asvs_level` and `block_on` were inherited
  from Phase 1 and Phase 3 (`2` and `high`). Same class as Phase 1's flag about `1-09-PLAN.md`.
- **Six of seven summaries carry no `## Threat Flags` section at all**; only `4-04-SUMMARY.md`
  has one, and it says "None." Phase 4 added the first outbound data path and the first remote
  delete in the project. An absent section is not an empty one.

---

## Carry-forward — Phase 5 must not ship without these

1. **`RemoteIndexEntry.offset`, `clen` and `true_len` are unauthenticated plaintext** read from
   the pointer, and Phase 5 will slice packs at them. Phase 4 only echoes them, so it needed no
   bound; Phase 5 needs one **before** any allocation or `seek`, and must check
   `offset + clen <= pack_len` against the pack actually fetched, not against a claimed size.
   This is Phase 1's "an id list read before its container authenticates needs its own bound",
   arriving at the phase that finally dereferences it.
2. **NEW-2's anchor read** must exist before restore trusts a pointer, and the push-side read
   proposed above is the same comparison — do not implement it twice differently.
3. **NEW-1's counter collision** makes "select the newest by counter" ill-defined. Phase 5's
   selection must break ties deterministically and loudly rather than picking the first match.

---

## Register verification

67 register entries across seven plans (T-4-01…T-4-56 plus T-4-35b, T-4-42b, T-4-43b, and
T-4-SC in each of the seven).

**Open:**

| Threat | Category | Severity | Disposition | Status |
|---|---|---|---|---|
| T-4-04 | Tampering | high | accept | **OPEN — blocking.** The named backstop (the local anchor's counter) is not read anywhere on the push path (NEW-2) |
| T-4-45 | Repudiation | critical | mitigate | **OPEN — blocking.** The confirming re-list is present, but the old wrapper survives system-wide via `ensure_keyfile` (NEW-3) |
| NEW-1 | Tampering | high | *unregistered* | **OPEN — blocking.** Snapshot counter collision under ordinary two-machine concurrency |

**Closed:** the other 64 entries, verified against code rather than against the plans' prose.
T-4-01 `write.rs:476-548` · T-4-02 `mod.rs:304,320,370` · T-4-03 `gate.rs:228,244`,
`mod.rs:404-414` · T-4-05 `write.rs:68,72`, `vendor.rs:88-99` · T-4-06 `write.rs:88,310-344` ·
T-4-07 `write.rs:143-176` · T-4-08 `write.rs:159-171`, `pointer.rs:178-180` ·
T-4-09 `http.rs:328-350` · T-4-10 `mod.rs:223-225`, `rekey.rs:876-891` ·
T-4-11 `prune.rs:99-108`, `mod.rs:89` · T-4-12/13 `packer.rs:179-190,216-226` ·
T-4-14 `tests/sync_push_e2e.rs:1497-1533` · T-4-15/16 `packer.rs:249-268` ·
T-4-17 `tests/sync_push_e2e.rs:1417-1489` · T-4-18 `packer.rs:174` ·
T-4-19/20 `upload.rs:214-220,271-303` · T-4-21 `upload.rs:14-26` · T-4-22 `write.rs:491` ·
T-4-23 `upload.rs:50,229-267` · T-4-24 `upload.rs:61-63` · T-4-25/27 `progress.rs:47,107,157` ·
T-4-26 `upload.rs:98-113` · T-4-28/29/30 `pointer.rs:134-170`, `mod.rs:336-368` ·
T-4-31 `pointer.rs:68-77,157` · T-4-32 `mod.rs:354-360` · T-4-33 `pointer.rs:61-66,194-201` ·
T-4-34 `pointer.rs:30` · T-4-35 `prune.rs:139-150` · T-4-35b `prune.rs:101` ·
T-4-36 `prune.rs:104` *(see F-7)* · T-4-37 `prune.rs:90-92` · T-4-38 `prune.rs:120-129` ·
T-4-39 `prune.rs:7-22,139` · T-4-40 `mod.rs:391-395,258-263` · T-4-41 `write.rs:88`,
`prune.rs:157-183` · T-4-42 `prune.rs:191` · T-4-42b `prune.rs:215-220` ·
T-4-43 `rekey.rs:89` · T-4-43b `rekey.rs:97` · T-4-44 `rekey.rs:115-164` ·
T-4-46 accepted · T-4-47 `rekey.rs:31-34`, `cli.rs:644-654` · T-4-48 `rekey.rs:876-891` ·
T-4-49 `rekey.rs:236-245` · T-4-50 `mod.rs:354-360` · T-4-51 hermetic fixtures throughout ·
T-4-52 `tests/sync_push_e2e.rs` request/byte assertions · T-4-53/54 guards, both proven red ·
T-4-55 `docs/sync-github.md:204-208`, `docs/sync-format.md:743-747` ·
T-4-56 `docs/sync-github.md:233-240` · T-4-SC ×7 `Cargo.toml` unchanged.

**Unregistered flags:** none declared; NEW-1 is the unregistered attack surface, reported above as
a finding rather than a flag because it is exploitable without an attacker.

---

**threats_open:** 3
