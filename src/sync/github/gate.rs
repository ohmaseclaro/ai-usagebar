//! The private-repo gate: the one thing that must be true before a byte moves.
//!
//! D-04 says the check runs *immediately before every push*, never from a cached
//! fact — a repository can be flipped public from the web UI at any moment. Two
//! things here make that structural rather than a convention:
//!
//! - [`assert_pushable`] is the **sole** constructor of [`PushClearance`], whose
//!   field is private and which is neither `Clone` nor `Copy`. A clearance
//!   cannot be forged, and cannot be duplicated into a cache.
//! - [`PushClearance::assert_fresh`] turns "immediately" into arithmetic. Phase
//!   4's upload entry point takes a clearance **by value**, calls `assert_fresh`
//!   before the first byte, and re-runs [`fetch_facts`] + [`assert_pushable`]
//!   inside the push rather than carrying one obtained at `sync setup`.
//!
//! Plan 3-04 filled [`assert_pushable`]'s remaining refusal conditions and its
//! warning cases behind the signature frozen here, and added the REPO-03 guard
//! test at the bottom of this file — the standing check that no
//! repository-creating endpoint is reachable from anywhere under `src/`.

use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde::Deserialize;

use crate::error::{AppError, Result};

use super::http;
use super::{Client, RepoRef};

/// How stale a [`PushClearance`] may be at the moment of a push.
///
/// Small on purpose: the window between the gate and the first byte is a few
/// HTTP round trips, and every second of slack is a second in which the
/// repository could have been flipped public.
pub const MAX_CLEARANCE_AGE: Duration = Duration::from_secs(30);

/// What `GET /repos/{owner}/{name}` says about the repository.
///
/// `private` alone is not enough: an *internal* repository on an enterprise
/// account also reports `private: true`, so `visibility` is carried too and
/// plan 3-04 asserts both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFacts {
    pub id: u64,
    pub private: bool,
    pub visibility: String,
    pub owner_login: String,
    /// The numeric owner id — what makes a delete-and-resquat of the repository
    /// *name* detectable. A login comparison alone would not see it.
    pub owner_id: u64,
    pub archived: bool,
    pub fork: bool,
    /// `permissions.admin`, absent for a fine-grained PAT without it. D-03 warns
    /// rather than fails when it is present: a token that could create a public
    /// repository silently weakens REPO-03's structural guarantee.
    pub admin_permission: bool,
}

/// The wire shape, kept separate so [`RepoFacts`] is flat for every consumer and
/// so a missing `permissions` object is a `false`, never a parse failure.
#[derive(Deserialize)]
struct RawRepo {
    id: u64,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    visibility: String,
    owner: RawOwner,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    permissions: Option<RawPermissions>,
}

#[derive(Deserialize)]
struct RawOwner {
    login: String,
    id: u64,
}

#[derive(Deserialize, Default)]
struct RawPermissions {
    #[serde(default)]
    admin: bool,
}

/// `GET /repos/{owner}/{name}` — the only request the gate makes.
///
/// A 404 becomes D-01's message: GitHub returns the same status for "no such
/// repository" and "your token is not scoped to it", so the message says both,
/// and names the command that creates one. This tool never creates a repository
/// (REPO-03); a 404 that auto-created one would also happily create it for a
/// name squatter.
pub async fn fetch_facts(client: &Client, repo: &RepoRef, now: DateTime<Utc>) -> Result<RepoFacts> {
    let (status, headers, body) = client
        .get_json(&format!("/repos/{}/{}", repo.owner, repo.name))
        .await?;

    if !status.is_success() {
        let err = http::classify(status, &headers, &body, now);
        return Err(match err {
            http::GithubError::NotFound { .. } => AppError::Other(missing_repo_message(repo)),
            other => AppError::from(other),
        });
    }

    let raw: RawRepo = serde_json::from_slice(&body).map_err(|e| {
        AppError::Schema(format!(
            "GitHub's description of {repo} was not the shape this build expects ({e})"
        ))
    })?;
    Ok(RepoFacts {
        id: raw.id,
        private: raw.private,
        visibility: raw.visibility,
        owner_login: raw.owner.login,
        owner_id: raw.owner.id,
        archived: raw.archived,
        fork: raw.fork,
        admin_permission: raw.permissions.unwrap_or_default().admin,
    })
}

