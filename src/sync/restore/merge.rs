//! Every manifest entry, turned into exactly one decision.
//!
//! Plan 5-03 owns this module. The bodies below are `todo!()` until the tests
//! beneath them say what they must do.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::config::SyncCategory;
use crate::error::Result;
use crate::sync::crypto::{ChunkId, Keys};
use crate::sync::model::{FileEntry, IndexObject};

use super::{
    Disposition, ItemPlan, PackSource, Resolved, RestoreCtx, RestoreOptions, RestorePlan, layout,
};

/// What this machine has at a destination.
struct LocalFacts {
    mtime: DateTime<Utc>,
    chunk_ids: Option<Vec<ChunkId>>,
}

/// What the snapshot has.
struct RemoteFacts<'a> {
    chunk_ids: &'a [ChunkId],
    created_at: DateTime<Utc>,
}

/// The destination as it is right now.
enum Local {
    Absent,
    Refused(String),
    File(LocalFacts),
}

/// Decide about every file in the snapshot.
pub fn plan(ctx: &RestoreCtx<'_>, resolved: &Resolved) -> Result<RestorePlan> {
    let keys = resolved.packs.keys();
    let created_at = resolved.root.created_at;

    let items: Vec<ItemPlan> = resolved
        .manifest
        .files
        .iter()
        .map(|file| {
            let category = category_of(&file.path);
            let (dest, disposition) = decide_entry(ctx, keys, file, category, created_at);
            ItemPlan {
                manifest_path: file.path.clone(),
                dest,
                category,
                true_len: file.true_len,
                chunks: file.chunks.clone(),
                disposition,
            }
        })
        .collect();

    let (packs_needed, bytes_to_fetch) = to_fetch(&resolved.index, &items);
    Ok(RestorePlan {
        items,
        counter: resolved.root.counter,
        created_at,
        repo_id: resolved.root.repo_id.clone(),
        packs_needed,
        bytes_to_fetch,
    })
}

fn decide_entry(
    ctx: &RestoreCtx<'_>,
    keys: &Keys,
    file: &FileEntry,
    category: SyncCategory,
    created_at: DateTime<Utc>,
) -> (Option<PathBuf>, Disposition) {
    let _ = (ctx, keys, file, category, created_at);
    todo!("plan 5-03")
}

fn decide(
    local: Option<&LocalFacts>,
    remote: &RemoteFacts<'_>,
    credential: bool,
    opts: &RestoreOptions,
) -> Disposition {
    let _ = (local, remote, credential, opts);
    todo!("plan 5-03")
}

fn credential_bearing(manifest_path: &str, category: SyncCategory) -> bool {
    let _ = (manifest_path, category);
    todo!("plan 5-03")
}

fn local_at(dest: &Path, entry: &FileEntry, keys: &Keys) -> Local {
    let _ = (dest, entry, keys);
    todo!("plan 5-03")
}

fn to_fetch(index: &IndexObject, items: &[ItemPlan]) -> (usize, u64) {
    let _ = (index, items);
    todo!("plan 5-03")
}

