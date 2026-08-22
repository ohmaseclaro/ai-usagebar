# Encryption design — password-protected sync to a private GitHub repo

**Researched:** 2026-08-19 · **Target:** `ai-usagebar` (Rust 1.96 toolchain, `rust-version = 1.88`, edition 2024)
**Confidence:** HIGH on crates/APIs (compiled and run locally, see §8), HIGH on KDF/AEAD choice (RFC + OWASP + measured), MEDIUM on attacker-throughput estimates (derived from memory-bandwidth first principles, not a published Argon2id hashcat table).

**Threat model as given:** attacker obtains the complete git repo (all history), mounts an unlimited offline attack on the password. Attacker may also *serve* a modified repo (swap/rollback/truncate).

---

## 0. Two decisions to make before any crypto

### 0.1 Do not sync OAuth credentials by default

`~/.claude/.credentials.json`, `~/.codex/auth.json`, the Kiro SSO refresh token, the Cursor `state.vscdb` token — these are bearer credentials for paid accounts. Putting them in a hosted git repo means a *single* password now stands between an attacker and every provider account, forever, because **git history is append-only**: once a wrapped key or a chunk is pushed, deleting it later does not remove it from clones, forks, or GitHub's dangling-object storage.

Recommendation: `sync.include_credentials = false` by default, opt-in with an explicit warning. Config + routines + chat indexes cover the actual "continue on another machine" need; OAuth re-auth on the new machine takes 10 seconds and is what every vendor's CLI already does. This is the cheapest security control available and it costs no code.

### 0.2 Consider not writing this at all

The `age` crate (`age = "0.12.1"`, actively maintained, spec-audited) gives passphrase encryption with scrypt + ChaCha20-Poly1305 STREAM in one dependency. It does **not** give you: a wrapped master key (so password change = rewriting every file header), content addressing, or a rollback-resistant snapshot root. Those three are hard requirements here, so a small purpose-built format is justified — but it should be *small*. The design below is ~250 lines of Rust and adds 5 direct dependencies.

---

## 1. KDF — Argon2id

### Recommended parameters

| Parameter | Value | Rationale |
|---|---|---|
| Algorithm | **Argon2id**, version 0x13 | RFC 9106 §4 step 2: "If you do not know the difference between them or you consider side-channel attacks as a viable threat, choose Argon2id." |
| `m_cost` | **1 048 576 KiB (1 GiB)** | Between RFC 9106's SECOND (64 MiB) and FIRST (2 GiB) recommendation; see cost analysis below. |
| `t_cost` | **3** | ~1.4 s measured (below); RFC's own second option uses t=3. |
| `p_cost` (lanes) | **1** | See note. |
| Salt | **16 random bytes**, per repo | RFC 9106 §3.1: "16 bytes is RECOMMENDED for password hashing." |
| Output | **32 bytes** | RFC 9106 recommends a 256-bit tag. |
| Secret (`K`) / AD (`X`) | **unused** | There is no server-side pepper to hide; adding one just creates a second thing to lose. |

**Why `p=1` and not RFC 9106's `p=4`:** RFC 9106 step 3 says to set lanes to the number of threads the *defender* can use in parallel. `argon2 0.5.3` has no threading (`rayon` support only lands in the 0.6 line, still RC). Measured on an Apple M3 Max at m=1 GiB, t=3: p=1 → 1582 ms, p=4 → 1429 ms. So p>1 buys the defender ~10% and hands a wide-SIMD attacker free intra-hash parallelism. Pick p=1. If you later adopt `argon2 0.6` + `rayon`, bump to p = physical cores *and bump the format version*, since it changes the derived key.

### Measured cost (Apple M3 Max, `--release`, `argon2 0.5.3`)

```
m=  64 MiB  t=3 p=4  ->   109 ms
m= 256 MiB  t=3 p=4  ->   391 ms
m= 512 MiB  t=3 p=4  ->   847 ms
m=1024 MiB  t=1 p=4  ->   787 ms
m=1024 MiB  t=3 p=1  ->  1582 ms   <-- recommended
m=1024 MiB  t=4 p=4  ->  1793 ms
m=2048 MiB  t=1 p=4  ->  1825 ms
```

A mid-range Linux laptop will be 2–3× slower, so budget ~3–4 s. That is charged **once per sync/restore**, amortized over a 115 MB transfer — it is free in UX terms. This is not an interactive login; do not use OWASP's interactive-login minimum here.

### Attacker cost (estimate — MEDIUM confidence)

