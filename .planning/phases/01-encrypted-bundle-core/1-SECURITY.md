# Phase 1 — Security Audit

**Verdict:** `SECURED` — both blockers closed and independently re-verified; one low-severity
item deliberately deferred to Phase 2 with its trigger recorded.
**ASVS level:** 2, with L3 depth on the AEAD/nonce path. `block_on: high`.
**Final state:** blocking open **0** · non-blocking open **0** · deferred **1** (NEW-3)

Re-verification was performed by an auditor that did **not** write the fixes and was instructed
to reproduce each finding from the current code rather than review the diff. It enumerated all
six AEAD call sites in the crate, and confirmed the id pins were untouched by extracting every
hex literal from `tests/sync_vectors.rs` at the pre-remediation commit and diffing against
`HEAD` — evidence independent of whether the suite passes. That mattered: the original F-1 flaw
passed all thirteen adversarial tests.

It also found that the first remediation had **re-introduced the very defect class it fixed
elsewhere in the same commit** — documentation asserting behaviour that does not exist (F-4b) —
and had left the F-4 floor seam `pub` while narrowing `Keys::seal` to `pub(crate)` for exactly
that reason. Both are now closed. Round two corrected six such statements, two more than the
audit named, including the user-facing `WEAKENED_KDF` message and this file.

Audited adversarially from the format and threat model directly. The existing adversarial test
suite was explicitly **not** treated as proof — it was written by the same effort that wrote the
code and shares its blind spots. That instruction paid: the blocking finding below is invisible
to all thirteen of those tests, and to the verifier.

## Threat model audited against

The bundle sits in a hosted GitHub repo and carries **live OAuth credentials**. An attacker may
obtain the whole thing and grind the password offline with no rate limit; an attacker with repo
write access may tamper, reorder, truncate, substitute or roll back any object; the host sees
every ciphertext, size and access pattern.

---

## F-1 — BLOCKER — the chunk nonce is derived from a *pre-image* of the message

`crypto.rs:383-393` (`Keys::seal`), `crypto.rs:489-494` (`nonce_for`), `chunk.rs:154-162`
(`seal_chunk`).

`seal_chunk` computes `id = chunk_id(data)` over the **raw plaintext**, then encrypts
`frame(data)` — the zstd-compressed frame — under `nonce_for(id)`.

So the nonce is a function of `P`, while the message actually sealed is `f_zstd(P)`. What makes
a derived nonce safe is **nonce ↔ message** injectivity; this code only has nonce ↔ *pre-image*
injectivity. `f_zstd` is not a function of `P` alone — it depends on the linked libzstd version,
and upstream guarantees format compatibility, **not byte-identical output**.

Two distinct messages sealed under one `(chunk_key, nonce, aad)` is textbook AEAD nonce reuse:

1. **Poly1305 one-time-key recovery → existential forgery.** Two valid `(message, tag)` pairs
   under one `(key, nonce)` give a solvable polynomial for `r`. The `chunk_id(plaintext) == id`
   recheck in `chunk.rs:174` contains the content-substitution leg — which is why this is high
   rather than critical — but that is a second line of defence doing the first line's job, and
   `Keys::open` is `pub` with the recheck one layer up.
2. **Keystream recovery.** `C_A ⊕ C_B = F_A ⊕ F_B`. Both frames open with an identical 4-byte
   `true_len` (exactly `262144` for full chunks) — free known keystream at that nonce.

**Reachability is ordinary, not exotic:** the same plaintext chunk sealed twice under two zstd
builds, both landing in the repo. A heterogeneous fleet (laptop on vN, desktop on vN+1 after a
routine crate bump), the orphan packs `pack.rs:48-53` already expects from crashed syncs, and
Phase 4's prune-repack all produce it. Dedup skips the *upload* when the id is indexed, but an
orphan pack, a cold cache or a concurrent sync defeats that.

**Origin, recorded honestly:** this was *introduced* by the fix for blocker B2. Before it,
`id` addressed the frame, so nonce and message were the same object — injective and safe, but
dedup broke on a zstd bump. Fixing dedup broke the nonce, and nobody noticed because everyone
was looking at dedup. Three places in the codebase currently document the trigger as
"harmless" (`chunk.rs:16`, `docs/sync-format.md:156`, `tests/sync_vectors.rs`) — true for
dedup, false for nonce safety. That phrasing was propagated by the orchestrator as an
invariant and must be corrected wherever it appears.

**Remediation applied:** derive the nonce from the bytes actually encrypted and store it
inline, exactly as `seal_root` already does:

```
nonce  = derive_key(CTX_NONCE, keyed_hash(name_key, framed))[..24]
sealed = nonce ‖ XChaCha20Poly1305(chunk_key).encrypt(nonce, framed, aad = id)
```

`id` stays `keyed_hash(name_key, plaintext)`, so dedup keys on an unchanged address and every
**id** pin in `sync_vectors.rs` holds; **ciphertext** pins move, which is exactly the
distinction 1-07 was built to make legible. Cost: 24 bytes per chunk (0.009% at 256 KiB).
`Keys::seal` narrowed to `pub(crate)` with the `(id, message)` binding stated as a safety
contract.

---

## F-3 — BLOCKER — the rollback anchor's key is attacker-influenced, and the constraint is written nowhere

`anchor.rs:67-96` (`accept`), `anchor.rs:104-119` (`read_from`), `model.rs:349-356`.

The anchor is the only defence against replay of *authentic old snapshots* — the one attack
that authenticates perfectly. Its `repo_id` guard is correct. But completeness rests on an
unwritten precondition: **the path given to `read_from` must not be derived from the remote's
claimed `repo_id`.**