/// Which category a bundle path belongs to, from its root prefix and the shape
/// beneath it — the same split `scope`'s collectors made on the way out.
fn category_of(manifest_path: &str) -> SyncCategory {
    let (prefix, rest) = manifest_path.split_once('/').unwrap_or((manifest_path, ""));
    match prefix {
        "desktop-profiles" => SyncCategory::Credentials,
        "desktop-data" if rest.starts_with("claude-code-sessions/") => SyncCategory::ChatIndex,
        "desktop-data" => SyncCategory::Routines,
        "claude-home" if rest.starts_with("projects/") => SyncCategory::Transcripts,
        "claude-home" => SyncCategory::Routines,
        _ => SyncCategory::Config,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{CHUNK_SIZE, SyncRoots};
    use crate::sync::crypto::{KdfParams, Keyfile};
    use crate::sync::github::token::TokenSource;
    use crate::sync::github::{Client, Endpoints, RepoRef};
    use crate::sync::model::{IndexEntry, Manifest, Root};
    use std::fs;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;
    use zeroize::Zeroizing;

    /// Microseconds instead of ~1.5 s and a gibibyte. The AUR `check()` runs
    /// these on an installer's machine.
    const CHEAP: KdfParams = KdfParams {
        m_kib: 8,
        t: 1,
        p: 1,
    };

    /// The snapshot's capture time. A fixed constant: nothing here reads a
    /// clock, which is what `decide` taking `created_at` as an argument buys.
    const SNAPSHOT: DateTime<Utc> = match DateTime::from_timestamp(1_700_000_000, 0) {
        Some(t) => t,
        None => panic!("a fixed timestamp"),
    };

    fn keys() -> Keys {
        Keyfile::create_with_floor(b"a-test-passphrase", CHEAP, CHEAP.m_kib)
            .expect("keyfile creation")
            .1
    }

    fn id(keys: &Keys, body: &[u8]) -> ChunkId {
        keys.chunk_id(body)
    }

    fn local(mtime: DateTime<Utc>, chunk_ids: Option<Vec<ChunkId>>) -> LocalFacts {
        LocalFacts { mtime, chunk_ids }
    }

    fn remote(chunk_ids: &[ChunkId]) -> RemoteFacts<'_> {
        RemoteFacts {
            chunk_ids,
            created_at: SNAPSHOT,
        }
    }

    fn opts(force: bool, force_credentials: bool) -> RestoreOptions {
        RestoreOptions {
            force,
            force_credentials,
            ..Default::default()
        }
    }

    const ORDINARY: bool = false;
    const CREDENTIAL: bool = true;

    // ---------------------------------------------------------------- decide

    #[test]
    fn an_absent_local_file_is_created() {
        let k = keys();
        let ids = vec![id(&k, b"remote")];
        assert_eq!(
            decide(None, &remote(&ids), ORDINARY, &opts(true, true)),
            Disposition::Create,
            "force must not change what a fresh machine does"
        );
    }

    /// D7: the digest short-circuits everything, so an interrupted restore
    /// re-run reports the conflicts it genuinely has — none.
    #[test]
    fn identical_chunk_ids_win_before_any_clock_is_consulted() {
        let k = keys();
        let ids = vec![id(&k, b"same"), id(&k, b"bytes")];
        for mtime in [
            SNAPSHOT - Duration::from_secs(3600),
            SNAPSHOT,
            SNAPSHOT + Duration::from_secs(3600),
        ] {
            for (force, force_creds) in [(false, false), (true, false), (true, true)] {
                for credential in [ORDINARY, CREDENTIAL] {
                    assert_eq!(
                        decide(
                            Some(&local(mtime, Some(ids.clone()))),
                            &remote(&ids),
                            credential,
                            &opts(force, force_creds),
                        ),
                        Disposition::SkipIdentical,
                        "{mtime} force={force} creds={force_creds} credential={credential}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_older_local_file_is_updated_without_any_force() {
        let k = keys();
        let mine = vec![id(&k, b"mine")];
        let theirs = vec![id(&k, b"theirs")];
        assert_eq!(
            decide(
                Some(&local(SNAPSHOT - Duration::from_secs(1), Some(mine))),
                &remote(&theirs),
                ORDINARY,
                &RestoreOptions::default(),
            ),
            Disposition::Update
        );
    }

    /// SAFE-03: the skip carries both times so the report can say *why*.
    #[test]
    fn a_locally_newer_file_is_skipped_and_names_both_times() {
        let k = keys();
        let mine = vec![id(&k, b"mine")];
        let theirs = vec![id(&k, b"theirs")];
        let newer = SNAPSHOT + Duration::from_secs(1);
        assert_eq!(
            decide(
                Some(&local(newer, Some(mine))),
                &remote(&theirs),
                ORDINARY,
                &RestoreOptions::default(),
            ),
            Disposition::SkipLocalNewer {
                local_mtime: newer,
                remote_mtime: SNAPSHOT,
            }
        );
    }

    #[test]
    fn force_overwrites_a_locally_newer_ordinary_file() {
        let k = keys();
        let newer = SNAPSHOT + Duration::from_secs(1);
        assert_eq!(
            decide(
                Some(&local(newer, Some(vec![id(&k, b"mine")]))),
                &remote(&[id(&k, b"theirs")]),
                ORDINARY,
                &opts(true, false),
            ),
            Disposition::Overwrite {
                local_mtime: newer,
                remote_mtime: SNAPSHOT,
            }
        );
    }

    /// D2, the whole point of `force_credentials` being a separate field.
    #[test]
    fn force_alone_never_overwrites_a_locally_newer_credential() {
        let k = keys();
        let newer = SNAPSHOT + Duration::from_secs(1);
        assert_eq!(
            decide(
                Some(&local(newer, Some(vec![id(&k, b"live-token")]))),
                &remote(&[id(&k, b"stale-token")]),
                CREDENTIAL,
                &opts(true, false),
            ),
            Disposition::NeedsCredentialConfirm {
                local_mtime: newer,
                remote_mtime: SNAPSHOT,
            }
        );
    }

    #[test]
    fn the_second_consent_promotes_a_credential_and_only_alongside_force() {
        let k = keys();
        let newer = SNAPSHOT + Duration::from_secs(1);
        let call = |o: RestoreOptions| {
            decide(
                Some(&local(newer, Some(vec![id(&k, b"live-token")]))),
                &remote(&[id(&k, b"stale-token")]),
                CREDENTIAL,
                &o,
            )
        };
        assert_eq!(
            call(opts(true, true)),
            Disposition::Overwrite {
                local_mtime: newer,
                remote_mtime: SNAPSHOT,
            }
        );
        assert_eq!(
            call(opts(false, true)),
            Disposition::SkipLocalNewer {
                local_mtime: newer,
                remote_mtime: SNAPSHOT,
            },
            "`force_credentials` is a second consent on top of `force`, not a substitute for it"
        );
    }

    /// The second confirmation guards the *loss*, not the category.
    #[test]
    fn a_credential_that_is_not_locally_newer_is_an_ordinary_update() {
        let k = keys();
        assert_eq!(
            decide(
                Some(&local(
                    SNAPSHOT - Duration::from_secs(60),
                    Some(vec![id(&k, b"stale")])
                )),
                &remote(&[id(&k, b"fresh")]),
                CREDENTIAL,
                &RestoreOptions::default(),
            ),
            Disposition::Update
        );
    }

    /// The boundary, pinned from both sides: equal is not newer.
    #[test]
    fn an_mtime_equal_to_the_snapshot_time_updates_and_one_nanosecond_later_does_not() {
        let k = keys();
        let mine = vec![id(&k, b"mine")];
        let theirs = vec![id(&k, b"theirs")];
        let at = |mtime| {
            decide(
                Some(&local(mtime, Some(mine.clone()))),
                &remote(&theirs),
                ORDINARY,
                &RestoreOptions::default(),
            )
        };
        assert_eq!(at(SNAPSHOT), Disposition::Update);
        assert_eq!(
            at(SNAPSHOT - Duration::from_nanos(1)),
            Disposition::Update,
            "a hair older is still older"
        );
        assert!(matches!(
            at(SNAPSHOT + Duration::from_nanos(1)),
            Disposition::SkipLocalNewer { .. }
        ));
    }

    /// An unreadable file has no chunk ids, and "no chunk ids" must never
    /// compare equal to a zero-chunk manifest entry.
    #[test]
    fn a_local_file_whose_bytes_could_not_be_read_is_never_identical() {
        assert_eq!(
            decide(
                Some(&local(SNAPSHOT - Duration::from_secs(1), None)),
                &remote(&[]),
                ORDINARY,
                &RestoreOptions::default(),
            ),
            Disposition::Update,
            "an empty remote entry must not swallow an unreadable local file"
        );
        assert!(
            matches!(
                decide(
                    Some(&local(SNAPSHOT + Duration::from_secs(1), None)),
                    &remote(&[]),
                    ORDINARY,
                    &RestoreOptions::default(),
                ),
                Disposition::SkipLocalNewer { .. }
            ),
            "and it still gets SAFE-03's protection"
        );
    }

    // ------------------------------------------------ credential classification

    #[test]
    fn the_credential_arm_covers_the_profile_store_and_every_dot_credentials_json() {
        for path in [
            "desktop-profiles/work/meta.json",
            "desktop-profiles/work/token-cache.json",
            "config/accounts/work/.credentials.json",
            "claude-home/.credentials.json",
        ] {
            assert!(
                credential_bearing(path, category_of(path)),
                "{path} would lose its second consent"
            );
        }
        for path in [
            "config/config.toml",
            "claude-home/scheduled-tasks/daily.json",
            "claude-home/projects/repo/session.jsonl",
            "desktop-data/claude-code-sessions/a/o/local_1.json",
        ] {
            assert!(
                !credential_bearing(path, category_of(path)),
                "{path} would demand a confirmation it does not need"
            );
        }
    }

    #[test]
    fn each_root_prefix_lands_in_the_category_its_collector_came_from() {
        for (path, expected) in [
            ("config/config.toml", SyncCategory::Config),
            (
                "config/accounts/work/.credentials.json",
                SyncCategory::Config,
            ),
            ("desktop-profiles/work/meta.json", SyncCategory::Credentials),
            (
                "desktop-data/claude-code-sessions/a/o/local_1.json",
                SyncCategory::ChatIndex,
            ),
            (
                "desktop-data/a/o/scheduled-tasks.json",
                SyncCategory::Routines,
            ),
            (
                "claude-home/scheduled-tasks/daily.json",
                SyncCategory::Routines,
            ),
            (
                "claude-home/projects/repo/s.jsonl",
                SyncCategory::Transcripts,
            ),
        ] {
            assert_eq!(category_of(path), expected, "{path}");
        }
    }

    // ------------------------------------------------------------------ plan

    /// Everything `plan` needs, with no network and no real `$HOME`.
    struct Machine {
        dir: TempDir,
        roots: SyncRoots,
        repo: RepoRef,
        passphrase: Zeroizing<String>,
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
                dir,
                roots,
                repo: RepoRef::parse("o/n").unwrap(),
                passphrase: Zeroizing::new("a-test-passphrase".into()),
            }
        }

        /// A destination, seeded with `body` and stamped with `mtime`.
        fn seed(&self, manifest_path: &str, body: &[u8], mtime: DateTime<Utc>) -> PathBuf {
            let dest = layout::from_manifest_path(&self.roots, manifest_path).unwrap();
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            fs::write(&dest, body).unwrap();
            let at = SystemTime::UNIX_EPOCH
                + Duration::from_nanos(u64::try_from(mtime.timestamp_nanos_opt().unwrap()).unwrap());
            fs::File::options()
                .write(true)
                .open(&dest)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(at))
                .unwrap();
            dest
        }

        fn client(&self) -> Client {
            Client::new(
                &Endpoints {
                    api_base: "http://127.0.0.1:1".into(),
                    uploads_base: "http://127.0.0.1:1".into(),
                },
                Zeroizing::new("github_pat_fixture_not_a_real_token".into()),
                TokenSource::Env,
            )
            .unwrap()
        }

        fn ctx<'a>(&'a self, client: &'a Client, opts: RestoreOptions) -> RestoreCtx<'a> {
            RestoreCtx {
                client,
                repo: &self.repo,
                roots: &self.roots,
                repo_id: "github:1",
                passphrase: &self.passphrase,
                anchor_path: &self.roots.config_dir,
                backups_dir: &self.roots.config_dir,
                opts,
                now: SNAPSHOT,
            }
        }
    }

    /// A snapshot of `(manifest path, body)` pairs, one chunk and one pack per
    /// entry — enough for every decision `plan` makes.
    fn snapshot(entries: &[(&str, &[u8])]) -> Resolved {
        let k = keys();
        let mut files = Vec::new();
        let mut index = Vec::new();
        for (i, (path, body)) in entries.iter().enumerate() {
            let chunk = k.chunk_id(body);
            files.push(FileEntry {
                path: (*path).to_string(),
                mode: 0o600,
                true_len: body.len() as u64,
                chunks: if body.is_empty() {
                    Vec::new()
                } else {
                    vec![chunk]
                },
            });
            index.push(IndexEntry {
                id: chunk,
                pack: ChunkId::from_bytes([i as u8; 32]),
                offset: 0,
                clen: 100 + i as u32,
                true_len: body.len() as u32,
            });
        }
        Resolved {
            root: Root::new(
                7,
                SNAPSHOT,
                "github:1".into(),
                Vec::new(),
                KdfParams::default(),
            ),
            manifest: Manifest::new(files),
            index: IndexObject::new(index, Vec::new()),
            packs: PackSource::empty(k),
        }
    }

    fn plan_with(machine: &Machine, resolved: &Resolved, opts: RestoreOptions) -> RestorePlan {
        let client = machine.client();
        plan(&machine.ctx(&client, opts), resolved).expect("planning is infallible here")
    }

    /// D6: N entries in, N `ItemPlan`s out — including the two that are refused.
    #[test]
    fn every_manifest_entry_becomes_exactly_one_item_plan_refusals_included() {
        let m = Machine::new();
        m.seed(
            "config/config.toml",
            b"[sync]\nenabled = true\n",
            SNAPSHOT - Duration::from_secs(60),
        );
        let identical = b"{\"tasks\":[]}";
        m.seed(
            "claude-home/scheduled-tasks/daily.json",
            identical,
            SNAPSHOT - Duration::from_secs(60),
        );
        m.seed(
            "config/accounts/work/.credentials.json",
            b"{\"token\":\"live\"}",
            SNAPSHOT + Duration::from_secs(60),
        );

        let resolved = snapshot(&[
            ("config/config.toml", b"[sync]\nenabled = false\n"),
            ("claude-home/scheduled-tasks/daily.json", identical),
            ("config/accounts/work/.credentials.json", b"{\"t\":\"old\"}"),
            ("claude-home/projects/new/session.jsonl", b"{}"),
            ("config/bridge-state.json", b"machine-bound"),
            ("config/../../../../etc/shadow", b"hostile"),
        ]);

        let plan = plan_with(&m, &resolved, RestoreOptions::default());
        assert_eq!(
            plan.items.len(),
            resolved.manifest.files.len(),
            "an entry was dropped on the floor"
        );

        let by_path = |p: &str| {
            plan.items
                .iter()
                .find(|i| i.manifest_path == p)
                .unwrap_or_else(|| panic!("{p} is missing from the plan"))
        };
        assert_eq!(
            by_path("config/config.toml").disposition,
            Disposition::Update
        );
        assert_eq!(
            by_path("claude-home/scheduled-tasks/daily.json").disposition,
            Disposition::SkipIdentical
        );
        assert!(matches!(
            by_path("config/accounts/work/.credentials.json").disposition,
            Disposition::SkipLocalNewer { .. }
        ));
        assert_eq!(
            by_path("claude-home/projects/new/session.jsonl").disposition,
            Disposition::Create
        );
        assert_eq!(
            by_path("config/bridge-state.json").disposition,
            Disposition::ExcludedByPolicy
        );
        assert!(matches!(
            by_path("config/../../../../etc/shadow").disposition,
            Disposition::RejectedPath(_)
        ));

        for refused in [
            "config/bridge-state.json",
            "config/../../../../etc/shadow",
        ] {
            assert!(
                by_path(refused).dest.is_none(),
                "{refused} was handed a destination"
            );
        }
        assert!(
            plan.items.iter().filter(|i| i.disposition.writes()).count() == 2,
            "only the update and the create may be written"
        );
    }

    /// T-5-22: a link planted at a destination is seen as a link, and no
    /// combination of consents writes through it.
    #[test]
    #[cfg(unix)]
    fn a_symlink_at_the_destination_is_refused_under_every_force() {
        let m = Machine::new();
        let outside = m.dir.path().join("elsewhere/secret.txt");
        fs::create_dir_all(outside.parent().unwrap()).unwrap();
        fs::write(&outside, b"not yours").unwrap();

        let dest = layout::from_manifest_path(&m.roots, "config/config.toml").unwrap();
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &dest).unwrap();

        let resolved = snapshot(&[("config/config.toml", b"[sync]\n")]);
        for o in [opts(false, false), opts(true, false), opts(true, true)] {
            let plan = plan_with(&m, &resolved, o);
            let item = &plan.items[0];
            match &item.disposition {
                Disposition::RejectedPath(why) => assert!(
                    why.contains("symbolic link"),
                    "the refusal does not say why: {why}"
                ),
                other => panic!("a symlink destination became {other:?}"),
            }
            assert!(!item.disposition.writes());
            assert!(item.dest.is_none());
        }
        assert_eq!(fs::read(&outside).unwrap(), b"not yours");
    }

    #[test]
    fn a_directory_where_a_file_is_expected_is_refused_rather_than_planned() {
        let m = Machine::new();
        let dest = layout::from_manifest_path(&m.roots, "config/config.toml").unwrap();
        fs::create_dir_all(&dest).unwrap();

        let resolved = snapshot(&[("config/config.toml", b"[sync]\n")]);
        let plan = plan_with(&m, &resolved, opts(true, true));
        match &plan.items[0].disposition {
            Disposition::RejectedPath(why) => assert!(why.contains("directory"), "{why}"),
            other => panic!("a directory destination became {other:?}"),
        }
    }

    /// T-5-24: identity is what is on disk, not what a size or an index row
    /// claims.
    #[test]
    fn a_file_of_the_same_length_but_different_bytes_is_not_identical() {
        let m = Machine::new();
        m.seed(
            "config/config.toml",
            b"aaaaaaaa",
            SNAPSHOT - Duration::from_secs(60),
        );
        let resolved = snapshot(&[("config/config.toml", b"bbbbbbbb")]);
        let plan = plan_with(&m, &resolved, RestoreOptions::default());
        assert_eq!(plan.items[0].disposition, Disposition::Update);
    }

    /// D7: apply the same snapshot onto its own output and nothing is a
    /// conflict, whatever the local mtime says.
    #[test]
    fn a_second_apply_is_a_no_op_even_when_the_local_copy_is_newer() {
        let m = Machine::new();
        let body = b"{\"tasks\":[\"daily\"]}";
        m.seed(
            "claude-home/scheduled-tasks/daily.json",
            body,
            SNAPSHOT + Duration::from_secs(86_400),
        );
        let resolved = snapshot(&[("claude-home/scheduled-tasks/daily.json", body)]);
        let plan = plan_with(&m, &resolved, RestoreOptions::default());
        assert_eq!(plan.items[0].disposition, Disposition::SkipIdentical);
        assert_eq!(plan.packs_needed, 0);
        assert_eq!(plan.bytes_to_fetch, 0);
    }

    /// A file longer than one chunk is hashed in the same buffers the push side
    /// used, so it is recognised across machines rather than re-fetched.
    #[test]
    fn a_multi_chunk_file_is_recognised_as_identical() {
        let m = Machine::new();
        let body: Vec<u8> = (0..CHUNK_SIZE + 17).map(|i| (i % 251) as u8).collect();
        let dest =
            layout::from_manifest_path(&m.roots, "claude-home/projects/r/s.jsonl").unwrap();
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(&dest, &body).unwrap();

        let k = keys();
        let chunks: Vec<ChunkId> = body.chunks(CHUNK_SIZE).map(|c| k.chunk_id(c)).collect();
        assert_eq!(chunks.len(), 2, "the fixture must span two chunks");

        let resolved = Resolved {
            root: Root::new(
                7,
                SNAPSHOT,
                "github:1".into(),
                Vec::new(),
                KdfParams::default(),
            ),
            manifest: Manifest::new(vec![FileEntry {
                path: "claude-home/projects/r/s.jsonl".into(),
                mode: 0o600,
                true_len: body.len() as u64,
                chunks,
            }]),
            index: IndexObject::new(Vec::new(), Vec::new()),
            packs: PackSource::empty(k),
        };
        let plan = plan_with(&m, &resolved, RestoreOptions::default());
        assert_eq!(plan.items[0].disposition, Disposition::SkipIdentical);
    }

    /// A dry run's headline number is what a real run would fetch — the packs
    /// behind the items that will actually be written, and no others.
    #[test]
    fn packs_needed_and_bytes_to_fetch_count_only_the_items_that_will_be_written() {
        let m = Machine::new();
        let kept = b"{\"tasks\":[]}";
        m.seed(
            "claude-home/scheduled-tasks/daily.json",
            kept,
            SNAPSHOT - Duration::from_secs(60),
        );
        let resolved = snapshot(&[
            ("claude-home/scheduled-tasks/daily.json", kept), // identical, pack 0
            ("config/config.toml", b"[sync]\n"),              // create,    pack 1
        ]);
        let plan = plan_with(&m, &resolved, RestoreOptions::default());
        assert_eq!(plan.packs_needed, 1, "the skipped item's pack is not needed");
        assert_eq!(
            plan.bytes_to_fetch, 101,
            "only pack 1's sealed length is counted"
        );
    }

    /// Permissions, not a panic and not a silent skip.
    #[test]
    #[cfg(unix)]
    fn an_unreadable_local_file_is_planned_rather_than_panicking() {
        use std::os::unix::fs::PermissionsExt;

        let m = Machine::new();
        let dest = m.seed(
            "config/config.toml",
            b"[sync]\n",
            SNAPSHOT - Duration::from_secs(60),
        );
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::File::open(&dest).is_ok() {
            return; // running as root: the premise does not hold
        }

        let resolved = snapshot(&[("config/config.toml", b"[sync]\nenabled = true\n")]);
        let plan = plan_with(&m, &resolved, RestoreOptions::default());
        assert_eq!(plan.items[0].disposition, Disposition::Update);
    }

    /// Restore is additive: a local file the snapshot never heard of is not in
    /// the plan at all, and nothing plans a deletion.
    #[test]
    fn a_local_file_the_manifest_does_not_mention_is_left_alone() {
        let m = Machine::new();
        let untouched = m.seed(
            "claude-home/scheduled-tasks/mine.json",
            b"{\"mine\":true}",
            SNAPSHOT + Duration::from_secs(1),
        );
        let resolved = snapshot(&[("config/config.toml", b"[sync]\n")]);
        let plan = plan_with(&m, &resolved, opts(true, true));
        assert_eq!(plan.items.len(), 1);
        assert!(
            !plan
                .items
                .iter()
                .any(|i| i.dest.as_deref() == Some(untouched.as_path()))
        );
        assert!(untouched.exists());
    }

    /// The plan's own header comes from the root, never from the pointer.
    #[test]
    fn the_plan_carries_the_roots_counter_repo_id_and_capture_time() {
        let m = Machine::new();
        let resolved = snapshot(&[("config/config.toml", b"[sync]\n")]);
        let plan = plan_with(&m, &resolved, RestoreOptions::default());
        assert_eq!(plan.counter, 7);
        assert_eq!(plan.repo_id, "github:1");
        assert_eq!(plan.created_at, SNAPSHOT);
    }
}
