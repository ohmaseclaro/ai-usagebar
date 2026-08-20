//! Pack assets on to the release.
//!
//! Plan 4-01 created this file with the straight-line case: upload each pack in
//! turn and verify each one. **Plan 4-03 owns it** and adds the resume scan (D4:
//! skip a pack already present at a matching size and state, delete a torn one
//! first), the four-at-a-time `JoinSet`, and the per-asset progress calls.
//!
//! Nothing in this file prints. Rendering belongs to the [`Progress`]
//! implementations, which keeps the uploader testable with
//! [`Silent`](super::progress::Silent).

use crate::error::{AppError, Result};
use crate::sync::crypto::content_address;
use crate::sync::github::gate;

use super::progress::Progress;
use super::{BuiltPack, PushCtx, pack_asset_name};

/// Upload `packs`, returning `(uploaded, skipped, bytes_uploaded)`.
///
/// **Verification is D3's precondition for the flip**, not a nicety: every asset
/// this run uploaded is fetched back and its content address compared against
/// the pack's id. A mismatch fails this function, so the orchestrator never
/// reaches the pointer `PUT` and no pointer can reference a pack that did not
/// verify.
///
/// Say plainly what that check is and is not: a corrupt pack would in any case
/// fail its per-blob Poly1305 tags on read, so this catches transport and
/// packaging bugs, not attacks. It costs one extra download of newly-uploaded
/// data — on a 115 MB first push, 115 MB — and that is the price D3 sets.
///
/// `permit` is the gate's, minted inside the push. It authorises every write
/// here; the flip gets a second one, minted by the re-gate afterwards.
pub async fn run(
    ctx: &PushCtx<'_>,
    release_id: u64,
    packs: &[BuiltPack],
    permit: &gate::Pushing,
    progress: &mut dyn Progress,
) -> Result<(usize, usize, u64)> {
    let total_bytes: u64 = packs.iter().map(|p| p.bytes.len() as u64).sum();
    progress.start(packs.len(), total_bytes);

    let mut uploaded = 0usize;
    let mut bytes = 0u64;
    for (index, pack) in packs.iter().enumerate() {
        let name = pack_asset_name(&pack.id);
        let asset = ctx
            .client
            .upload_asset(
                ctx.repo,
                release_id,
                &name,
                pack.bytes.clone(),
                permit,
                ctx.now,
            )
            .await?;

        let fetched = ctx
            .client
            .download_asset(ctx.repo, asset.id, ctx.now)
            .await?;
        if content_address(&fetched) != pack.id {
            return Err(AppError::Other(format!(
                "the pack uploaded as {name} does not read back as the bytes that were sent. \
                 Nothing was published — the snapshot pointer is untouched — and re-running \
                 the command re-uploads it. If this repeats, report it at \
                 https://github.com/akitaonrails/ai-usagebar/issues."
            )));
        }

        uploaded += 1;
        bytes += pack.bytes.len() as u64;
        progress.asset_done(index, &name, pack.bytes.len() as u64);
    }

    progress.finish();
    Ok((uploaded, 0, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SyncConfig;
    use crate::sync::SyncRoots;
    use crate::sync::crypto::{KdfParams, Keyfile, Keys};
    use crate::sync::github::token::TokenSource;
    use crate::sync::github::{Client, Endpoints, RepoRef};
    use crate::sync::index::Index;
    use crate::sync::push::progress::Silent;
    use chrono::{DateTime, Utc};
    use mockito::Matcher;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;
    use zeroize::Zeroizing;

    const TOKEN: &str = "github_pat_fixture_not_a_real_token";
    const RELEASE: u64 = 9;

    const NOW: DateTime<Utc> = match DateTime::from_timestamp(1_700_000_000, 0) {
        Some(t) => t,
        None => panic!("a fixed timestamp"),
    };

    /// Microseconds instead of a gibibyte and 1.5 s. The AUR `check()` runs
    /// these on an installer's machine.
    const CHEAP: KdfParams = KdfParams {
        m_kib: 8,
        t: 1,
        p: 1,
    };

    /// A permit through the only door there is — there is no constructor to
    /// shortcut, so this is also a standing check that the gate clears a
    /// private repository.
    fn permit() -> gate::Pushing {
        let facts = gate::RepoFacts {
            id: 1,
            private: true,
            visibility: "private".into(),
            owner_login: "o".into(),
            owner_id: 7,
            archived: false,
            fork: false,
            admin_permission: false,
        };
        gate::assert_pushable(&facts, &RepoRef::parse("o/n").unwrap(), true, NOW)
            .expect("a private repository clears")
            .0
            .spend(NOW)
            .expect("freshly minted")
    }

    /// Everything a [`PushCtx`] borrows, owned in one place so the context can
    /// be handed out by reference. Every path is under a temp directory: no
    /// test here reads a real `$HOME`.
    struct Local {
        _dir: TempDir,
        roots: SyncRoots,
        cfg: SyncConfig,
        keys: Keys,
        index: Index,
        repo: RepoRef,
        client: Client,
    }

    impl Local {
        fn at(base: &str) -> Self {
            let dir = TempDir::new().unwrap();
            let roots = SyncRoots::at(
                dir.path().join("config.toml"),
                dir.path().to_path_buf(),
                dir.path().join("desktop"),
                dir.path().join("profiles"),
                dir.path().join("claude-home"),
            );
            let index = Index::at(&roots.index_file).unwrap();
            let keys = Keyfile::create_with_floor(b"a-test-passphrase", CHEAP, CHEAP.m_kib)
                .unwrap()
                .1;
            let client = Client::new(
                &Endpoints {
                    api_base: base.into(),
                    uploads_base: base.into(),
                },
                Zeroizing::new(TOKEN.into()),
                TokenSource::Env,
            )
            .unwrap();
            Self {
                _dir: dir,
                roots,
                cfg: SyncConfig::default(),
                keys,
                index,
                repo: RepoRef::parse("o/n").unwrap(),
                client,
            }
        }

        fn ctx(&self) -> PushCtx<'_> {
            PushCtx {
                client: &self.client,
                repo: &self.repo,
                cfg: &self.cfg,
                roots: &self.roots,
                keys: &self.keys,
                kdf: CHEAP,
                index: &self.index,
                repo_id: "github:1".into(),
                keyfile_asset: "keyfile-unset.json".into(),
                previous: None,
                now: NOW,
            }
        }
    }

    fn pack(fill: u8, len: usize) -> BuiltPack {
        let bytes = vec![fill; len];
        BuiltPack {
            id: content_address(&bytes),
            bytes,
        }
    }

    fn asset_json(id: u64, name: &str, size: usize, state: &str) -> String {
        format!(
            r#"{{"id":{id},"name":"{name}","size":{size},"state":"{state}",
               "created_at":"2023-11-14T22:13:20Z"}}"#
        )
    }

    /// The resume scan's one `GET`. `body` is the JSON array it answers with.
    async fn mock_listing(server: &mut mockito::Server, body: String) -> mockito::Mock {
        server
            .mock("GET", "/repos/o/n/releases/9/assets")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_body(body)
            .create_async()
            .await
    }

    /// The upload and the verifying download for one pack, wired as a pair so
    /// the download answers with the very bytes that were sent.
    async fn mock_pack(
        server: &mut mockito::Server,
        pack: &BuiltPack,
        asset_id: u64,
    ) -> (mockito::Mock, mockito::Mock) {
        let name = pack_asset_name(&pack.id);
        let upload = server
            .mock("POST", "/repos/o/n/releases/9/assets")
            .match_query(Matcher::UrlEncoded("name".into(), name.clone()))
            .with_status(201)
            .with_body(asset_json(
                asset_id,
                &name,
                pack.bytes.len(),
                ASSET_STATE_UPLOADED,
            ))
            .expect(1)
            .create_async()
            .await;
        let download = server
            .mock("GET", format!("/repos/o/n/releases/assets/{asset_id}").as_str())
            .with_status(200)
            .with_body(pack.bytes.clone())
            .create_async()
            .await;
        (upload, download)
    }

    // ---- the resume scan, D4 ----------------------------------------------

    #[tokio::test]
    async fn an_asset_present_at_a_matching_size_and_state_is_skipped_and_the_rest_upload() {
        let mut server = mockito::Server::new_async().await;
        let packs = [pack(1, 10), pack(2, 20), pack(3, 30)];
        let landed = pack_asset_name(&packs[0].id);
        let _list = mock_listing(
            &mut server,
            format!("[{}]", asset_json(50, &landed, 10, ASSET_STATE_UPLOADED)),
        )
        .await;
        let skipped_upload = server
            .mock("POST", "/repos/o/n/releases/9/assets")
            .match_query(Matcher::UrlEncoded("name".into(), landed))
            .expect(0)
            .create_async()
            .await;
        let (b_up, _b_down) = mock_pack(&mut server, &packs[1], 101).await;
        let (c_up, _c_down) = mock_pack(&mut server, &packs[2], 102).await;

        let local = Local::at(&server.url());
        let (uploaded, skipped, bytes) = run(
            &local.ctx(),
            RELEASE,
            &packs,
            &permit(),
            &mut Silent,
        )
        .await
        .unwrap();

        assert_eq!((uploaded, skipped), (2, 1));
        // Measured from the packs' own lengths, never projected.
        assert_eq!(bytes, (packs[1].bytes.len() + packs[2].bytes.len()) as u64);
        skipped_upload.assert_async().await;
        b_up.assert_async().await;
        c_up.assert_async().await;
    }

    #[tokio::test]
    async fn an_asset_in_any_other_state_is_deleted_before_the_pack_is_uploaded() {
        let mut server = mockito::Server::new_async().await;
        let packs = [pack(1, 10)];
        let name = pack_asset_name(&packs[0].id);
        let _list = mock_listing(
            &mut server,
            format!("[{}]", asset_json(50, &name, 10, "starter")),
        )
        .await;
        let delete = server
            .mock("DELETE", "/repos/o/n/releases/assets/50")
            .with_status(204)
            .expect(1)
            .create_async()
            .await;
        let (upload, _download) = mock_pack(&mut server, &packs[0], 101).await;

        let local = Local::at(&server.url());
        let (uploaded, skipped, _) =
            run(&local.ctx(), RELEASE, &packs, &permit(), &mut Silent)
                .await
                .unwrap();

        assert_eq!((uploaded, skipped), (1, 0));
        delete.assert_async().await;
        upload.assert_async().await;
    }

    #[tokio::test]
    async fn an_asset_whose_size_disagrees_is_deleted_before_the_pack_is_uploaded() {
        let mut server = mockito::Server::new_async().await;
        let packs = [pack(1, 10)];
        let name = pack_asset_name(&packs[0].id);
        // The state says uploaded, and the name is the content address — only
        // the size disagrees, and that is enough.
        let _list = mock_listing(
            &mut server,
            format!("[{}]", asset_json(50, &name, 7, ASSET_STATE_UPLOADED)),
        )
        .await;
        let delete = server
            .mock("DELETE", "/repos/o/n/releases/assets/50")
            .with_status(204)
            .expect(1)
            .create_async()
            .await;
        let (upload, _download) = mock_pack(&mut server, &packs[0], 101).await;

        let local = Local::at(&server.url());
        run(&local.ctx(), RELEASE, &packs, &permit(), &mut Silent)
            .await
            .unwrap();

        delete.assert_async().await;
        upload.assert_async().await;
    }

    /// T-4-26: assets belonging to other snapshots are prune's business, and
    /// only after a successful flip.
    #[tokio::test]
    async fn an_asset_matching_no_pack_in_this_run_is_left_completely_alone() {
        let mut server = mockito::Server::new_async().await;
        let packs = [pack(1, 10)];
        let stranger = pack_asset_name(&pack(9, 99).id);
        let _list = mock_listing(
            &mut server,
            format!("[{}]", asset_json(50, &stranger, 99, ASSET_STATE_UPLOADED)),
        )
        .await;
        let delete = server
            .mock("DELETE", Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let (upload, _download) = mock_pack(&mut server, &packs[0], 101).await;

        let local = Local::at(&server.url());
        run(&local.ctx(), RELEASE, &packs, &permit(), &mut Silent)
            .await
            .unwrap();

        delete.assert_async().await;
        upload.assert_async().await;
    }

    // ---- the ceiling on bodies in flight -----------------------------------

    #[derive(Default)]
    struct Flight {
        started: usize,
        finished: usize,
        max: usize,
    }

    /// Six packs, four at a time. `started - finished` is the number of packs
    /// whose upload has reached the server and whose verifying download has
    /// not, which is exactly the window this function bounds.
    #[tokio::test]
    async fn no_more_than_four_bodies_are_in_flight_at_once() {
        let mut server = mockito::Server::new_async().await;
        let packs: Vec<BuiltPack> = (1..=6u8).map(|i| pack(i, 10 + i as usize)).collect();
        let _list = mock_listing(&mut server, "[]".into()).await;

        let flight: Arc<Mutex<Flight>> = Arc::default();
        let mut keep = Vec::new();
        for (i, p) in packs.iter().enumerate() {
            let asset_id = 200 + i as u64;
            let name = pack_asset_name(&p.id);
            let on_upload = Arc::clone(&flight);
            keep.push(
                server
                    .mock("POST", "/repos/o/n/releases/9/assets")
                    .match_query(Matcher::UrlEncoded("name".into(), name.clone()))
                    .match_request(move |_| {
                        let mut f = on_upload.lock().unwrap();
                        f.started += 1;
                        f.max = f.max.max(f.started - f.finished);
                        true
                    })
                    .with_status(201)
                    .with_body(asset_json(
                        asset_id,
                        &name,
                        p.bytes.len(),
                        ASSET_STATE_UPLOADED,
                    ))
                    .create_async()
                    .await,
            );
            let on_download = Arc::clone(&flight);
            keep.push(
                server
                    .mock(
                        "GET",
                        format!("/repos/o/n/releases/assets/{asset_id}").as_str(),
                    )
                    .match_request(move |_| {
                        on_download.lock().unwrap().finished += 1;
                        true
                    })
                    .with_status(200)
                    .with_body(p.bytes.clone())
                    .create_async()
                    .await,
            );
        }

        let local = Local::at(&server.url());
        let (uploaded, _, _) = run(&local.ctx(), RELEASE, &packs, &permit(), &mut Silent)
            .await
            .unwrap();

        assert_eq!(uploaded, 6);
        assert_eq!(
            flight.lock().unwrap().max,
            MAX_IN_FLIGHT,
            "four bodies concurrent, and never a fifth"
        );
    }

    // ---- verification, D3's precondition for the flip ----------------------

    #[tokio::test]
    async fn a_download_that_reads_back_different_bytes_fails_the_run() {
        let mut server = mockito::Server::new_async().await;
        let packs = [pack(1, 10)];
        let name = pack_asset_name(&packs[0].id);
        let _list = mock_listing(&mut server, "[]".into()).await;
        let _upload = server
            .mock("POST", "/repos/o/n/releases/9/assets")
            .match_query(Matcher::Any)
            .with_status(201)
            .with_body(asset_json(101, &name, 10, ASSET_STATE_UPLOADED))
            .create_async()
            .await;
        let _download = server
            .mock("GET", "/repos/o/n/releases/assets/101")
            .with_status(200)
            .with_body(vec![0xff; 10])
            .create_async()
            .await;

        let local = Local::at(&server.url());
        let err = run(&local.ctx(), RELEASE, &packs, &permit(), &mut Silent)
            .await
            .expect_err("altered bytes must fail the run, so the caller never flips");
        assert!(err.to_string().contains("does not read back"), "{err}");
    }

    /// D7: a 401 retried is a slower failure. `with_retry`'s own arms are
    /// tested in `write.rs`; this asserts the uploader inherits them rather
    /// than adding a second loop.
    #[tokio::test]
    async fn an_unauthorized_upload_fails_on_the_first_attempt() {
        let mut server = mockito::Server::new_async().await;
        let packs = [pack(1, 10)];
        let _list = mock_listing(&mut server, "[]".into()).await;
        let upload = server
            .mock("POST", "/repos/o/n/releases/9/assets")
            .match_query(Matcher::Any)
            .with_status(401)
            .with_body(r#"{"message":"Bad credentials"}"#)
            .expect(1)
            .create_async()
            .await;

        let local = Local::at(&server.url());
        run(&local.ctx(), RELEASE, &packs, &permit(), &mut Silent)
            .await
            .expect_err("a 401 is terminal");
        upload.assert_async().await;
    }
}
