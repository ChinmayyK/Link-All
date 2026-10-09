use super::*;

pub(crate) async fn read_outbound_chunks(
    shared: crate::engine::EngineShared,
    transfer_id: [u8; 16],
    batch_size: usize,
    send_run: u32,
) -> Option<(
    Vec<AppMessage>,
    Vec<(crate::file_transfer::TransferProgress, String)>,
)> {
    let mut instrs = Vec::with_capacity(batch_size);
    let io_ctx;
    let rehash_bytes;

    {
        let mut mgr = shared.file_transfers.lock().await;
        let t = mgr.get_outbound_mut(&transfer_id)?;
        // A newer send loop owns this transfer (it resumed after a reconnect).
        if !t.is_send_run(send_run) {
            return None;
        }
        io_ctx = t.take_io_context();
        rehash_bytes = t.take_rehash_bytes();
        let effective_batch = t.adaptive_batch_size(batch_size);
        for _ in 0..effective_batch {
            match t.next_chunk_instruction() {
                Ok(Some(i)) => instrs.push(i),
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to get next chunk instruction");
                    mgr.cancel_outbound(&transfer_id);
                    return None;
                }
            }
        }
    }

    if instrs.is_empty() {
        if let Some((f, h)) = io_ctx {
            if let Some(t) = shared
                .file_transfers
                .lock()
                .await
                .get_outbound_mut(&transfer_id)
            {
                t.restore_io_context(f, h);
            }
        }
        return None;
    }

    type FileChunkResult = anyhow::Result<(
        Option<(Option<std::fs::File>, sha2::Sha256)>,
        // (chunk_index, data, compressed, sample_result)
        // sample_result is None when compression wasn't attempted at all for
        // this chunk (either extension-excluded or the transfer already gave
        // up on compression), Some(true/false) otherwise.
        Vec<(u32, Vec<u8>, bool, Option<bool>)>,
        // Checksum state after each chunk, see OutboundTransfer::record_hash_checkpoint.
        Vec<(u32, sha2::Sha256)>,
    )>;

    // Determine if we should try LZ4 based on file extension, and whether
    // this transfer has already proven (over a streak of prior chunks) that
    // its content doesn't compress — in which case we skip the per-chunk
    // sample entirely instead of burning CPU on every chunk forever.
    let try_compress = {
        let mgr = shared.file_transfers.lock().await;
        mgr.get_outbound(&transfer_id)
            .map(|t| {
                should_try_compress(&t.meta.file_name)
                    && t.compression_verdict
                        != crate::file_transfer::CompressionVerdict::SkipRestOfTransfer
            })
            .unwrap_or(true)
    };

    let res = tokio::task::spawn_blocking(move || -> FileChunkResult {
        use sha2::Digest;
        use std::io::{Read, Seek};

        let mut chunk_data = Vec::with_capacity(instrs.len());
        let mut checkpoints = Vec::with_capacity(instrs.len());
        let (mut f, mut hasher) = io_ctx.unwrap_or((None, sha2::Sha256::new())); // Memory chunks might not have io_ctx, but we'll return it anyway

        // After a resume, hash the delivered prefix before reading on.
        if rehash_bytes > 0 {
            if f.is_none() {
                if let Some(crate::file_transfer::ChunkInstruction::File { path, .. }) =
                    instrs.first()
                {
                    f = Some(std::fs::File::open(path)?);
                }
            }
            if let Some(file) = f.as_mut() {
                file.seek(std::io::SeekFrom::Start(0))?;
                let mut buf = vec![0u8; 1 << 20];
                let mut left = rehash_bytes;
                while left > 0 {
                    let want = (left as usize).min(buf.len());
                    let n = file.read(&mut buf[..want])?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                    left -= n as u64;
                }
            }
        }

        for instr in instrs {
            match instr {
                crate::file_transfer::ChunkInstruction::Memory { chunk_index, data } => {
                    hasher.update(&data);
                    checkpoints.push((chunk_index, hasher.clone()));
                    let sample_result = if try_compress {
                        let sample_len = data.len().min(4096);
                        if sample_len > 0 {
                            let sample = &data[..sample_len];
                            let mut c_sample = [0u8; 4096 + 32];
                            let c_len = lz4_flex::block::compress_into(sample, &mut c_sample)
                                .unwrap_or(usize::MAX);
                            Some(c_len < sample_len * 95 / 100)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if sample_result == Some(true) {
                        let compressed = lz4_flex::compress_prepend_size(&data);
                        if compressed.len() < data.len() {
                            chunk_data.push((chunk_index, compressed, true, sample_result));
                        } else {
                            chunk_data.push((chunk_index, data.to_vec(), false, sample_result));
                        }
                    } else {
                        chunk_data.push((chunk_index, data.to_vec(), false, sample_result));
                    }
                }
                crate::file_transfer::ChunkInstruction::File {
                    chunk_index,
                    path,
                    offset,
                    len,
                } => {
                    if f.is_none() {
                        f = Some(std::fs::File::open(&path)?);
                    }
                    if let Some(ref mut file) = f {
                        let current_pos = file.stream_position().unwrap_or(u64::MAX);
                        if current_pos != offset {
                            file.seek(std::io::SeekFrom::Start(offset))?;
                        }
                        let mut buf = crate::network::get_buffer(len);
                        let mut read_bytes = 0;
                        while read_bytes < len {
                            let n = file.read(&mut buf[read_bytes..])?;
                            if n == 0 {
                                break;
                            }
                            read_bytes += n;
                        }
                        // `len` comes from the size announced at the start, so a short
                        // read means the file shrank since. Sending the short chunk only
                        // fails the receiver's size/checksum check after the whole file.
                        if read_bytes < len {
                            anyhow::bail!(
                                "file was modified or truncated on disk during transfer \
                                 (chunk {}: read {} of {} bytes)",
                                chunk_index,
                                read_bytes,
                                len
                            );
                        }
                        hasher.update(&buf);
                        checkpoints.push((chunk_index, hasher.clone()));
                        let sample_result = if try_compress {
                            let sample_len = buf.len().min(4096);
                            if sample_len > 0 {
                                let sample = &buf[..sample_len];
                                let mut c_sample = [0u8; 4096 + 32];
                                let c_len = lz4_flex::block::compress_into(sample, &mut c_sample)
                                    .unwrap_or(usize::MAX);
                                Some(c_len < sample_len * 95 / 100)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        if sample_result == Some(true) {
                            let compressed = lz4_flex::compress_prepend_size(&buf);
                            if compressed.len() < buf.len() {
                                chunk_data.push((chunk_index, compressed, true, sample_result));
                            } else {
                                chunk_data.push((chunk_index, buf, false, sample_result));
                            }
                        } else {
                            chunk_data.push((chunk_index, buf, false, sample_result));
                        }
                    }
                }
            }
        }
        Ok((Some((f, hasher)), chunk_data, checkpoints))
    })
    .await
    .unwrap();

    let (io_ctx, chunk_data, checkpoints) = match res {
        Ok(res) => res,
        Err(e) => {
            tracing::warn!(error = %e, "failed to read outbound file chunks");
            let target = {
                let mut mgr = shared.file_transfers.lock().await;
                let target = mgr.get_outbound(&transfer_id).and_then(|t| t.target_device);
                mgr.cancel_outbound(&transfer_id);
                target
            };
            // Tell the receiver now instead of letting it wait for chunks that never come.
            let reason = e.to_string();
            if let Some(peer) = target {
                if let Some(tx) = shared.peer_manager.sender(peer) {
                    let _ = tx
                        .send(AppMessage::FileTransferCancel {
                            transfer_id,
                            reason: reason.clone(),
                        })
                        .await;
                }
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::FileTransferFailed {
                    in_folder: false,
                    transfer_id,
                    from_device: target.unwrap_or_default(),
                    reason,
                })
                .await;
            return None;
        }
    };

    let mut msgs = Vec::with_capacity(chunk_data.len());
    let mut progs = Vec::with_capacity(chunk_data.len());

    {
        let mut mgr = shared.file_transfers.lock().await;
        let t = mgr.get_outbound_mut(&transfer_id)?;
        if let Some((f, h)) = io_ctx {
            t.restore_io_context(f, h);
        }
        for (chunk, state) in checkpoints {
            t.record_hash_checkpoint(chunk, state);
        }
        let fname = t.meta.file_name.clone();
        for (c_idx, data, compressed, sample_result) in chunk_data {
            if let Some(compressed_well) = sample_result {
                t.record_compression_sample(compressed_well);
            }
            let msg = t.process_chunk_data(c_idx, data, compressed);
            if let crate::file_transfer::FileTransferMessage::Chunk {
                transfer_id,
                chunk_index,
                total_chunks,
                data,
                compressed,
            } = msg
            {
                msgs.push(AppMessage::FileChunk {
                    transfer_id,
                    chunk_index,
                    total_chunks,
                    data,
                    compressed,
                });
                progs.push((t.progress(), fname.clone()));
            }
        }
    }

    if msgs.is_empty() {
        None
    } else {
        Some((msgs, progs))
    }
}

/// Returns false for file extensions that are already compressed.
/// LZ4 on these formats wastes CPU and always produces larger output.
fn should_try_compress(file_name: &str) -> bool {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    !matches!(
        ext.as_str(),
        "jpg"
            | "jpeg"
            | "png"
            | "gif"
            | "webp"
            | "avif"
            | "heic"
            | "heif"
            | "mp4"
            | "mkv"
            | "mov"
            | "avi"
            | "webm"
            | "mp3"
            | "aac"
            | "ogg"
            | "opus"
            | "flac"
            | "m4a"
            | "wma"
            | "zip"
            | "gz"
            | "bz2"
            | "xz"
            | "zst"
            | "lz4"
            | "7z"
            | "rar"
            | "tar.gz"
            | "tgz"
            | "apk"
            | "ipa"
            | "dmg"
            | "iso"
            | "pdf"
    )
}
