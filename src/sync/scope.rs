//! What gets collected, and — more importantly — what never does.
//!
//! One bounded walker, one exclusion predicate, five categories funnelling
//! through both. Owned by plan 2-01; plan 2-02 fills the three collector arms
//! it left empty and plan 2-04 fills [`super::transcripts`].
//!
//! The exclusion predicate lives *in the walker*, not in each collector. D2's
//! entries are not a size optimisation — each one is wrong to carry to another
//! machine — so a category added later inherits the rule instead of having to
//! remember it.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::config::{SyncCategory, SyncConfig};
use crate::sync::SyncRoots;

/// Ceiling on entries visited per walk. Mirrors [`crate::context`]: a runaway
/// tree stops and says so rather than scanning the machine. Generous enough for
/// this user's measured worst case (4110 transcripts, ~1300 session indexes).
const MAX_WALK_ENTRIES: usize = 200_000;

/// Never collected, whatever the category. See D2.
///
/// `bridge-state.json` holds a volatile remote-control session id — restoring a
/// stale one has already broken `/remote-control` here with a `session_url`
/// crash, and it is deleted on every account switch anyway.
/// `ant-device-registry.json` is a browser-extension pairing authorised
/// server-side per account; it cannot be made valid elsewhere.
const EXCLUDED_NAMES: [&str; 5] = [
    "bridge-state.json",
    "ant-device-registry.json",
    ".stale",
    ".last_error",
    ".fetch.lock",
];

/// Directory names that are never descended into, and never collected.
///
/// `backups`/`prelogin-backup`/`hidden` are local rollback state whose meaning
/// is machine-specific. `local-agent-mode-sessions` is Cowork: its paths embed
/// the owning account UUID plus an unreconstructable suffix, so a copy renders
/// as an empty chat — already documented as unmigratable.
const EXCLUDED_DIRS: [&str; 4] = [
    "backups",
    "prelogin-backup",
    "hidden",
    "local-agent-mode-sessions",
];

/// Regenerable or in-flight. `.tmp.` is the prefix [`crate::cache::atomic_write`]
/// gives its tempfiles, so a concurrent write is never half-collected.
const EXCLUDED_SUFFIXES: [&str; 3] = [".lock", ".tmp", "-journal"];
const EXCLUDED_PREFIX: &str = ".tmp.";

// Names owned by claude-acc's profile store and Claude Desktop's data dir.
// They mirror the private consts in [`crate::claude_desktop`] (`META_JSON`,
// `TOKEN_CACHE`, `TOKEN_CACHE_V2`, `DESKTOP_STATE`, `SESSIONS_DIR`) and
// `claude_desktop::merge::SCHEDULED_TASKS`. Restated here rather than reached
// for as a literal at each use site: if that layout moves, both blocks move.
const META_JSON: &str = "meta.json";
const TOKEN_CACHE: &str = "config-tokenCache";
const TOKEN_CACHE_V2: &str = "config-tokenCacheV2";
const DESKTOP_STATE: &str = "desktop-state";
const SESSIONS_DIR: &str = "claude-code-sessions";
/// The per-account registry, one per `<account>/<org>/`.
const SCHEDULED_TASKS: &str = "scheduled-tasks.json";
/// `~/.claude/scheduled-tasks/` — Claude Code's own routine definitions, a
/// different tree from the per-account registry above.
const SCHEDULED_TASKS_DIR: &str = "scheduled-tasks";
/// A chat session index. The sibling `scheduled-tasks.json` lives in the same
/// folder but belongs to the routines category, so the two never double-count.
const SESSION_PREFIX: &str = "local_";

/// One collected file and the three quarters of D5's change-detection key that
/// come from its metadata; the fourth is the path itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ns: i128,
    pub inode: u64,
}

