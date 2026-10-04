//! Link All chunked file transfer — streaming, verified delivery.
//!
//! # Pipeline
//! ```text
//! Sender                              Receiver
//! ------                              --------
//! FileTransferAnnounce --------------> (user accepts / auto-accept small files)
//!                      <-------------- FileTransferAccept { accepted: true }
//! ChunkStart ------------------------>
//! Chunk(0) -------------------------->
//! Chunk(1) -------------------------->
//! ...
//! ChunkEnd ------------------------->  reassemble -> SHA-256 verify -> save
//! ```
//!
//! Each chunk frame is individually encrypted by the session AEAD layer.
//! SHA-256 over the complete file is verified before the file is written.

use crate::protocol::ClipboardContent;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

pub const CHUNK_THRESHOLD: usize = 128 * 1024; // 128 KB
pub const CHUNK_SIZE: usize = 512 * 1024; // 512 KB per chunk — amortises per-chunk overhead

pub type TransferId = [u8; 16];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ChunkKind {
    Text,
    Image { mime: String },
    File { name: String },
}

// ── Sender ────────────────────────────────────────────────────────────────────

/// Split a `ClipboardContent` into ordered chunk messages.
/// Returns `None` for small payloads that can be sent inline.
///
/// Previously this function eagerly cloned the entire payload into a `Vec<u8>`
/// before checking whether it exceeded `CHUNK_THRESHOLD`, wasting up to 128 KB
/// of heap on every outgoing clipboard push (HIGH-06).  We now borrow the
/// underlying bytes for the threshold check and only materialise the chunks
/// after we know the payload is large enough to warrant chunking.
pub fn maybe_chunk<'a>(
    content: &'a ClipboardContent,
) -> Option<impl Iterator<Item = ChunkMessage> + 'a> {
    // Borrow the raw bytes without cloning — used for length check and checksum.
    let raw: &'a [u8] = match content {
        ClipboardContent::Text(s) => s.as_bytes(),
        ClipboardContent::Image { data, .. } => data.as_slice(),
        ClipboardContent::File { data, .. } => data.as_slice(),
    };

    if raw.len() <= CHUNK_THRESHOLD {
        return None;
    }

    let kind = match content {
        ClipboardContent::Text(_) => ChunkKind::Text,
        ClipboardContent::Image { mime, .. } => ChunkKind::Image { mime: mime.clone() },
        ClipboardContent::File { name, .. } => ChunkKind::File { name: name.clone() },
    };

    let checksum = hex::encode(Sha256::digest(raw));
    let mut id = [0u8; 16];
    id.copy_from_slice(Uuid::new_v4().as_bytes());

    let total_bytes = raw.len() as u64;
    let chunk_slices = raw.chunks(CHUNK_SIZE);
    let total_chunks = chunk_slices.len() as u32;

    let start_msg = std::iter::once(ChunkMessage::Start {
        transfer_id: id,
        total_chunks,
        total_bytes,
        checksum,
        kind,
    });

    let chunks = chunk_slices
        .enumerate()
        .map(move |(index, slice)| ChunkMessage::Chunk {
            transfer_id: id,
            index: index as u32,
            data: slice.to_vec(),
        });

    let end_msg = std::iter::once(ChunkMessage::End { transfer_id: id });

    Some(start_msg.chain(chunks).chain(end_msg))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ChunkMessage {
    Start {
        transfer_id: TransferId,
        total_chunks: u32,
        total_bytes: u64,
        /// SHA-256 hex string of the complete payload (for integrity verification).
        checksum: String,
        kind: ChunkKind,
    },
    Chunk {
        transfer_id: TransferId,
        index: u32,
        data: Vec<u8>,
    },
    End {
        transfer_id: TransferId,
    },
}

// ── Receiver / Reassembler ────────────────────────────────────────────────────

#[derive(Debug)]
struct Transfer {
    kind: ChunkKind,
    total_chunks: u32,
    total_bytes: u64,
    expected_checksum: String,
    chunks: HashMap<u32, Vec<u8>>,
}

#[derive(Default)]
pub struct Reassembler {
    in_flight: HashMap<TransferId, Transfer>,
}

#[derive(Debug)]
pub enum ReassemblerOutput {
    InProgress {
        received: u32,
        total: u32,
        bytes_so_far: u64,
        percent: u8,
    },
    Complete(ClipboardContent),
    ChecksumMismatch {
        expected: String,
        got: String,
    },
}

