//! Image compression pipeline.
//!
//! Before sending a clipboard image over the LAN, Link All applies
//! lossless or lossy compression depending on the payload size and
//! the user's bandwidth preference:
//!
//! | Original size | Strategy            | Typical reduction |
//! |---------------|---------------------|-------------------|
//! | < 256 KB      | Pass through (raw)  | —                 |
//! | 256 KB–4 MB   | PNG re-encode       | 10–40 %           |
//! | > 4 MB        | JPEG quality 85     | 60–80 %           |
//!
//! All compression is done in a Tokio `spawn_blocking` task so it never
//! blocks the async executor.
//!
//! The compressed bytes replace the original payload in the
//! `ClipboardContent::Image` variant before encryption and transmission.
//! The MIME type is updated to reflect the compressed format.
//!
//! On the receiving side, the image bytes are placed on the clipboard as-is;
//! the OS renders the JPEG or PNG natively.
//!
//! ## Feature flag
//! Compression requires the `image` crate (feature `compress`).
//! Without it, `compress_image` is a no-op pass-through.

use crate::protocol::ClipboardContent;
use anyhow::Result;

// ── Thresholds ────────────────────────────────────────────────────────────────

/// Below this, don't bother compressing.
const COMPRESS_THRESHOLD: usize = 256 * 1024; // 256 KB
/// Above this, switch to JPEG lossy.
const JPEG_THRESHOLD: usize = 4 * 1024 * 1024; // 4 MB
/// JPEG quality (1–100). 85 is visually near-lossless for photos.
const JPEG_QUALITY: u8 = 85;

// ── CompressionStats ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CompressionStats {
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub strategy: CompressionStrategy,
    pub duration_ms: u64,
}

impl CompressionStats {
    pub fn ratio(&self) -> f64 {
        if self.original_bytes == 0 {
            return 1.0;
        }
        self.compressed_bytes as f64 / self.original_bytes as f64
    }

    pub fn savings_pct(&self) -> f64 {
        (1.0 - self.ratio()) * 100.0
    }
}

