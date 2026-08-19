//! The snapshot object graph: the root (fresh random nonce, monotonic counter),
//! the manifest carried as an ordinary sealed chunk, and the index object with
//! its `supersedes` link.
//!
//! Owned by plan 1-04.