`accept(None, …)` returns `Ok(())` *before* comparing `repo_id`. So if a later phase shards
anchors as `anchors/<repo_id>.json` — the obvious way to support multiple bundles, and
unavoidable eventually since one `Anchor` holds exactly one `repo_id` — a served root with a
different `repo_id` resolves to an absent anchor, reads as first contact, and is accepted. The
guard becomes vacuous and rollback protection is silently nullified by a *reasonable* refactor.

Compounding: `Root::open` never compares `repo_id` to anything, and the root's AAD is a fixed
literal rather than repo-scoped, so a root from bundle A opens as bundle B whenever they share
a master key.

**Remediation applied:** the constraint stated in `anchor.rs`'s module doc and
`docs/sync-format.md` §9; `repo_id` bound into the root's AAD so a repo swap fails the Poly1305
tag instead of depending on local state.

---

## F-2 — non-blocking (medium) — `check_memory_budget` has zero production call sites

`crypto.rs:513-524` (the function), `crypto.rs:309-349` (`unwrap_master_key`, where it is absent).

Verified: every call site is a unit test or the `#[ignore]`d CAL-3 probe. `unwrap_master_key`
passes `m_kib` straight from attacker-supplied JSON to `derive_kek`. `argon2` 0.5.3 sets
`MAX_M_COST = u32::MAX` and allocates with an **infallible** `vec![]` — so an attacker editing
one integer gets `handle_alloc_error` → abort, or an OOM-kill. The AAD binding makes the unwrap
*fail*, but only after the allocation. This also violates the project's hard invariant that the
widget always exits 0.

**Remediation applied:** a `MAX_KDF_MEMORY_KIB` ceiling checked inside `unwrap_master_key`,
before `derive_kek`, alongside the version check.

---

## F-4 — non-blocking (medium) — the passphrase floor and the KDF cost are calibrated against each other, uncoupled

`passphrase.rs:1-13,126-132`, `crypto.rs:60-79,282-307`.

The 12-character floor is derived from "~10^5 Argon2id guesses per second — the rate the locked
parameters buy". Nothing enforces those parameters: `Keyfile::wrap` accepts anything clearing
argon2's own 8 KiB floor, and `check_memory_budget`'s message actively suggests lowering it. At
m=8 KiB a 12-character password is trivially crackable offline.

Backstop keeping this at medium: `generate()` is the documented default and yields 100 bits,
uncrackable at any KDF cost. The residual bites only user-supplied password + lowered KDF.

**Remediation applied:** `MIN_KDF_MEMORY_KIB` enforced on the write path, plus coupling — below
the default memory, `passphrase::check` raises the accepted length from 12 characters to 20.

**Corrected in 1-11 (F-4b):** this line first read "rejects anything short of generated
strength", which the code has never done. `check` counts characters and `GENERATED_CHARS ==
RECOMMENDED_CHARS == 20`, so a typed 20-character password and a generated one are one input to
it. The coupling is real but is a *length* rule; the entropy claim was not implemented anywhere.

---

## Controls verified sound

Nonce/key-pair collision for the deliberate random-nonce root exception (OS CSPRNG, 192-bit
nonce, disjoint key domain). Unkeyed hashing — exactly one production `content_address` call,
naming a pack whose bytes are already public. AAD binding across chunks, pack headers,
manifests, index objects and the wrapped master key, with the key doc serialized as a struct so
field order is declaration order rather than hash-iteration order. In-transit KDF downgrade
genuinely closed (salt inside the AAD-bound doc; the test mutates the *serialized* form).
Error-message oracle line drawn correctly — wrong password and in-range downgrade collapse by
cryptographic necessity, and that collision is pinned by a test. Resource exhaustion closed:
bounds precede allocation everywhere, and nothing unauthenticated is read except a
bounds-checked 36-byte trailer. Secret hygiene wired, not merely imported — `Zeroizing`
throughout, hand-written `Debug`, zero `env`/`argv` reads under `src/sync/`, mode checked on the
opened handle so there is no TOCTOU. Reordering closed by construction at both levels.
Version evolution at-or-below everywhere, membership for the chunker.

## Accepted residual risks, each with a real backstop in prose

No `mlock` (documented). Aggregate size/timing leakage (documented, with the honest reason that
constant-rate cover traffic is the only real fix). First-contact TOFU (documented in those
words in both `anchor.rs` and the format doc). Password change is not revocation (documented,
including that future data is exposed to an old-keyfile holder). Six new crates, all pinned and
matching the audited set.

## Process flags

- **`1-09-PLAN.md` carries no `<threat_model>` block** while holding `model.rs` edit authority
  and changing on-disk shape. Its summary asserts "None new" — an assertion, not a register.
  The plan was written by the orchestrator; the omission is the orchestrator's.
- **`Root.kdf` vs the keyfile's KDF:** `model.rs:286-289` says a mismatch "is a signal worth
  reporting". No code compares them — documentation describing behaviour that does not exist.
- **`manifest_chunks` is unbounded**, correctly assessed as safe because it sits inside
  authenticated plaintext. The carry-forward it names — any Phase 2 path reading an id list
  *before* its container authenticates needs its own bound — is tracked in the Phase 2 context.

---

## Remediation status

| Finding | Severity | Status |
|---|---|---|
| F-1 nonce ↔ message injectivity | blocker | remediated in `1-10` |
| F-3 anchor keying + root repo scoping | blocker | remediated in `1-10` |
| F-2 KDF memory ceiling | medium | remediated in `1-10` |
| F-4 KDF floor coupled to passphrase policy | medium | remediated in `1-10` |
| `Root.kdf` comparison | flag | remediated in `1-10` |

Re-verification after remediation is by an agent that did **not** write the fix, reproducing
each issue independently rather than reviewing the diff.