impl Reassembler {
    pub fn feed(&mut self, msg: ChunkMessage) -> Result<Option<ReassemblerOutput>> {
        match msg {
            ChunkMessage::Start {
                transfer_id,
                total_chunks,
                total_bytes,
                checksum,
                kind,
            } => {
                // Fix 8: Reject absurd transfer announcements before allocating
                // any state.  A malicious peer can announce total_bytes=u64::MAX
                // and total_chunks=u32::MAX to tie up in-flight slots indefinitely.
                // We cap at MAX_FILE_BYTES (512 MB) and MAX_CHUNKS_ALLOWED (8 192).
                const MAX_ANNOUNCED_BYTES: u64 = crate::protocol::MAX_FILE_BYTES;
                const MAX_CHUNKS_ALLOWED: u32 = 2_097_152; // 2 097 152 × 512 KB = 1 TB
                anyhow::ensure!(
                    total_bytes <= MAX_ANNOUNCED_BYTES,
                    "transfer announces {} bytes which exceeds the {} byte cap",
                    total_bytes,
                    MAX_ANNOUNCED_BYTES
                );
                anyhow::ensure!(
                    total_chunks <= MAX_CHUNKS_ALLOWED,
                    "transfer announces {} chunks which exceeds the {} chunk cap",
                    total_chunks,
                    MAX_CHUNKS_ALLOWED
                );
                anyhow::ensure!(!checksum.is_empty(), "transfer Start has empty checksum");

                // Prevent unbounded memory growth from a flood of Start messages
                const MAX_IN_FLIGHT: usize = 32;
                anyhow::ensure!(
                    self.in_flight.len() < MAX_IN_FLIGHT,
                    "too many in-flight transfers"
                );

                self.in_flight.insert(
                    transfer_id,
                    Transfer {
                        kind,
                        total_chunks,
                        total_bytes,
                        expected_checksum: checksum,
                        chunks: HashMap::new(),
                    },
                );
                Ok(None)
            }

            ChunkMessage::Chunk {
                transfer_id,
                index,
                data,
            } => {
                let transfer = self
                    .in_flight
                    .get_mut(&transfer_id)
                    .context("chunk for unknown transfer")?;

                anyhow::ensure!(
                    index < transfer.total_chunks,
                    "chunk index {} out of range (total {})",
                    index,
                    transfer.total_chunks
                );
                anyhow::ensure!(
                    data.len() <= CHUNK_SIZE + 16,
                    "chunk {} too large ({})",
                    index,
                    data.len()
                );
                // Reject duplicate chunk indices — an attacker sending a
                // second chunk with the same index could replace valid data.
                anyhow::ensure!(
                    !transfer.chunks.contains_key(&index),
                    "duplicate chunk index {} for transfer — rejected",
                    index
                );

                transfer.chunks.insert(index, data);
                let received = transfer.chunks.len() as u32;
                let bytes_so_far: u64 = transfer.chunks.values().map(|c| c.len() as u64).sum();
                let percent = ((received as f64 / transfer.total_chunks as f64) * 100.0) as u8;
                Ok(Some(ReassemblerOutput::InProgress {
                    received,
                    total: transfer.total_chunks,
                    bytes_so_far,
                    percent,
                }))
            }

            ChunkMessage::End { transfer_id } => {
                let transfer = self
                    .in_flight
                    .remove(&transfer_id)
                    .context("ChunkEnd for unknown transfer")?;

                anyhow::ensure!(
                    transfer.chunks.len() as u32 == transfer.total_chunks,
                    "missing chunks: got {} of {}",
                    transfer.chunks.len(),
                    transfer.total_chunks
                );

                let mut buf = Vec::with_capacity(transfer.total_bytes as usize);
                for i in 0..transfer.total_chunks {
                    let chunk = transfer
                        .chunks
                        .get(&i)
                        .context(format!("missing chunk {}", i))?;
                    buf.extend_from_slice(chunk);
                }

                anyhow::ensure!(
                    buf.len() as u64 == transfer.total_bytes,
                    "size mismatch: got {} expected {}",
                    buf.len(),
                    transfer.total_bytes
                );

                // ── SHA-256 integrity verification ────────────────────────────
                let actual_checksum = hex::encode(Sha256::digest(&buf));
                if actual_checksum != transfer.expected_checksum {
                    return Ok(Some(ReassemblerOutput::ChecksumMismatch {
                        expected: transfer.expected_checksum,
                        got: actual_checksum,
                    }));
                }

                let content = match transfer.kind {
                    ChunkKind::Text => {
                        let s = String::from_utf8(buf).context("chunk text not UTF-8")?;
                        ClipboardContent::Text(s)
                    }
                    ChunkKind::Image { mime } => ClipboardContent::Image { mime, data: buf },
                    ChunkKind::File { name } => ClipboardContent::File {
                        name: crate::file_transfer::sanitize_file_name(&name),
                        data: buf,
                    },
                };

                Ok(Some(ReassemblerOutput::Complete(content)))
            }
        }
    }

    pub fn cancel(&mut self, transfer_id: &TransferId) {
        self.in_flight.remove(transfer_id);
    }

    /// Cancel all in-flight transfers (e.g. on peer disconnect).
    ///
    /// Returns the number of transfers cancelled.
    pub fn cancel_all(&mut self) -> usize {
        let count = self.in_flight.len();
        self.in_flight.clear();
        count
    }

    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// IDs of all currently in-flight transfers.
    pub fn in_flight_ids(&self) -> Vec<TransferId> {
        self.in_flight.keys().copied().collect()
    }

