//! Automated 4-Tier E2E Integration Test Suite for Link All Remote File Queries.
//!
//! Exercising end-to-end RPC behavior across dual in-process `Engine` instances,
//! wire protocol serialization, waiter management, and timeout/error handling.

use linkall_core::engine::{Engine, EngineConfig, EngineEvent};
use linkall_core::identity::IdentityStore;
use linkall_core::protocol::{
    RemoteFileCategory, RemoteFileCategoryCounts, RemoteFileEntry, RemoteFileSource,
    RemoteFileSourceCounts, RemoteFilesSummary,
};
use linkall_core::trust::TrustStore;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Helper: Setup two in-process Engine instances (Node A & Node B) with mutual trust.
async fn setup_test_nodes() -> (
    Engine,
    Uuid,
    Engine,
    Uuid,
    mpsc::Receiver<EngineEvent>,
    TempDir,
) {
    let tmp = TempDir::new().unwrap();
    let (tx_a, _rx_a) = mpsc::channel(64);
    let (tx_b, rx_b) = mpsc::channel(64);

    let dev_a = Uuid::new_v4();
    let dev_b = Uuid::new_v4();

    let id_path_a = tmp.path().join("id_a.key");
    let id_path_b = tmp.path().join("id_b.key");
    let trust_path_a = tmp.path().join("trust_a.json");
    let trust_path_b = tmp.path().join("trust_b.json");
    let peer_path_a = tmp.path().join("peer_a.json");
    let peer_path_b = tmp.path().join("peer_b.json");

    let id_a = IdentityStore::new(&id_path_a).load_or_create().unwrap();
    let id_b = IdentityStore::new(&id_path_b).load_or_create().unwrap();

    let mut trust_a = TrustStore::load(&trust_path_a).unwrap();
    trust_a
        .trust(dev_b, "NodeB".into(), &id_b.public_bytes)
        .unwrap();

    let mut trust_b = TrustStore::load(&trust_path_b).unwrap();
    trust_b
        .trust(dev_a, "NodeA".into(), &id_a.public_bytes)
        .unwrap();

    let cfg_a = EngineConfig {
        device_id: dev_a,
        device_name: "NodeA".into(),
        port: 0,
        trust_store_path: trust_path_a,
        peer_store_path: peer_path_a,
        identity_path: id_path_a,
        data_dir: tmp.path().join("data_a"),
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        ..Default::default()
    };

    let cfg_b = EngineConfig {
        device_id: dev_b,
        device_name: "NodeB".into(),
        port: 0,
        trust_store_path: trust_path_b,
        peer_store_path: peer_path_b,
        identity_path: id_path_b,
        data_dir: tmp.path().join("data_b"),
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        ..Default::default()
    };

    let engine_a = Engine::start(cfg_a, tx_a).await.unwrap();
    let engine_b = Engine::start(cfg_b, tx_b).await.unwrap();

    let port_b = engine_b.bound_port().await;

    let mut ready = false;
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port_b))
            .await
            .is_ok()
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ready, "NodeB failed to listen on port");

    engine_a
        .connect_to_peer("127.0.0.1".into(), port_b)
        .await
        .unwrap();

    // Allow connection handshake to settle
    tokio::time::sleep(Duration::from_millis(150)).await;

    (engine_a, dev_a, engine_b, dev_b, rx_b, tmp)
}

