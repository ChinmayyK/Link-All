//! A second Link All device for smoke-testing an installed app on one machine.
//!
//! It starts its own engine (own identity, stores in a temp folder), dials the
//! app, asks to pair, then exchanges a clip and a file with it. A test script
//! drives the app's side through linkall-cli and follows this one's progress
//! on stdout:
//!
//! ```text
//! PEER <our id>              started; the app sees a pairing request from it
//! APP <app id>               connected to the app
//! PAIRED                     the app accepted (linkall-cli pair accept <our id>)
//! SENT_CLIP <text>           pushed a clip to the app
//! SENT_FILE <name>           sent a file to the app
//! GOT_CLIP <text>            received a clip from the app
//! GOT_FILE <name> <bytes>    received a file from the app
//! DONE                       received both; exit code 0
//! ```
//!
//! cargo run --example smoke_peer -- 127.0.0.1 47823

use linkall_core::engine::{Engine, EngineConfig, EngineEvent};
use linkall_core::protocol::ClipboardContent;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{timeout, Instant};
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let host = args.next().unwrap_or_else(|| "127.0.0.1".into());
    let port: u16 = args.next().map(|p| p.parse()).transpose()?.unwrap_or(47823);
    let wait = Duration::from_secs(args.next().map(|s| s.parse()).transpose()?.unwrap_or(180));

    let dir = tempfile::tempdir()?;
    let id = Uuid::new_v4();
    let cfg = EngineConfig {
        device_id: id,
        device_name: "Smoke Peer".into(),
        port: 0,
        trust_store_path: dir.path().join("trust.json"),
        peer_store_path: dir.path().join("peers.json"),
        identity_path: dir.path().join("identity.key"),
        settings_path: dir.path().join("settings.json"),
        data_dir: dir.path().join("data"),
        file_save_dir: Some(dir.path().join("received")),
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        ..EngineConfig::default()
    };
    let (tx, mut events) = mpsc::channel(256);
    let engine = Engine::start(cfg, tx).await?;
    say(format!("PEER {id}"));

    engine.connect_to_peer(host, port).await?;
    let app = engine
        .status_snapshot()
        .await
        .peers
        .into_iter()
        .find(|p| p.id != id)
        .map(|p| p.id)
        .ok_or_else(|| anyhow::anyhow!("connected, but no peer in the snapshot"))?;
    say(format!("APP {app}"));

    engine.send_pairing_request(app).await;
    let deadline = Instant::now() + wait;
    while !engine.is_trusted(app).await {
        anyhow::ensure!(Instant::now() < deadline, "the app never accepted pairing");
        drain(&mut events);
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    say("PAIRED".into());
    // Let the app's side finish recording the pairing before sending.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let clip = format!("smoke clip from peer {}", &id.to_string()[..8]);
    engine
        .push_clipboard(ClipboardContent::Text(clip.clone()))
        .await;
    say(format!("SENT_CLIP {clip}"));

    let name = format!("smoke-from-peer-{}.bin", &id.to_string()[..8]);
    let path = dir.path().join(&name);
    std::fs::write(&path, vec![0x5a; 3 * 1024 * 1024 + 17])?;
    engine
        .send_file_path(
            path,
            name.clone(),
            "application/octet-stream".into(),
            Some(app),
            None,
            false,
            1,
        )
        .await?;
    say(format!("SENT_FILE {name}"));

    let (mut got_clip, mut got_file) = (false, false);
    while !(got_clip && got_file) {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = timeout(left, events.recv())
            .await
            .map_err(|_| anyhow::anyhow!("timed out: clip={got_clip} file={got_file}"))?
            .ok_or_else(|| anyhow::anyhow!("engine stopped"))?;
        match event {
            EngineEvent::ClipboardReceived {
                from_device,
                content,
                ..
            } if from_device == app => {
                if let ClipboardContent::Text(text) = &*content {
                    say(format!("GOT_CLIP {text}"));
                    got_clip = true;
                }
            }
            EngineEvent::FileTransferComplete {
                from_device,
                file_name,
                dest_path,
                ..
            } if from_device == app => {
                let bytes = std::fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0);
                say(format!("GOT_FILE {file_name} {bytes}"));
                got_file = true;
            }
            _ => {}
        }
    }
    say("DONE".into());
    Ok(())
}

fn say(line: String) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn drain(events: &mut mpsc::Receiver<EngineEvent>) {
    while events.try_recv().is_ok() {}
}
