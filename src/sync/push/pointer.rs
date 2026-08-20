//! The pointer: read it, and swap it.
//!
//! **The single linearization point of the whole format.** Packs are immutable
//! and content-addressed, so publishing a snapshot is exactly one operation —
//! the `PUT` in [`commit`], carrying the `sha` of the blob it expects to
//! replace. Everything before it is inert; everything after it is visible.
//!
//! Plan 4-01 created this file with [`load`] complete and [`commit`]'s
//! no-conflict path working. **Plan 4-04 owns it** and fills the 409 arm — the
//! `rebuild` closure exists from the tracer precisely so that retry can be added
//! here without touching the orchestrator that supplies the closure.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::sync::check_version;
use crate::sync::github::write::MAX_POINTER_BYTES;
use crate::sync::github::{Client, RepoRef, gate};

use super::{MAX_SUPPORTED_POINTER, POINTER_PATH, Pointer};

/// Fixed, non-identifying, and permanent: it lands in the repository's git
/// history in the clear forever, so it carries no path, no hostname, no user
/// name and no byte count.
const COMMIT_MESSAGE: &str = "ai-usagebar sync: update snapshot pointer";

/// Just the version, so a pointer from the future can be refused by version
/// rather than by a missing-field complaint about a field this build has never
/// heard of.
#[derive(Deserialize)]
struct VersionProbe {
    format: u32,
}

/// Read the pointer, with the blob `sha` a later [`commit`] needs.
///
/// `(None, None)` is a first push and is not an error.
///
/// Two refusals, in this order and for different reasons:
/// - a `format` above this build's ceiling, probed **before** deserializing
///   because a newer pointer may carry required fields this build has never
///   heard of, and a full deserialize would complain about a missing field
///   instead of about the real problem;
/// - a `repo_id` that is not the caller's own, which is a different bundle.
///   `expect_repo_id` comes from local configuration, never from the response.
pub async fn load(
    client: &Client,
    repo: &RepoRef,
    expect_repo_id: &str,
    now: DateTime<Utc>,
) -> Result<(Option<Pointer>, Option<String>)> {
    let Some((sha, body)) = client.get_contents(repo, POINTER_PATH, now).await? else {
        return Ok((None, None));
    };

    let probe: VersionProbe = serde_json::from_slice(&body)
        .map_err(|_| AppError::Other("the remote snapshot pointer is malformed".into()))?;
    check_version(probe.format, MAX_SUPPORTED_POINTER, "snapshot pointer")?;

    let pointer: Pointer = serde_json::from_slice(&body)
        .map_err(|_| AppError::Other("the remote snapshot pointer is malformed".into()))?;

    if pointer.repo_id != expect_repo_id {
        return Err(AppError::Other(format!(
            "the snapshot pointer in this repository identifies as bundle {:?}, but this \
             machine is paired with {expect_repo_id:?}. Refusing: pushing here would mix two \
             bundles in one release, and neither would be restorable. Check `repo` under \
             [sync] in config.toml.",
            pointer.repo_id
        )));
    }
    Ok((Some(pointer), Some(sha)))
}