/// Sample dataset for mock responder.
fn sample_file_dataset() -> Vec<RemoteFileEntry> {
    vec![
        RemoteFileEntry {
            file_id: 101,
            display_name: "vacation_sunset.jpg".into(),
            size_bytes: 2048500,
            mime_type: "image/jpeg".into(),
            date_modified: 1770000000,
            category: RemoteFileCategory::Images,
            source: RemoteFileSource::Camera,
            content_uri: "content://media/external/file/101".into(),
        },
        RemoteFileEntry {
            file_id: 102,
            display_name: "beach_party.png".into(),
            size_bytes: 1024300,
            mime_type: "image/png".into(),
            date_modified: 1770001000,
            category: RemoteFileCategory::Images,
            source: RemoteFileSource::WhatsApp,
            content_uri: "content://media/external/file/102".into(),
        },
        RemoteFileEntry {
            file_id: 103,
            display_name: "family_photo.jpg".into(),
            size_bytes: 3096000,
            mime_type: "image/jpeg".into(),
            date_modified: 1770002000,
            category: RemoteFileCategory::Images,
            source: RemoteFileSource::Downloads,
            content_uri: "content://media/external/file/103".into(),
        },
        RemoteFileEntry {
            file_id: 201,
            display_name: "birthday_video.mp4".into(),
            size_bytes: 52428800,
            mime_type: "video/mp4".into(),
            date_modified: 1770003000,
            category: RemoteFileCategory::Videos,
            source: RemoteFileSource::Camera,
            content_uri: "content://media/external/file/201".into(),
        },
        RemoteFileEntry {
            file_id: 301,
            display_name: "podcast_ep1.mp3".into(),
            size_bytes: 10485760,
            mime_type: "audio/mpeg".into(),
            date_modified: 1770004000,
            category: RemoteFileCategory::Audio,
            source: RemoteFileSource::Downloads,
            content_uri: "content://media/external/file/301".into(),
        },
        RemoteFileEntry {
            file_id: 401,
            display_name: "annual_report.pdf".into(),
            size_bytes: 512000,
            mime_type: "application/pdf".into(),
            date_modified: 1770005000,
            category: RemoteFileCategory::Documents,
            source: RemoteFileSource::Downloads,
            content_uri: "content://media/external/file/401".into(),
        },
        RemoteFileEntry {
            file_id: 501,
            display_name: "game_v1.apk".into(),
            size_bytes: 20971520,
            mime_type: "application/vnd.android.package-archive".into(),
            date_modified: 1770006000,
            category: RemoteFileCategory::Apks,
            source: RemoteFileSource::Downloads,
            content_uri: "content://media/external/file/501".into(),
        },
        RemoteFileEntry {
            file_id: 601,
            display_name: "backup_files.zip".into(),
            size_bytes: 41943040,
            mime_type: "application/zip".into(),
            date_modified: 1770007000,
            category: RemoteFileCategory::Archives,
            source: RemoteFileSource::Other,
            content_uri: "content://media/external/file/601".into(),
        },
    ]
}

/// Helper: Spawn mock event loop on Node B to respond to RemoteFilesQueryReceived.
fn spawn_mock_responder(
    engine_b: Engine,
    mut rx_b: mpsc::Receiver<EngineEvent>,
    dataset: Vec<RemoteFileEntry>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = rx_b.recv().await {
            if let EngineEvent::RemoteFilesQueryReceived {
                request_id,
                from_device,
                summary_only,
                category,
                source,
                search_query,
                offset,
                limit,
            } = event
            {
                let mut filtered: Vec<RemoteFileEntry> = dataset
                    .iter()
                    .filter(|&f| {
                        if let Some(cat) = &category {
                            if *cat != RemoteFileCategory::All && f.category != *cat {
                                return false;
                            }
                        }
                        if let Some(src) = &source {
                            if *src != RemoteFileSource::All && f.source != *src {
                                return false;
                            }
                        }
                        if let Some(q) = &search_query {
                            if !f.display_name.to_lowercase().contains(&q.to_lowercase()) {
                                return false;
                            }
                        }
                        true
                    })
                    .cloned()
                    .collect();

                let total_matching = filtered.len() as u32;

                let summary = if summary_only {
                    let mut type_counts = RemoteFileCategoryCounts::default();
                    let mut source_counts = RemoteFileSourceCounts::default();
                    for entry in &dataset {
                        match entry.category {
                            RemoteFileCategory::Images => type_counts.images += 1,
                            RemoteFileCategory::Videos => type_counts.videos += 1,
                            RemoteFileCategory::Audio => type_counts.audio += 1,
                            RemoteFileCategory::Documents => type_counts.documents += 1,
                            RemoteFileCategory::Apks => type_counts.apks += 1,
                            RemoteFileCategory::Archives => type_counts.archives += 1,
                            _ => {}
                        }
                        match entry.source {
                            RemoteFileSource::WhatsApp => source_counts.whatsapp += 1,
                            RemoteFileSource::Downloads => source_counts.downloads += 1,
                            RemoteFileSource::Camera => source_counts.camera += 1,
                            _ => {}
                        }
                    }
                    Some(RemoteFilesSummary {
                        type_counts,
                        source_counts,
                    })
                } else {
                    None
                };

                let files_to_send = if summary_only {
                    vec![]
                } else {
                    let start = offset as usize;
                    if start >= filtered.len() {
                        vec![]
                    } else {
                        let end = (start + limit as usize).min(filtered.len());
                        filtered.drain(start..end).collect()
                    }
                };

                engine_b
                    .send_remote_files_response(
                        from_device,
                        request_id,
                        summary,
                        files_to_send,
                        total_matching,
                        None,
                    )
                    .await;
            }
        }
    })
}

