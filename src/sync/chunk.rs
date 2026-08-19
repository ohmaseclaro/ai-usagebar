//! Fixed-size chunking, zstd compression, and the length-prefixed frame that
//! carries a chunk's true length so tails can be padded without ambiguity.
//!
//! Also home to `open_chunk`, which performs the `chunk_id(plaintext) == id`
//! identity recheck *after* unframing — the recheck cannot live in
//! [`crate::sync::crypto::Keys::open`], because the id addresses the raw
//! plaintext while that function returns the framed-and-compressed form.
//!
//! Owned by plan 1-02.
