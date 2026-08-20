//! The wire spelling of a path inside a bundle — and the **only** place in the
//! crate where a string from a remote becomes a [`PathBuf`].
//!
//! Every string in a manifest arrives from a remote the threat model treats as
//! hostile (D5): anyone with write access to the paired repository can put an
//! arbitrary manifest there, and every entry in it turns into a local write on
//! whatever machine runs `sync pull`. So the whole of
//! [`from_manifest_path`] refuses **before** it touches the filesystem, and the
//! result is built one component at a time onto the root rather than by joining
//! an untrusted remainder — a single absolute component handed to
//! [`Path::join`] replaces the root wholesale, which is the classic shape of
//! this bug.
//!
//! # The encoding is root-prefixed and relative, and that is the whole point
//!
//! A bundle pushed from one machine is restored on a *second* machine, with a
//! different username and therefore a different `$HOME`. An absolute path in
//! the manifest is unresolvable there — and is also precisely what
//! [`from_manifest_path`] refuses, so a bundle carrying one could only be
//! restored by disabling its own traversal defence. The push side already
//! renders the relocatable form ([`crate::sync::push::packer::manifest_path`],
//! which errors rather than falling back to an absolute path); this module is
//! the other direction, plus the D4 policy check the write side applies.
//!
//! The separator is `/`, always, including on Windows. The bundle is portable
//! or it is nothing.
//!
//! # What this module deliberately does not do
//!
//! It never calls `canonicalize`. The destination usually does not exist yet on
//! a fresh machine, and resolving symlinks the bundle can influence is exactly
//! how an escape sneaks back in after the textual checks have passed. Symlinks
//! are handled at the write boundary instead — plan 5-04 owns that.
//!
//! Owned by plan 5-01. Called by `merge` (5-03) and `write` (5-04); neither
//! restates a path rule.

use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, Result};
use crate::sync::{SyncRoots, scope};

// There is deliberately no encoder in this module. `push::packer::manifest_path`
// has emitted the wire spelling since Phase 4, and the round-trip tests below
// call it directly — a local mirror would make the drift test compare a copy
// against itself, which is exactly the drift it exists to catch.

// The four prefixes. `config_file` needs no fifth: `SyncRoots::resolve` derives
// `config_dir` as its parent, so the file itself already lives under `config/`.
fn config_dir(roots: &SyncRoots) -> &Path {
    roots.config_dir.as_path()
}
fn desktop_data_dir(roots: &SyncRoots) -> &Path {
    roots.desktop_data_dir.as_path()
}
fn desktop_profiles_dir(roots: &SyncRoots) -> &Path {
    roots.desktop_profiles_dir.as_path()
}
fn claude_home(roots: &SyncRoots) -> &Path {
    roots.claude_home.as_path()
}

type RootOf = fn(&SyncRoots) -> &Path;

/// The prefix table, once: name on the wire, and the root it resolves against
/// on *this* machine. A second table is a second thing to get wrong.
///
/// The push direction reads the same four literals from
/// [`crate::sync::push::packer::manifest_path`], which predates this module;
/// `the_two_directions_agree_on_every_prefix` below is the mechanical guard
/// that keeps them one vocabulary rather than two.
const ROOT_PREFIXES: [(&str, RootOf); 4] = [
    ("config", config_dir),
    ("desktop-data", desktop_data_dir),
    ("desktop-profiles", desktop_profiles_dir),
    ("claude-home", claude_home),
];

/// Resolve one manifest entry against *this* machine's roots.
///
/// The hostile-input boundary. Every refusal has its own message, because
/// "invalid path" tells a user nothing about a bundle that may have been
/// tampered with.
pub fn from_manifest_path(roots: &SyncRoots, s: &str) -> Result<PathBuf> {
    let refuse = |why: &str| AppError::Other(format!("refusing the manifest entry {s:?}: {why}"));

    if s.is_empty() {
        return Err(refuse("it is empty"));
    }
    if s.contains('\0') {
        return Err(refuse("it contains a NUL byte"));
    }
    if s.starts_with('/') {
        return Err(refuse(
            "it is an absolute path, and every path in a bundle is relative to one of its roots",
        ));
    }
    if s.contains('\\') {
        return Err(refuse(
            "it contains a backslash — a bundle's only separator is `/`, on every platform",
        ));
    }
    if is_drive_prefixed(s) {
        return Err(refuse(
            "it begins with a Windows drive letter, which is an absolute path in disguise",
        ));
    }

    let Some((prefix, rest)) = s.split_once('/') else {
        return Err(refuse(
            "it names no root: a bundle path is `<root>/<path beneath it>`",
        ));
    };
    let root = ROOT_PREFIXES
        .iter()
        .find(|(name, _)| *name == prefix)
        .map(|(_, resolve)| resolve(roots))
        .ok_or_else(|| refuse("it names a root this build does not know"))?;
    if rest.is_empty() {
        return Err(refuse("it names a root with nothing beneath it"));
    }

    // One component at a time onto the root. Never `root.join(rest)`: a single
    // absolute or drive-rooted component in `rest` would replace the root.
    let mut out = root.to_path_buf();
    for part in rest.split('/') {
        match part {
            "" => return Err(refuse("it has an empty path component")),
            "." => return Err(refuse("it contains a `.` component")),
            ".." => return Err(refuse("it contains a `..` component")),
            _ => {}
        }
        let mut components = Path::new(part).components();
        if !matches!(
            (components.next(), components.next()),
            (Some(Component::Normal(_)), None)
        ) {
            return Err(refuse("it has a component that is not a plain file name"));
        }
        out.push(part);
    }

    // Belt and braces on the loop above: if it ever stops holding, this is what
    // notices before anything is written.
    if !out.starts_with(root) {
        return Err(refuse("it resolves outside the root it names"));
    }
    Ok(out)
}