/// The result of scanning one category.
#[derive(Debug, Clone)]
pub struct CategoryScan {
    pub category: SyncCategory,
    pub files: Vec<FileEntry>,
    pub bytes: u64,
    /// Files dropped by a *bound*, not by an exclusion. Stays zero for every
    /// category except transcripts, whose D3 age/byte bounds leave a remainder
    /// the user needs told about. Declared here rather than by plan 2-04 so
    /// that plan owns exactly one file.
    pub excluded_files: usize,
    pub excluded_bytes: u64,
    /// The walk hit [`MAX_WALK_ENTRIES`] and stopped. Reported, never silent.
    pub walk_capped: bool,
    /// Entries whose directory or metadata could not be read.
    pub skipped: usize,
}

impl CategoryScan {
    /// A category that is switched off, unimplemented, or has no tree on disk.
    /// Also the starting point every collector accumulates into.
    pub fn empty(category: SyncCategory) -> Self {
        Self {
            category,
            files: Vec::new(),
            bytes: 0,
            excluded_files: 0,
            excluded_bytes: 0,
            walk_capped: false,
            skipped: 0,
        }
    }

    fn push(&mut self, entry: FileEntry) {
        self.bytes += entry.size;
        self.files.push(entry);
    }
}

/// D2 in full. True means "never carry this to another machine".
pub fn is_excluded(path: &Path) -> bool {
    // Any excluded directory anywhere above the entry disqualifies it. The
    // walker already refuses to descend into one, so this is belt-and-braces
    // for the paths that are added directly rather than walked to.
    if path
        .components()
        .any(|c| EXCLUDED_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()))
    {
        return true;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        // A non-UTF-8 name cannot be matched against the rules above, and this
        // is a security predicate: refuse what cannot be checked.
        return true;
    };
    EXCLUDED_NAMES.contains(&name)
        || name.starts_with(EXCLUDED_PREFIX)
        || EXCLUDED_SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// `(size, mtime_ns, inode)` — the single place the platform split lives, so
/// D5's change-detection key has one producer.
fn stats(md: &fs::Metadata) -> (u64, i128, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mtime_ns = i128::from(md.mtime()) * 1_000_000_000 + i128::from(md.mtime_nsec());
        (md.size(), mtime_ns, md.ino())
    }
    #[cfg(not(unix))]
    {
        // No inode concept: 0 is a sentinel the comparison treats as "no
        // opinion", leaving (path, size, mtime_ns) to carry the key.
        let mtime_ns = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0i128, |d| i128::from(d.as_nanos() as u64));
        (md.len(), mtime_ns, 0)
    }
}

/// Collect one explicitly named file — the config.toml case, where there is no
/// directory to walk. Honours the same exclusion and symlink rules as the
/// walker; a missing file is simply absent.
pub(crate) fn push_path(path: &Path, out: &mut CategoryScan) {
    if is_excluded(path) {
        return;
    }
    let Ok(md) = fs::symlink_metadata(path) else {
        return;
    };
    if !md.is_file() {
        return;
    }
    let (size, mtime_ns, inode) = stats(&md);
    out.push(FileEntry {
        path: path.to_path_buf(),
        size,
        mtime_ns,
        inode,
    });
}

/// The single walker. Every category funnels through it, so [`is_excluded`]
/// cannot be bypassed by a collector added later.
///
/// A missing root is not an error — an account that has never run the Desktop
/// app simply has no tree — and symlinks are never followed, for files or
/// directories alike, so a link planted in a scanned tree cannot pull an
/// arbitrary host file into the bundle set.
pub(crate) fn walk(root: &Path, out: &mut CategoryScan) {
    walk_bounded(root, out, MAX_WALK_ENTRIES);
}

fn walk_bounded(root: &Path, out: &mut CategoryScan, max_entries: usize) {
    if !root.is_dir() {
        return;
    }
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0usize;

    'walk: while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                out.skipped += 1;
                continue;
            }
        };
        for entry in entries {
            if visited >= max_entries {
                out.walk_capped = true;
                break 'walk;
            }
            visited += 1;

            let Ok(entry) = entry else {
                out.skipped += 1;
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                out.skipped += 1;
                continue;
            };
            // Never followed. `file_type` here comes from `read_dir`, which
            // does not traverse the link, so this is the symlink itself.
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if is_excluded(&path) {
                continue;
            }
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let Ok(md) = entry.metadata() else {
                out.skipped += 1;
                continue;
            };
            let (size, mtime_ns, inode) = stats(&md);
            out.push(FileEntry {
                path,
                size,
                mtime_ns,
                inode,
            });
        }
    }
}

