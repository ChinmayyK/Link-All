use dhat::{Alloc, Profiler};
use linkall_core::chunked::{maybe_chunk, Reassembler};
use linkall_core::protocol::ClipboardContent;

#[global_allocator]
static ALLOC: Alloc = Alloc;

fn main() {
    let _profiler = Profiler::builder().build();

    println!("Benchmarking 200MB Transfer Allocations...");
    let size = 200 * 1024 * 1024; // 200 MB
    let content = ClipboardContent::Image {
        mime: "image/png".into(),
        data: vec![0xAB; size], // Simulate large byte vector
    };

    // Simulate sending chunked payloads
    let msgs = maybe_chunk(&content).unwrap();

    let mut r = Reassembler::default();
    for msg in msgs {
        r.feed(msg).unwrap();
    }

    println!("Reassembly complete. Profiler will output dhat-heap.json on exit.");
}
