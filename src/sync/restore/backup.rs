//! The archive taken before the first restore write.
//!
//! It is the last line of defence (D3, SAFE-04), so it is taken even for a
//! partial restore and even under `--force` — `--force` is exactly when it is
//! needed. A backup that cannot be taken aborts the restore rather than
//! warning.
//!
//! `Ok(None)` means there was genuinely nothing on disk to preserve: a restore
//! onto an empty machine, which is the milestone's headline case, has nothing
//! to undo.
//!
//! Plan 5-01 filled the signature and the nothing-to-preserve arm. Plan 5-05
//! owns the archive itself: `~/.claude-acc/backups/sync-restore-<stamp>.tar.gz`
//! at mode 0600 inside a mode 0700 directory, rooted at the user's home so one
//! `-C` covers all four sync roots, through an injected `tar` program path.

use std::path::PathBuf;

use crate::error::Result;

use super::{BackupRecord, RestoreCtx};

/// Preserve exactly the paths the restore is about to write.
///
/// `targets` comes from the plan, so this structurally cannot archive something
/// the restore will not touch.
pub fn take(_ctx: &RestoreCtx<'_>, targets: &[PathBuf]) -> Result<Option<BackupRecord>> {
    let present: Vec<&PathBuf> = targets.iter().filter(|p| p.exists()).collect();
    if present.is_empty() {
        return Ok(None);
    }

    // Plan 5-05 fills the archive. Until it does, a restore that *would*
    // overwrite something refuses rather than proceeding without an undo —
    // the one thing this module must never do is return `None` for a tree it
    // simply did not archive.
    Err(crate::error::AppError::Other(format!(
        "{} existing file(s) would be overwritten and the pre-restore backup is not yet \
         implemented — refusing, because a restore without an undo is not one this build \
         will run",
        present.len()
    )))
}