Argon2id at m=1 GiB, t=3 forces roughly 3–6 GiB of DRAM traffic **and** a 1 GiB live working set per guess. An RTX 4090 (1008 GB/s, 24 GB VRAM) is capped at 24 concurrent guesses and ~168 guesses/s by bandwidth alone; real cracker efficiency lands around 20–40% of that, so **~40–150 guesses/s per top-tier GPU**. A well-funded 1000-GPU farm gets ~10⁵/s.

At m=64 MiB (Bitwarden's default) the same GPU holds 16× more slots and does 16× less traffic — roughly **30–50× cheaper to attack**. The 1 GiB choice is the single highest-leverage decision in this document.

**Caveat you must document:** restoring requires the same 1 GiB allocation. `ai-usagebar` ships aarch64 Linux binaries; a 1 GB Raspberry Pi cannot restore a repo initialised at 1 GiB. Expose `--kdf-memory` at `sync init` time only, store the chosen value in the keyfile, and warn that lowering it weakens every future restore.

### Storing parameters (so they can change later)

Store them **in cleartext next to the salt** in the keyfile, and **bind them into the AEAD as associated data** so an attacker cannot downgrade them without failing authentication:

```json
{
  "format": 1,
  "kdf": { "algo": "argon2id", "version": 19, "m_kib": 1048576, "t": 3, "p": 1,
           "salt": "<base64, 16 bytes>" },
  "nonce": "<base64, 24 bytes>",
  "wrapped_master_key": "<base64, 48 bytes = 32 ct + 16 tag>"
}
```

The AAD for the wrap is the canonical serialization of `format` + `kdf` (see §3). Downgrading `m_kib` to 8 in transit does not help the attacker: the resulting KEK will not unwrap.

To raise parameters later: re-run the KDF with new params and rewrite **only the keyfile**. Data is untouched. This is the same mechanism as password change.

*(Alternative: store a PHC string, `$argon2id$v=19$m=1048576,t=3,p=1$<salt>$`, via `argon2`'s `password-hash` feature. It is one field instead of five and is a documented format — but it invites accidentally storing the *hash* alongside, which is a plaintext-equivalent verifier for offline cracking. Store params only, never a verifier: the AEAD tag is the verifier.)*

### Password-strength minimum

At ~10⁵ guesses/s sustained, one year of cracking covers ~2^41.5 candidates; allow 100 years and 100× budget growth and you need ~2^55 to be safe. Therefore:

- **Default: generate the passphrase.** 20 characters of Crockford base32 from `getrandom` is ~94 bits and needs no embedded wordlist (~3 lines of code). Print once, tell the user to store it in their password manager, and make it clear there is no recovery.
- **If the user supplies one:** hard-reject `< 12` characters; warn below 20. A 6-word EFF diceware phrase is 77.5 bits and is the right thing to suggest in the warning text.
- Skip `zxcvbn` (3.1.1, maintained but ~2 MB of embedded dictionaries) unless the generate-by-default path gets rejected in review. Length floor + generate-by-default covers the realistic failure mode.

### Why not scrypt / PBKDF2

| | Verdict |
|---|---|
| **scrypt** | Fine, not better. `scrypt 0.12.0` is maintained; restic and `age` both use it. Its single `N` knob conflates memory and time, and it has no data-independent first pass, so it is weaker against side-channel-assisted attacks than Argon2id. No reason to choose it for a greenfield format. |
| **PBKDF2** | **No.** Negligible memory hardness → GPU/ASIC-friendly. OWASP's 600 000-iteration PBKDF2-HMAC-SHA256 figure exists for FIPS-140 compliance, not because it is good. The project already depends on `pbkdf2 0.13` — that is for reading Chromium `safeStorage` blobs (a fixed, externally imposed format). **Do not reuse it here.** |

---

## 2. AEAD — XChaCha20-Poly1305

**Choice: `XChaCha20Poly1305` (192-bit nonce) for everything.**

| | XChaCha20-Poly1305 | AES-256-GCM |
|---|---|---|
| Nonce width | 192 bit | 96 bit |
| Random-nonce safety | Collision at ~2^96 messages; libsodium states random nonces are safe for a practically unlimited number of messages | NIST SP 800-38D caps random-nonce use at **2^32 invocations per key** |
| No AES-NI (aarch64 without crypto ext., older x86) | Constant-time, fast in software | Software GHASH is slow **and** table-driven AES is cache-timing-vulnerable |
| Nonce-misuse blast radius | Catastrophic (keystream reuse) | Catastrophic **plus authentication-key recovery** — forgery of *all* future messages under that key |
| RustCrypto advisories | none | RUSTSEC-2023-0096 (`decrypt_in_place_detached` exposed plaintext on tag failure; fixed, but a reminder that the detached API is easy to hold wrong) |

The deciding factor is that GCM's failure mode under nonce reuse is strictly worse and its 2^32 ceiling is a real budget you must track across many independent chunks. XChaCha removes the accounting problem entirely. AES-GCM's only advantage — AES-NI throughput — does not matter for a 115 MB payload where the network and git are the bottleneck.

### Nonce strategy: derive it from the chunk id (deterministic)

For content-addressed chunks, **do not** use a random nonce. Derive it:

```
id    = BLAKE3::keyed_hash(name_key, plaintext)          // 32 bytes, the chunk's address
nonce = BLAKE3::derive_key("…chunk-nonce", id)[..24]     // 24 bytes
ct    = XChaCha20Poly1305(chunk_key).encrypt(nonce, plaintext, aad = id)
```

Why this is safe and why it is better:

- **No nonce reuse is possible.** The nonce is a function of `id`, and `id` is a collision-resistant keyed hash of the plaintext. Two different plaintexts get different ids and therefore different nonces. The only way to repeat a nonce is to encrypt the *identical* plaintext — which produces the identical ciphertext, leaking nothing new (you already published that chunk).
- **Ciphertext is byte-stable.** Re-running sync on unchanged data produces identical blobs → git sees no change → no history churn. With random nonces every re-upload of the same chunk is a new git object and the repo grows without bound.
- **24 bytes saved per chunk** (nonce is recomputed from the filename), and one less field to validate.
- This is the same reasoning as a synthetic-IV / deterministic-AEAD construction, restricted to the case where the "message number" *is* the content hash.

**The mutable snapshot root is the exception** — its plaintext changes on every sync and its nonce cannot be content-derived without leaking equality. Give the root a **fresh random 24-byte nonce** stored inline. XChaCha's 192-bit nonce makes random generation safe with no counter accounting.

### Per-chunk subkeys: not needed

With XChaCha, a single `chunk_key` for all chunks is fine — there is no message-count budget to exhaust. Deriving a per-chunk subkey (`derive_key(ctx, chunk_key || id)`) adds a BLAKE3 call per chunk and buys nothing that the AAD binding does not already give. Skip it.

*(If you were forced onto AES-256-GCM, the per-chunk-subkey derivation would become mandatory to escape the 2^32 bound. That is another argument for XChaCha.)*

---

## 3. Key hierarchy

```
                password (user)   +   salt (16B, in keyfile)
                        │
                        ▼   Argon2id  m=1 GiB  t=3  p=1
                    kek [32B]                              ephemeral, zeroized
                        │
                        │  XChaCha20-Poly1305 unwrap
                        │  aad = canonical_json(format, kdf)   ← binds params, blocks downgrade
                        ▼
                master_key mk [32B]                        random at init, never leaves memory
                        │
        ┌───────────────┼───────────────┬──────────────────┐
        ▼               ▼               ▼                  ▼
  BLAKE3 derive_key with distinct context strings (RFC-8452-style domain separation)
        │               │               │                  │
   chunk_key       name_key        root_key           (future: index_key)
   AEAD for        keyed-BLAKE3    AEAD for the
   chunk bodies    chunk address   snapshot root
```

**Why BLAKE3 `derive_key` and not HKDF-SHA256:** `blake3::derive_key(context, key_material)` is exactly HKDF's extract-then-expand with the context string as a hardcoded, compile-time-constant domain separator. It is one function call with no salt/info/length parameters to get wrong, it is already a dependency you want for content addressing, and it is ~5× faster. `hkdf 0.13` would work equally well cryptographically; it just costs another crate and three more chances to pass the wrong `info`.

**Context strings must be unique, versioned string literals** — BLAKE3's contract is that they be hardcoded, application-specific, and globally unique. Include the format version so a v2 key hierarchy cannot collide with v1.

### Password change without re-encrypting data

```
old password → kek_old → unwrap(keyfile) → mk
new password + fresh salt → kek_new → wrap(mk) → new keyfile
```

One 48-byte file rewritten; 115 MB untouched. Multiple keyfiles = multiple passwords (restic does exactly this — "A repository can have several different passwords, with a key file for each").

**Document the limit honestly:** the old keyfile still exists in git history. Anyone who has ever cloned the repo, plus anyone who obtains it later, can still unwrap `mk` with the *old* password. **Password change is not revocation.** If the old password is believed compromised, the only real remedy is §3.1.

### Key rotation (new master key)

Rotating `mk` changes `name_key`, hence every chunk id, hence every chunk — it is a full re-encrypt. Because git history is append-only, "re-encrypt in place" leaves the old ciphertext reachable forever. So implement rotation as: **generate a fresh `mk`, write to a new orphan branch (or a new repo), force-push, and tell the user to delete + recreate the GitHub repo** if the old data was sensitive. Ship this as `sync rotate --new-repo`, not as an in-place operation, so nobody believes they got revocation when they did not.

### Code sketch (compiled and executed — see §8 for the harness)

```rust
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::XChaCha20Poly1305;
use zeroize::{Zeroize, Zeroizing};

const CTX_CHUNK: &str = "ai-usagebar.sync.v1 chunk-encryption-key";
const CTX_NAME:  &str = "ai-usagebar.sync.v1 chunk-name-key";
const CTX_ROOT:  &str = "ai-usagebar.sync.v1 snapshot-root-key";
const CTX_NONCE: &str = "ai-usagebar.sync.v1 chunk-nonce";

#[derive(Clone, Copy)]
pub struct KdfParams { pub m_kib: u32, pub t: u32, pub p: u32 }

impl Default for KdfParams {
    fn default() -> Self { Self { m_kib: 1_048_576, t: 3, p: 1 } }
}

/// Password + salt -> key-encryption key. Pure: no path, no env, no clock.
pub fn derive_kek(pw: &[u8], salt: &[u8; 16], k: KdfParams) -> Zeroizing<[u8; 32]> {
    let params = Params::new(k.m_kib, k.t, k.p, Some(32)).expect("static params");
    let a2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; 32]);
    a2.hash_password_into(pw, salt, out.as_mut()).expect("argon2");
    out
}

/// `aad` is the canonical serialization of {format, kdf} — binds params, blocks downgrade.
pub fn wrap(kek: &[u8; 32], mk: &[u8; 32], aad: &[u8], nonce: &[u8; 24]) -> Vec<u8> {
    XChaCha20Poly1305::new(kek.into())
        .encrypt(&(*nonce).into(), Payload { msg: mk, aad })
        .expect("wrap")
}

pub fn unwrap(kek: &[u8; 32], ct: &[u8], aad: &[u8], nonce: &[u8; 24])
    -> Option<Zeroizing<[u8; 32]>>
{
    let mut pt = XChaCha20Poly1305::new(kek.into())
        .decrypt(&(*nonce).into(), Payload { msg: ct, aad })
        .ok()?;                       // wrong password / tampered params land here
    let mut mk = Zeroizing::new([0u8; 32]);
    mk.copy_from_slice(&pt);
    pt.zeroize();                     // Vec<u8> from the AEAD is NOT auto-zeroized
    Some(mk)
}

pub struct Keys {
    chunk: Zeroizing<[u8; 32]>,
    name:  Zeroizing<[u8; 32]>,
    pub root: Zeroizing<[u8; 32]>,
}

pub fn subkeys(mk: &[u8; 32]) -> Keys {
    Keys {
        chunk: Zeroizing::new(blake3::derive_key(CTX_CHUNK, mk)),
        name:  Zeroizing::new(blake3::derive_key(CTX_NAME,  mk)),
        root:  Zeroizing::new(blake3::derive_key(CTX_ROOT,  mk)),
    }
}

fn chunk_id(name_key: &[u8; 32], pt: &[u8]) -> [u8; 32] {
    *blake3::keyed_hash(name_key, pt).as_bytes()
}

fn chunk_nonce(id: &[u8; 32]) -> [u8; 24] {
    let d = blake3::derive_key(CTX_NONCE, id);
    let mut n = [0u8; 24];
    n.copy_from_slice(&d[..24]);
    n
}

/// Returns (address, ciphertext||tag). Deterministic: same plaintext -> same bytes.
pub fn seal_chunk(k: &Keys, pt: &[u8]) -> ([u8; 32], Vec<u8>) {
    let id = chunk_id(&k.name, pt);
    let ct = XChaCha20Poly1305::new((&*k.chunk).into())
        .encrypt(&chunk_nonce(&id).into(), Payload { msg: pt, aad: &id })
        .expect("seal");
    (id, ct)
}

/// `id` comes from the (attacker-controlled) filename; binding it as AAD makes
/// a swapped or renamed chunk fail authentication.
pub fn open_chunk(k: &Keys, id: &[u8; 32], ct: &[u8]) -> Option<Vec<u8>> {
    let pt = XChaCha20Poly1305::new((&*k.chunk).into())
        .decrypt(&chunk_nonce(id).into(), Payload { msg: ct, aad: id })
        .ok()?;
    (chunk_id(&k.name, &pt) == *id).then_some(pt)   // belt-and-braces; catches our own bugs
}
```

Note the RustCrypto `aead 0.6` / `hybrid-array 0.4` idiom: `Array::from_slice` is **deprecated**; use `kek.into()` / `nonce.into()` with an explicit `&`. This bites immediately under the project's `clippy -D warnings` gate.

---

## 4. Integrity beyond the AEAD

AEAD protects each blob in isolation. An attacker who controls the repo attacks the *relationships between* blobs. Four attacks, four answers:

| Attack | Defence |
|---|---|
| **Chunk swap** — serve chunk B's bytes under chunk A's name | `id` is bound as **AAD**. Decryption of B's ciphertext under A's id fails the Poly1305 tag. Also caught by the `chunk_id(pt) == id` recheck. |
| **Truncation** — drop the tail of a chunk, or drop chunks from the manifest | The manifest is itself a sealed chunk (AAD = its own id) listing `(path, mode, true_len, [chunk ids])`. Truncating the manifest breaks its tag. Dropping a *referenced* chunk is detected as "chunk missing", never as "shorter file". |
| **Snapshot rollback** — re-serve an old, valid root | Cannot be solved with crypto alone; a rollback is a replay of genuinely authentic data. Requires a **local monotonic anchor** — see below. |
| **Repo substitution** — swap the whole repo for a different one | Master key is per-repo. The wrong repo's keyfile will not unwrap under the user's password. Additionally pin the repo's identity (remote URL + init timestamp) inside the root plaintext. |

### Snapshot root

```
refs/root  =  nonce(24B random) || XChaCha20Poly1305(root_key,
                  plaintext = { format: 1, counter: u64, created_at, repo_id,
                                manifest_id: [u8;32] },
                  aad = b"ai-usagebar.sync.v1 root")
```

The chain `root → manifest_id → manifest → chunk ids → chunks` is authenticated at every hop, and every hop's identifier is bound as AAD into the thing it names. That closes swap and truncation completely.

### Rollback: local anchor + git fast-forward

1. **Local high-water mark.** Persist the last-seen `counter` in the *config* directory (not the wipeable cache), mode 0600. Refuse to restore a root whose `counter` is lower, unless the user passes `--allow-rollback`.
2. **First contact is TOFU.** A brand-new machine has no anchor; accept the first root and pin its counter. Document this residual gap — it is inherent, not a bug.
3. **Refuse non-fast-forward fetches.** Free with git, and it catches history rewrites (the usual delivery mechanism for a rollback) before you even decrypt.

### Do not add signatures

Every machine that can read the repo already holds the same symmetric `mk`, so an Ed25519 root signature is authenticated by a key every reader also possesses — it adds no property the AEAD tag does not already provide. Skip `ed25519-dalek`. (Revisit only if you ever support a *reader-only* sharing mode.)

---

## 5. Metadata leakage

### What leaks even with perfect encryption

| Observable | Leaks |
|---|---|
| Number of chunks | Approximate total payload size |
| Ciphertext lengths | Plaintext lengths (XChaCha is a stream cipher: `len(ct) = len(pt) + 16`) |
| Commit timestamps / frequency | When the user works, on which machines, how often |
| Chunk-id stability across commits | **Which** chunks changed — i.e. change rates per logical file, even though names are opaque |
| Repo/branch names, `.gitattributes`, commit authorship | Identity, machine names, email |

### The content-defined-chunking trap

*Chunking Attacks on File Backup Services using Content-Defined Chunking* (arXiv:2504.02095) shows that CDC boundary positions — visible as encrypted chunk *sizes* — are a fingerprint of the plaintext. It demonstrates known-file detection and content fingerprinting against restic, Borg, and Duplicacy. The recommended mitigation is fixed-size chunking or padding.

**Therefore: use fixed-size chunking, not CDC.** For this payload it is also the better engineering choice:

- Chat indexes are append-only JSONL or page-aligned SQLite. Fixed 1 MiB blocks *aligned to each file's start* dedup near-perfectly against appends and page-level edits — CDC's shift-resilience buys almost nothing here.
- 115 MB / 1 MiB ≈ 120 git objects per full snapshot; deltas are far smaller. Sane for git.
- Chunk each file **independently** (not one concatenated tar), so a change to `config.toml` does not re-chunk the chat indexes.

### Cheap mitigations, in order of value

1. **Opaque names.** `chunks/<hex[0..2]>/<hex-of-keyed-blake3>` — a *keyed* hash, so an attacker without `name_key` cannot confirm a guessed plaintext by recomputing its address. (Restic/Borg use unkeyed content hashes for dedup; that permits confirmation-of-file attacks. Keying costs nothing.) Two-level fanout keeps git's directory listings small.
2. **Pad the tails.** With fixed 1 MiB chunking, every chunk except each file's last is already exactly 1 MiB. Pad each tail to the next power of two (capped at 1 MiB) and length-prefix the plaintext (`u32 true_len || data || zeros`). ~4 lines. Reduces the tail-size signal to `log2(len)`. Padmé is the fancier option if you ever need a tighter bound.
3. **Encrypt the manifest as a normal chunk.** No filenames, sizes, or directory structure in the clear.
4. **Squash the git history.** Amend-and-force-push a single commit per sync, or periodically re-init the branch. Otherwise the commit graph is a detailed timeline of the user's activity — and it grows forever.
5. **Do not name the repo `ai-usagebar-sync`.** The repo name is metadata too.

Accept and document what stays: total size, sync timing, and per-sync change volume. Hiding those requires constant-rate dummy traffic, which is absurd for this feature.

---

## 6. Crates

### Add

```toml
# --- sync encryption ---------------------------------------------------------
# Argon2id password KDF. `default-features = false` drops the PHC-string
# machinery (password-hash) we deliberately do not use: we store KDF params
# ourselves and never store a password *verifier* — the AEAD tag is the verifier.
argon2 = { version = "0.5.3", default-features = false, features = ["alloc", "zeroize"] }
# XChaCha20-Poly1305: 192-bit nonce removes all nonce-budget accounting.
chacha20poly1305 = { version = "0.11", default-features = false, features = ["alloc"] }
# Keyed hashing (content addressing) + derive_key (subkey derivation).
blake3 = { version = "1.8", default-features = false, features = ["std"] }
zeroize = "1.9"
getrandom = "0.4"
```

Verified: resolves and builds clean on this toolchain; 39 transitive crates total, no duplicate-version conflicts with the project's existing `aes 0.9` / `sha2 0.11` tree.

| Crate | Version | Status | Note |
|---|---|---|---|
| `argon2` | **0.5.3** (2024-01-20) | maintained, no RUSTSEC advisories | `0.6.0-rc.8` exists (2026-03-22) and has been in RC for 15 months. **Ship 0.5.3.** It pulls the older RustCrypto core generation (`digest 0.10`, `crypto-common 0.1`, `generic-array 0.14`, `password-hash 0.5`) — a harmless duplicate alongside your `sha2 0.11` tree, costing a little compile time. 0.6 brings `rayon` (parallel lanes) and the new `kdf` trait; revisit when it goes stable and treat the `p` change as a format bump. |
| `chacha20poly1305` | **0.11.0** (2026-06-28) | maintained, no advisories | New `aead 0.6` / `hybrid-array 0.4` generation. `Array::from_slice` is deprecated → use `.into()`. |
| `blake3` | **1.8.6** | maintained, official reference impl | `default-features = false, features = ["std"]` avoids the optional `rayon`/`memmap2`/`digest` pulls. |
| `zeroize` | **1.9.0** | maintained | `Zeroizing<T>` wrapper is all you need; skip the `derive` feature until a struct requires it. |
| `getrandom` | **0.4.3** | maintained | `getrandom::fill(&mut buf)` (renamed from `getrandom()` in 0.3). Verified working. |

### Do not add

| Crate | Why not |
|---|---|
| `aes-gcm 0.11` | See §2. Also carries RUSTSEC-2023-0096 history around `decrypt_in_place_detached`. |
| `rand 0.10` | You need 16 salt bytes and 24 nonce bytes. `getrandom::fill` is the OS CSPRNG directly — no `ThreadRng`, no reseeding, no `RUSTSEC-2026-0097` (unsound `rand::rng()` with a custom logger; patched in 0.10.1, but the whole dependency is unnecessary). |
| `hkdf 0.13` | `blake3::derive_key` covers it with fewer footguns and no extra crate. |
| `secrecy 0.10.3` | Maintained and fine, but it is `Zeroizing` + `Debug` redaction. `zeroize::Zeroizing` alone is enough; add `secrecy` only if key material starts flowing through `#[derive(Debug)]` structs. |
| `ed25519-dalek 3.0` | No asymmetric property is needed (§4). |
| `scrypt 0.12` / `pbkdf2 0.13` | §1. The existing `pbkdf2` dependency is for Chromium `safeStorage` only — do not reuse it here. |
| `zxcvbn 3.1.1` | Maintained, but ~2 MB of dictionaries to replace "generate the passphrase by default + a length floor". |

**No unmaintained crates in this set.** Advisory-DB scan (`RustSec/advisory-db`, fetched 2026-08-19) over `argon2`, `chacha20poly1305`, `blake3`, `zeroize`, `secrecy`, `hkdf`, `getrandom`, `password-hash`, `aes`, `cbc`, `pbkdf2`: zero hits. Only `aes-gcm` (2023, fixed) and `rand` (2026, informational/unsound, patched) had entries — neither is in the recommended set.

---

## 7. Pitfalls

### 7.1 Zeroization — the parts that actually matter

`Zeroizing<[u8; 32]>` handles the derived keys. The leaks are elsewhere:

- **The `Vec<u8>` returned by `Aead::encrypt`/`decrypt` is not zeroized.** Decrypting the wrapped master key yields a plain `Vec<u8>` holding the master key. Copy it out and `.zeroize()` the Vec explicitly (the sketch above does this). Worse, `Vec` may have reallocated during construction — the abandoned buffer is unreachable and unzeroizable. This is unavoidable with the allocating API; prefer `encrypt_in_place`/`decrypt_in_place` on a pre-sized buffer for anything key-shaped.
- **The password string itself.** Read it into a `Zeroizing<String>`. `rpassword`'s `read_password()` returns a plain `String`; wrap it immediately. Never accept the password from `--password` (visible in `/proc/*/cmdline` to every local user) or from an env var (visible in `/proc/*/environ`, leaks into crash dumps, and this project's `CLAUDE.md` already has a rule against `env | grep`). Interactive TTY prompt, or stdin, or a mode-0600 file path — nothing else.
- **Do not `mlock`.** The 1 GiB Argon2 working set cannot be locked on a default Linux `RLIMIT_MEMLOCK` (64 KiB–8 MiB), and `argon2 0.5` gives you no hook to do it anyway. Accept swap exposure and say so in the docs. Half-measures here are theatre.
- **Never log key material, not even lengths at debug level, and never `Debug`-derive a struct that holds one.** If a struct must be `Debug`, hand-implement it to print `<redacted>`.

### 7.2 No plaintext temp files

Restore writes decrypted config and (opt-in) credentials to disk. The project already has the right primitive — reuse it:

- Create the temp file **in the destination directory** via `tempfile::NamedTempFile::new_in(dir)`, then `persist()`. Never `/tmp`: it is world-readable, often a different filesystem (so `persist` degrades to a copy that leaves a plaintext original behind), and may be a tmpfs that survives in swap.
- `NamedTempFile` creates with mode 0600 — but `persist()` **keeps** that mode, so set the final mode explicitly rather than relying on it, exactly as the Settings overlay already `chmod 600`s `config.toml`.
- If a restore fails mid-way, delete partial outputs. A half-written credentials file is worse than none.
- Never stage decrypted data through the git working tree.

### 7.3 Keeping the crypto hermetically testable

The project's hard rule ("a `#[test]` must never read or write a real `$HOME`/`$XDG` path or branch on an ambient env var") is satisfied for free if the crypto layer is written as pure functions:

- **Every function in §3's sketch takes `&[u8]` and explicit params.** No `Path`, no env, no clock, no `Keychain`. They are unit-testable with array literals and nothing else. Keep path resolution, credential reading, and git invocation in a separate module that *calls* these — mirroring how `Cache::at` and `creds::read_from` already separate transform from resolver.
- **Give tests a cheap KDF seam.** `KdfParams { m_kib: 8, t: 1, p: 1 }` runs in microseconds. Take `KdfParams` as an argument everywhere (as the sketch does) rather than reading a const inside `derive_kek` — that is the whole seam. A 1.5 s KDF in a unit test would also break the AUR `check()` budget.
- **Pin a known-answer test.** Hardcode `(password, salt, params) -> expected 32-byte kek` and `(mk, plaintext) -> expected (id, ciphertext)` hex vectors. This is what catches a crate upgrade silently changing semantics (e.g. an `argon2 0.6` bump, or a BLAKE3 context-string typo) — an AEAD round-trip test alone will not, since it stays self-consistent under any wrong-but-stable transform. Add RFC 9106's Argon2id test vector as a separate guard on the crate itself.
- **Test the adversary, not just the happy path.** The §8 harness asserts: wrong password fails, tampered KDF params fail, chunk swap in both directions fails, single bit flip fails, and identical plaintext produces identical ciphertext. Those five assertions are the security properties; a round-trip test proves none of them.
- **No `#[ignore]`d live tests needed.** Unlike the vendor fetchers, this layer has no network dependency at all.

### 7.4 Operational footguns

- **There is no recovery.** Say it in the prompt, in the docs, and in the README. A lost password means lost data — that is the whole point.
- **Widget always exits 0** (existing invariant). Any sync failure surfaced through the Waybar path must still produce the fallback `⚠` JSON, never a nonzero exit.
- **GitHub repo size.** 115 MB per snapshot with append-only history will hit GitHub's soft 1 GB / hard 5 GB limits within a year of daily syncs. Squashing history (§5, item 4) is a correctness requirement here, not just a privacy nicety.
- **`sync init` must refuse a non-empty or public repo** and must verify the remote is actually private via the GitHub API before the first push.

---

## 8. Verification performed

A scratch crate at `/private/tmp/claude-501/-Users-augustoclaro-ohmaseclaro-ai-usagebar/310ad95f-7653-45d2-ac90-38db6e05087e/scratchpad/cryptoprobe` was built and run against the exact crate versions and feature flags recommended in §6. It:

- resolves and compiles clean on `rustc 1.96.0`, edition 2024 (39 transitive crates, no version conflicts with the project's existing tree);
- executes the full §3 key hierarchy — Argon2id → KEK → unwrap master key → BLAKE3 subkeys → seal/open chunks;
- asserts wrong-password failure, tampered-AAD (KDF-param downgrade) failure, chunk-swap failure in both directions, single-bit-flip failure, and deterministic ciphertext for identical plaintext — all pass;
- confirms the AEAD overhead is exactly 16 bytes per chunk with the derived-nonce scheme;
- produced the Argon2id timing table in §1 via a `--release` benchmark binary.

The deprecated-API and borrow-checker issues in the `aead 0.6` / `hybrid-array 0.4` idiom were hit and fixed during that build, so the §3 sketch is the corrected form, not an untested transcription.

---

## Sources

- [RFC 9106 — Argon2 Memory-Hard Function](https://www.rfc-editor.org/rfc/rfc9106.html) — §3.1 salt length, §4 parameter-choice procedure and the two RECOMMENDED options
- [OWASP Password Storage Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html) — Argon2id/scrypt/PBKDF2 interactive-login minimums (deliberately exceeded here)
- [draft-arciszewski-xchacha-03 — XChaCha: eXtended-nonce ChaCha and AEAD_XChaCha20_Poly1305](https://www.ietf.org/archive/id/draft-arciszewski-xchacha-03.xml)
- [libsodium — XChaCha20-Poly1305 construction](https://libsodium.gitbook.io/doc/secret-key_cryptography/aead/chacha20-poly1305/xchacha20-poly1305_construction) — "random nonces are safe to use"
- [Soatok — Understanding Extended-Nonce Constructions](https://soatok.blog/2021/03/12/understanding-extended-nonce-constructions/) — birthday-bound comparison vs the 96-bit nonce case
- [restic — References / Cryptography](https://restic.readthedocs.io/en/stable/100_references.html) — scrypt-wrapped master key, multiple key files for multiple passwords
- [BorgBackup — Security](https://borgbackup.readthedocs.io/en/stable/internals/security.html) — encrypt-then-MAC, manifest-spoofing history, repo-swap analysis
- [arXiv:2504.02095 — Chunking Attacks on File Backup Services using Content-Defined Chunking](https://arxiv.org/pdf/2504.02095) — chunk-size fingerprinting against restic/Borg/Duplicacy; padding and fixed-size chunking as mitigations
- [RustSec advisory database](https://github.com/RustSec/advisory-db) — scanned 2026-08-19; RUSTSEC-2023-0096 (`aes-gcm`), RUSTSEC-2026-0097 (`rand`, informational)
- [BLAKE3 `derive_key` docs](https://docs.rs/blake3/latest/blake3/fn.derive_key.html) — hardcoded, globally unique context-string contract
- crates.io API, queried 2026-08-19, for every version number in §6
