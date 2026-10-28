//! A power cut can leave store files truncated. The app must still start: each
//! unreadable store is set aside next to the original, the engine starts empty
//! for that store, and the user is told what was lost.

mod common;

use common::{start_node, twin_config};
use linkall_core::engine::EngineEvent;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_engine_starts_when_every_store_file_is_truncated() {
    let tmp = TempDir::new().unwrap();
    let cfg = twin_config(&tmp, "phone", Uuid::new_v4(), "Phone");
    let truncated = br#"{"devices": {"6b1f"#;
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    let history = cfg.data_dir.join("history.json");
    for path in [
        &cfg.trust_store_path,
        &cfg.peer_store_path,
        &cfg.settings_path,
        &history,
    ] {
        std::fs::write(path, truncated).unwrap();
    }

    let mut node = start_node(cfg.clone()).await;

    let mut warnings = Vec::new();
    while let Ok(Some(event)) =
        tokio::time::timeout(Duration::from_secs(2), node.events.recv()).await
    {
        if let EngineEvent::Warning(text) = event {
            warnings.push(text);
        }
    }
    for name in [
        "trust_phone.json",
        "peers_phone.json",
        "settings_phone.json",
    ] {
        assert!(
            warnings.iter().any(|w| w.contains(name)),
            "no warning names {name}: {warnings:?}"
        );
    }

    let set_aside = |dir: &std::path::Path, prefix: &str| {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with(prefix) && name.contains("corrupt")
            })
    };
    for prefix in [
        "trust_phone.json",
        "peers_phone.json",
        "settings_phone.json",
    ] {
        assert!(set_aside(tmp.path(), prefix), "{prefix} was not set aside");
    }
    assert!(
        set_aside(&cfg.data_dir, "history"),
        "history was not set aside"
    );
    assert!(node.engine.bound_port().await > 0);
}
