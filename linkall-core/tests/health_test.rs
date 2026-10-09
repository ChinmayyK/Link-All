//! A working pair of devices reports no health issues.

mod common;

use common::{connect, start_pair};
use tempfile::TempDir;

#[tokio::test]
async fn connected_pair_is_healthy() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = start_pair(&tmp, true, true).await;
    for node in [&a, &b] {
        // Test engines load this machine's settings; sync must be on here.
        let mut s = node.engine.current_settings().await;
        s.sync_enabled = true;
        node.engine.apply_settings(s).await;
    }
    connect(&a, &b).await;
    assert_eq!(a.engine.health().await, vec![]);
    assert_eq!(b.engine.health().await, vec![]);
}
