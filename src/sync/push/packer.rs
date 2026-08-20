//! `SyncPlan` to `PushBundle`: sealed chunks packed into remote-sized objects,
//! plus the manifest, the index object and the snapshot root.
//!
//! Plan 4-01 created this file with the straight-line case working end to end.
//! **Plan 4-02 owns it** and adds: the local `chunk` table so an already-packed
//! chunk is neither re-sealed nor re-packed, the multi-pack fill loop's size
//! assertions, and the worst-case header test built at `PACK_MAX` — the constant
//! `pack::should_seal` actually compares against, `PACK_TARGET` being advisory.
//!
//! # Manifest paths are root-relative, never absolute
//!
//! [`FilePlan::path`](crate::sync::plan::FilePlan) is an absolute local path
//! carrying the pushing user's home directory and username. Storing it would
//! make the bundle unresolvable on a second machine, leak the username to
//! anyone who obtains the repository, and — worst — be rejected by Phase 5's
//! traversal defence, so the bundle would only be restorable by disabling the
//! very check protecting the machine restoring it.
//!
//! [`manifest_path`] renders the root-prefixed relative form instead: the name of
//! the [`SyncRoots`] root the file was collected under, then the path beneath it.
//! A file under none of the roots is an error rather than a fallback to the
//! absolute path — there is no correct absolute path to fall back to.

use std::collections::HashSet;
use std::path::Path;

use crate::error::{AppError, Result};
use crate::sync::SyncRoots;
use crate::sync::chunk::{Blob, seal_chunk};
use crate::sync::crypto::ChunkId;
use crate::sync::model::{FileEntry, IndexEntry, IndexObject, Manifest, Root};
use crate::sync::pack::{PackWriter, should_seal};
use crate::sync::plan::SyncPlan;

use super::{BuiltPack, PushBundle, PushCtx, RemoteIndexEntry};

/// Turn a plan into the bytes a push puts on the wire.
///
/// The tracer's straight-line case: every new chunk is read, sealed and packed;
/// the manifest and the index object follow it into the same writer, sealing
/// only when [`should_seal`] says so. Plan 4-02 restructures the fill loop but
/// must preserve the **order**, which is not cosmetic:
///
/// 1. the manifest is built and packed first, because
/// 2. the index object describes every pack entry this snapshot references —
///    the data chunks *and* the manifest's chunks. Build it first and the root's
///    `manifest_chunks` name ids the index object does not describe, so a
///    restore cannot find the manifest at all.
/// 3. the index object's own chunks are necessarily not described by the index
///    object, which is exactly why [`PushBundle::index_chunks`] exists as a
///    plaintext bootstrap in the pointer.
pub fn build(ctx: &PushCtx<'_>, plan: &SyncPlan) -> Result<PushBundle> {
    let wanted: HashSet<ChunkId> = plan
        .new_chunk_ids
        .iter()
        .map(|raw| ChunkId::from_bytes(*raw))
        .collect();

    let mut packs = Packing::default();
    let mut entries: Vec<(ChunkId, IndexEntry)> = Vec::new();
    let mut files: Vec<FileEntry> = Vec::new();
    let mut sealed: HashSet<ChunkId> = HashSet::new();

    for file in &plan.file_plans {
        let bytes = std::fs::read(&file.path).map_err(|e| AppError::io_at(&file.path, e))?;
        for (raw, plaintext) in file.chunk_ids.iter().zip(crate::sync::chunk::split(&bytes)) {
            let id = ChunkId::from_bytes(*raw);
            if !wanted.contains(&id) || !sealed.insert(id) {
                continue;
            }
            packs.push(seal_chunk(ctx.keys, plaintext)?, ctx)?;
        }
        files.push(FileEntry {
            path: manifest_path(ctx.roots, &file.path)?,
            mode: mode_of(&file.path),
            true_len: bytes.len() as u64,
            chunks: file
                .chunk_ids
                .iter()
                .map(|r| ChunkId::from_bytes(*r))
                .collect(),
        });
    }

    // 1. The manifest, and its chunks are packed like any other.
    let manifest = Manifest::new(files);
    let manifest_chunks: Vec<ChunkId> = manifest
        .seal(ctx.keys)?
        .into_iter()
        .map(|blob| {
            let id = blob.id;
            packs.push(blob, ctx).map(|()| id)
        })
        .collect::<Result<_>>()?;

    // 2. The index object, over every entry packed so far — data and manifest.
    entries.extend(packs.entries.iter().copied());
    let index_object = IndexObject::new(entries.iter().map(|(_, e)| *e).collect(), supersedes(ctx));
    let before_index = packs.entries.len();
    for blob in index_object.seal(ctx.keys)? {
        packs.push(blob, ctx)?;
    }

    // 3. The index object's own chunks, as the plaintext bootstrap.
    let index_chunks: Vec<RemoteIndexEntry> = packs.entries[before_index..]
        .iter()
        .map(|(pack, e)| RemoteIndexEntry {
            id: e.id,
            pack: *pack,
            offset: e.offset,
            clen: e.clen,
            true_len: e.true_len,
        })
        .collect();

    let counter = next_counter(ctx)?;
    let root = Root::new(
        counter,
        ctx.now,
        ctx.repo_id.clone(),
        manifest_chunks,
        ctx.kdf,
    )
    .seal(ctx.keys)?;

    let built = packs.finish(ctx)?;
    let referenced_packs = built.iter().map(|p| p.id).collect();
    Ok(PushBundle {
        packs: built,
        root,
        index_chunks,
        referenced_packs,
        counter,
    })
}

