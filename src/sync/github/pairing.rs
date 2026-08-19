//! The pairing record: which repository this machine is bound to, and what it
//! looked like when the binding was made.
//!
//! **Empty by design: plan 3-04 fills this file.** Plan 3-01 creates it so the
//! module tree is fixed up front and no two plans ever edit the same file.
//!
//! What it is for: `owner/name` is a *name*, and names on GitHub can be released
//! and re-taken. Recording the numeric `owner_id` and repository `id` from
//! [`RepoFacts`](super::gate::RepoFacts) at pairing time is what makes a
//! delete-and-resquat detectable on the next push — a login comparison alone
//! would not see it. It is also where a private→public transition is noticed as
//! *drift* rather than as a first-contact fact, which is what lets SAFE-02 name
//! the credentials to rotate.
//!
//! Where it lives: the config directory (from the injected
//! [`SyncRoots`](crate::sync::SyncRoots)), **not** the cache — a wipeable
//! pairing check silently degrades to first-contact trust. Written atomically at
//! mode 0600, like every other credential-adjacent file in this codebase.