/// Publish a pointer with a compare-and-swap precondition.
///
/// `sha` is what [`load`] returned: `Some` replaces exactly that blob, `None` is
/// "create, and fail if it exists". The two are different requests, and
/// `write::put_contents` omits the field rather than sending null for the second.
///
/// # The rebuild closure, and the three rules it must obey
///
/// `rebuild` produces the pointer to write from whatever the remote currently
/// holds, so the conflict path is a second call to *the same* function rather
/// than a special case. It is written in `push/mod.rs`'s orchestrator, which
/// this file does not own; the rules are restated here because **here is where
/// they are tested**, and a closure in this file's tests reproduces all three so
/// an edit there that breaks one fails here.
///
/// 1. Snapshot records the caller did not produce are **carried forward**, never
///    dropped. The competing machine's snapshot references packs that exist;
///    discarding its record makes those packs unreferenced, which makes the next
///    prune delete them, which strands its backup.
/// 2. Truncation to `keep_snapshots` drops from the **oldest** end only, and it
///    happens inside the pointer being written — so the record is removed by the
///    flip itself, strictly before any pack is deleted, because deletion happens
///    after this function returns. D2's ordering is structural rather than a
///    step someone has to remember.
/// 3. The `keyfile` field is taken from the remote's current value unless this
///    run is the one changing it. A push that overwrote it with a stale name
///    would point every future reader at an asset a rekey has already deleted.
///
/// Nothing in this file deletes anything. A losing race costs one extra round
/// trip; it must never cost remote data.
///
/// **Plan 4-04 fills the 409 arm**: re-`load`, call `rebuild` again with what is
/// now there, `PUT` once more, and on a second conflict stop rather than loop.
/// The shared `with_retry` helper deliberately does not retry a `Conflict`,
/// which is what leaves this bounded retry as the only path.
pub async fn commit<F>(
    client: &Client,
    repo: &RepoRef,
    current: Option<&Pointer>,
    sha: Option<&str>,
    rebuild: F,
    permit: &gate::Pushing,
    now: DateTime<Utc>,
) -> Result<(Pointer, String)>
where
    F: Fn(Option<&Pointer>) -> Result<Pointer>,
{
    let next = rebuild(current)?;
    let body = serde_json::to_vec(&next).map_err(|e| {
        AppError::Other(format!("the snapshot pointer could not be serialized: {e}"))
    })?;
    if body.len() as u64 > MAX_POINTER_BYTES {
        return Err(AppError::Other(format!(
            "the snapshot pointer would be {} bytes, past the {MAX_POINTER_BYTES}-byte limit \
             the Contents API fully supports. Lower `keep_snapshots` under [sync] in \
             config.toml.",
            body.len()
        )));
    }
    let new_sha = client
        .put_contents(repo, POINTER_PATH, COMMIT_MESSAGE, &body, sha, permit, now)
        .await?;
    Ok((next, new_sha))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::ChunkId;
    use crate::sync::github::token::TokenSource;
    use crate::sync::github::{Endpoints, gate::RepoFacts};
    use crate::sync::push::{POINTER_VERSION, SnapshotRecord};
    use zeroize::Zeroizing;

    const NOW: DateTime<Utc> = match DateTime::from_timestamp(1_700_000_000, 0) {
        Some(t) => t,
        None => panic!("a fixed timestamp"),
    };

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

    fn permit() -> gate::Pushing {
        let facts = RepoFacts {
            id: 1,
            private: true,
            visibility: "private".into(),
            owner_login: "o".into(),
            owner_id: 7,
            archived: false,
            fork: false,
            admin_permission: false,
        };
        gate::assert_pushable(&facts, &repo(), true, NOW)
            .expect("a private repository clears")
            .0
            .spend(NOW)
            .expect("freshly minted")
    }

    fn record(tag: &str) -> SnapshotRecord {
        SnapshotRecord {
            root: tag.to_owned(),
            index_chunks: Vec::new(),
            packs: vec![ChunkId::from_bytes([tag.as_bytes()[0]; 32])],
        }
    }

    fn pointer(records: &[&str]) -> Pointer {
        Pointer {
            format: POINTER_VERSION,
            repo_id: "github:1".into(),
            keyfile: "keyfile-x.json".into(),
            snapshots: records.iter().map(|t| record(t)).collect(),
        }
    }

    /// The response body the Contents API returns for a `GET`.
    fn contents_body(pointer: &Pointer, sha: &str) -> String {
        use base64::Engine;
        format!(
            r#"{{"sha":"{sha}","content":"{}"}}"#,
            super::super::B64.encode(serde_json::to_vec(pointer).unwrap())
        )
    }

    #[tokio::test]
    async fn a_missing_pointer_is_first_push_and_carries_no_sha() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n/contents/sync/pointer.json")
            .with_status(404)
            .with_body(r#"{"message":"Not Found"}"#)
            .create_async()
            .await;

        let (found, sha) = load(&client_at(&server.url()), &repo(), "github:1", NOW)
            .await
            .unwrap();
        assert!(found.is_none());
        assert!(sha.is_none());
    }

    #[tokio::test]
    async fn a_present_pointer_yields_itself_and_the_sha_a_flip_needs() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n/contents/sync/pointer.json")
            .with_status(200)
            .with_body(contents_body(&pointer(&["a"]), "blob1"))
            .create_async()
            .await;

        let (found, sha) = load(&client_at(&server.url()), &repo(), "github:1", NOW)
            .await
            .unwrap();
        assert_eq!(found.unwrap(), pointer(&["a"]));
        assert_eq!(sha.as_deref(), Some("blob1"));
    }

    /// The version is probed *before* the deserialize, so a pointer carrying
    /// required fields this build has never heard of says "upgrade" rather than
    /// "missing field".
    #[tokio::test]
    async fn a_pointer_from_the_future_says_to_upgrade_rather_than_naming_a_missing_field() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n/contents/sync/pointer.json")
            .with_status(200)
            .with_body(format!(r#"{{"sha":"s","content":"{}"}}"#, {
                use base64::Engine;
                super::super::B64.encode(br#"{"format":99,"something_new":true}"#)
            }))
            .create_async()
            .await;

        let err = load(&client_at(&server.url()), &repo(), "github:1", NOW)
            .await
            .expect_err("a pointer from the future");
        assert!(err.to_string().contains("upgrade ai-usagebar"), "{err}");
        assert!(!err.to_string().contains("missing field"), "{err}");
    }

    #[tokio::test]
    async fn a_pointer_belonging_to_another_bundle_is_refused() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n/contents/sync/pointer.json")
            .with_status(200)
            .with_body(contents_body(&pointer(&["a"]), "blob1"))
            .create_async()
            .await;

        let err = load(&client_at(&server.url()), &repo(), "github:999", NOW)
            .await
            .expect_err("that is a different bundle");
        assert!(err.to_string().contains("github:1"), "{err}");
        assert!(err.to_string().contains("github:999"), "{err}");
    }

    /// The compare-and-swap, both halves, driven from `commit` rather than from
    /// `put_contents` — a first push must send no `sha` at all.
    #[tokio::test]
    async fn a_first_flip_sends_no_sha_and_a_later_one_sends_the_loaded_sha() {
        use std::sync::{Arc, Mutex};

        let mut server = mockito::Server::new_async().await;
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let recorder = Arc::clone(&seen);
        let _m = server
            .mock("PUT", "/repos/o/n/contents/sync/pointer.json")
            .with_status(201)
            .with_body(r#"{"content":{"sha":"blob2"}}"#)
            .match_request(move |req| {
                recorder
                    .lock()
                    .unwrap()
                    .push(req.utf8_lossy_body().unwrap().into_owned());
                true
            })
            .expect(2)
            .create_async()
            .await;

        let client = client_at(&server.url());
        let (landed, sha) = commit(
            &client,
            &repo(),
            None,
            None,
            |arriving| Ok(rebuild_like_the_orchestrator(arriving, "new", 10)),
            &permit(),
            NOW,
        )
        .await
        .unwrap();
        assert_eq!(sha, "blob2");
        assert_eq!(landed.snapshots.len(), 1);

        commit(
            &client,
            &repo(),
            Some(&pointer(&["a"])),
            Some("blob1"),
            |arriving| Ok(rebuild_like_the_orchestrator(arriving, "new", 10)),
            &permit(),
            NOW,
        )
        .await
        .unwrap();

        let bodies = seen.lock().unwrap();
        assert!(!bodies[0].contains("sha"), "first push: {}", bodies[0]);
        assert!(bodies[1].contains(r#""sha":"blob1""#), "{}", bodies[1]);
    }

    /// **The merge rule, reproduced.** Plan 4-04 drives its conflict tests with
    /// this closure; the real one lives in `push/mod.rs`, which 4-04 does not
    /// own. If an edit there breaks one of the three rules, the assertions below
    /// stop describing it.
    fn rebuild_like_the_orchestrator(
        arriving: Option<&Pointer>,
        mine: &str,
        keep: usize,
    ) -> Pointer {
        let mut snapshots = arriving.map(|p| p.snapshots.clone()).unwrap_or_default();
        snapshots.retain(|existing| existing.root != mine);
        snapshots.push(record(mine));
        if snapshots.len() > keep {
            snapshots.drain(..snapshots.len() - keep);
        }
        Pointer {
            format: POINTER_VERSION,
            repo_id: "github:1".into(),
            keyfile: arriving
                .map(|p| p.keyfile.clone())
                .unwrap_or_else(|| "keyfile-x.json".into()),
            snapshots,
        }
    }

    #[test]
    fn rule_one_carries_forward_every_record_this_run_did_not_produce() {
        let competitor = pointer(&["a", "b"]);
        let next = rebuild_like_the_orchestrator(Some(&competitor), "new", 10);
        let roots: Vec<&str> = next.snapshots.iter().map(|r| r.root.as_str()).collect();
        assert_eq!(roots, vec!["a", "b", "new"]);
    }

    #[test]
    fn rule_two_truncates_from_the_oldest_end_only() {
        let full = pointer(&["a", "b", "c"]);
        let next = rebuild_like_the_orchestrator(Some(&full), "new", 2);
        let roots: Vec<&str> = next.snapshots.iter().map(|r| r.root.as_str()).collect();
        assert_eq!(roots, vec!["c", "new"], "the newest survive");
    }

    #[test]
    fn rule_three_takes_the_keyfile_from_the_pointer_that_arrived() {
        let rekeyed = Pointer {
            keyfile: "keyfile-after-a-rekey.json".into(),
            ..pointer(&["a"])
        };
        let next = rebuild_like_the_orchestrator(Some(&rekeyed), "new", 10);
        assert_eq!(next.keyfile, "keyfile-after-a-rekey.json");
        // …and a first push has nothing to take it from, so it uses its own.
        assert_eq!(
            rebuild_like_the_orchestrator(None, "new", 10).keyfile,
            "keyfile-x.json"
        );
    }

    /// Re-running `rebuild` — which is what a 409 does — must not append the
    /// same record twice.
    #[test]
    fn a_second_rebuild_over_a_pointer_that_already_carries_this_run_does_not_duplicate_it() {
        let once = rebuild_like_the_orchestrator(None, "new", 10);
        let twice = rebuild_like_the_orchestrator(Some(&once), "new", 10);
        assert_eq!(twice.snapshots.len(), 1);
    }
}
