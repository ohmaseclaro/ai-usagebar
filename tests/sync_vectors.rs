//! Pinned known-answer vectors for the encrypted sync bundle format.
//!
//! `src/safe_storage.rs` already pins Chromium compatibility with
//! `key_derivation_matches_the_chromium_compatibility_vector` and
//! `encryption_matches_the_chromium_compatibility_vector`, for exactly one
//! reason: so a crypto-crate upgrade cannot silently change the on-disk format.
//! **D-03** asks for the same here, and this file is it.
//!
//! A round-trip test cannot do this job. It stays self-consistent under any
//! wrong-but-stable transform, so an `argon2` 0.6 bump or a mistyped BLAKE3
//! context string would pass every round trip in the repository while making
//! every bundle already on a user's disk unreadable.
//!
//! # Provenance, per pin
//!
//! These four vectors come from the primitives' **own specifications**, so
//! the expected bytes originate somewhere other than this codebase and the test
//! genuinely guards the crate. Each literal names its source above it, in the
//! style of `safe_storage.rs`'s "Independently reproduced with OpenSSL's
//! PBKDF2-HMAC-SHA1 implementation". This is the only file outside
//! `src/sync/crypto.rs` that reaches for `argon2` or `chacha20poly1305`
//! directly, and it does so deliberately: calling the primitive itself is what
//! makes it a guard on the crate rather than on our wrapper. (The containment
//! invariant `only_the_crypto_module_imports_the_cryptographic_crates` walks
//! `src/sync/`, so it is unaffected.)
//!
//! # Crate versions these vectors were pinned against
//!
//! `argon2` 0.5.3, `chacha20poly1305` 0.11.0, `blake3` 1.8.6, `zstd` 0.13.3,
//! `zeroize` 1.9.0, `getrandom` 0.4.2.
//!
//! Hermetic: no `$HOME`, no `$XDG`, no Keychain, no network, no clock, and no
//! randomness — the AUR `check()` runs this file on an installer's machine.