/// Immediate subdirectories, by directory listing alone — a profile is a
/// directory, exactly as [`crate::claude_desktop::load_profiles`] already
/// defines one, and an account is a directory whether or not its profile was
/// ever captured.
///
/// A symlinked directory is not returned, on the same rule as [`walk`]:
/// `file_type` comes from `read_dir`, which does not traverse the link, so a
/// link planted in the store cannot pull an arbitrary host tree into the scan.
/// An unreadable directory reads as empty rather than as an error.
fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

/// Every `<sessions_root>/<account>/<org>/`.
fn account_org_dirs(sessions_root: &Path) -> Vec<PathBuf> {
    subdirs(sessions_root)
        .iter()
        .flat_map(|account| subdirs(account))
        .collect()
}

/// Scan one category against the injected roots.
///
/// `now` is threaded in because the transcripts arm's D3 bounds are
/// time-dependent and no test in this crate reads the wall clock. The other
/// four arms ignore it; declaring it here means plan 2-04 never edits this file.
pub fn collect(
    cat: SyncCategory,
    roots: &SyncRoots,
    cfg: &SyncConfig,
    now: DateTime<Utc>,
) -> CategoryScan {
    if !cfg.includes(cat) {
        return CategoryScan::empty(cat);
    }
    let mut scan = CategoryScan::empty(cat);
    match cat {
        SyncCategory::Config => {
            // D1: config.toml itself, plus `accounts/*/.credentials.json` —
            // the credential, not the rest of a CLAUDE_CONFIG_DIR account tree.
            walk(&roots.config_dir.join("accounts"), &mut scan);
            scan.files
                .retain(|f| f.path.file_name().is_some_and(|n| n == ".credentials.json"));
            scan.bytes = scan.files.iter().map(|f| f.size).sum();
            push_path(&roots.config_file, &mut scan);
        }
        SyncCategory::Credentials => {
            // D1: per profile, `meta.json`, both token caches and
            // `desktop-state/` — and nothing else in the store. A profile
            // without a readable meta.json is skipped rather than failing the
            // other accounts, exactly as `load_profiles` already treats one.
            for profile in subdirs(&roots.desktop_profiles_dir) {
                let meta = profile.join(META_JSON);
                if !meta.is_file() {
                    continue;
                }
                push_path(&meta, &mut scan);
                push_path(&profile.join(TOKEN_CACHE), &mut scan);
                push_path(&profile.join(TOKEN_CACHE_V2), &mut scan);
                walk(&profile.join(DESKTOP_STATE), &mut scan);
            }
        }
        SyncCategory::Routines => {
            // D1: Claude Code's own routine definitions, plus each account's
            // registry. The account and org levels are enumerated by listing,
            // not from a profile's meta.json, so an account that was never
            // captured still has its routines carried.
            walk(&roots.claude_home.join(SCHEDULED_TASKS_DIR), &mut scan);
            for org in account_org_dirs(&roots.desktop_data_dir.join(SESSIONS_DIR)) {
                push_path(&org.join(SCHEDULED_TASKS), &mut scan);
            }
        }
        SyncCategory::ChatIndex => {
            // D1: `claude-code-sessions/<account>/<org>/local_*.json`. Walking
            // the root and filtering by name keeps `local-agent-mode-sessions/`
            // on the shared D2 predicate — a Cowork transcript's path embeds an
            // unreconstructable account suffix, so a copy renders as an empty
            // chat. No file body is read; a stat per entry is the whole cost.
            walk(&roots.desktop_data_dir.join(SESSIONS_DIR), &mut scan);
            scan.files.retain(|f| {
                f.path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(SESSION_PREFIX) && n.ends_with(".json"))
            });
            scan.bytes = scan.files.iter().map(|f| f.size).sum();
        }
        // Owned by plan 2-04.
        SyncCategory::Transcripts => return super::transcripts::collect_bounded(roots, cfg, now),
    }
    scan
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn seed(dir: &Path, rel: &str, body: &str) -> PathBuf {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    /// Roots that all point into one TempDir. Nothing here resolves `$HOME`.
    fn roots_at(dir: &TempDir) -> SyncRoots {
        SyncRoots::at(
            dir.path().join("config.toml"),
            dir.path().to_path_buf(),
            dir.path().join("desktop"),
            dir.path().join("profiles"),
            dir.path().join("claude-home"),
        )
    }

    fn names(scan: &CategoryScan) -> Vec<String> {
        let mut v: Vec<String> = scan
            .files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn a_file_under_the_root_is_collected_with_size_mtime_and_inode() {
        let dir = TempDir::new().unwrap();
        seed(dir.path(), "a/one.json", "hello");
        let mut scan = CategoryScan::empty(SyncCategory::Config);
        walk(dir.path(), &mut scan);

        assert_eq!(scan.files.len(), 1);
        assert_eq!(scan.files[0].size, 5);
        assert_eq!(scan.bytes, 5);
        assert!(scan.files[0].mtime_ns > 0);
        #[cfg(unix)]
        assert!(scan.files[0].inode > 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_file_outside_the_root_contributes_nothing() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let secret = seed(outside.path(), "id_rsa", "PRIVATE KEY");
        fs::create_dir_all(dir.path().join("a")).unwrap();
        symlink(&secret, dir.path().join("a/linked.json")).unwrap();

        let mut scan = CategoryScan::empty(SyncCategory::Config);
        walk(dir.path(), &mut scan);
        assert!(scan.files.is_empty(), "{:?}", scan.files);
        assert_eq!(scan.bytes, 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_symlink_is_not_descended_into() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        seed(outside.path(), "deep/secret.json", "{}");
        symlink(outside.path(), dir.path().join("elsewhere")).unwrap();

        let mut scan = CategoryScan::empty(SyncCategory::Config);
        walk(dir.path(), &mut scan);
        assert!(scan.files.is_empty(), "{:?}", scan.files);
    }

    #[cfg(unix)]
    #[test]
    fn an_explicitly_named_path_that_is_a_symlink_is_refused_too() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let secret = seed(outside.path(), "id_rsa", "PRIVATE KEY");
        let link = dir.path().join("config.toml");
        symlink(&secret, &link).unwrap();

        let mut scan = CategoryScan::empty(SyncCategory::Config);
        push_path(&link, &mut scan);
        assert!(scan.files.is_empty());
    }

    #[test]
    fn every_d2_hard_excluded_name_is_rejected() {
        let dir = TempDir::new().unwrap();
        for name in EXCLUDED_NAMES {
            seed(dir.path(), name, "x");
        }
        seed(dir.path(), "keep.json", "x");

        let mut scan = CategoryScan::empty(SyncCategory::Config);
        walk(dir.path(), &mut scan);
        assert_eq!(names(&scan), vec!["keep.json"]);
    }

    #[test]
    fn every_d2_excluded_directory_component_is_rejected() {
        let dir = TempDir::new().unwrap();
        for d in EXCLUDED_DIRS {
            seed(dir.path(), &format!("{d}/inner.json"), "x");
            seed(dir.path(), &format!("nested/{d}/inner.json"), "x");
            assert!(is_excluded(&dir.path().join(d).join("inner.json")));
        }
        seed(dir.path(), "keep.json", "x");

        let mut scan = CategoryScan::empty(SyncCategory::Config);
        walk(dir.path(), &mut scan);
        assert_eq!(names(&scan), vec!["keep.json"]);
    }

    #[test]
    fn every_d2_suffix_and_the_atomic_write_tempfile_prefix_are_rejected() {
        let dir = TempDir::new().unwrap();
        for name in ["a.lock", "b.tmp", "state-journal", ".tmp.abc123"] {
            seed(dir.path(), name, "x");
            assert!(is_excluded(&dir.path().join(name)), "{name}");
        }
        seed(dir.path(), "keep.json", "x");

        let mut scan = CategoryScan::empty(SyncCategory::Config);
        walk(dir.path(), &mut scan);
        assert_eq!(names(&scan), vec!["keep.json"]);
    }

    #[test]
    fn a_tree_wider_than_the_cap_stops_and_says_it_was_capped() {
        let dir = TempDir::new().unwrap();
        for i in 0..10 {
            seed(dir.path(), &format!("f{i}.json"), "x");
        }
        let mut scan = CategoryScan::empty(SyncCategory::Config);
        walk_bounded(dir.path(), &mut scan, 3);

        assert!(scan.walk_capped);
        assert_eq!(scan.files.len(), 3);
    }

    #[test]
    fn a_missing_root_is_an_empty_scan_not_an_error() {
        let dir = TempDir::new().unwrap();
        let mut scan = CategoryScan::empty(SyncCategory::Config);
        walk(&dir.path().join("never-ran"), &mut scan);
        assert!(scan.files.is_empty());
        assert!(!scan.walk_capped);
    }

    #[test]
    fn the_config_category_takes_config_toml_and_account_credentials_only() {
        let dir = TempDir::new().unwrap();
        seed(dir.path(), "config.toml", "[sync]\n");
        seed(dir.path(), "accounts/work/.credentials.json", "{}");
        seed(dir.path(), "accounts/personal/.credentials.json", "{}");
        // The rest of a CLAUDE_CONFIG_DIR account tree is not ours to carry.
        seed(dir.path(), "accounts/work/history.jsonl", "{}");
        seed(dir.path(), "accounts/work/.credentials.json.lock", "");

        let scan = collect(
            SyncCategory::Config,
            &roots_at(&dir),
            &SyncConfig::default(),
            Utc::now(),
        );
        assert_eq!(
            names(&scan),
            vec![".credentials.json", ".credentials.json", "config.toml"]
        );
        assert_eq!(scan.bytes, scan.files.iter().map(|f| f.size).sum::<u64>());
    }

    fn scan_of(cat: SyncCategory, dir: &TempDir) -> CategoryScan {
        collect(cat, &roots_at(dir), &SyncConfig::default(), Utc::now())
    }

    // ---- credentials: the four D1 profile members, and nothing else --------

    #[test]
    fn a_profile_yields_its_meta_both_token_caches_and_the_whole_desktop_state_tree() {
        let dir = TempDir::new().unwrap();
        seed(dir.path(), "profiles/gmail/meta.json", "{}");
        seed(dir.path(), "profiles/gmail/config-tokenCache", "x");
        seed(dir.path(), "profiles/gmail/config-tokenCacheV2", "x");
        seed(dir.path(), "profiles/gmail/desktop-state/Cookies", "x");
        seed(
            dir.path(),
            "profiles/gmail/desktop-state/Local Storage/leveldb/000003.log",
            "x",
        );
        // The rest of a profile directory is not a D1 member.
        seed(dir.path(), "profiles/gmail/config.json", "{}");

        let scan = scan_of(SyncCategory::Credentials, &dir);
        assert_eq!(
            names(&scan),
            vec![
                "000003.log",
                "Cookies",
                "config-tokenCache",
                "config-tokenCacheV2",
                "meta.json",
            ]
        );
        assert_eq!(scan.bytes, scan.files.iter().map(|f| f.size).sum::<u64>());
    }

    #[test]
    fn every_profile_is_collected_and_one_without_meta_json_does_not_fail_the_others() {
        let dir = TempDir::new().unwrap();
        // Four accounts is this user's normal case, not an edge case.
        for label in ["gmail", "hotmail", "struct", "toptal"] {
            seed(dir.path(), &format!("profiles/{label}/meta.json"), "{}");
            seed(
                dir.path(),
                &format!("profiles/{label}/config-tokenCache"),
                "x",
            );
        }
        // Hand-mangled: no readable meta.json, skipped as `load_profiles` does.
        seed(dir.path(), "profiles/broken/config-tokenCache", "x");

        let scan = scan_of(SyncCategory::Credentials, &dir);
        assert_eq!(scan.files.len(), 8, "{:?}", names(&scan));
        assert!(
            !scan
                .files
                .iter()
                .any(|f| f.path.to_string_lossy().contains("broken"))
        );
    }

    #[test]
    fn bridge_state_and_the_device_registry_never_leave_a_profile() {
        let dir = TempDir::new().unwrap();
        seed(dir.path(), "profiles/gmail/meta.json", "{}");
        for name in ["bridge-state.json", "ant-device-registry.json"] {
            seed(dir.path(), &format!("profiles/gmail/{name}"), "{}");
            seed(
                dir.path(),
                &format!("profiles/gmail/desktop-state/{name}"),
                "{}",
            );
        }
        seed(dir.path(), "profiles/gmail/desktop-state/Cookies", "x");

        let scan = scan_of(SyncCategory::Credentials, &dir);
        assert_eq!(names(&scan), vec!["Cookies", "meta.json"]);
    }

    #[test]
    fn rollback_state_beside_and_inside_the_profile_store_contributes_nothing() {
        let dir = TempDir::new().unwrap();
        seed(dir.path(), "profiles/gmail/meta.json", "{}");
        // Siblings of the store: outside the only root this collector is handed.
        seed(dir.path(), "backups/gmail/config-tokenCache", "x");
        seed(dir.path(), "prelogin-backup/config-tokenCache", "x");
        // The same names inside it, where the shared D2 predicate catches them.
        seed(dir.path(), "profiles/backups/meta.json", "{}");
        seed(dir.path(), "profiles/hidden/meta.json", "{}");

        let scan = scan_of(SyncCategory::Credentials, &dir);
        assert_eq!(names(&scan), vec!["meta.json"]);
    }

    #[test]
    fn a_missing_profile_store_is_an_empty_scan_not_an_error() {
        let dir = TempDir::new().unwrap();
        let scan = scan_of(SyncCategory::Credentials, &dir);
        assert!(scan.files.is_empty());
        assert_eq!(scan.bytes, 0);
        assert_eq!(scan.category, SyncCategory::Credentials);
    }

    #[test]
    fn unchecking_credentials_scans_nothing_even_with_a_full_profile_store() {
        let dir = TempDir::new().unwrap();
        seed(dir.path(), "profiles/gmail/meta.json", "{}");
        seed(dir.path(), "profiles/gmail/config-tokenCache", "x");
        let cfg = SyncConfig {
            categories: vec![SyncCategory::Config],
            ..SyncConfig::default()
        };
        let scan = collect(SyncCategory::Credentials, &roots_at(&dir), &cfg, Utc::now());
        assert!(scan.files.is_empty());
        assert_eq!(scan.bytes, 0);
    }

    // ---- routines: ~/.claude/scheduled-tasks/** plus each account registry --

    #[test]
    fn routines_take_the_claude_home_tree_and_every_account_registry() {
        let dir = TempDir::new().unwrap();
        seed(
            dir.path(),
            "claude-home/scheduled-tasks/daily-skill-update/task.json",
            "{}",
        );
        seed(
            dir.path(),
            "claude-home/scheduled-tasks/standup/report.md",
            "x",
        );
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-1/org-1/scheduled-tasks.json",
            "{}",
        );
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-2/org-2/scheduled-tasks.json",
            "{}",
        );
        // Chat indexes are the other category's.
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-1/org-1/local_abc.json",
            "{}",
        );

        let scan = scan_of(SyncCategory::Routines, &dir);
        assert_eq!(
            names(&scan),
            vec![
                "report.md",
                "scheduled-tasks.json",
                "scheduled-tasks.json",
                "task.json",
            ]
        );
    }

    #[test]
    fn a_missing_routines_tree_is_an_empty_scan_not_an_error() {
        let dir = TempDir::new().unwrap();
        let scan = scan_of(SyncCategory::Routines, &dir);
        assert!(scan.files.is_empty());
        assert_eq!(scan.bytes, 0);
    }

    // ---- chat_index: local_*.json only -------------------------------------

    #[test]
    fn the_chat_index_takes_local_session_files_from_every_account_and_nothing_else() {
        let dir = TempDir::new().unwrap();
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-1/org-1/local_abc.json",
            "{}",
        );
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-2/org-2/local_def.json",
            "{}",
        );
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-1/org-1/scheduled-tasks.json",
            "{}",
        );
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-1/org-1/notes.json",
            "{}",
        );

        let scan = scan_of(SyncCategory::ChatIndex, &dir);
        assert_eq!(names(&scan), vec!["local_abc.json", "local_def.json"]);
        assert_eq!(scan.bytes, scan.files.iter().map(|f| f.size).sum::<u64>());

        let paths: Vec<String> = scan
            .files
            .iter()
            .map(|f| f.path.to_string_lossy().into_owned())
            .collect();
        for keyed in ["acct-1", "org-1", "acct-2", "org-2"] {
            assert!(
                paths.iter().any(|p| p.contains(keyed)),
                "{keyed}: {paths:?}"
            );
        }
    }

    #[test]
    fn a_cowork_local_agent_mode_sessions_tree_contributes_nothing() {
        let dir = TempDir::new().unwrap();
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-1/org-1/local_keep.json",
            "{}",
        );
        // At the account level and below it — the shared predicate is
        // component-level, so both are refused without a second rule here.
        seed(
            dir.path(),
            "desktop/claude-code-sessions/local-agent-mode-sessions/acct-1/local_cowork.json",
            "{}",
        );
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-1/local-agent-mode-sessions/local_cowork.json",
            "{}",
        );

        let scan = scan_of(SyncCategory::ChatIndex, &dir);
        assert_eq!(names(&scan), vec!["local_keep.json"]);
    }

    #[test]
    fn a_missing_sessions_root_is_an_empty_chat_index_scan() {
        let dir = TempDir::new().unwrap();
        let scan = scan_of(SyncCategory::ChatIndex, &dir);
        assert!(scan.files.is_empty());
        assert_eq!(scan.bytes, 0);
        assert_eq!(scan.category, SyncCategory::ChatIndex);
    }

    #[test]
    fn an_account_registry_lands_in_routines_and_never_in_the_chat_index() {
        let dir = TempDir::new().unwrap();
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-1/org-1/scheduled-tasks.json",
            "{}",
        );
        seed(
            dir.path(),
            "desktop/claude-code-sessions/acct-1/org-1/local_abc.json",
            "{}",
        );

        assert_eq!(
            names(&scan_of(SyncCategory::Routines, &dir)),
            vec!["scheduled-tasks.json"]
        );
        assert_eq!(
            names(&scan_of(SyncCategory::ChatIndex, &dir)),
            vec!["local_abc.json"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_profile_or_account_directory_is_not_entered() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        seed(outside.path(), "meta.json", "{}");
        seed(outside.path(), "config-tokenCache", "x");
        seed(outside.path(), "org-1/scheduled-tasks.json", "{}");

        fs::create_dir_all(dir.path().join("profiles")).unwrap();
        symlink(outside.path(), dir.path().join("profiles/linked")).unwrap();
        assert!(scan_of(SyncCategory::Credentials, &dir).files.is_empty());

        let sessions = dir.path().join("desktop/claude-code-sessions");
        fs::create_dir_all(&sessions).unwrap();
        symlink(outside.path(), sessions.join("acct-1")).unwrap();
        assert!(scan_of(SyncCategory::Routines, &dir).files.is_empty());
    }

    #[test]
    fn a_category_absent_from_the_configured_set_scans_nothing() {
        let dir = TempDir::new().unwrap();
        seed(dir.path(), "config.toml", "[sync]\n");
        let cfg = SyncConfig {
            categories: vec![SyncCategory::Routines],
            ..SyncConfig::default()
        };
        let scan = collect(SyncCategory::Config, &roots_at(&dir), &cfg, Utc::now());
        assert!(scan.files.is_empty());
        assert_eq!(scan.bytes, 0);
        assert_eq!(scan.category, SyncCategory::Config);
    }
}
