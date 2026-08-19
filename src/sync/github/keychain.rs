//! macOS Keychain storage for the sync token — service `ai-usagebar-sync-token`.
//!
//! **Empty by design: plan 3-02 fills this file.** Plan 3-01 creates it so the
//! module tree is fixed up front and no two plans ever edit the same file.
//!
//! The rule this module exists to follow is already implemented in
//! [`crate::anthropic::keychain`] and must not be written a second time:
//!
//! - **Reads** go through `security(1)`. A read takes no secret, so a command
//!   line is harmless.
//! - **Writes** go through Security.framework. `security add-generic-password -w
//!   <token>` would put the token in `argv`, where every process on the machine
//!   can read it out of `ps`.
//!
//! `anthropic::keychain`'s `read_raw_service` / `write_raw_service` /
//! `delete_raw_service` are already parameterized by service name; plan 3-02
//! makes them `pub(crate)` and wraps them here with this module's constant.
//!
//! Whatever lands here is reached only through
//! [`TokenChain::keychain`](super::token::TokenChain::keychain) — a closure — so
//! no test ever touches the real login Keychain.
