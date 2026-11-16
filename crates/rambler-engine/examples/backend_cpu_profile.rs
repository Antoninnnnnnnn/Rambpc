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
static MAX_REQUEST: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
static RETAINED: AtomicU64 = AtomicU64::new(0);

fn allocated(size: usize, realloc: bool) {
    if ENABLED.load(Relaxed) {
        let live = LIVE.fetch_add(size as u64, Relaxed) + size as u64;
        if realloc {
            REALLOCS.fetch_add(1, Relaxed);
        } else {
            ALLOCS.fetch_add(1, Relaxed);
        }
        REQUESTED.fetch_add(size as u64, Relaxed);
        MAX_REQUEST.fetch_max(size as u64, Relaxed);
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
    MAX_REQUEST.store(0, Relaxed);
    let baseline = LIVE.load(Relaxed);
    PEAK.store(baseline, Relaxed);
    RETAINED.store(baseline, Relaxed);
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
        "realloc_calls":reallocations, "requested_bytes":requested, "peak_increment_bytes":peak,
        "max_request_bytes":MAX_REQUEST.load(Relaxed),
        "retained_increment_bytes":RETAINED.load(Relaxed).saturating_sub(baseline)})
    );
}

// Additional allocation/capacity shapes. Fixtures and assertions are outside both
// timing and allocation passes; baseline and candidate use this identical harness.
fn decode_shapes(iterations: usize, repeats: usize) {
    use rambler_protocol::{RECOGNITION_RESPONSE, wire::bytes_field};
    fn blob(number: u32, body: &[u8]) -> Vec<u8> {
        bytes_field(number, body).unwrap()
    }
    fn response(recognition: &[u8]) -> Vec<u8> {
        blob(RECOGNITION_RESPONSE, &blob(1, recognition))
    }
    let mut cases = vec![
        ("empty".to_owned(), Vec::new(), iterations),
        ("status".to_owned(), vec![8, 0], iterations),
        (
            "speech".to_owned(),
            blob(RECOGNITION_RESPONSE, &blob(2, &[8, 0])),
            iterations,
        ),
    ];
    for count in [0, 1, 2, 4, 16, 257] {
        let hypothesis = blob(1, b"candidate e/UTF8");
        let list = blob(3, &hypothesis).repeat(count);
        let raw = response(&blob(3, &list));
        let decoded = decode_response(&raw).unwrap();
        assert_eq!(decoded.hypotheses().len(), count);
        let items = &decoded
            .result
            .as_ref()
            .unwrap()
            .recognition
            .as_ref()
            .unwrap()
            .hypothesis_result
            .as_ref()
            .unwrap()
            .hypotheses;
        println!(
            "{}",
            serde_json::json!({"kind":"shape_census", "count":count,
            "hypotheses_capacity":items.capacity(),
            "hypothesis_size":std::mem::size_of::<rambler_protocol::Hypothesis>(),
            "root_size":std::mem::size_of::<rambler_protocol::S3Response>(),
            "wire_bytes":raw.len()})
        );
        cases.push((
            format!("hypotheses_{count}"),
            raw,
            (iterations / count.max(1)).max(256),
        ));
    }
    let partial = blob(1, &blob(1, b"partial candidate"));
    cases.push((
        "partials_16".into(),
        response(&blob(4, &partial.repeat(16))),
        iterations / 16,
    ));
    let item = blob(3, &blob(1, b"merged candidate"));
    let merged = blob(3, &item).repeat(16);
    let raw = response(&merged);
    assert_eq!(decode_response(&raw).unwrap().hypotheses().len(), 16);
    cases.push(("merged_16".into(), raw, iterations / 16));
    let switches = [blob(1, &[8, 1]), blob(2, &[8, 0])].concat().repeat(8);
    let raw = blob(RECOGNITION_RESPONSE, &switches);
    assert_eq!(decode_response(&raw).unwrap().speech_event(), Some(0));
    cases.push(("oneof_runs_16".into(), raw, iterations / 16));
    cases.push((
        "root_fields_100000".into(),
        [8, 0].repeat(100_000),
        (iterations / 1000).max(16),
    ));
    for (name, raw, iterations) in cases {
        let iterations = iterations.max(1);
        bench(&format!("shape_{name}"), iterations, repeats, 1, || {
            let decoded = decode_response(black_box(&raw)).unwrap();
            black_box(&decoded);
            decoded.raw.len()
        });
    }
}

// Error fixtures deliberately keep all wrappers within the global field budget.
// A bad first child must not speculatively reserve a large output model Vec.
fn decode_errors(iterations: usize, repeats: usize) {
    use rambler_protocol::{Error, RECOGNITION_RESPONSE, wire::bytes_field};
    for (kind, container, child) in [("hypotheses", 3, 3), ("partials", 4, 1)] {
        for (count, valid_prefix) in [(1, 0), (2, 0), (4, 0), (257, 0), (90_000, 0), (90_000, 1)] {
            let good = bytes_field(child, b"").unwrap();
            let bad = bytes_field(child, &[0]).unwrap();
            let list = [good.repeat(valid_prefix), bad.repeat(count - valid_prefix)].concat();
            let recognition = bytes_field(container, &list).unwrap();
            let result = bytes_field(1, &recognition).unwrap();
            let raw = bytes_field(RECOGNITION_RESPONSE, &result).unwrap();
            assert_eq!(decode_response(&raw), Err(Error::FieldNumber));
            let name = format!("error_{kind}_{count}_prefix{valid_prefix}");
            bench(&name, iterations.min(32), repeats, 1, || {
                let decoded = decode_response(black_box(&raw));
                assert_eq!(black_box(&decoded), &Err(Error::FieldNumber));
                raw.len()
            });
        }
    }
}