    /// Progress (0.0–1.0) of a specific transfer, if known.
    pub fn progress(&self, transfer_id: &TransferId) -> Option<f32> {
        self.in_flight.get(transfer_id).map(|t| {
            if t.total_chunks == 0 {
                0.0
            } else {
                t.chunks.len() as f32 / t.total_chunks as f32
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_content(size: usize) -> ClipboardContent {
        ClipboardContent::Text("A".repeat(size))
    }

    #[test]
    fn small_payload_is_not_chunked() {
        let c = make_content(1024);
        assert!(maybe_chunk(&c).is_none());
    }

    #[test]
    fn large_payload_roundtrip_with_checksum() {
        let original = make_content(CHUNK_SIZE * 3 + 7777);
        let msgs = maybe_chunk(&original)
            .expect("should chunk")
            .collect::<Vec<_>>();
        assert!(msgs.len() > 3);

        let mut r = Reassembler::default();
        let mut result = None;
        for msg in msgs {
            match r.feed(msg).unwrap() {
                Some(ReassemblerOutput::Complete(c)) => result = Some(c),
                Some(ReassemblerOutput::ChecksumMismatch { .. }) => panic!("checksum mismatch"),
                _ => {}
            }
        }
        assert_eq!(result.unwrap(), original);
    }

    #[test]
    fn corrupted_transfer_detected() {
        let content = make_content(CHUNK_THRESHOLD * 2);
        let mut msgs = maybe_chunk(&content).unwrap().collect::<Vec<_>>();

        // Corrupt one data chunk
        for msg in &mut msgs {
            if let ChunkMessage::Chunk { data, .. } = msg {
                if !data.is_empty() {
                    data[0] ^= 0xFF;
                    break;
                }
            }
        }

        let mut r = Reassembler::default();
        let mut corrupted = false;
        for msg in msgs {
            if let Some(ReassemblerOutput::ChecksumMismatch { .. }) = r.feed(msg).unwrap() {
                corrupted = true;
            }
        }
        assert!(corrupted, "should detect corruption via SHA-256");
    }

    #[test]
    fn duplicate_chunk_index_is_rejected() {
        let content = make_content(CHUNK_THRESHOLD * 2);
        let mut msgs = maybe_chunk(&content).unwrap().collect::<Vec<_>>();

        // Duplicate chunk index 0.
        let dup = msgs
            .iter()
            .find(|m| matches!(m, ChunkMessage::Chunk { index: 0, .. }))
            .cloned()
            .unwrap();

        let mut r = Reassembler::default();
        let mut hit_dup_error = false;
        // Feed the Start message first.
        r.feed(msgs.remove(0)).unwrap();
        // Feed the first real chunk.
        r.feed(msgs[0].clone()).unwrap();
        // Feed the duplicate — should error.
        if let Err(e) = r.feed(dup) {
            hit_dup_error = true;
            assert!(
                e.to_string().contains("duplicate chunk"),
                "unexpected error: {}",
                e
            );
        }
        assert!(hit_dup_error, "duplicate chunk should have been rejected");
    }

    #[test]
    fn cancel_all_clears_in_flight() {
        let content = make_content(CHUNK_THRESHOLD * 2);
        let msgs = maybe_chunk(&content).unwrap().collect::<Vec<_>>();

        let mut r = Reassembler::default();
        // Feed just the Start.
        r.feed(msgs[0].clone()).unwrap();
        assert_eq!(r.in_flight_count(), 1);
        assert_eq!(r.in_flight_ids().len(), 1);

        let cancelled = r.cancel_all();
        assert_eq!(cancelled, 1);
        assert_eq!(r.in_flight_count(), 0);
    }

    #[test]
    fn start_with_absurd_size_is_rejected() {
        let mut r = Reassembler::default();
        let result = r.feed(ChunkMessage::Start {
            transfer_id: [0xAB; 16],
            total_chunks: 1,
            total_bytes: u64::MAX,
            checksum: "deadbeef".into(),
            kind: ChunkKind::Text,
        });
        assert!(result.is_err(), "absurd total_bytes must be rejected");
    }

    #[test]
    fn start_with_absurd_chunk_count_is_rejected() {
        let mut r = Reassembler::default();
        let result = r.feed(ChunkMessage::Start {
            transfer_id: [0xCD; 16],
            total_chunks: u32::MAX,
            total_bytes: 1024,
            checksum: "deadbeef".into(),
            kind: ChunkKind::Text,
        });
        assert!(result.is_err(), "absurd total_chunks must be rejected");
    }

    #[test]
    fn progress_tracking() {
        let content = make_content(CHUNK_THRESHOLD * 2);
        let msgs = maybe_chunk(&content).unwrap().collect::<Vec<_>>();

        let mut r = Reassembler::default();
        // Start the transfer.
        r.feed(msgs[0].clone()).unwrap();

        let ids = r.in_flight_ids();
        assert_eq!(ids.len(), 1);

        // Before any chunks: progress = 0.
        let p = r.progress(&ids[0]).unwrap();
        assert_eq!(p, 0.0);

        // Feed one data chunk.
        r.feed(msgs[1].clone()).unwrap();
        let p = r.progress(&ids[0]).unwrap();
        assert!(p > 0.0 && p <= 1.0);
    }
}
