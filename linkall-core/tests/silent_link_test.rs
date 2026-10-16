//! A link that goes quiet without closing: the phone left Wi-Fi, or its OS
//! froze it, while the TCP connection still looks open. With the phone's
//! screen off the heartbeat is relaxed, so silence alone proves nothing;
//! a send that gets no answer must.

mod common;

use common::{connected, eventually, eventually_within, start_pair};
use linkall_core::protocol::ClipboardContent;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Forwards one TCP port to another until told to go silent; then it keeps
/// both connections open and drops everything, like a network that stopped
/// delivering packets.
struct Relay {
    port: u16,
    silent: Arc<AtomicBool>,
}

impl Relay {
    async fn start(target: u16) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let silent = Arc::new(AtomicBool::new(false));
        let flag = silent.clone();
        tokio::spawn(async move {
            while let Ok((inbound, _)) = listener.accept().await {
                let outbound = TcpStream::connect(("127.0.0.1", target)).await.unwrap();
                let (ri, wi) = inbound.into_split();
                let (ro, wo) = outbound.into_split();
                tokio::spawn(pipe(ri, wo, flag.clone()));
                tokio::spawn(pipe(ro, wi, flag.clone()));
            }
        });
        Relay { port, silent }
    }

    fn go_silent(&self) {
        self.silent.store(true, Ordering::SeqCst);
    }

    fn deliver_again(&self) {
        self.silent.store(false, Ordering::SeqCst);
    }
}

async fn pipe(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    silent: Arc<AtomicBool>,
) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match from.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if silent.load(Ordering::SeqCst) {
            continue;
        }
        if to.write_all(&buf[..n]).await.is_err() {
            return;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sleeping_phone_that_went_silent_is_noticed_after_a_send() {
    let tmp = TempDir::new().unwrap();
    let (phone, computer) = start_pair(&tmp, true, true).await;
    let relay = Relay::start(computer.engine.bound_port().await).await;
    phone
        .engine
        .connect_to_peer("127.0.0.1".into(), relay.port)
        .await
        .expect("the phone dials the computer through the relay");
    eventually("both sides connected", async || {
        connected(&computer, phone.id).await && connected(&phone, computer.id).await
    })
    .await;

    // The phone's screen goes off, then the network stops delivering.
    phone.engine.notify_sleep_state(true).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    relay.go_silent();

    computer
        .engine
        .push_clipboard(ClipboardContent::Text("hello".into()))
        .await;
    eventually_within(
        Duration::from_secs(30),
        "the computer notices the phone is gone",
        async || !connected(&computer, phone.id).await,
    )
    .await;
}

/// The network drops out for a while (the computer slept, the router
/// restarted) and comes back: both sides must notice, then get back together
/// without anyone opening the app.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn after_an_outage_both_sides_reconnect_by_themselves() {
    let tmp = TempDir::new().unwrap();
    let (phone, computer) = start_pair(&tmp, true, true).await;
    let relay = Relay::start(computer.engine.bound_port().await).await;
    phone
        .engine
        .connect_to_peer("127.0.0.1".into(), relay.port)
        .await
        .expect("the phone dials the computer through the relay");
    eventually("both sides connected", async || {
        connected(&computer, phone.id).await && connected(&phone, computer.id).await
    })
    .await;

    // With the phone's screen off, neither side times out on silence alone.
    phone.engine.notify_sleep_state(true).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    relay.go_silent();
    // Asleep, each side pings once a minute, so with nothing sent the dead
    // link shows up within about 72 s (a minute plus the reply timeout).
    eventually_within(
        Duration::from_secs(90),
        "both sides notice the link is gone",
        async || !connected(&computer, phone.id).await && !connected(&phone, computer.id).await,
    )
    .await;

    relay.deliver_again();
    eventually_within(
        Duration::from_secs(60),
        "they reconnect by themselves",
        async || connected(&computer, phone.id).await && connected(&phone, computer.id).await,
    )
    .await;
}