/// D-01's 404 text, `pub(crate)` so every entry point that can produce a
/// "there is no such repository" answer says the same thing — including the
/// ones plan 3-07 adds. GitHub deliberately 404s an unauthorised private
/// repository exactly as it 404s a missing one, so the message names both and
/// then names the command that fixes either.
pub(crate) fn missing_repo_message(repo: &RepoRef) -> String {
    format!(
        "{repo} is not there. Either it does not exist, or this token is not scoped to it — \
         GitHub returns the same 404 for both, deliberately, so both are possible.\n\
         ai-usagebar never creates a repository. Create it yourself, private:\n\
         \x20   gh repo create {repo} --private"
    )
}

/// The only `visibility` this gate accepts. GitHub reports exactly one of
/// `"public"`, `"private"`, or `"internal"`.
const VISIBILITY_PRIVATE: &str = "private";
/// Called out by name rather than caught by a catch-all: an internal repository
/// reports `private: true`, so it is the one value that slips past a `private`
/// check, and its refusal has to explain why.
const VISIBILITY_INTERNAL: &str = "internal";

/// Proof that a private-repo check passed, and when.
///
/// Private field, no public constructor, no `Clone`, no `Copy`: the only way to
/// hold one is to have just run [`assert_pushable`]. That is the whole design —
/// a clearance that could be stashed and duplicated *is* a cached check, which
/// is exactly what D-04 forbids.
#[derive(Debug)]
pub struct PushClearance {
    checked_at: DateTime<Utc>,
}

impl PushClearance {
    pub fn checked_at(&self) -> DateTime<Utc> {
        self.checked_at
    }

    /// D-04's "immediately", as arithmetic Phase 4 can call.
    ///
    /// A clearance dated in the *future* fails too: that is a clock that moved,
    /// and an unbounded-looking age is not something to wave a push through on.
    pub fn assert_fresh(&self, now: DateTime<Utc>, max_age: Duration) -> Result<()> {
        let age = now.signed_duration_since(self.checked_at);
        let fresh = age >= TimeDelta::zero() && age.to_std().is_ok_and(|a| a <= max_age);
        if fresh {
            return Ok(());
        }
        Err(AppError::Other(format!(
            "the private-repo check is {}s old (limit {}s) — re-run it immediately before \
             pushing, because the repository can be made public at any moment",
            age.num_seconds(),
            max_age.as_secs()
        )))
    }
}

