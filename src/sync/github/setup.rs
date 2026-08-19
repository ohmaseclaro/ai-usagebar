//! `ai-usagebar sync setup` — pair this machine with the private repository
//! named in `[sync] repo`.
//!
//! **Uploads nothing.** The whole flow is: read the configured repository name,
//! resolve a token, ask GitHub what the repository is, and refuse unless it is
//! private (D-05).
//!
//! Unlike the rest of this module's signatures, [`run`]'s is **not** frozen:
//! plan 3-07 owns this file outright, in a later wave, and adds the interactive
//! prompt seam. Its only caller is `sync/cli.rs`, which 3-07 also owns.

use chrono::{DateTime, Utc};

use crate::config::{SyncCategory, SyncConfig};
use crate::error::{AppError, Result};
use crate::sync::SyncRoots;

use super::gate::{self, PushClearance};
use super::token::{self, TokenSource};
use super::{Client, Endpoints, RepoRef};

/// What setup learned. Carries the [`PushClearance`] rather than a `bool`,
/// because a `bool` is a cached check and D-04 forbids one.
#[derive(Debug)]
pub struct SetupOutcome {
    pub repo: RepoRef,
    /// Reported as a *source*; the token's value is never rendered anywhere.
    pub token_source: TokenSource,
    pub visibility: String,
    pub warnings: Vec<String>,
    pub clearance: PushClearance,
}

/// Read config → resolve token → ask GitHub → refuse unless private.
///
/// `roots` is the **only** way this module reaches a filesystem path. The
/// keyfile, the pairing record, the token file, and the `config.toml`
/// write-back plan 3-07 adds all resolve from it — so that no path here is ever
/// derived from a real `$HOME`, which the AUR `check()` would run against on an
/// installer's machine.
pub async fn run(
    cfg: &SyncConfig,
    roots: &SyncRoots,
    endpoints: &Endpoints,
    chain: &token::TokenChain,
    now: DateTime<Utc>,
) -> Result<SetupOutcome> {
    // Plan 3-04's pairing record and plan 3-07's config write-back both resolve
    // from here. Nothing in the tracer needs a path yet.
    let _ = roots;

    let Some(configured) = cfg.repo.as_deref() else {
        return Err(AppError::Other(no_repo_message()));
    };
    let repo = RepoRef::parse(configured)?;

    let (value, source) = token::resolve(chain)?;
    let client = Client::new(endpoints, value, source)?;

    let facts = gate::fetch_facts(&client, &repo, now).await?;
    let (clearance, warnings) =
        gate::assert_pushable(&facts, &repo, cfg.includes(SyncCategory::Credentials), now)?;

    Ok(SetupOutcome {
        repo,
        token_source: source,
        visibility: facts.visibility,
        warnings,
        clearance,
    })
}

/// D-01: nothing is guessed, and the failure names the exact fix.
fn no_repo_message() -> String {
    "[sync] repo is unset, and ai-usagebar never guesses or creates one.\n\
     Create a private repository, then name it in config.toml:\n\
     \x20   gh repo create <owner>/<name> --private\n\
     \n\
     \x20   [sync]\n\
     \x20   repo = \"<owner>/<name>\""
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::github::token::TokenChain;
    use tempfile::TempDir;

    const FIXTURE: &str = "github_pat_fixture_not_a_real_token";

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn roots_at(dir: &TempDir) -> SyncRoots {
        SyncRoots::at(
            dir.path().join("config.toml"),
            dir.path().to_path_buf(),
            dir.path().join("desktop"),
            dir.path().join("profiles"),
            dir.path().join("claude-home"),
        )
    }

    fn chain() -> TokenChain {
        TokenChain {
            env_value: Some(FIXTURE.into()),
            ..TokenChain::default()
        }
    }

    fn endpoints_at(base: &str) -> Endpoints {
        Endpoints {
            api_base: base.into(),
            uploads_base: base.into(),
        }
    }

    fn cfg_for(repo: Option<&str>) -> SyncConfig {
        SyncConfig {
            repo: repo.map(str::to_owned),
            ..SyncConfig::default()
        }
    }

    #[tokio::test]
    async fn a_private_repository_pairs_and_reports_the_token_source_only() {
        let dir = TempDir::new().unwrap();
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(
                r#"{"id":1,"private":true,"visibility":"private","owner":{"login":"o","id":7},
                    "archived":false,"fork":false}"#,
            )
            .create_async()
            .await;

        let out = run(
            &cfg_for(Some("o/n")),
            &roots_at(&dir),
            &endpoints_at(&server.url()),
            &chain(),
            now(),
        )
        .await
        .unwrap();

        assert_eq!(out.repo.to_string(), "o/n");
        assert_eq!(out.visibility, "private");
        assert_eq!(out.token_source, TokenSource::Env);
        assert!(out.warnings.is_empty());
        assert_eq!(out.clearance.checked_at(), now());
        assert!(!format!("{out:?}").contains(FIXTURE));
        m.assert_async().await;
    }

    /// SAFE-01: the refusal happens with the socket already closed and nothing
    /// sent — there is no method on `Client` that could have sent anything.
    #[tokio::test]
    async fn a_public_repository_is_refused_before_anything_else_happens() {
        let dir = TempDir::new().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n")
            .with_status(200)
            .with_body(
                r#"{"id":1,"private":false,"visibility":"public","owner":{"login":"o","id":7},
                    "archived":false,"fork":false}"#,
            )
            .create_async()
            .await;

        let err = run(
            &cfg_for(Some("o/n")),
            &roots_at(&dir),
            &endpoints_at(&server.url()),
            &chain(),
            now(),
        )
        .await
        .expect_err("a public repository is not pairable");
        assert!(err.to_string().contains("REFUSING TO PUSH"), "{err}");
    }

    /// D-01. No network is reached at all: the repository was never named.
    #[tokio::test]
    async fn an_unset_repo_names_the_config_key_and_the_create_command() {
        let dir = TempDir::new().unwrap();
        let err = run(
            &cfg_for(None),
            &roots_at(&dir),
            // A dead port, so a regression that skipped the check would fail
            // loudly rather than silently reaching the real GitHub.
            &endpoints_at("http://127.0.0.1:1"),
            &chain(),
            now(),
        )
        .await
        .expect_err("no repository is configured");
        let text = err.to_string();
        assert!(
            text.contains("gh repo create <owner>/<name> --private"),
            "{text}"
        );
        assert!(text.contains("[sync]"), "{text}");
    }
}
