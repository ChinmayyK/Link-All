// Link All benchmarks
//
// Run:  cargo bench
// HTML: target/criterion/report/index.html

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use linkall_core::{
    chunked::{maybe_chunk, Reassembler},
    crypto::EphemeralKeypair,
    dedup::{hash_content, Deduplicator},
    protocol::ClipboardContent,
};

// ── Crypto benchmarks ─────────────────────────────────────────────────────────

fn bench_handshake(c: &mut Criterion) {
    c.bench_function("x25519_ecdh_hkdf", |b| {
        b.iter(|| {
            let alice = EphemeralKeypair::generate();
            let bob = EphemeralKeypair::generate();
            let bob_pub = bob.public_bytes;
            let (_alice_sess, _, _) = alice.derive_session_key(black_box(bob_pub), true).unwrap();
        })
    });
}

fn bench_encryption(c: &mut Criterion) {
    let mut group = c.benchmark_group("chacha20_poly1305");

    for size in [1_024usize, 64_000, 1_024_000, 4_096_000] {
        let payload = vec![0u8; size];
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("encrypt", size), &payload, |b, payload| {
            let alice = EphemeralKeypair::generate();
            let bob = EphemeralKeypair::generate();
            let bob_pub = bob.public_bytes;
            let (mut sess, _, _) = alice.derive_session_key(bob_pub, true).unwrap();
            b.iter(|| sess.encrypt(black_box(payload)).unwrap())
        });

        group.bench_with_input(
            BenchmarkId::new("encrypt_decrypt", size),
            &payload,
            |b, payload| {
                let alice = EphemeralKeypair::generate();
                let bob = EphemeralKeypair::generate();
                let _a_pub = alice.public_bytes;
                let b_pub = bob.public_bytes;
                let (_send, _, _) = alice.derive_session_key(b_pub, true).unwrap();
                let alice2 = EphemeralKeypair::generate();
                let bob2 = EphemeralKeypair::generate();
                let a2_pub = alice2.public_bytes;
                let b2_pub = bob2.public_bytes;
                let (mut recv, _, _) = bob2.derive_session_key(a2_pub, false).unwrap();
                let (mut send2, _, _) = alice2.derive_session_key(b2_pub, true).unwrap();

                b.iter(|| {
                    let ct = send2.encrypt(black_box(payload)).unwrap();
                    let _ = recv.decrypt(black_box(&ct)).unwrap();
                })
            },
        );
    }
    group.finish();
}

fn bench_large_transfers(c: &mut Criterion) {
    let mut group = c.benchmark_group("simulated_large_transfers");
    group.sample_size(10); // Keep iterations low for large benchmarks

    // We simulate 100MB, 1GB, 5GB, 20GB by processing chunks of 512KB in a loop
    for gb_size in [0.1, 1.0, 5.0, 20.0] {
        let size_bytes = (gb_size * 1024.0 * 1024.0 * 1024.0) as u64;
        let chunk_size = 512 * 1024;
        let num_chunks = size_bytes / chunk_size;

        let chunk_data = vec![0u8; chunk_size as usize];

        group.throughput(Throughput::Bytes(size_bytes));
        group.bench_with_input(
            BenchmarkId::new("encrypt_stream", format!("{}GB", gb_size)),
            &chunk_data,
            |b, data| {
                let alice = EphemeralKeypair::generate();
                let bob = EphemeralKeypair::generate();
                let (mut sess, _, _) = alice.derive_session_key(bob.public_bytes, true).unwrap();

                b.iter(|| {
                    for _ in 0..num_chunks {
                        let _ = sess.encrypt(black_box(data)).unwrap();
                    }
                })
            },
        );
    }
    group.finish();
}

fn bench_concurrent_transfers(c: &mut Criterion) {
    let mut group = c.benchmark_group("simulated_concurrent_transfers");
    group.sample_size(10);

    for num_concurrent in [1, 5, 10, 20] {
        let size_per_transfer = 40 * 1024 * 1024; // 40 MB
        let chunk_size = 512 * 1024;
        let num_chunks = size_per_transfer / chunk_size;

        let chunk_data = vec![0u8; chunk_size];

        group.throughput(Throughput::Bytes(
            (size_per_transfer * num_concurrent) as u64,
        ));
        group.bench_with_input(
            BenchmarkId::new("concurrent_40MB_encrypt", num_concurrent),
            &chunk_data,
            |b, data| {
                let alice = EphemeralKeypair::generate();
                let bob = EphemeralKeypair::generate();

                b.iter(|| {
                    // Simulate multiple active sessions encrypting at the same time
                    let mut sessions = (0..num_concurrent)
                        .map(|_| alice.derive_session_key(bob.public_bytes, true).unwrap().0)
                        .collect::<Vec<_>>();

                    for _ in 0..num_chunks {
                        for sess in sessions.iter_mut() {
                            let _ = sess.encrypt(black_box(data)).unwrap();
                        }
                    }
                })
            },
        );
    }
    group.finish();
}

// ── Content hash ──────────────────────────────────────────────────────────────

fn bench_content_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("content_hash_sha256");
    for size in [256usize, 8_192, 65_536, 1_048_576] {
        let content = ClipboardContent::Text("A".repeat(size));
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &content, |b, c| {
            b.iter(|| hash_content(black_box(c)))
        });
    }
    group.finish();
}

// ── Chunked transfer ──────────────────────────────────────────────────────────

fn bench_chunk_and_reassemble(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunked_transfer");

    for mb in [1u64, 4, 16, 32] {
        let size = (mb * 1024 * 1024) as usize;
        let content = ClipboardContent::Image {
            mime: "image/png".into(),
            data: vec![0xAB; size],
        };
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("chunk_and_reassemble_MB", mb),
            &content,
            |b, content| {
                b.iter(|| {
                    let msgs = maybe_chunk(black_box(content)).unwrap();
                    let mut r = Reassembler::default();
                    for msg in msgs {
                        r.feed(msg).unwrap();
                    }
                })
            },
        );
    }
    group.finish();
}

// ── Deduplication ─────────────────────────────────────────────────────────────

fn bench_dedup(c: &mut Criterion) {
    c.bench_function("dedup_should_apply", |b| {
        let mut dedup = Deduplicator::new();
        let content = ClipboardContent::Text("hello world".into());
        let hash = hash_content(&content);
        b.iter(|| dedup.should_apply(uuid::Uuid::new_v4(), black_box(hash)))
    });
}

criterion_group!(
    benches,
    bench_handshake,
    bench_encryption,
    bench_large_transfers,
    bench_concurrent_transfers,
    bench_content_hash,
    bench_chunk_and_reassemble,
    bench_dedup,
);
criterion_main!(benches);