// ==============================================================================
// TIER 1: FEATURE COVERAGE TESTS
// ==============================================================================

#[tokio::test]
async fn test_tier1_feature_query_images_category() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Images),
            None,
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Query Images should succeed");

    assert_eq!(res.total_matching, 3);
    assert_eq!(res.files.len(), 3);
    for file in &res.files {
        assert_eq!(file.category, RemoteFileCategory::Images);
    }
}

#[tokio::test]
async fn test_tier1_feature_query_videos_category() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Videos),
            None,
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Query Videos should succeed");

    assert_eq!(res.total_matching, 1);
    assert_eq!(res.files[0].display_name, "birthday_video.mp4");
    assert_eq!(res.files[0].category, RemoteFileCategory::Videos);
}

#[tokio::test]
async fn test_tier1_feature_query_audio_category() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Audio),
            None,
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Query Audio should succeed");

    assert_eq!(res.total_matching, 1);
    assert_eq!(res.files[0].display_name, "podcast_ep1.mp3");
    assert_eq!(res.files[0].category, RemoteFileCategory::Audio);
}

#[tokio::test]
async fn test_tier1_feature_query_documents_category() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Documents),
            None,
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Query Documents should succeed");

    assert_eq!(res.total_matching, 1);
    assert_eq!(res.files[0].display_name, "annual_report.pdf");
    assert_eq!(res.files[0].category, RemoteFileCategory::Documents);
}

#[tokio::test]
async fn test_tier1_feature_query_apks_category() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Apks),
            None,
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Query Apks should succeed");

    assert_eq!(res.total_matching, 1);
    assert_eq!(res.files[0].display_name, "game_v1.apk");
    assert_eq!(res.files[0].category, RemoteFileCategory::Apks);
}

#[tokio::test]
async fn test_tier1_feature_query_archives_category() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Archives),
            None,
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Query Archives should succeed");

    assert_eq!(res.total_matching, 1);
    assert_eq!(res.files[0].display_name, "backup_files.zip");
    assert_eq!(res.files[0].category, RemoteFileCategory::Archives);
}

#[tokio::test]
async fn test_tier1_feature_query_search_substring() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(dev_b, false, None, None, Some("photo".into()), 0, 50, 5)
        .await
        .expect("Search substring should succeed");

    assert_eq!(res.total_matching, 1);
    assert_eq!(res.files[0].display_name, "family_photo.jpg");
}

#[tokio::test]
async fn test_tier1_feature_pagination_offset_limit() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 1, 2, 5)
        .await
        .expect("Pagination query should succeed");

    assert_eq!(res.total_matching, 8);
    assert_eq!(res.files.len(), 2);
    assert_eq!(res.files[0].display_name, "beach_party.png");
    assert_eq!(res.files[1].display_name, "family_photo.jpg");
}

#[tokio::test]
async fn test_tier1_feature_summary_only_aggregation() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(dev_b, true, None, None, None, 0, 50, 5)
        .await
        .expect("Summary-only query should succeed");

    assert!(res.files.is_empty());
    let summary = res.summary.expect("Summary must be present");
    assert_eq!(summary.type_counts.images, 3);
    assert_eq!(summary.type_counts.videos, 1);
    assert_eq!(summary.type_counts.audio, 1);
    assert_eq!(summary.type_counts.documents, 1);
    assert_eq!(summary.type_counts.apks, 1);
    assert_eq!(summary.type_counts.archives, 1);
    assert_eq!(summary.source_counts.camera, 2);
    assert_eq!(summary.source_counts.whatsapp, 1);
    assert_eq!(summary.source_counts.downloads, 4);
}

