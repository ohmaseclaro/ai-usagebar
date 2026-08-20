//! Change the sync password without re-uploading the bundle (CRYPTO-04).
//!
//! Plan 4-01 created this file with its frozen signature and the gate.
//! **Plan 4-06 owns it** and fills the four remote steps, in this order and no
//! other: upload the new keyfile, flip the pointer to it, delete the old asset,
//! then re-list to confirm the deletion. An interruption before the flip leaves
//! the pointer naming the old keyfile, which still opens under the old password.
//!
//! **Nothing under `src/sync/push/` reads a password.** Both arrive as
//! arguments, already `Zeroizing`, because the prompting happens at the CLI,
//! which owns the terminal — never argv and never an environment variable, which
//! is Phase 1's rule and is not relaxed here.

use zeroize::Zeroizing;

use crate::error::{AppError, Result};

use super::PushCtx;

/// Rewrap the master key under `new_pw` and republish the keyfile, returning the
/// new keyfile asset's name.
///
/// Data packs are untouched — that is CRYPTO-04's "without re-uploading the
/// entire bundle" — and it is deliberately **not revocation**: the data subkeys
/// do not move, so anyone holding a copy of the old keyfile can still open it
/// with the old password forever. The CLI says so in its output.
pub async fn run(
    ctx: &PushCtx<'_>,
    old_pw: &Zeroizing<String>,
    new_pw: &Zeroizing<String>,
) -> Result<String> {
    // Bound without reading: neither password may reach a message, a log line,
    // or an error, and the compiler is what enforces that they are unused here.
    let _ = (old_pw, new_pw);
    // The gate runs before anything, including the rewrap, so a public
    // repository refuses before a single request that carries a body.
    let _permit = super::gate_now(
        ctx,
        ctx.cfg.includes(crate::config::SyncCategory::Credentials),
    )
    .await?;
    Err(AppError::Other(
        "this build cannot change the sync password yet. The rewrap primitive exists and the \
         gate above passed; the remote half — publishing the new keyfile, flipping the pointer \
         to it, and verifiably deleting the old asset — is not in this build."
            .into(),
    ))
}
