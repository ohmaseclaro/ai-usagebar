//! The only place in the crate where a synced file's plaintext reaches a
//! filesystem.
//!
//! # SAFE-05, and why the tempfile lives in the destination's own directory
//!
//! Every decrypted byte goes through a tempfile created in the **destination's
//! own directory**, chmod 0600 *before* any content is written, then persisted.
//! Never a shared temporary directory, and it is refused there three times
//! over: it is world-readable, so a plaintext credential sits where anyone can
//! read it; it is frequently a different filesystem, where `persist` cannot
//! rename and degrades to a copy that leaves the plaintext original behind at
//! the temporary path — the precise failure SAFE-05 names; and it is often
//! tmpfs, which can reach swap and outlive the process on disk.
//!
//! The chmod happens on the tempfile rather than after the rename because
//! `persist` keeps the tempfile's mode — a chmod afterwards leaves a window
//! where the file exists at its real name with whatever mode the umask gave it.
//!
//! # The order of the steps *is* the mitigation
//!
//! 1. Every writable item's manifest path is resolved again, for the whole plan,
//!    **before the first byte** — defence in depth against a plan mutated
//!    between planning and applying. A disagreement aborts the restore rather
//!    than skipping the item: a plan that contradicts itself is not a situation
//!    to continue from. Doing the whole plan first is what makes `Err` from
//!    [`apply`] mean *nothing was written*.
//! 2. The parent directory is created at 0700 **before** the tempfile, so the
//!    tempfile is never briefly parented by a world-listable directory.
//!    Directories that already existed are left exactly as their owner set
//!    them: a surprise 0700 on `~/.claude` is its own bug.
//! 3. The tempfile is created in that directory, with the `.tmp.` prefix
//!    [`crate::cache::atomic_write`] uses, so `scope`'s exclusion rules already
//!    ignore it if a collection scan runs concurrently.
//! 4. Mode 0600, then the content, then `sync_all`, then the snapshot's mtime,
//!    then `persist`. The real name is only ever reached by that rename; no
//!    path here opens the destination for writing, so a half-written credential
//!    cannot exist at its real name.
//!
//! # The manifest's recorded mode is ignored
//!
//! Every restored file is 0600 and every directory this module creates is 0700.
//! The manifest does record a mode, and it is attacker-controllable: a bundle
//! that could talk this side into 0644 on a credential file would be a bundle
//! that leaks it. The narrowing is safe in the only direction that matters, and
//! it is structural rather than merely unread — [`ItemPlan`] does not carry the
//! field, so there is no value here to apply by accident. The one user-visible
//! consequence is that a restored executable does not come back executable; no
//! category in the bundle contains one.
//!
//! # What an interrupted restore leaves, and why it is not rolled back
//!
//! A partial restore is **reported as one, not undone**. Items before the
//! failure stay written, the failing item does not exist at its real name, and
//! the items after it are untouched; [`Applied::failed_at`] names where the run
//! reached. Automatically undoing the successful writes would mean writing
//! again, from an archive, on a machine that has just demonstrated it cannot
//! complete a write — more failure surface at exactly the wrong moment. The
//! user gets the rollback command from the pre-restore backup and decides.
//!
//! Killing the process mid-restore leaves at most one unpersisted `.tmp.` file
//! inside a destination directory and nothing anywhere else. Re-running the
//! restore finishes it: every write is idempotent, and 5-03's `SkipIdentical`
//! means the second run does not even reopen what the first one completed.
//!
//! # This module assumes the backup was already taken
//!
//! [`super::run`] calls [`super::backup::take`] over exactly the destinations
//! whose [`Disposition::writes`] — step 5, before step 6 — so *nothing archived
//! implies nothing overwritten*. `apply` deliberately offers no way to reach a
//! write without going through that order: it takes no "skip the backup" option
//! and reads no flag that would let one exist.
//!
//! # Symlinks
//!
//! Nothing here ever creates one, and `persist` is a rename, so an existing
//! symlink *at* the destination is replaced rather than written through. A
//! symlinked directory *above* the destination is followed — deliberately: it
//! is a configuration the user made on this machine (`~/.claude` pointed at
//! another disk is a real setup), it cannot be created by the bundle, and
//! refusing it would break the legitimate case far more often than the hostile
//! one, which needs write access to the user's home directory to arrange and
//! would not need a restore to exploit it.
//!
//! Plan 5-01 filled the happy path. Plan 5-04 owns the preflight, the directory
//! modes, the failure-path cleanup, the mtime stamp that makes 5-03's
//! newer-local comparison exact, and the partial-restore report.