#[tokio::test]
async fn test_tier1_feature_source_filtering_whatsapp() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            None,
            Some(RemoteFileSource::WhatsApp),
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Query source WhatsApp should succeed");

    assert_eq!(res.total_matching, 1);
    assert_eq!(res.files[0].display_name, "beach_party.png");
    assert_eq!(res.files[0].source, RemoteFileSource::WhatsApp);
}

#[tokio::test]
async fn test_tier1_feature_source_filtering_camera() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            None,
            Some(RemoteFileSource::Camera),
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Query source Camera should succeed");

    assert_eq!(res.total_matching, 2);
    for file in &res.files {
        assert_eq!(file.source, RemoteFileSource::Camera);
    }
}

// ==============================================================================
// TIER 2: BOUNDARY & CORNER CASE TESTS
// ==============================================================================

#[tokio::test]
async fn test_tier2_boundary_empty_results() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, vec![]);

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Images),
            None,
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Empty dataset query should succeed");

    assert_eq!(res.total_matching, 0);
    assert!(res.files.is_empty());
    assert!(res.error.is_none());
}

#[tokio::test]
async fn test_tier2_boundary_untrusted_peer_drop() {
    let tmp = TempDir::new().unwrap();
    let (tx_a, _rx_a) = mpsc::channel(64);
    let (tx_b, rx_b) = mpsc::channel(64);

    let dev_a = Uuid::new_v4();
    let dev_b = Uuid::new_v4();

    let id_path_a = tmp.path().join("id_a.key");
    let id_path_b = tmp.path().join("id_b.key");
    let trust_path_a = tmp.path().join("trust_a.json");
    let trust_path_b = tmp.path().join("trust_b.json");

    let _id_a = IdentityStore::new(&id_path_a).load_or_create().unwrap();
    let id_b = IdentityStore::new(&id_path_b).load_or_create().unwrap();

    // Node A trusts Node B, BUT Node B DOES NOT trust Node A
    let mut trust_a = TrustStore::load(&trust_path_a).unwrap();
    trust_a
        .trust(dev_b, "NodeB".into(), &id_b.public_bytes)
        .unwrap();

    let _trust_b = TrustStore::load(&trust_path_b).unwrap(); // Empty trust store

    let cfg_a = EngineConfig {
        device_id: dev_a,
        device_name: "NodeA".into(),
        port: 0,
        trust_store_path: trust_path_a,
        peer_store_path: tmp.path().join("peer_a.json"),
        identity_path: id_path_a,
        data_dir: tmp.path().join("data_a"),
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        ..Default::default()
    };

    let cfg_b = EngineConfig {
        device_id: dev_b,
        device_name: "NodeB".into(),
        port: 0,
        trust_store_path: trust_path_b,
        peer_store_path: tmp.path().join("peer_b.json"),
        identity_path: id_path_b,
        data_dir: tmp.path().join("data_b"),
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        ..Default::default()
    };

    let engine_a = Engine::start(cfg_a, tx_a).await.unwrap();
    let engine_b = Engine::start(cfg_b, tx_b).await.unwrap();

    let port_b = engine_b.bound_port().await;
    engine_a
        .connect_to_peer("127.0.0.1".into(), port_b)
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    // Expect query from untrusted peer to be dropped by Node B engine, timing out on Node A
    let err = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 2)
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("timed out"),
        "Expected timeout error for untrusted peer, got: {}",
        err
    );
}

#[tokio::test]
async fn test_tier2_boundary_zero_limit() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 0, 0, 5)
        .await
        .expect("Zero limit query should succeed");

    assert_eq!(res.total_matching, 8);
    assert!(res.files.is_empty());
}

#[tokio::test]
async fn test_tier2_boundary_timeout_expiry() {
    let (engine_a, _dev_a, _engine_b, dev_b, mut rx_b, _tmp) = setup_test_nodes().await;

    // Spawn non-responding handler (receives event but never sends response)
    tokio::spawn(async move {
        while let Some(_event) = rx_b.recv().await {
            // Intentionally ignore RemoteFilesQueryReceived
        }
    });

    let err = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 1)
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("timed out after 1s"),
        "Expected RPC timeout error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_tier2_boundary_custom_timeout_expiry() {
    let (engine_a, _dev_a, _engine_b, dev_b, mut rx_b, _tmp) = setup_test_nodes().await;

    tokio::spawn(async move { while let Some(_event) = rx_b.recv().await {} });

    let err = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 2)
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("timed out after 2s"),
        "Expected custom 2s timeout error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_tier2_boundary_disconnect_cleanup() {
    let (engine_a, _dev_a, engine_b, dev_b, _rx_b, _tmp) = setup_test_nodes().await;

    // Drop engine_b mid-query to simulate peer process crash/disconnect
    let query_task = tokio::spawn(async move {
        engine_a
            .query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 5)
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(engine_b);

    let res = query_task.await.unwrap();
    assert!(
        res.is_err(),
        "Query should fail when target peer disconnects mid-flight"
    );
}

