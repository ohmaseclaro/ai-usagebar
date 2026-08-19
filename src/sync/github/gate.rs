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
//! Plan 3-04 owns this file next: it fills [`assert_pushable`]'s remaining
//! refusal conditions and its warning cases, behind the signature frozen here.

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

fn missing_repo_message(repo: &RepoRef) -> String {
    format!(
        "{repo} is not there. Either it does not exist, or this token is not scoped to it — \
         GitHub returns the same 404 for both, deliberately, so both are possible.\n\
         ai-usagebar never creates a repository. Create it yourself, private:\n\
         \x20   gh repo create {repo} --private"
    )
}

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

/// The gate. **Signature frozen by plan 3-01**; plan 3-04 fills the rest of the
/// conditions behind it.
///
/// `credentials_in_bundle` is D-04 read exactly as written: a public repository
/// aborts *when credentials are in the bundle*, and is allowed-with-a-warning
/// when the credentials category is off. Without the parameter this function and
/// plan 3-04's `check_drift` would contradict each other — one hard-refusing on
/// `private == false` while knowing nothing about the bundle, the other carving
/// out the credentials-off case — and since `setup.rs` calls `check_drift` and
/// then this, the second would silently kill the first's carve-out. One function
/// decides, and it is this one.
///
/// Warnings ride in the tuple rather than arriving by a later signature change,
/// so plan 3-04 can add warning cases without touching `setup.rs`, a file it does
/// not own. That is why the `Vec` exists while nothing yet fills it.
///
/// **Tracer scope:** only the `private` refusal is implemented here, in both its
/// arms, because it is the one the tracer has to prove end to end. Plan 3-04
/// adds `visibility`, `owner_login`/`owner_id`, `archived`, and `fork`.
pub fn assert_pushable(
    facts: &RepoFacts,
    repo: &RepoRef,
    credentials_in_bundle: bool,
    now: DateTime<Utc>,
) -> Result<(PushClearance, Vec<String>)> {
    let warnings: Vec<String> = Vec::new();

    if !facts.private {
        // Two distinct messages: the credentials-bearing one has to say what to
        // rotate, because a *previous* push may already have landed while the
        // repository was public. Plan 3-04 turns the second arm into a warning
        // that proceeds; the tracer has no bundle to inspect, so it refuses.
        return Err(AppError::Other(if credentials_in_bundle {
            format!(
                "REFUSING TO PUSH: {repo} is public (visibility {:?}).\n\
                 The bundle carries the credentials category, so nothing is uploaded.\n\
                 Make the repository private again, then rotate every credential it may \
                 already hold — an earlier push could have landed while it was public, and \
                 bytes that were published cannot be un-published.",
                facts.visibility
            )
        } else {
            format!(
                "REFUSING TO PUSH: {repo} is public (visibility {:?}).\n\
                 The credentials category is off, but chat indexes and config are personal \
                 data too. Make the repository private.",
                facts.visibility
            )
        }));
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

    #[test]
    fn a_public_repository_refuses_distinctly_when_credentials_are_not_in_the_bundle() {
        let with = assert_pushable(&facts(false), &repo(), true, now())
            .expect_err("public")
            .to_string();
        let without = assert_pushable(&facts(false), &repo(), false, now())
            .expect_err("public")
            .to_string();
        assert_ne!(with, without);
        assert!(without.contains("chat indexes and config"), "{without}");
        assert!(!without.contains("rotate"), "{without}");
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
