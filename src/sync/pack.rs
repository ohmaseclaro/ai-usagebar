//! restic-shaped pack files: concatenated sealed blobs, a sealed header, a u32
//! little-endian header length, and a sharded content-addressed name.
//!
//! `read_header` repeats the `chunk_id(plaintext) == id` identity recheck after
//! deserializing, for the same reason [`crate::sync::chunk`] does.
//!
//! Owned by plan 1-03.