use std::fs::{DirBuilder, FileTimes};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::error::{AppError, Result};

use super::{Applied, Disposition, ItemPlan, PackSource, RestoreCtx, RestorePlan, layout};

/// Write every item the plan decided to write.
///
/// `Err` means **nothing was written**: the only errors are the preflight's,
/// and it runs over the whole plan before the first tempfile exists. A failure
/// during the writing itself is a partial restore, which is an `Ok` carrying
/// [`Applied::failed_at`] — the caller must not advance the rollback anchor on
/// one, and [`super::run`] does not.
pub fn apply(ctx: &RestoreCtx<'_>, plan: &RestorePlan, packs: &PackSource) -> Result<Applied> {
    let mut out = Applied::default();
    let mut queue: Vec<(&ItemPlan, PathBuf)> = Vec::new();

    // Preflight, over every item, before any of them is written.
    for item in &plan.items {
        if matches!(item.disposition, Disposition::NeedsCredentialConfirm { .. }) {
            return Err(AppError::Other(format!(
                "{:?} reached the write path still awaiting the credential confirmation the CLI \
                 resolves before a restore applies — refusing rather than guessing which way it \
                 was answered",
                item.manifest_path
            )));
        }
        if !item.disposition.writes() {
            out.skipped += 1;
            continue;
        }

        // `RejectedPath` and `ExcludedByPolicy` are the only dispositions that
        // carry no destination, and neither of them writes. A writable item
        // without one is a bug upstream, not a case to skip quietly.
        let dest = item.dest.as_ref().ok_or_else(|| {
            AppError::Other(format!(
                "{:?} is planned as a write but carries no destination — refusing to guess one",
                item.manifest_path
            ))
        })?;

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
        queue.push((item, checked));
    }

    // Manifest order, so a partial restore stops in the same place twice and is
    // therefore debuggable.
    for (item, dest) in queue {
        if let Err(why) = write_one(packs, item, &dest, plan.created_at) {
            // `Applied` carries where the run stopped, which is what the summary
            // renders; the cause is only useful now, so it goes to stderr rather
            // than being swallowed.
            eprintln!("sync: restore stopped at {}: {why}", item.manifest_path);
            out.failed_at = Some(item.manifest_path.clone());
            break;
        }
        out.written += 1;
        if matches!(item.disposition, Disposition::Overwrite { .. }) {
            out.overwritten.push(item.manifest_path.clone());
        }
    }

    Ok(out)
}

/// One file: reassemble its chunks in order, straight into a tempfile beside
/// where it is going.
///
/// Every error path drops the [`tempfile::NamedTempFile`], whose `Drop` removes
/// it. That is why nothing here calls `into_temp_path().keep()` and why no
/// staging name outside the destination directory exists to be renamed later.
fn write_one(
    packs: &PackSource,
    item: &ItemPlan,
    dest: &Path,
    created_at: DateTime<Utc>,
) -> Result<()> {
    let dir = dest.parent().ok_or_else(|| {
        AppError::Other(format!(
            "the destination for {:?} has no parent directory",
            item.manifest_path
        ))
    })?;
    ensure_dir(dir)?;

    let mut tmp = tempfile::Builder::new()
        .prefix(".tmp.")
        .tempfile_in(dir)
        .map_err(|e| AppError::io_at(dir, e))?;

    // Before a single byte, so the plaintext is never briefly world-readable —
    // and `persist` keeps this mode, so it is never readable at the real name
    // either.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| AppError::io_at(tmp.path(), e))?;
    }

    // One chunk in memory at a time: a multi-gigabyte transcript is not
    // assembled in RAM just to be written out.
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

    // The snapshot's time, not this restore's: it is what makes the next pull's
    // newer-local comparison exact rather than merely conservative. Stamped
    // before the rename, which preserves it, so there is no second open of the
    // destination to fail. Best effort — a filesystem that will not take a
    // timestamp is not a reason to fail a restore that has already succeeded.
    let _ = tmp
        .as_file()
        .set_times(FileTimes::new().set_modified(created_at.into()));

    tmp.persist(dest)
        .map_err(|e| AppError::io_at(dest, e.error))?;
    Ok(())
}