#[tokio::test]
async fn test_tier2_boundary_max_limit() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 0, 1000, 5)
        .await
        .expect("Max limit query should succeed");

    assert_eq!(res.total_matching, 8);
    assert_eq!(res.files.len(), 8);
}

// ==============================================================================
// TIER 3: PAIRWISE COMBINATION TESTS
// ==============================================================================

#[tokio::test]
async fn test_tier3_pairwise_category_search_pagination() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Images),
            None,
            Some("sunset".into()),
            0,
            10,
            5,
        )
        .await
        .expect("Pairwise category + search + pagination query should succeed");

    assert_eq!(res.total_matching, 1);
    assert_eq!(res.files[0].display_name, "vacation_sunset.jpg");
    assert_eq!(res.files[0].category, RemoteFileCategory::Images);
}

#[tokio::test]
async fn test_tier3_pairwise_source_summary_only() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            true,
            None,
            Some(RemoteFileSource::Camera),
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Pairwise source + summary_only query should succeed");

    assert!(res.files.is_empty());
    assert!(res.summary.is_some());
}

#[tokio::test]
async fn test_tier3_pairwise_timeout_with_disconnect() {
    let (engine_a, _dev_a, engine_b, dev_b, _rx_b, _tmp) = setup_test_nodes().await;

    // Call with long timeout (10s) but drop peer immediately
    let query_task = tokio::spawn(async move {
        engine_a
            .query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 10)
            .await
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    drop(engine_b);

    let res = query_task.await.unwrap();
    assert!(
        res.is_err(),
        "Query should fail quickly on disconnect despite 10s timeout setting"
    );
}

// ==============================================================================
// TIER 4: REAL-WORLD APPLICATION SCENARIO TESTS
// ==============================================================================

/// Scenario 1: User opens "Images" remote folder tab (verifying requirement from ORIGINAL_REQUEST.md).
#[tokio::test]
async fn test_tier4_scenario_open_images_folder() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let start_time = std::time::Instant::now();
    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Images),
            None,
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Opening Images remote folder should succeed without timeout");

    let elapsed = start_time.elapsed();
    assert!(
        elapsed < Duration::from_millis(1000),
        "Query latency should be under 1s, took {:?}",
        elapsed
    );

    assert_eq!(res.total_matching, 3);
    assert_eq!(res.files.len(), 3);
    for file in &res.files {
        assert_eq!(file.category, RemoteFileCategory::Images);
        assert!(!file.display_name.is_empty());
        assert!(file.size_bytes > 0);
        assert!(!file.mime_type.is_empty());
        assert!(!file.content_uri.is_empty());
    }
}

/// Scenario 2: Browsing "Downloads" remote folder and typing search query ("report").
#[tokio::test]
async fn test_tier4_scenario_open_downloads_search() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b, rx_b, sample_file_dataset());

    let res = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Documents),
            Some(RemoteFileSource::Downloads),
            Some("report".into()),
            0,
            50,
            5,
        )
        .await
        .expect("Downloads search scenario should succeed");

    assert_eq!(res.total_matching, 1);
    assert_eq!(res.files[0].display_name, "annual_report.pdf");
    assert_eq!(res.files[0].source, RemoteFileSource::Downloads);
}

