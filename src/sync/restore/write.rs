//! The only place in the crate where a synced file's plaintext reaches a
//! filesystem.
//!
//! # SAFE-05, and why the tempfile lives in the destination's own directory
//!
//! Every decrypted byte goes through a [`NamedTempFile::new_in`] created in the
//! **destination's own directory**, chmod 0600 *before* any content is written,
//! then persisted. Never `/tmp`, and it is refused there three times over: it
//! is world-readable; it is usually a different filesystem, so `persist`
//! degrades to a copy that leaves the plaintext original behind; and it is
//! often tmpfs, which can reach swap.
//!
//! The chmod happens on the tempfile rather than after the rename because
//! `persist` keeps the tempfile's mode — a chmod afterwards leaves a window
//! where the file exists at its real name with whatever mode the umask gave it.
//!
//! # The manifest's recorded mode is ignored
//!
//! Every restored file is 0600. The mode in the manifest is attacker-
//! controllable, and a bundle that could talk this side into 0644 on a
//! credential file would be a bundle that leaks it. A deliberate narrowing,
//! recorded here rather than inferred from the absence of a read.
//!
//! Plan 5-01 filled the happy path. Plan 5-04 owns directory modes, the
//! failure-path cleanup, the mtime stamp that makes 5-03's comparison exact,
//! and the symlink refusal at this boundary.

use std::io::Write as _;

use tempfile::NamedTempFile;

use crate::error::{AppError, Result};

use super::{Applied, ItemPlan, PackSource, RestoreCtx, RestorePlan, layout};

/// Write every item the plan decided to write.
pub fn apply(ctx: &RestoreCtx<'_>, plan: &RestorePlan, packs: &PackSource) -> Result<Applied> {
    let mut out = Applied::default();

    for item in &plan.items {
        if !item.disposition.writes() {
            out.skipped += 1;
            continue;
        }
        let Some(dest) = item.dest.as_ref() else {
            out.skipped += 1;
            continue;
        };

        // Defence in depth: the path rule runs again at the write boundary, and
        // a destination that fails it here is a hard error rather than a skip —
        // if the two disagree, something between them rewrote the plan.
        let checked = layout::from_manifest_path(ctx.roots, &item.manifest_path)?;
        if &checked != dest {
            return Err(AppError::Other(format!(
                "the planned destination for {:?} is not what its manifest path resolves to — \
                 refusing to write",
                item.manifest_path
            )));
        }

        write_one(packs, item, &checked)?;
        out.written += 1;
        if matches!(item.disposition, super::Disposition::Overwrite { .. }) {
            out.overwritten.push(item.manifest_path.clone());
        }
    }

    Ok(out)
}

/// One file: reassemble its chunks in order, straight into a tempfile beside
/// where it is going.
fn write_one(packs: &PackSource, item: &ItemPlan, dest: &std::path::Path) -> Result<()> {
    let dir = dest.parent().ok_or_else(|| {
        AppError::Other(format!(
            "the destination for {:?} has no parent directory",
            item.manifest_path
        ))
    })?;
    std::fs::create_dir_all(dir).map_err(|e| AppError::io_at(dir, e))?;

    let mut tmp = NamedTempFile::new_in(dir).map_err(|e| AppError::io_at(dir, e))?;

    // Before a single byte, so the plaintext is never briefly world-readable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| AppError::io_at(tmp.path(), e))?;
    }

    let mut written: u64 = 0;
    for id in &item.chunks {
        let bytes = packs.chunk(id)?;
        tmp.write_all(&bytes)
            .map_err(|e| AppError::io_at(tmp.path(), e))?;
        written += bytes.len() as u64;
    }

    // A short file is a detected truncation, not a successful restore: the
    // difference between noticing and silently corrupting a secret.
    if written != item.true_len {
        return Err(AppError::Other(format!(
            "{:?} reassembled to {written} bytes but the snapshot records {} — refusing to \
             write a truncated file",
            item.manifest_path, item.true_len
        )));
    }

    tmp.as_file_mut()
        .sync_all()
        .map_err(|e| AppError::io_at(tmp.path(), e))?;
    tmp.persist(dest)
        .map_err(|e| AppError::io_at(dest, e.error))?;
    Ok(())
}
