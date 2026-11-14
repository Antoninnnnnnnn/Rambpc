//! Release-only, synchronous CPU profiling. Allocator instrumentation is confined
//! to this executable; the loopback executable uses the ordinary system allocator.
mod profile_support;

use rambler_protocol::{
    ProtocolConfig, audio_request, context_request, decode_response, initial_request,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    time::Instant,
};

struct CountingAllocator;
static ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static REQUESTED: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);

fn allocated(size: usize, realloc: bool) {
    if ENABLED.load(Relaxed) {
        let live = LIVE.fetch_add(size as u64, Relaxed) + size as u64;
        if realloc {
            REALLOCS.fetch_add(1, Relaxed);
        } else {
            ALLOCS.fetch_add(1, Relaxed);
        }
        REQUESTED.fetch_add(size as u64, Relaxed);
        PEAK.fetch_max(live, Relaxed);
    }
}
// SAFETY: every operation forwards the identical pointer/layout to System. The
// counters allocate nothing. Reallocation changes LIVE only after success.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            allocated(layout.size(), false);
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            allocated(layout.size(), false);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ENABLED.load(Relaxed) {
            LIVE.fetch_sub(layout.size() as u64, Relaxed);
        }
        unsafe {
            System.dealloc(ptr, layout);
        }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, size) };
        if !new.is_null() {
            if ENABLED.load(Relaxed) {
                LIVE.fetch_sub(layout.size() as u64, Relaxed);
            }
            allocated(size, true);
        }
        new
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn bench(name: &str, iterations: usize, repeats: usize, units: usize, run: impl FnMut() -> usize) {
    bench_at(name, iterations, repeats, units, 0, run);
}

fn bench_at(
    name: &str,
    iterations: usize,
    repeats: usize,
    units: usize,
    repeat_base: usize,
    mut run: impl FnMut() -> usize,
) {
    for _ in 0..128 {
        black_box(run());
    }
    for repeat in 0..repeats {
        let start = Instant::now();
        let mut bytes = 0u64;
        for _ in 0..iterations {
            bytes += black_box(run()) as u64;
        }
        let nanos = start.elapsed().as_nanos();
        println!(
            "{}",
            serde_json::json!({"kind":"wall", "case":name, "repeat":repeat + repeat_base,
            "iterations":iterations, "units_per_iteration":units, "protobuf_bytes":bytes,
            "wall_ns":nanos, "ns_per_unit":nanos as f64 / (iterations * units) as f64})
        );
    }
    ALLOCS.store(0, Relaxed);
    REALLOCS.store(0, Relaxed);
    REQUESTED.store(0, Relaxed);
    let baseline = LIVE.load(Relaxed);
    PEAK.store(baseline, Relaxed);
    ENABLED.store(true, Relaxed);
    for _ in 0..iterations {
        black_box(run());
    }
    ENABLED.store(false, Relaxed);
    let allocations = ALLOCS.load(Relaxed);
    let reallocations = REALLOCS.load(Relaxed);
    let requested = REQUESTED.load(Relaxed);
    let peak = PEAK.load(Relaxed).saturating_sub(baseline);
    assert_eq!(LIVE.load(Relaxed), baseline, "case leaked live allocations");
    println!(
        "{}",
        serde_json::json!({"kind":"allocations", "case":name,
        "iterations":iterations,"units_per_iteration":units, "alloc_calls":allocations,
        "realloc_calls":reallocations, "requested_bytes":requested, "peak_increment_bytes":peak})
    );
}

// Runtime rejection keeps debug test builds possible; timing requires release.
#[allow(clippy::assertions_on_constants)]
fn main() {
    assert!(!cfg!(debug_assertions), "use --release");
    let iterations = profile_support::arg("--iterations", 20_000);
    let repeats = profile_support::arg("--repeats", 5);
    let config = ProtocolConfig::default();
    let context = profile_support::context();
    bench("initial", iterations, repeats, 1, || {
        black_box(initial_request(black_box(&config), 0).unwrap()).len()
    });
    bench("context", iterations, repeats, 1, || {
        black_box(context_request(black_box(&context)).unwrap()).len()
    });
    for size in [640, 4096, 65_536] {
        let chunk = profile_support::audio(7, size);
        assert_eq!(
            audio_request(&chunk, 123_456),
            profile_support::reference_audio_request(&chunk, 123_456)
        );
        for repeat in 0..repeats {
            for offset in 0..2 {
                let reference = (repeat + offset) % 2 == 0;
                let name = if reference {
                    format!("audio_reference_{size}")
                } else {
                    format!("audio_{size}")
                };
                bench_at(&name, iterations, 1, 1, repeat, || {
                    let payload = if reference {
                        profile_support::reference_audio_request(black_box(&chunk), 123_456)
                    } else {
                        audio_request(black_box(&chunk), 123_456)
                    }
                    .unwrap();
                    black_box(payload).len()
                });
            }
        }
    }
    for size in [128, 4096] {
        let raw = profile_support::response(7, size, true);
        let expected = decode_response(&raw).unwrap();
        assert_eq!(expected.raw, raw.as_ref());
        assert_eq!(expected.recognition_state(), Some(1));
        assert_eq!(expected.hypotheses()[0].safety_status, Some(0));
        assert_eq!(expected.unknown_fields.len(), 1);
        assert_eq!(expected.hypotheses()[0].unknown_fields.len(), 1);
        let name = format!("decode_{size}");
        bench(&name, iterations, repeats, 1, || {
            let response = decode_response(black_box(&raw)).unwrap();
            black_box(&response);
            response.raw.len()
        });
        // Exact production-shaped move retention, not a replacement collector.
        // Real collect_session is exercised separately over h2 in the other example.
        let name = format!("decode_retain128_{size}");
        bench(&name, (iterations / 128).max(1), repeats, 128, || {
            let mut updates = Vec::new();
            let mut bytes = 0;
            for _ in 0..128 {
                let response = decode_response(black_box(&raw)).unwrap();
                bytes += response.raw.len();
                assert!(bytes <= 8 * 1024 * 1024);
                updates.push(response);
            }
            black_box(&updates);
            bytes
        });
    }
}