/// Scenario 3: Scrolling a large remote directory in multiple pages (infinite scroll).
#[tokio::test]
async fn test_tier4_scenario_multi_page_infinite_scroll() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, _tmp) = setup_test_nodes().await;

    // Generate 100 mock files
    let mut large_dataset = Vec::new();
    for i in 1..=100 {
        large_dataset.push(RemoteFileEntry {
            file_id: i,
            display_name: format!("file_{:03}.dat", i),
            size_bytes: i * 1000,
            mime_type: "application/octet-stream".into(),
            date_modified: 1770000000 + i,
            category: RemoteFileCategory::Other,
            source: RemoteFileSource::Other,
            content_uri: format!("content://media/external/file/{}", i),
        });
    }

    let _responder = spawn_mock_responder(engine_b, rx_b, large_dataset);

    // Page 1: 0..50
    let page1 = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 5)
        .await
        .expect("Page 1 query should succeed");

    assert_eq!(page1.total_matching, 100);
    assert_eq!(page1.files.len(), 50);
    assert_eq!(page1.files[0].display_name, "file_001.dat");
    assert_eq!(page1.files[49].display_name, "file_050.dat");

    // Page 2: 50..50
    let page2 = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 50, 50, 5)
        .await
        .expect("Page 2 query should succeed");

    assert_eq!(page2.total_matching, 100);
    assert_eq!(page2.files.len(), 50);
    assert_eq!(page2.files[0].display_name, "file_051.dat");
    assert_eq!(page2.files[49].display_name, "file_100.dat");
}

/// Scenario 4: Query failure followed by reconnect and successful retry.
#[tokio::test]
async fn test_tier4_scenario_device_reconnect_retry() {
    let (engine_a, _dev_a, engine_b, dev_b, rx_b, tmp) = setup_test_nodes().await;
    let _responder = spawn_mock_responder(engine_b.clone(), rx_b, sample_file_dataset());

    let _port_b = engine_b.bound_port().await;

    // Successful first query
    let res1 = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Images),
            None,
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Initial query should succeed");
    assert_eq!(res1.total_matching, 3);

    // Stop engine_b and responder task, disconnect on engine_a
    _responder.abort();
    let _ = engine_a.disconnect_peer(dev_b).await;
    drop(engine_b);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Attempting query now should fail
    let err = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 1)
        .await;
    assert!(err.is_err());

    // Restart Node B engine on same port (or new port)
    let (tx_b2, rx_b2) = mpsc::channel(64);
    let id_path_b = tmp.path().join("id_b.key");
    let trust_path_b = tmp.path().join("trust_b.json");

    let cfg_b2 = EngineConfig {
        device_id: dev_b,
        device_name: "NodeB".into(),
        port: 0,
        trust_store_path: trust_path_b,
        peer_store_path: tmp.path().join("peer_b.json"),
        identity_path: id_path_b,
        data_dir: tmp.path().join("data_b2"),
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        ..Default::default()
    };

    let engine_b2 = Engine::start(cfg_b2, tx_b2).await.unwrap();
    let _responder2 = spawn_mock_responder(engine_b2.clone(), rx_b2, sample_file_dataset());

    let port_a = engine_a.bound_port().await;

    // Let engine_b2 connect to engine_a
    engine_b2
        .connect_to_peer("127.0.0.1".into(), port_a)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Retried query should succeed
    let res2 = engine_a
        .query_remote_files_sync(
            dev_b,
            false,
            Some(RemoteFileCategory::Images),
            None,
            None,
            0,
            50,
            5,
        )
        .await
        .expect("Retry query after reconnect should succeed");

    assert_eq!(res2.total_matching, 3);
}

/// Desktop host: Node B answers with the shared local_files server (as the
/// macOS daemon and the Windows in-process engine do), not a mock.
#[cfg(not(target_os = "android"))]
#[tokio::test]
async fn test_desktop_host_serves_listing_and_thumbnail() {
    let (engine_a, _dev_a, engine_b, dev_b, mut rx_b, _tmp) = setup_test_nodes().await;
    tokio::spawn(async move {
        while let Some(event) = rx_b.recv().await {
            linkall_core::local_files::handle_event(&engine_b, &event);
        }
    });

    let res = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 20)
        .await
        .expect("desktop host should answer the listing query");
    assert!(res.error.is_none(), "unexpected error: {:?}", res.error);
    assert!(res.summary.is_some());
    assert!(res.files.len() as u32 <= res.total_matching);

    let thumb = engine_a
        .request_remote_thumbnail_sync(dev_b, 0xdead_beef, 128, 20)
        .await
        .expect("desktop host should answer the thumbnail request");
    assert_eq!(thumb.error.as_deref(), Some("File not found"));
}
