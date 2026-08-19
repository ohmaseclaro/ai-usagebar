//! The local monotonic rollback anchor: the last-seen snapshot counter,
//! persisted mode-0600 in the config directory (not the wipeable cache), so a
//! replayed-but-authentic old root is refused.
//!
//! Owned by plan 1-05.