fn decode_sparse(iterations: usize, repeats: usize) {
    use rambler_protocol::{RECOGNITION_RESPONSE, wire::bytes_field};
    for count in [1, 2] {
        let child = bytes_field(3, &bytes_field(1, b"candidate").unwrap()).unwrap();
        let list = [
            child.clone(),
            [0x98, 0x06, 0].repeat(20_000),
            child.repeat(count - 1),
        ]
        .concat();
        let recognition = bytes_field(3, &list).unwrap();
        let result = bytes_field(1, &recognition).unwrap();
        let raw = bytes_field(RECOGNITION_RESPONSE, &result).unwrap();
        let decoded = decode_response(&raw).unwrap();
        assert_eq!(decoded.hypotheses().len(), count);
        assert_eq!(
            decoded
                .result
                .as_ref()
                .unwrap()
                .recognition
                .as_ref()
                .unwrap()
                .hypothesis_result
                .as_ref()
                .unwrap()
                .unknown_fields
                .len(),
            20_000
        );
        bench(
            &format!("sparse_{count}_unknown20000"),
            iterations.min(64),
            repeats,
            1,
            || {
                let decoded = decode_response(black_box(&raw)).unwrap();
                black_box(&decoded);
                decoded.raw.len()
            },
        );
    }
}

// Runtime rejection keeps debug test builds possible; timing requires release.
#[allow(clippy::assertions_on_constants)]
fn main() {
    assert!(!cfg!(debug_assertions), "use --release");
    let iterations = profile_support::arg("--iterations", 20_000);
    let repeats = profile_support::arg("--repeats", 5);
    let decode_only = std::env::args().any(|arg| arg == "--decode-only");
    if std::env::args().any(|arg| arg == "--decode-sparse") {
        decode_sparse(iterations, repeats);
        return;
    }
    if std::env::args().any(|arg| arg == "--decode-errors") {
        decode_errors(iterations, repeats);
        return;
    }
    if std::env::args().any(|arg| arg == "--decode-shapes") {
        decode_shapes(iterations, repeats);
        return;
    }
    if !decode_only {
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
    }
    for size in [128, 4096] {
        let raw = profile_support::response(7, size, true);
        let expected = decode_response(&raw).unwrap();
        assert_eq!(expected.raw, raw.as_ref());
        assert_eq!(expected.recognition_state(), Some(1));
        assert_eq!(expected.hypotheses()[0].safety_status, Some(0));
        assert_eq!(expected.unknown_fields.len(), 1);
        assert_eq!(expected.hypotheses()[0].unknown_fields.len(), 1);
        let result = expected.result.as_ref().unwrap();
        let recognition = result.recognition.as_ref().unwrap();
        let list = recognition.hypothesis_result.as_ref().unwrap();
        let hypothesis = &list.hypotheses[0];
        let edit = hypothesis.voice_edit.as_ref().unwrap();
        let unknown_heap: usize = [&expected.unknown_fields, &hypothesis.unknown_fields]
            .into_iter()
            .map(|fields| {
                fields.capacity() * std::mem::size_of::<rambler_protocol::wire::Field>()
                    + fields
                        .iter()
                        .map(|field| {
                            field.raw.capacity()
                                + match &field.value {
                                    rambler_protocol::wire::Value::Bytes(value) => value.capacity(),
                                    _ => 0,
                                }
                        })
                        .sum::<usize>()
            })
            .sum();
        println!(
            "{}",
            serde_json::json!({"kind":"retention_census", "text_size":size,
            "wire_bytes":raw.len(),
            "raw_lengths":{"root":expected.raw.len(),"result":result.raw.len(),
                "active_raw":result.active_raw.as_ref().unwrap().len(),
                "recognition":recognition.raw.len(),"hypotheses":list.raw.len(),
                "hypothesis":hypothesis.raw.len(),"voice_edit":edit.raw.len()},
            "string_capacity_bytes":result.language.as_ref().unwrap().capacity()
                + hypothesis.text.as_ref().unwrap().capacity()
                + edit.transcript.as_ref().unwrap().capacity()
                + edit.edited_text.as_ref().unwrap().capacity(),
            "unknown_heap_bytes":unknown_heap,
            "hypotheses_vec_bytes":list.hypotheses.capacity() * std::mem::size_of::<rambler_protocol::Hypothesis>(),
            "s3_response_size":std::mem::size_of::<rambler_protocol::S3Response>()})
        );
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
            if ENABLED.load(Relaxed) {
                RETAINED.fetch_max(LIVE.load(Relaxed), Relaxed);
            }
            bytes
        });
    }
}