/// The fill loop's state: one writer at a time, sealed when [`should_seal`] says
/// so, plus every entry's `(pack id, entry)` so the index object can be built
/// from what actually landed.
#[derive(Default)]
struct Packing {
    writer: PackWriter,
    done: Vec<BuiltPack>,
    /// Filled in as each pack is finished — a pack's id does not exist until its
    /// header is sealed, so an entry recorded before that names nothing.
    entries: Vec<(ChunkId, IndexEntry)>,
    pending: Vec<IndexEntry>,
}

impl Packing {
    fn push(&mut self, blob: Blob, ctx: &PushCtx<'_>) -> Result<()> {
        if !self.writer.is_empty() && should_seal(self.writer.len_bytes(), blob.ciphertext.len()) {
            self.seal(ctx)?;
        }
        self.pending.push(IndexEntry {
            id: blob.id,
            pack: ChunkId::from_bytes([0; 32]), // filled in at `seal`
            offset: self.writer.len_bytes() as u64,
            clen: blob.ciphertext.len() as u32,
            true_len: blob.true_len,
        });
        self.writer.push(blob);
        Ok(())
    }

    fn seal(&mut self, ctx: &PushCtx<'_>) -> Result<()> {
        let (id, bytes) = std::mem::take(&mut self.writer).finish(ctx.keys)?;
        for mut entry in std::mem::take(&mut self.pending) {
            entry.pack = id;
            self.entries.push((id, entry));
        }
        self.done.push(BuiltPack { id, bytes });
        Ok(())
    }

    fn finish(mut self, ctx: &PushCtx<'_>) -> Result<Vec<BuiltPack>> {
        if !self.writer.is_empty() {
            self.seal(ctx)?;
        }
        Ok(self.done)
    }
}

/// The index-object chunk ids the previous snapshot used, which the new one
/// supersedes.
fn supersedes(ctx: &PushCtx<'_>) -> Vec<ChunkId> {
    ctx.previous
        .as_ref()
        .and_then(|p| p.snapshots.last())
        .map(|s| s.index_chunks.iter().map(|e| e.id).collect())
        .unwrap_or_default()
}

/// One above the newest published snapshot's, or 1 on a first push.
///
/// **Plan 4-02 owns the full version**, which opens the newest root the pointer
/// carries and reads its counter. The tracer takes the snapshot count as its
/// lower bound, which is monotone for this machine and never reuses a value.
/// Do **not** advance the local anchor from here: Phase 1's rule is that the
/// anchor advances only after a snapshot verifies, and this code is producing
/// one, not verifying it.
fn next_counter(ctx: &PushCtx<'_>) -> Result<u64> {
    Ok(ctx
        .previous
        .as_ref()
        .map(|p| p.snapshots.len() as u64)
        .unwrap_or(0)
        + 1)
}

/// The root-prefixed relative encoding — see the module docs.
///
/// The prefix is the *name of the root*, not its value, so nothing about this
/// machine's layout survives into the bundle. Phase 5 resolves it back against
/// that machine's own [`SyncRoots`].
pub fn manifest_path(roots: &SyncRoots, path: &Path) -> Result<String> {
    // Longest root first, so a nested root wins over its parent.
    let mut candidates = [
        ("config", roots.config_dir.as_path()),
        ("desktop-data", roots.desktop_data_dir.as_path()),
        ("desktop-profiles", roots.desktop_profiles_dir.as_path()),
        ("claude-home", roots.claude_home.as_path()),
    ];
    candidates.sort_by_key(|(_, root)| std::cmp::Reverse(root.as_os_str().len()));

    for (name, root) in candidates {
        if let Ok(rest) = path.strip_prefix(root) {
            let rest = rest
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            return Ok(format!("{name}/{rest}"));
        }
    }
    Err(AppError::Other(format!(
        "refusing to record {} in the manifest: it lies under none of the sync roots, and an \
         absolute path in a bundle is unresolvable on another machine",
        path.display()
    )))
}

/// Unix permission bits, or 0o600 where the platform has none.
fn mode_of(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o7777)
            .unwrap_or(0o600)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        0o600
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn roots_at(dir: &TempDir) -> SyncRoots {
        SyncRoots::at(
            dir.path().join("config.toml"),
            dir.path().to_path_buf(),
            dir.path().join("desktop"),
            dir.path().join("profiles"),
            dir.path().join("claude-home"),
        )
    }

    /// The defect Phase 5's planning found: an absolute path carries the
    /// username, is unresolvable elsewhere, and is exactly what the traversal
    /// defence is written to reject.
    #[test]
    fn a_manifest_path_is_root_relative_and_never_absolute() {
        let dir = TempDir::new().unwrap();
        let roots = roots_at(&dir);

        let rendered =
            manifest_path(&roots, &dir.path().join("accounts/work/.credentials.json")).unwrap();
        assert_eq!(rendered, "config/accounts/work/.credentials.json");

        let home = manifest_path(&roots, &roots.claude_home.join("projects/a.jsonl")).unwrap();
        assert_eq!(home, "claude-home/projects/a.jsonl");

        for path in [rendered, home] {
            assert!(!path.starts_with('/'), "{path}");
            assert!(!path.contains(".."), "{path}");
            assert!(
                !path.contains(dir.path().to_str().unwrap()),
                "no local prefix survives: {path}"
            );
        }
    }

    #[test]
    fn a_file_under_no_root_is_an_error_rather_than_an_absolute_path() {
        let dir = TempDir::new().unwrap();
        let err = manifest_path(&roots_at(&dir), Path::new("/etc/passwd"))
            .expect_err("that is under no sync root");
        assert!(err.to_string().contains("unresolvable"), "{err}");
    }
}