/// Create the destination's directory chain at 0700, leaving anything that
/// already existed exactly as its owner set it.
///
/// [`DirBuilder`]'s mode applies to the directories it creates and to nothing
/// else, which is precisely the distinction wanted: the restore closes what it
/// opens and does not narrow the user's own directories.
fn ensure_dir(dir: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir).map_err(|e| AppError::io_at(dir, e))
}
#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::SyncCategory;
    use crate::sync::crypto::{ChunkId, KdfParams, Keyfile};
    use crate::sync::github::token::TokenSource;
    use crate::sync::github::{Client, Endpoints, RepoRef};
    use crate::sync::pack::PackWriter;
    use crate::sync::restore::RestoreOptions;
    use crate::sync::{CHUNK_SIZE, SyncRoots, chunk};
    use chrono::{DateTime, Utc};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;
    use zeroize::Zeroizing;

    /// Microseconds instead of ~1.5 s and a gibibyte. Never production
    /// parameters in a unit test: the AUR `check()` runs these on an
    /// installer's machine.
    const CHEAP: KdfParams = KdfParams {
        m_kib: 8,
        t: 1,
        p: 1,
    };

    /// When the snapshot was taken — deliberately far from `NOW`, so a restored
    /// file stamped with the wrong one of the two is visible.
    const SNAPSHOT: DateTime<Utc> = match DateTime::from_timestamp(1_600_000_000, 0) {
        Some(t) => t,
        None => panic!("a fixed timestamp"),
    };

    /// When the restore runs.
    const NOW: DateTime<Utc> = match DateTime::from_timestamp(1_700_000_000, 0) {
        Some(t) => t,
        None => panic!("a fixed timestamp"),
    };

    const REPO_ID: &str = "github:1";
    const PASSWORD: &str = "correct horse battery staple";

    const CREDENTIAL: &str = "config/accounts/work/.credentials.json";

    /// One machine's roots, entirely inside a `TempDir`. Nothing here resolves a
    /// real `$HOME`, `$XDG_*`, or the network.
    struct Machine {
        _dir: TempDir,
        roots: SyncRoots,
        repo: RepoRef,
        client: Client,
        passphrase: Zeroizing<String>,
        anchor: PathBuf,
        backups: PathBuf,
    }

    impl Machine {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("bob");
            let roots = SyncRoots::at(
                home.join(".config/ai-usagebar/config.toml"),
                home.join(".config/ai-usagebar"),
                home.join("desktop"),
                home.join("profiles"),
                home.join(".claude"),
            );
            Self {
                anchor: dir.path().join("state/anchor.json"),
                backups: dir.path().join("backups"),
                roots,
                _dir: dir,
                repo: RepoRef::parse("o/n").unwrap(),
                // Parked at a closed port: `apply` is a pure disk path and a
                // regression that made it dial should fail rather than reach
                // anything real.
                client: Client::new(
                    &Endpoints {
                        api_base: "http://127.0.0.1:1".into(),
                        uploads_base: "http://127.0.0.1:1".into(),
                    },
                    Zeroizing::new("github_pat_fixture_not_a_real_token".into()),
                    TokenSource::Env,
                )
                .unwrap(),
                passphrase: Zeroizing::new(PASSWORD.into()),
            }
        }

        fn ctx(&self) -> RestoreCtx<'_> {
            RestoreCtx {
                client: &self.client,
                repo: &self.repo,
                roots: &self.roots,
                repo_id: REPO_ID,
                passphrase: &self.passphrase,
                anchor_path: &self.anchor,
                backups_dir: &self.backups,
                opts: RestoreOptions::default(),
                now: NOW,
            }
        }

        fn dest(&self, manifest_path: &str) -> PathBuf {
            layout::from_manifest_path(&self.roots, manifest_path).unwrap()
        }
    }

    /// Seal every body into one pack exactly as the push side would, and hand
    /// back the source `apply` reads from plus each body's chunk ids.
    fn packed(bodies: &[&[u8]]) -> (PackSource, Vec<Vec<ChunkId>>) {
        let (_keyfile, keys) =
            Keyfile::create_with_floor(PASSWORD.as_bytes(), CHEAP, CHEAP.m_kib).unwrap();
        let mut writer = PackWriter::new();
        let mut ids = Vec::new();
        for body in bodies {
            let mut per_file = Vec::new();
            for block in chunk::split(body) {
                let blob = chunk::seal_chunk(&keys, block).unwrap();
                per_file.push(blob.id);
                writer.push(blob);
            }
            ids.push(per_file);
        }
        let (id, bytes) = writer.finish(&keys).unwrap();
        let mut packs = PackSource::empty(keys);
        packs.add(id, bytes).unwrap();
        (packs, ids)
    }

    fn item(
        m: &Machine,
        manifest_path: &str,
        body: &[u8],
        chunks: Vec<ChunkId>,
        disposition: Disposition,
    ) -> ItemPlan {
        ItemPlan {
            dest: layout::from_manifest_path(&m.roots, manifest_path).ok(),
            manifest_path: manifest_path.into(),
            category: SyncCategory::Config,
            true_len: body.len() as u64,
            chunks,
            disposition,
        }
    }

    fn plan_of(items: Vec<ItemPlan>) -> RestorePlan {
        RestorePlan {
            items,
            counter: 4,
            created_at: SNAPSHOT,
            repo_id: REPO_ID.into(),
            packs_needed: 1,
            bytes_to_fetch: 0,
        }
    }

    /// Every regular file anywhere under `root` — the walk that proves a
    /// tempfile did not survive.
    fn files_under(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
        out.sort();
        out
    }

    fn secs(t: SystemTime) -> i64 {
        t.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
    }

    #[cfg(unix)]
    fn mode_of(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    /// The headline: a credential spanning several chunks comes back byte for
    /// byte, readable by nobody else, carrying the snapshot's time rather than
    /// the restore's.
    #[test]
    fn a_multi_chunk_file_lands_byte_for_byte_at_mode_0600_stamped_with_the_snapshot() {
        let m = Machine::new();
        let body: Vec<u8> = (0..CHUNK_SIZE * 2 + 7).map(|i| (i % 251) as u8).collect();
        let (packs, ids) = packed(&[&body]);
        let plan = plan_of(vec![item(
            &m,
            CREDENTIAL,
            &body,
            ids[0].clone(),
            Disposition::Create,
        )]);

        let applied = apply(&m.ctx(), &plan, &packs).unwrap();
        assert_eq!(applied.written, 1);
        assert_eq!(applied.failed_at, None);

        let dest = m.dest(CREDENTIAL);
        assert_eq!(std::fs::read(&dest).unwrap(), body, "the bytes differ");
        #[cfg(unix)]
        assert_eq!(mode_of(&dest), 0o600, "a restored credential must be 0600");
        assert_eq!(
            secs(std::fs::metadata(&dest).unwrap().modified().unwrap()),
            SNAPSHOT.timestamp(),
            "the restored mtime is not the snapshot's — 5-03's comparison needs it exact"
        );
    }

    /// A tempfile born in a world-listable directory is the window the ordering
    /// exists to remove, so the directory is closed before it is created.
    #[cfg(unix)]
    #[test]
    fn every_directory_the_restore_creates_is_closed_to_other_users() {
        let m = Machine::new();
        let (packs, ids) = packed(&[b"{}"]);
        let path = "claude-home/projects/repo/session.jsonl";
        let plan = plan_of(vec![item(
            &m,
            path,
            b"{}",
            ids[0].clone(),
            Disposition::Create,
        )]);

        apply(&m.ctx(), &plan, &packs).unwrap();

        let mut dir = m.dest(path).parent().unwrap().to_path_buf();
        loop {
            assert_eq!(
                mode_of(&dir) & 0o077,
                0,
                "{dir:?} is listable by someone other than its owner"
            );
            if dir == m.roots.claude_home {
                break;
            }
            dir = dir.parent().unwrap().to_path_buf();
        }
    }

    /// The user's own permissions on a directory that was already there are not
    /// this command's to narrow.
    #[cfg(unix)]
    #[test]
    fn a_directory_that_already_existed_keeps_the_mode_its_owner_gave_it() {
        use std::os::unix::fs::PermissionsExt;

        let m = Machine::new();
        std::fs::create_dir_all(&m.roots.config_dir).unwrap();
        std::fs::set_permissions(&m.roots.config_dir, std::fs::Permissions::from_mode(0o755))
            .unwrap();

        let (packs, ids) = packed(&[b"[sync]\n"]);
        let plan = plan_of(vec![item(
            &m,
            "config/config.toml",
            b"[sync]\n",
            ids[0].clone(),
            Disposition::Create,
        )]);
        apply(&m.ctx(), &plan, &packs).unwrap();

        assert_eq!(
            mode_of(&m.roots.config_dir),
            0o755,
            "a pre-existing directory was re-chmodded"
        );
    }

    /// The failure that happens after the tempfile exists and before the file
    /// does: nothing is left in the destination directory, at either name.
    #[test]
    fn a_chunk_the_packs_do_not_carry_leaves_nothing_in_the_destination_directory() {
        let m = Machine::new();
        let (packs, ids) = packed(&[b"real bytes"]);
        let absent = ChunkId::from_bytes([9u8; 32]);
        let plan = plan_of(vec![item(
            &m,
            CREDENTIAL,
            b"real bytes",
            vec![ids[0][0], absent],
            Disposition::Create,
        )]);

        let applied = apply(&m.ctx(), &plan, &packs).unwrap();
        assert_eq!(applied.failed_at.as_deref(), Some(CREDENTIAL));
        assert_eq!(applied.written, 0);
        assert!(!m.dest(CREDENTIAL).exists());
        assert!(
            files_under(&m.roots.config_dir).is_empty(),
            "a partial write survived: {:?}",
            files_under(&m.roots.config_dir)
        );
    }

    /// A short reassembly is a detected truncation, not a successful restore.
    #[test]
    fn a_reassembly_shorter_than_the_snapshot_records_is_refused_and_leaves_nothing() {
        let m = Machine::new();
        let body = b"the whole file";
        let (packs, ids) = packed(&[body]);
        let mut only = item(&m, CREDENTIAL, body, ids[0].clone(), Disposition::Create);
        only.true_len += 1;
        let plan = plan_of(vec![only]);

        let applied = apply(&m.ctx(), &plan, &packs).unwrap();
        assert_eq!(applied.failed_at.as_deref(), Some(CREDENTIAL));
        assert!(!m.dest(CREDENTIAL).exists());
        assert!(files_under(&m.roots.config_dir).is_empty());
    }

    /// The failure before the tempfile: reported, not panicked.
    #[test]
    fn a_destination_whose_parent_is_a_file_stops_the_restore_rather_than_panicking() {
        let m = Machine::new();
        std::fs::create_dir_all(&m.roots.config_dir).unwrap();
        std::fs::write(m.roots.config_dir.join("accounts"), b"not a directory").unwrap();

        let (packs, ids) = packed(&[b"{}"]);
        let plan = plan_of(vec![item(
            &m,
            CREDENTIAL,
            b"{}",
            ids[0].clone(),
            Disposition::Create,
        )]);

        let applied = apply(&m.ctx(), &plan, &packs).unwrap();
        assert_eq!(applied.failed_at.as_deref(), Some(CREDENTIAL));
        assert_eq!(applied.written, 0);
    }

    /// Five of the eight dispositions must not put a byte anywhere.
    #[test]
    fn only_the_three_writable_dispositions_put_a_file_on_disk() {
        let m = Machine::new();
        let (packs, ids) = packed(&[b"a", b"b", b"c"]);
        let times = (SNAPSHOT, NOW);
        let mut items = vec![
            item(
                &m,
                "config/a.toml",
                b"a",
                ids[0].clone(),
                Disposition::Create,
            ),
            item(
                &m,
                "config/b.toml",
                b"b",
                ids[1].clone(),
                Disposition::Update,
            ),
            item(
                &m,
                "config/c.toml",
                b"c",
                ids[2].clone(),
                Disposition::Overwrite {
                    local_mtime: times.1,
                    remote_mtime: times.0,
                },
            ),
            item(
                &m,
                "config/d.toml",
                b"a",
                ids[0].clone(),
                Disposition::SkipIdentical,
            ),
            item(
                &m,
                "config/e.toml",
                b"a",
                ids[0].clone(),
                Disposition::SkipLocalNewer {
                    local_mtime: times.1,
                    remote_mtime: times.0,
                },
            ),
            item(
                &m,
                "config/f.toml",
                b"a",
                ids[0].clone(),
                Disposition::ExcludedByPolicy,
            ),
            item(
                &m,
                "config/g.toml",
                b"a",
                ids[0].clone(),
                Disposition::RejectedPath("it contains a `..` component".into()),
            ),
        ];
        // The two that carry no destination, because their plan refused one.
        items[5].dest = None;
        items[6].dest = None;

        let applied = apply(&m.ctx(), &plan_of(items), &packs).unwrap();

        assert_eq!(applied.written, 3);
        assert_eq!(applied.skipped, 4);
        assert_eq!(applied.failed_at, None);
        assert_eq!(applied.overwritten, vec!["config/c.toml".to_string()]);
        assert_eq!(
            files_under(&m.roots.config_dir),
            vec![
                m.dest("config/a.toml"),
                m.dest("config/b.toml"),
                m.dest("config/c.toml"),
            ]
        );
    }

    /// `overwritten` is what 5-06 lists, so it is in manifest order rather than
    /// whatever order a set iterates in.
    #[test]
    fn overwritten_names_every_item_it_replaced_in_manifest_order() {
        let m = Machine::new();
        let (packs, ids) = packed(&[b"a", b"b", b"c"]);
        let overwrite = Disposition::Overwrite {
            local_mtime: NOW,
            remote_mtime: SNAPSHOT,
        };
        let plan = plan_of(vec![
            item(&m, "config/z.toml", b"a", ids[0].clone(), overwrite.clone()),
            item(&m, "config/m.toml", b"b", ids[1].clone(), overwrite.clone()),
            item(&m, "config/a.toml", b"c", ids[2].clone(), overwrite),
        ]);

        let applied = apply(&m.ctx(), &plan, &packs).unwrap();
        assert_eq!(
            applied.overwritten,
            vec![
                "config/z.toml".to_string(),
                "config/m.toml".to_string(),
                "config/a.toml".to_string(),
            ]
        );
    }

    /// The interrupted restore, in miniature: what was written stays, what was
    /// not is absent, and the report names where it stopped.
    #[test]
    fn a_failure_part_way_keeps_what_it_wrote_and_names_where_it_stopped() {
        let m = Machine::new();
        let (packs, ids) = packed(&[b"first", b"third"]);
        let absent = ChunkId::from_bytes([7u8; 32]);
        let plan = plan_of(vec![
            item(
                &m,
                "config/1.toml",
                b"first",
                ids[0].clone(),
                Disposition::Create,
            ),
            item(
                &m,
                "config/2.toml",
                b"second",
                vec![absent],
                Disposition::Create,
            ),
            item(
                &m,
                "config/3.toml",
                b"third",
                ids[1].clone(),
                Disposition::Create,
            ),
        ]);

        let applied = apply(&m.ctx(), &plan, &packs).unwrap();

        assert_eq!(applied.written, 1);
        assert_eq!(applied.failed_at.as_deref(), Some("config/2.toml"));
        assert_eq!(
            files_under(&m.roots.config_dir),
            vec![m.dest("config/1.toml")],
            "either a tempfile survived or the run did not stop"
        );
    }

    /// The CLI resolves this one before `apply` runs. Reaching here means the
    /// gate was skipped, which is not a thing to guess about.
    #[test]
    fn a_credential_awaiting_confirmation_reaching_apply_is_an_internal_error() {
        let m = Machine::new();
        let (packs, ids) = packed(&[b"a", b"b"]);
        let plan = plan_of(vec![
            item(
                &m,
                "config/a.toml",
                b"a",
                ids[0].clone(),
                Disposition::Create,
            ),
            item(
                &m,
                CREDENTIAL,
                b"b",
                ids[1].clone(),
                Disposition::NeedsCredentialConfirm {
                    local_mtime: NOW,
                    remote_mtime: SNAPSHOT,
                },
            ),
        ]);

        let err = apply(&m.ctx(), &plan, &packs).expect_err("an unresolved credential");
        assert!(err.to_string().contains(CREDENTIAL), "{err}");
        assert!(
            files_under(&m.roots.config_dir).is_empty(),
            "the plan was refused after it had already written"
        );
    }

    /// Defence in depth against a plan mutated between planning and applying —
    /// and it is checked for every item before any of them is written.
    #[test]
    fn a_destination_that_disagrees_with_its_manifest_path_is_refused_before_a_byte_is_written() {
        let m = Machine::new();
        let (packs, ids) = packed(&[b"a", b"b"]);
        let mut tampered = item(
            &m,
            "config/b.toml",
            b"b",
            ids[1].clone(),
            Disposition::Create,
        );
        tampered.dest = Some(m.roots.config_dir.join("elsewhere.toml"));
        let plan = plan_of(vec![
            item(
                &m,
                "config/a.toml",
                b"a",
                ids[0].clone(),
                Disposition::Create,
            ),
            tampered,
        ]);

        let err = apply(&m.ctx(), &plan, &packs).expect_err("a plan that disagrees with itself");
        assert!(err.to_string().contains("config/b.toml"), "{err}");
        assert!(
            files_under(&m.roots.config_dir).is_empty(),
            "the earlier item was written before the plan was checked"
        );
    }

    /// `RejectedPath` and `ExcludedByPolicy` are the only dispositions that
    /// carry no destination; a writable one without one is a bug upstream.
    #[test]
    fn a_writable_item_with_no_destination_is_a_bug_not_a_skip() {
        let m = Machine::new();
        let (packs, ids) = packed(&[b"a"]);
        let mut orphan = item(
            &m,
            "config/a.toml",
            b"a",
            ids[0].clone(),
            Disposition::Create,
        );
        orphan.dest = None;

        let err = apply(&m.ctx(), &plan_of(vec![orphan]), &packs)
            .expect_err("a writable item with nowhere to go");
        assert!(err.to_string().contains("config/a.toml"), "{err}");
    }

    /// D7 at the write level: re-running the same plan is safe. (Whether the
    /// *second* plan even lists the item is 5-03's `SkipIdentical`.)
    #[test]
    fn applying_the_same_plan_twice_leaves_the_same_bytes_and_no_leftovers() {
        let m = Machine::new();
        let body = b"{\"token\":\"a-fixture-not-a-real-token\"}";
        let (packs, ids) = packed(&[body]);
        let plan = plan_of(vec![item(
            &m,
            CREDENTIAL,
            body,
            ids[0].clone(),
            Disposition::Create,
        )]);

        apply(&m.ctx(), &plan, &packs).unwrap();
        apply(&m.ctx(), &plan, &packs).unwrap();

        let dest = m.dest(CREDENTIAL);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(files_under(&m.roots.config_dir), vec![dest.clone()]);
        #[cfg(unix)]
        assert_eq!(mode_of(&dest), 0o600);
    }

    /// SAFE-05, asserted against the source rather than against a `TMPDIR` this
    /// process would have to mutate to observe: plaintext has no route to a
    /// shared temporary directory because no such call exists.
    #[test]
    fn nothing_in_this_module_reaches_for_a_shared_temporary_directory() {
        let src = include_str!("write.rs");
        let code: Vec<&str> = src
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        let code = code.join("\n");
        // Assembled at runtime so the needles do not match themselves.
        for needle in [
            ["temp", "_dir"].concat(),
            ["/", "tmp\""].concat(),
            ["into_temp", "_path"].concat(),
        ] {
            assert!(
                !code.contains(&needle),
                "{needle} appears in the write path — plaintext must never leave the \
                 destination's own directory"
            );
        }
    }
}