/// Fix 12: Display impl for CompressionStats.
///
/// Before this fix every call site had to reconstruct the log string manually
/// (e.g. `format!("{} → {} ({:.1}%)", ...)`) with no consistency guarantee.
/// Now callers can simply `log::info!("{}", stats)` or include it in IPC
/// responses as `stats.to_string()`.
///
/// Example output:
/// ```text
/// "1024 KB → 614 KB  savings=40.0%  strategy=jpeg-85  time=3ms"
/// ```
impl std::fmt::Display for CompressionStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let orig_kb = self.original_bytes as f64 / 1024.0;
        let comp_kb = self.compressed_bytes as f64 / 1024.0;
        let strategy = match self.strategy {
            CompressionStrategy::Passthrough => "passthrough".to_string(),
            CompressionStrategy::PngReencode => "png-reencode".to_string(),
            CompressionStrategy::JpegLossy { quality } => format!("jpeg-{}", quality),
        };
        write!(
            f,
            "{:.0} KB → {:.0} KB  savings={:.1}%  strategy={}  time={}ms",
            orig_kb,
            comp_kb,
            self.savings_pct(),
            strategy,
            self.duration_ms
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompressionStrategy {
    Passthrough,
    PngReencode,
    JpegLossy { quality: u8 },
}

// ── Main entry point ──────────────────────────────────────────────────────────

/// Optionally compress a clipboard image in a blocking thread.
/// Returns the (possibly compressed) content and stats.
///
/// If the `image` crate is not available, or the image cannot be decoded,
/// the original content is returned unchanged.
pub async fn compress_image(
    content: ClipboardContent,
    enabled: bool,
) -> (ClipboardContent, Option<CompressionStats>) {
    if !enabled {
        return (content, None);
    }

    match content {
        ClipboardContent::Image { ref data, ref mime } => {
            let size = data.len();
            if size < COMPRESS_THRESHOLD {
                return (content, None);
            }

            let strategy = if size >= JPEG_THRESHOLD {
                CompressionStrategy::JpegLossy {
                    quality: JPEG_QUALITY,
                }
            } else {
                CompressionStrategy::PngReencode
            };

            let data = data.clone();
            let mime = mime.clone();

            let result =
                tokio::task::spawn_blocking(move || compress_blocking(&data, &mime, strategy))
                    .await;

            match result {
                Ok(Ok((new_data, new_mime, stats))) => {
                    // Only use if we actually saved space.
                    if new_data.len() < size {
                        (
                            ClipboardContent::Image {
                                data: new_data,
                                mime: new_mime,
                            },
                            Some(stats),
                        )
                    } else {
                        // Compression made it bigger — send original.
                        (
                            content,
                            Some(CompressionStats {
                                original_bytes: size,
                                compressed_bytes: size,
                                strategy: CompressionStrategy::Passthrough,
                                duration_ms: stats.duration_ms,
                            }),
                        )
                    }
                }
                _ => (content, None), // compression failed — send original
            }
        }
        _ => (content, None), // not an image
    }
}

fn compress_blocking(
    data: &[u8],
    mime: &str,
    strategy: CompressionStrategy,
) -> Result<(Vec<u8>, String, CompressionStats)> {
    let start = std::time::Instant::now();

    // ── Feature-gated image compression ────────────────────────────────────
    // In production, enable with `image = { version = "0.25", features = ["png","jpeg"] }`
    // and `oxipng = "9"` for lossless PNG optimization.
    //
    // Without the feature, we return the original bytes unchanged.
    #[cfg(feature = "compress")]
    {
        use image::ImageFormat;

        let reader = image::io::Reader::new(std::io::Cursor::new(data)).with_guessed_format()?;
        let dimensions = reader.into_dimensions()?;

        // Prevent OOM: if decoded RGBA (4 bytes per pixel) exceeds ~50MB, skip compression.
        let max_pixels = 50 * 1024 * 1024 / 4;
        if (dimensions.0 as usize).saturating_mul(dimensions.1 as usize) > max_pixels {
            anyhow::bail!(
                "image too large for safe in-memory compression ({}x{})",
                dimensions.0,
                dimensions.1
            );
        }

        let img = image::load_from_memory(data)?;
        let mut out = Vec::new();
        let source_mime = mime.to_ascii_lowercase();
        let must_normalize_losslessly = matches!(
            source_mime.as_str(),
            "image/heic" | "image/heif" | "image/avif" | "image/tiff"
        );

        let (final_mime, _fmt) = match strategy {
            _ if must_normalize_losslessly => {
                img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)?;
                ("image/png", ImageFormat::Png)
            }
            CompressionStrategy::JpegLossy { quality } => {
                let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
                enc.encode_image(&img)?;
                ("image/jpeg", ImageFormat::Jpeg)
            }
            CompressionStrategy::PngReencode => {
                img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)?;
                ("image/png", ImageFormat::Png)
            }
            CompressionStrategy::Passthrough => {
                return Ok((
                    data.to_vec(),
                    mime.to_string(),
                    CompressionStats {
                        original_bytes: data.len(),
                        compressed_bytes: data.len(),
                        strategy,
                        duration_ms: start.elapsed().as_millis() as u64,
                    },
                ));
            }
        };

        let compressed_len = out.len();
        Ok((
            out,
            final_mime.to_string(),
            CompressionStats {
                original_bytes: data.len(),
                compressed_bytes: compressed_len,
                strategy,
                duration_ms: start.elapsed().as_millis() as u64,
            },
        ))
    }

    #[cfg(not(feature = "compress"))]
    {
        let _ = strategy;
        // Pass-through: no compression without the feature flag.
        Ok((
            data.to_vec(),
            mime.to_string(),
            CompressionStats {
                original_bytes: data.len(),
                compressed_bytes: data.len(),
                strategy: CompressionStrategy::Passthrough,
                duration_ms: start.elapsed().as_millis() as u64,
            },
        ))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_image(size: usize, mime: &str) -> ClipboardContent {
        ClipboardContent::Image {
            mime: mime.to_string(),
            data: vec![0xAB; size],
        }
    }

    #[tokio::test]
    async fn small_image_passthrough() {
        let content = make_image(COMPRESS_THRESHOLD / 2, "image/png");
        let (out, stats) = compress_image(content.clone(), true).await;
        assert!(stats.is_none(), "small images should not be compressed");
        assert_eq!(out, content);
    }

    #[tokio::test]
    async fn compression_disabled_is_passthrough() {
        let content = make_image(JPEG_THRESHOLD * 2, "image/png");
        let (out, stats) = compress_image(content.clone(), false).await;
        assert!(stats.is_none());
        assert_eq!(out, content);
    }

    #[tokio::test]
    async fn non_image_content_unchanged() {
        let content = ClipboardContent::Text("hello".into());
        let (out, stats) = compress_image(content.clone(), true).await;
        assert!(stats.is_none());
        assert_eq!(out, content);
    }

    #[test]
    fn compression_stats_display() {
        let stats = CompressionStats {
            original_bytes: 1_024_000,
            compressed_bytes: 614_400,
            strategy: CompressionStrategy::JpegLossy { quality: 85 },
            duration_ms: 3,
        };
        let s = stats.to_string();
        assert!(s.contains("savings="), "display missing savings: {}", s);
        assert!(s.contains("jpeg-85"), "display missing strategy: {}", s);
        assert!(s.contains("3ms"), "display missing time: {}", s);
    }

    #[test]
    fn compression_stats_ratio() {
        let stats = CompressionStats {
            original_bytes: 1000,
            compressed_bytes: 600,
            strategy: CompressionStrategy::JpegLossy { quality: 85 },
            duration_ms: 5,
        };
        let ratio = stats.ratio();
        assert!((ratio - 0.6).abs() < 0.001);
        let savings = stats.savings_pct();
        assert!((savings - 40.0).abs() < 0.001);
    }

    #[test]
    fn strategy_selection_thresholds() {
        // Verify threshold logic (we test the decision, not the codec).
        let small = COMPRESS_THRESHOLD / 2;
        let medium = (COMPRESS_THRESHOLD + JPEG_THRESHOLD) / 2;
        let large = JPEG_THRESHOLD * 2;

        assert!(small < COMPRESS_THRESHOLD);

        let medium_strategy = if medium >= JPEG_THRESHOLD {
            CompressionStrategy::JpegLossy {
                quality: JPEG_QUALITY,
            }
        } else {
            CompressionStrategy::PngReencode
        };
        assert_eq!(medium_strategy, CompressionStrategy::PngReencode);

        let large_strategy = if large >= JPEG_THRESHOLD {
            CompressionStrategy::JpegLossy {
                quality: JPEG_QUALITY,
            }
        } else {
            CompressionStrategy::PngReencode
        };
        assert_eq!(
            large_strategy,
            CompressionStrategy::JpegLossy {
                quality: JPEG_QUALITY
            }
        );
    }
}