use argon2::{Algorithm, Argon2, AssociatedData, ParamsBuilder, Version};
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Lowercase hex, so a failed assertion prints something a human can diff
/// against the literal above it rather than a wall of decimal bytes.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Decode a hex literal, ignoring whitespace, so a specification's hex block can
/// be pasted in exactly as it is printed there.
fn unhex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    assert_eq!(digits.len() % 2, 0, "a hex literal must have even length");
    digits
        .chunks(2)
        .map(|pair| {
            let s = std::str::from_utf8(pair).expect("ascii hex");
            u8::from_str_radix(s, 16).expect("valid hex")
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Part 1 — primitive vectors, each from the primitive's own specification
// ---------------------------------------------------------------------------

/// Argon2id against RFC 9106 §5.3, the parameter set and tag the specification
/// itself publishes.
///
/// This guards the **crate**, not our format: our own parameters are elsewhere
/// (`KdfParams::default`), and this vector's `p = 4`, secret and associated data
/// are things the bundle format never uses. If `argon2` ever changes what it
/// computes, this is the test that says so.
#[test]
fn argon2_crate_matches_the_rfc_9106_test_vector() {
    // RFC 9106 §5.3, "Argon2id Test Vectors": memory 32 KiB, 3 passes, 4 lanes,
    // 32-byte tag, version number 19.
    let params = ParamsBuilder::new()
        .m_cost(32)
        .t_cost(3)
        .p_cost(4)
        .output_len(32)
        .data(AssociatedData::new(&[0x04; 12]).expect("12 bytes of associated data"))
        .build()
        .expect("the RFC's parameters are valid");
    let argon = Argon2::new_with_secret(&[0x03; 8], Algorithm::Argon2id, Version::V0x13, params)
        .expect("an 8-byte secret");

    let mut tag = [0u8; 32];
    argon
        .hash_password_into(&[0x01; 32], &[0x02; 16], &mut tag)
        .expect("hashing at the RFC's parameters");

    // RFC 9106 §5.3, "Tag:". Copied from
    // https://www.rfc-editor.org/rfc/rfc9106.txt — not produced by this
    // codebase, which is the entire point of a primitive guard.
    assert_eq!(
        hex(&tag),
        "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659",
        "argon2 no longer computes RFC 9106's published Argon2id tag — the \
         crate's semantics changed, and every KEK it derives changed with them"
    );
}

/// The 32-byte ASCII key the official BLAKE3 test vectors use, given in their
/// `key` field.
const B3_KEY: &[u8; 32] = b"whats the Elvish word for friend";

/// The context string the official BLAKE3 test vectors use, given in their
/// `context_string` field.
const B3_CONTEXT: &str = "BLAKE3 2019-12-27 16:29:52 test vectors context";

/// The input every official BLAKE3 case uses: "a repeating sequence of 251
/// bytes: 0, 1, 2, ..., 249, 250, 0, 1, ...".
fn b3_input(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// BLAKE3 `keyed_hash` — the mode that addresses every chunk in the format.
#[test]
fn blake3_keyed_hash_matches_the_official_reference_test_vector() {
    // https://raw.githubusercontent.com/BLAKE3-team/BLAKE3/master/test_vectors/test_vectors.json,
    // the `input_len: 1024` case, first 32 bytes of its `keyed_hash` output.
    // 1024 is BLAKE3's own chunk size, so this case crosses the tree-hashing
    // boundary a single-block vector would never reach.
    assert_eq!(
        hex(blake3::keyed_hash(B3_KEY, &b3_input(1024)).as_bytes()),
        "75c46f6f3d9eb4f55ecaaee480db732e6c2105546f1e675003687c31719c7ba4",
        "blake3::keyed_hash no longer matches the official reference vectors — \
         every chunk id in the format is this function"
    );
}

/// BLAKE3 `derive_key` — the mode that splits the master key into three
/// subkeys. This is the one that matters most: a typo in a context string is
/// invisible to every round-trip test in the repository and silently forks the
/// key hierarchy.
#[test]
fn blake3_derive_key_matches_the_official_reference_test_vector() {
    // The same source and the same `input_len: 1024` case, first 32 bytes of its
    // `derive_key` output, using the vectors' own `context_string`.
    assert_eq!(
        hex(&blake3::derive_key(B3_CONTEXT, &b3_input(1024))),
        "7356cd7720d5b66b6d0697eb3177d9f8d73a4a5c5e968896eb6a689684302706",
        "blake3::derive_key no longer matches the official reference vectors — \
         the whole subkey hierarchy is derived through this function"
    );
}

/// XChaCha20-Poly1305 against the published AEAD vector, key, 24-byte nonce,
/// associated data, plaintext, ciphertext and tag all from the draft.
#[test]
fn xchacha20poly1305_matches_the_draft_arciszewski_xchacha_03_test_vector() {
    // draft-arciszewski-xchacha-03 §A.3.1, "AEAD_XCHACHA20_POLY1305", the
    // developer-friendly form. Fetched from
    // https://www.ietf.org/archive/id/draft-arciszewski-xchacha-03.txt
    let key = unhex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
    let nonce = unhex("404142434445464748494a4b4c4d4e4f5051525354555657");
    let aad = unhex("50515253c0c1c2c3c4c5c6c7");
    let plaintext = unhex(
        "4c616469657320616e642047656e746c656d656e206f662074686520636c6173
         73206f66202739393a204966204920636f756c64206f6666657220796f75206f
         6e6c79206f6e652074697020666f7220746865206675747572652c2073756e73
         637265656e20776f756c642062652069742e",
    );
    // The draft prints the ciphertext and the 16-byte tag separately; the
    // `chacha20poly1305` crate's `encrypt` returns them concatenated, in that
    // order, which is what the on-disk format stores.
    let expected = unhex(
        "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb
         731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452
         2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9
         21f9664c97637da9768812f615c68b13b52e
         c0875924c1c7987947deafd8780acf49",
    );

    let key: [u8; 32] = key.try_into().expect("32-byte key");
    let nonce: [u8; 24] = nonce.try_into().expect("24-byte nonce");
    let sealed = XChaCha20Poly1305::new((&key).into())
        .encrypt(
            &nonce.into(),
            Payload {
                msg: &plaintext,
                aad: &aad,
            },
        )
        .expect("sealing the draft's plaintext");

    assert_eq!(
        hex(&sealed),
        hex(&expected),
        "chacha20poly1305 no longer matches draft-arciszewski-xchacha-03's \
         published AEAD vector — every sealed object in the format is this call"
    );
}
