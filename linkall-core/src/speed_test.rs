use crate::protocol::AppMessage;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use uuid::Uuid;

/// 4 MB static buffer for high-speed benchmark data without CPU generation overhead.
/// (4MB is a good balance between postcard overhead and socket backpressure).
const CHUNK_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedTestPhase {
    Idle,
    /// We are sending data to the peer
    Sending,
    /// We are receiving data from the peer
    Receiving,
}

pub struct SpeedTestState {
    pub test_id: Option<Uuid>,
    pub phase: SpeedTestPhase,
    pub bytes_transferred: Arc<AtomicU64>,
    pub start_time: Option<Instant>,
    pub duration_secs: u32,
    pub tx_msg: mpsc::Sender<AppMessage>,
    pub abort_handle: Option<tokio::task::JoinHandle<()>>,
    pub last_tick_time: Option<Instant>,
}

impl SpeedTestState {
    pub fn new(tx_msg: mpsc::Sender<AppMessage>) -> Self {
        Self {
            test_id: None,
            phase: SpeedTestPhase::Idle,
            bytes_transferred: Arc::new(AtomicU64::new(0)),
            start_time: None,
            duration_secs: 10,
            tx_msg,
            abort_handle: None,
            last_tick_time: None,
        }
    }

    pub fn reset(&mut self) {
        if let Some(handle) = self.abort_handle.take() {
            handle.abort();
        }
        self.test_id = None;
        self.phase = SpeedTestPhase::Idle;
        self.bytes_transferred.store(0, Ordering::Relaxed);
        self.start_time = None;
    }

    pub fn start_receiving(&mut self, test_id: Uuid, duration_secs: u32) {
        self.reset();
        self.test_id = Some(test_id);
        self.duration_secs = duration_secs;
        self.phase = SpeedTestPhase::Receiving;
        self.start_time = Some(Instant::now());
        self.last_tick_time = Some(Instant::now());
    }

    pub fn handle_chunk(&mut self, data_len: usize) {
        if self.phase == SpeedTestPhase::Receiving {
            self.bytes_transferred
                .fetch_add(data_len as u64, Ordering::Relaxed);
        }
    }

    pub fn start_sending(&mut self, test_id: Uuid, duration_secs: u32) {
        self.reset();
        self.test_id = Some(test_id);
        self.duration_secs = duration_secs;
        self.phase = SpeedTestPhase::Sending;
        self.start_time = Some(Instant::now());

        let tx = self.tx_msg.clone();
        let bytes_transferred = self.bytes_transferred.clone();

        let handle = tokio::spawn(async move {
            let buffer = vec![0u8; CHUNK_SIZE]; // pre-allocate
            let mut seq = 0;
            let start = Instant::now();
            let duration = Duration::from_secs(duration_secs as u64);

            while start.elapsed() < duration {
                let msg = AppMessage::SpeedTestData {
                    test_id,
                    seq,
                    data: buffer.clone(),
                };

                if tx.send(msg).await.is_err() {
                    break;
                }

                seq += 1;
                bytes_transferred.fetch_add(CHUNK_SIZE as u64, Ordering::Relaxed);

                tokio::task::yield_now().await;
            }

            let _ = tx.send(AppMessage::SpeedTestComplete { test_id }).await;
        });

        self.abort_handle = Some(handle);
    }
}