/// The gate. **Signature frozen by plan 3-01, and byte-identical to it.**
///
/// `credentials_in_bundle` is D-04 read exactly as written: a public repository
/// aborts *when credentials are in the bundle*, and is allowed-with-a-warning
/// when the credentials category is off. Without the parameter this function and
/// [`pairing::check_drift`](super::pairing::check_drift) would contradict each
/// other — one hard-refusing on `private == false` while knowing nothing about
/// the bundle, the other carving out the credentials-off case — and since
/// `setup.rs` calls `check_drift` and then this, the second would silently kill
/// the first's carve-out. One function decides, and it is this one.
///
/// Six conditions refuse, each with its own message, because "the gate failed"
/// tells a user nothing they can act on:
///
/// | condition | why it is not merely paranoia |
/// |---|---|
/// | `private == false` + credentials | D-04. Names rotation, because an *earlier* push may already have landed while it was public |
/// | `visibility == "internal"` | Reports `private: true` and is readable by the whole enterprise — the one value a `private` check cannot see |
/// | `owner_login` ≠ the configured owner | GitHub follows renames and transfers silently; the name can end up pointing elsewhere |
/// | `archived` | Every write is rejected by GitHub, so this fails later at a worse moment |
/// | `fork` | A fork shares its upstream's object network, which reaches contents the owner did not intend to share |
/// | 404 | [`missing_repo_message`], from [`fetch_facts`] |
///
/// The warning list is the tuple's second element, so warning cases arrive
/// without a signature change and `setup.rs` — a file plan 3-04 does not own —
/// keeps rendering them unchanged.
pub fn assert_pushable(
    facts: &RepoFacts,
    repo: &RepoRef,
    credentials_in_bundle: bool,
    now: DateTime<Utc>,
) -> Result<(PushClearance, Vec<String>)> {
    let mut warnings: Vec<String> = Vec::new();

    // ponytail: `admin_permission` is parsed and deliberately *not* warned on,
    // and this is a reading of D-03 rather than an omission. `permissions.admin`
    // on `GET /repos/{owner}/{repo}` reports the **authenticated user's role on
    // the repository**, not the token's granted permissions — and D-01 has the
    // user create the repository themselves, which makes them its admin. So a
    // correctly-scoped `Contents: read/write` token would still see
    // `admin: true`, the warning would fire on essentially every legitimate
    // install, and a warning that always fires trains its reader to ignore it.
    // Whether a fine-grained PAT narrows the field is undocumented; plan 3-06's
    // `#[ignore]`d probe measures it against a real token. Turning the warning
    // on is one line once that measurement exists. D-03's real force is the
    // token recipe in `docs/sync-github.md`, which is what actually determines
    // the token's scope.
    let _ = facts.admin_permission;

    // D-04, and the only place the credentials carve-out is decided.
    if !facts.private {
        if credentials_in_bundle {
            // The rotation advice is not politeness. A *previous* push may have
            // landed while the repository was public, and nothing here can
            // un-publish those bytes.
            return Err(AppError::Other(format!(
                "REFUSING TO PUSH: {repo} is public (visibility {:?}).\n\
                 The bundle carries the credentials category, so nothing is uploaded.\n\
                 Make the repository private again, then rotate every credential it may \
                 already hold — an earlier push could have landed while it was public, and \
                 bytes that were published cannot be un-published.",
                facts.visibility
            )));
        }
        // D-04's closing paragraph: with the credentials category off there is
        // nothing to rotate, so this warns and proceeds rather than refusing.
        warnings.push(format!(
            "{repo} is public. The credentials category is off, so there is nothing to \
             rotate — but chat indexes and config are personal data too, and anything \
             pushed to a public repository is readable by anyone. Make it private."
        ));
    }

    // `private: true` is not the same thing as "private": an *internal*
    // repository on an enterprise account reports `private: true` and is
    // readable by every member of that enterprise. Matched by name so the
    // refusal states the reason rather than falling through a catch-all.
    if facts.private && facts.visibility != VISIBILITY_PRIVATE {
        return Err(AppError::Other(
            if facts.visibility == VISIBILITY_INTERNAL {
                format!(
                    "REFUSING TO PUSH: {repo} is an internal repository.\n\
                 An internal repository reports private: true, but every member of the \
                 enterprise that owns it can read it — that is not private enough to hold \
                 credentials. Change its visibility to private."
                )
            } else {
                format!(
                    "REFUSING TO PUSH: {repo} reports visibility {:?}, which this build does not \
                 recognise. Only \"private\" is accepted. Set the repository to private, or \
                 update ai-usagebar if GitHub has added a visibility since this release.",
                    facts.visibility
                )
            },
        ));
    }

    // GitHub follows a rename or a transfer silently, answering for the new
    // owner at the old path. The repository this answered for is then not the
    // one `[sync] repo` names. Owner names are case-insensitive on GitHub.
    if !facts.owner_login.eq_ignore_ascii_case(&repo.owner) {
        return Err(AppError::Other(format!(
            "REFUSING TO PUSH: {repo} is owned by {:?}, not {:?}.\n\
             GitHub answers for a renamed or transferred repository at its old path, so the \
             name in [sync] repo can quietly end up pointing somewhere else. Point [sync] \
             repo at the repository you mean, then re-run `ai-usagebar sync setup`.",
            facts.owner_login, repo.owner
        )));
    }

    if facts.archived {
        return Err(AppError::Other(format!(
            "REFUSING TO PUSH: {repo} is archived.\n\
             GitHub rejects every write to an archived repository, so a push would fail \
             later — mid-upload, with a partly-written snapshot. Unarchive it in the \
             repository's settings, then re-run."
        )));
    }

    if facts.fork {
        return Err(AppError::Other(format!(
            "REFUSING TO PUSH: {repo} is a fork.\n\
             A fork shares its upstream's object network, which can make its contents \
             reachable in ways the owner did not intend. Use a repository created on its \
             own, not one forked from another."
        )));
    }

    Ok((PushClearance { checked_at: now }, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::github::Endpoints;
    use crate::sync::github::token::TokenSource;
    use zeroize::Zeroizing;

    const PRIVATE_BODY: &str = r#"{"id":1,"private":true,"visibility":"private",
        "owner":{"login":"o","id":7},"archived":false,"fork":false}"#;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn repo() -> RepoRef {
        RepoRef::parse("o/n").unwrap()
    }

    fn client_at(base: &str) -> Client {
        Client::new(
            &Endpoints {
                api_base: base.into(),
                uploads_base: base.into(),
            },
            Zeroizing::new("github_pat_fixture_not_a_real_token".into()),
            TokenSource::Env,
        )
        .unwrap()
    }

    /// One valid fact set with a single field bent, so each test names exactly
    /// the condition it is about.
    fn with(bend: impl FnOnce(&mut RepoFacts)) -> RepoFacts {
        let mut f = facts(true);
        bend(&mut f);
        f
    }

    /// `private: true` *and* invisible to the `private` check — the shape the
    /// `visibility` field exists to catch.
    fn internal() -> RepoFacts {
        with(|f| f.visibility = VISIBILITY_INTERNAL.into())
    }

    fn facts(private: bool) -> RepoFacts {
        RepoFacts {
            id: 1,
            private,
            visibility: if private { "private" } else { "public" }.into(),
            owner_login: "o".into(),
            owner_id: 7,
            archived: false,
            fork: false,
            admin_permission: false,
        }
    }

    #[tokio::test]
    async fn a_private_repository_yields_the_five_facts_the_gate_asserts_on() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(PRIVATE_BODY)
            .create_async()
            .await;

        let got = fetch_facts(&client_at(&server.url()), &repo(), now())
            .await
            .unwrap();
        assert_eq!(got, facts(true));
        m.assert_async().await;
    }

    /// `permissions.admin` is D-03's warning input, and the object is absent for
    /// a correctly-scoped fine-grained PAT — absence must not be a parse error.
    #[tokio::test]
    async fn an_admin_permission_is_read_when_present_and_false_when_absent() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(
                r#"{"id":1,"private":true,"visibility":"private","owner":{"login":"o","id":7},
                    "archived":false,"fork":false,"permissions":{"admin":true,"push":true}}"#,
            )
            .create_async()
            .await;

        let got = fetch_facts(&client_at(&server.url()), &repo(), now())
            .await
            .unwrap();
        assert!(got.admin_permission);
    }

    /// D-01: GitHub 404s an unauthorised private repository exactly as it 404s a
    /// missing one, so the message says both and names the fix.
    #[tokio::test]
    async fn a_404_names_both_causes_and_prints_the_create_command() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n")
            .with_status(404)
            .with_body(r#"{"message":"Not Found"}"#)
            .create_async()
            .await;

        let err = fetch_facts(&client_at(&server.url()), &repo(), now())
            .await
            .expect_err("a 404 is not a repository");
        let text = err.to_string();
        assert!(text.contains("gh repo create o/n --private"), "{text}");
        assert!(text.contains("not scoped to it"), "{text}");
        assert!(text.contains("never creates a repository"), "{text}");
    }

    #[tokio::test]
    async fn a_body_that_is_not_a_repository_description_is_a_schema_failure() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;

        let err = fetch_facts(&client_at(&server.url()), &repo(), now())
            .await
            .expect_err("that is not a repository");
        assert!(matches!(err, AppError::Schema(_)), "{err}");
    }

    #[test]
    fn a_private_repository_clears_with_no_warnings_and_the_injected_clock() {
        let (clearance, warnings) = assert_pushable(&facts(true), &repo(), true, now()).unwrap();
        assert_eq!(clearance.checked_at(), now());
        assert!(warnings.is_empty());
    }

    /// SAFE-01. The credential-bearing arm has to name rotation: an earlier push
    /// may have landed while the repository was public.
    #[test]
    fn a_public_repository_refuses_and_says_to_rotate_when_credentials_are_in_the_bundle() {
        let err = assert_pushable(&facts(false), &repo(), true, now())
            .expect_err("a public repository is not pushable");
        let text = err.to_string();
        assert!(text.contains("REFUSING TO PUSH"), "{text}");
        assert!(text.contains("rotate"), "{text}");
        assert!(text.contains("cannot be un-published"), "{text}");
    }

    /// D-04's closing paragraph. With nothing to rotate the right answer is a
    /// warning, not a refusal — and the warning still has to be said, because a
    /// chat index is personal data.
    #[test]
    fn a_public_repository_warns_and_clears_when_credentials_are_not_in_the_bundle() {
        let (clearance, warnings) = assert_pushable(&facts(false), &repo(), false, now())
            .expect("with the credentials category off, public is a warning");
        assert_eq!(clearance.checked_at(), now());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("o/n is public"), "{warnings:?}");
        assert!(warnings[0].contains("chat indexes"), "{warnings:?}");

        // Distinct from the refusal, which is the one that must say "rotate".
        let refused = assert_pushable(&facts(false), &repo(), true, now())
            .expect_err("public")
            .to_string();
        assert_ne!(refused, warnings[0]);
        // It says there is *nothing* to rotate, and never the un-publishable
        // line — that one belongs to the refusal alone.
        assert!(warnings[0].contains("nothing to rotate"), "{warnings:?}");
        assert!(!warnings[0].contains("un-published"), "{warnings:?}");
    }

    /// The whole reason `visibility` is carried alongside `private`: an internal
    /// repository reports `private: true` and would otherwise walk straight
    /// through the gate.
    #[test]
    fn an_internal_repository_refuses_in_both_bundle_configurations() {
        for credentials in [true, false] {
            let err = assert_pushable(&internal(), &repo(), credentials, now())
                .expect_err("internal is not a public-repository carve-out");
            let text = err.to_string();
            assert!(text.contains("internal repository"), "{text}");
            assert!(text.contains("not private enough"), "{text}");
        }
    }

    #[test]
    fn an_unrecognised_visibility_refuses_rather_than_being_waved_through() {
        let err = assert_pushable(
            &with(|f| f.visibility = "some-future-value".into()),
            &repo(),
            true,
            now(),
        )
        .expect_err("only the private literal is accepted");
        let text = err.to_string();
        assert!(text.contains("some-future-value"), "{text}");
        assert!(text.contains("does not recognise"), "{text}");
    }

    #[test]
    fn a_renamed_or_transferred_owner_refuses_and_case_does_not_matter() {
        let err = assert_pushable(
            &with(|f| f.owner_login = "someone-else".into()),
            &repo(),
            true,
            now(),
        )
        .expect_err("that is not the repository [sync] repo names");
        let text = err.to_string();
        assert!(text.contains("someone-else"), "{text}");
        assert!(text.contains("[sync] repo"), "{text}");

        // GitHub owner names are case-insensitive; a differing case is the same
        // account and must not refuse.
        assert!(
            assert_pushable(&with(|f| f.owner_login = "O".into()), &repo(), true, now()).is_ok()
        );
    }

    #[test]
    fn an_archived_repository_refuses_before_the_write_that_would_fail_later() {
        let err = assert_pushable(&with(|f| f.archived = true), &repo(), true, now())
            .expect_err("GitHub rejects every write to an archived repository");
        assert!(err.to_string().contains("archived"), "{err}");
    }

    #[test]
    fn a_fork_refuses_because_it_shares_its_upstreams_object_network() {
        let err = assert_pushable(&with(|f| f.fork = true), &repo(), true, now())
            .expect_err("a fork's contents are reachable from its upstream");
        assert!(err.to_string().contains("fork"), "{err}");
    }

    /// The phase's first success criterion: each refusal is actionable on its
    /// own, so none of them may share a message with another.
    #[test]
    fn all_six_refusals_say_six_different_things() {
        let messages = [
            assert_pushable(&facts(false), &repo(), true, now())
                .unwrap_err()
                .to_string(),
            assert_pushable(&internal(), &repo(), true, now())
                .unwrap_err()
                .to_string(),
            assert_pushable(
                &with(|f| f.owner_login = "elsewhere".into()),
                &repo(),
                true,
                now(),
            )
            .unwrap_err()
            .to_string(),
            assert_pushable(&with(|f| f.archived = true), &repo(), true, now())
                .unwrap_err()
                .to_string(),
            assert_pushable(&with(|f| f.fork = true), &repo(), true, now())
                .unwrap_err()
                .to_string(),
            missing_repo_message(&repo()),
        ];
        let unique: std::collections::BTreeSet<&String> = messages.iter().collect();
        assert_eq!(unique.len(), messages.len(), "{messages:#?}");
    }

    /// D-03. The field is read from the wire and deliberately produces no
    /// warning — see the comment in `assert_pushable`, and plan 3-06's probe.
    #[test]
    fn an_administrative_permission_produces_no_warning_yet() {
        let (_, warnings) =
            assert_pushable(&with(|f| f.admin_permission = true), &repo(), true, now()).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// D-01, asserted on the message itself rather than only through a mock, so
    /// every caller of it inherits the same coverage.
    #[test]
    fn the_missing_repository_message_names_both_causes_and_substitutes_the_repo() {
        let text = missing_repo_message(&RepoRef::parse("acme/backups").unwrap());
        assert!(text.contains("does not exist"), "{text}");
        assert!(text.contains("not scoped to it"), "{text}");
        assert!(
            text.contains("gh repo create acme/backups --private"),
            "{text}"
        );
    }

    /// REPO-03, as a standing check rather than a one-time grep.
    ///
    /// Withholding `Administration: write` is what makes creating a repository —
    /// and therefore a **public** one — impossible rather than merely
    /// disallowed. A `grep` in a plan's verify step proves that for one
    /// afternoon; REPO-03 is a property of the shipped crate, so this walks
    /// `src/` on every `cargo test`, and therefore on every `make test`, and
    /// therefore in every later phase. The cost is one directory walk.
    ///
    /// All four creating endpoints, not just the obvious one:
    ///
    /// - the user-namespace create path,
    /// - the organization-namespace create path — D-01 explicitly permits an
    ///   organization owner, so this is a live route and not a hypothetical,
    /// - create-from-template,
    /// - **fork**, which is the dangerous one: a fork of a public upstream is
    ///   public, which is exactly the outcome REPO-03 exists to make impossible.
    ///
    /// The organization fragment is deliberately broader than its create path.
    /// This crate calls no organization endpoint at all, so matching every one
    /// of them costs nothing and catches every spelling of the create path,
    /// including ones a `format!` would break into pieces.
    ///
    /// **This file is excluded from the walk, deliberately.** The fragments
    /// below are the things being searched for, so a guard that scanned its own
    /// source would fail on the day it was written. That exclusion is also the
    /// rule for everyone else: do not write any of these fragments anywhere
    /// under `src/` — not in a call, not in a test fixture, and not in a comment
    /// explaining that the endpoint is never used. To this test a comment and a
    /// call site are indistinguishable. Say what is true instead: the tool
    /// refuses and prints the command the user should run.
    ///
    /// The braces to plan 3-01's belt, which is the stronger half: `Client`
    /// exposes no method that can carry a request body, and none of these four
    /// endpoints is reachable without one. Both are cheap; having both is right.
    #[test]
    fn no_repository_creating_endpoint_is_reachable_from_the_crate() {
        const FORBIDDEN: [&str; 4] = ["/user/repos", "/orgs/", "/generate", "/forks"];

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        collect_rs(&root, &mut files);

        let mut scanned = 0usize;
        let mut skipped = 0usize;
        for path in &files {
            if path.ends_with(file!()) {
                skipped += 1;
                continue;
            }
            scanned += 1;
            let text = std::fs::read_to_string(path).unwrap();
            for fragment in FORBIDDEN {
                assert!(
                    !text.contains(fragment),
                    "REPO-03: {} contains {fragment:?}. ai-usagebar holds no permission that \
                     could create a repository, and that is a structural guarantee rather \
                     than a policy — creating one is how a *public* repository comes to \
                     exist. Remove the path. If a repository is missing, print the \
                     `gh repo create <owner>/<name> --private` line and exit non-zero.",
                    path.display()
                );
            }
        }

        // Non-vacuity: a guard that asserts an absence must also prove it looked
        // at something. A refactor that moved this file, or a walk that silently
        // found nothing, would otherwise report green forever.
        assert_eq!(skipped, 1, "this file must be excluded exactly once");
        assert!(scanned > 50, "only {scanned} files walked under {root:?}");
    }

    fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_rs(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// D-04's "immediately", enforceable rather than merely described.
    #[test]
    fn a_clearance_goes_stale_and_a_clearance_from_the_future_is_refused() {
        let (clearance, _) = assert_pushable(&facts(true), &repo(), true, now()).unwrap();
        assert!(clearance.assert_fresh(now(), MAX_CLEARANCE_AGE).is_ok());
        assert!(
            clearance
                .assert_fresh(now() + TimeDelta::seconds(5), MAX_CLEARANCE_AGE)
                .is_ok()
        );

        let stale = clearance
            .assert_fresh(now() + TimeDelta::seconds(31), MAX_CLEARANCE_AGE)
            .expect_err("31s is past the 30s limit");
        assert!(stale.to_string().contains("immediately before"), "{stale}");

        assert!(
            clearance
                .assert_fresh(now() - TimeDelta::seconds(1), MAX_CLEARANCE_AGE)
                .is_err(),
            "a clearance dated in the future is a clock that moved"
        );
    }
}