/// `C:` / `c:` and friends, before the string is split on `/`.
fn is_drive_prefixed(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// D4 on the **write** side: may this bundle path be written at all?
///
/// Reuses [`scope::is_excluded`] rather than restating its lists. The whole
/// point of D4 is that the collector and the restorer agree about what is
/// machine-bound, and two copies of a list diverge on the day one of them is
/// edited. `is_excluded` already refuses any path with an excluded *directory*
/// anywhere above it, so `claude-home/local-agent-mode-sessions/x` is refused
/// whatever prefix it arrives under.
///
/// `rel` is the **manifest path** — prefix included — as a `Path`, never the
/// resolved destination. The resolved path carries this machine's own directory
/// names above the root, and one of them happening to be called `backups` is
/// not the bundle's fault.
pub fn accept_for_write(rel: &Path) -> bool {
    !scope::is_excluded(rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::push;
    use tempfile::TempDir;

    /// Two machines. Same bundle, different usernames — which is the entire
    /// reason this module exists.
    fn machine(dir: &Path, user: &str) -> SyncRoots {
        let home = dir.join("Users").join(user);
        SyncRoots::at(
            home.join(".config/ai-usagebar/config.toml"),
            home.join(".config/ai-usagebar"),
            home.join("Library/Application Support/Claude"),
            home.join(".claude-acc/profiles"),
            home.join(".claude"),
        )
    }

    /// One realistic path per category, as the collectors would produce them.
    fn realistic(roots: &SyncRoots) -> Vec<PathBuf> {
        vec![
            roots.config_file.clone(),
            roots.config_dir.join("accounts/work/.credentials.json"),
            roots.claude_home.join("scheduled-tasks/daily.json"),
            roots
                .desktop_data_dir
                .join("claude-code-sessions/acct/org/local_1.json"),
            roots.claude_home.join("projects/repo/session.jsonl"),
            roots.desktop_profiles_dir.join("work/meta.json"),
        ]
    }

    #[test]
    fn the_encoding_is_a_bijection_over_every_category() {
        let dir = TempDir::new().unwrap();
        let roots = machine(dir.path(), "alice");
        for path in realistic(&roots) {
            let wire = push::packer::manifest_path(&roots, &path).expect("a path under a root");
            let back = from_manifest_path(&roots, &wire).expect("its own spelling resolves");
            assert_eq!(back, path, "round trip through {wire:?}");
        }
    }

    #[test]
    fn a_bundle_pushed_by_one_user_resolves_under_a_second_users_roots() {
        let dir = TempDir::new().unwrap();
        let alice = machine(dir.path(), "alice");
        let bob = machine(dir.path(), "bob");

        for path in realistic(&alice) {
            let wire = push::packer::manifest_path(&alice, &path).unwrap();
            let on_bob = from_manifest_path(&bob, &wire).unwrap();
            assert!(
                on_bob.starts_with(dir.path().join("Users/bob")),
                "{wire:?} resolved to {on_bob:?}, which is not under bob's home"
            );
            assert!(
                !on_bob.to_string_lossy().contains("alice"),
                "{on_bob:?} still carries the pushing machine's username"
            );
        }
    }

    #[test]
    fn no_manifest_path_carries_an_absolute_path_or_a_username() {
        let dir = TempDir::new().unwrap();
        let roots = machine(dir.path(), "alice");
        for path in realistic(&roots) {
            let wire = push::packer::manifest_path(&roots, &path).unwrap();
            assert!(!wire.starts_with('/'), "{wire:?} is absolute");
            assert!(!wire.contains("alice"), "{wire:?} names the pushing user");
        }
    }

    #[test]
    fn a_path_under_no_root_is_an_error_naming_the_path() {
        let dir = TempDir::new().unwrap();
        let roots = machine(dir.path(), "alice");
        let stray = dir.path().join("etc/shadow");
        let err = push::packer::manifest_path(&roots, &stray).expect_err("under none of the roots");
        assert!(
            err.to_string().contains("shadow"),
            "the error must name the path: {err}"
        );
    }

    /// A customised install can nest one root inside another. The shortest
    /// match would file the path under the wrong tree and restore it to the
    /// wrong place on the second machine.
    #[test]
    fn nested_roots_resolve_to_the_longer_one() {
        let dir = TempDir::new().unwrap();
        let home = dir.path().join("home");
        let roots = SyncRoots::at(
            home.join("config.toml"),
            home.clone(),
            home.join("desktop"),
            home.join("profiles"),
            // Deliberately nested inside `config_dir`.
            home.join("nested/claude"),
        );
        let inside = roots.claude_home.join("projects/x.jsonl");
        let wire = push::packer::manifest_path(&roots, &inside).unwrap();
        assert_eq!(wire, "claude-home/projects/x.jsonl");
        assert_eq!(from_manifest_path(&roots, &wire).unwrap(), inside);
    }

    #[test]
    fn every_hostile_spelling_is_refused_with_its_own_message() {
        let dir = TempDir::new().unwrap();
        let roots = machine(dir.path(), "alice");

        let cases = [
            ("", "empty"),
            ("config/a\0b", "NUL"),
            ("/etc/shadow", "absolute"),
            ("config\\..\\etc", "backslash"),
            ("C:/Users/alice/x", "Windows drive"),
            ("config", "names no root"),
            ("config/", "nothing beneath it"),
            ("elsewhere/x", "does not know"),
            ("config/../../etc/shadow", "`..` component"),
            ("config/./x", "`.` component"),
            ("config/a//b", "empty path component"),
        ];

        // The reason, with the echoed input stripped off — otherwise every
        // message is trivially unique because it quotes its own input.
        let mut seen: Vec<String> = Vec::new();
        for (input, needle) in cases {
            let err = from_manifest_path(&roots, input)
                .expect_err(&format!("{input:?} must be refused"))
                .to_string();
            assert!(
                err.contains(needle),
                "{input:?} was refused, but not for the stated reason: {err}"
            );
            let reason = err
                .split_once("}: ")
                .or_else(|| err.split_once("\": "))
                .map_or(err.clone(), |(_, why)| why.to_string());
            assert!(
                !seen.contains(&reason),
                "{input:?} shares a refusal with an earlier case: {reason}"
            );
            seen.push(reason);
        }
    }

    /// The traversal that actually matters: the escape must not reach the
    /// filesystem as a resolved parent outside the root.
    #[test]
    fn a_traversal_never_yields_a_path_outside_its_root() {
        let dir = TempDir::new().unwrap();
        let roots = machine(dir.path(), "alice");
        for hostile in [
            "config/../../../../../../etc/shadow",
            "claude-home/projects/../../../../.ssh/id_ed25519",
            "desktop-data/..",
        ] {
            assert!(
                from_manifest_path(&roots, hostile).is_err(),
                "{hostile:?} resolved instead of being refused"
            );
        }
    }

    #[test]
    fn the_two_directions_agree_on_every_prefix() {
        let dir = TempDir::new().unwrap();
        let roots = machine(dir.path(), "alice");
        for (name, resolve) in ROOT_PREFIXES {
            let under = resolve(&roots).join("probe.json");
            let wire = push::packer::manifest_path(&roots, &under)
                .unwrap_or_else(|e| panic!("the push side refuses the {name} root: {e}"));
            assert_eq!(
                wire,
                format!("{name}/probe.json"),
                "the push side spells the {name} root differently"
            );
            assert_eq!(from_manifest_path(&roots, &wire).unwrap(), under);
        }
    }

    /// D4: a bundle naming machine-bound state is dropped on the write side,
    /// whatever it claims, and whatever prefix it arrives under.
    #[test]
    fn machine_bound_state_is_refused_under_every_prefix() {
        for refused in [
            "claude-home/local-agent-mode-sessions/x.json",
            "desktop-data/local-agent-mode-sessions/deep/x.json",
            "config/bridge-state.json",
            "desktop-profiles/work/ant-device-registry.json",
            "config/sync/index.sqlite3-journal",
            "config/accounts/work/.tmp.credentials",
            "desktop-data/backups/old.tar.gz",
            "config/.fetch.lock",
        ] {
            assert!(
                !accept_for_write(Path::new(refused)),
                "{refused} would have been written"
            );
        }
    }

    #[test]
    fn ordinary_synced_state_is_accepted() {
        for accepted in [
            "config/config.toml",
            "config/accounts/work/.credentials.json",
            "claude-home/scheduled-tasks/daily.json",
            "desktop-data/claude-code-sessions/acct/org/local_1.json",
            "claude-home/projects/repo/session.jsonl",
        ] {
            assert!(
                accept_for_write(Path::new(accepted)),
                "{accepted} would have been dropped"
            );
        }
    }
}
