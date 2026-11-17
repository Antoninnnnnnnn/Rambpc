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

// Developer-only synthetic safety decode cases. Keep this identical in baseline
// and candidate builds; fixtures and full manual model oracles precede all passes.
fn decode_safety(iterations: usize, repeats: usize) {
    fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            out.push(byte | if value == 0 { 0 } else { 0x80 });
            if value == 0 {
                break;
            }
        }
    }
    fn varint(number: u32, value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        push_varint(&mut out, u64::from(number) << 3);
        push_varint(&mut out, value);
        out
    }
    fn blob(number: u32, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        push_varint(&mut out, (u64::from(number) << 3) | 2);
        push_varint(&mut out, body.len() as u64);
        out.extend_from_slice(body);
        out
    }
    struct Case {
        name: &'static str,
        raw: Vec<u8>,
        expected: rambler_protocol::S3Response,
        marker_count: usize,
        decode_name: String,
        retain_name: String,
    }
    fn fixture(
        name: &'static str,
        markers: &[u8],
        marker_count: usize,
        status: Option<i32>,
        unknown_fields: Vec<rambler_protocol::wire::Field>,
    ) -> Case {
        let hypothesis = [blob(1, b"x"), markers.to_vec()].concat();
        // Exact path: root RECOGNITION_RESPONSE / result 1 / container 3 /
        // hypothesis 3. No state, event, engine, or caller policy is involved.
        let container = blob(3, &hypothesis);
        let recognition = blob(3, &container);
        let result = blob(1, &recognition);
        let raw = blob(rambler_protocol::RECOGNITION_RESPONSE, &result);
        // Independent public-model construction, never a decoded/cloned oracle.
        // Every wrapper's raw and the active recognition raw are explicit.
        let expected = rambler_protocol::S3Response {
            raw: raw.clone().into(),
            result: Some(rambler_protocol::RecognitionResponse {
                raw: result.into(),
                active_field: Some(1),
                active_raw: Some(recognition.clone().into()),
                recognition: Some(rambler_protocol::RecognitionResult {
                    raw: recognition.into(),
                    hypothesis_result: Some(rambler_protocol::HypothesisResult {
                        raw: container.into(),
                        hypotheses: vec![rambler_protocol::Hypothesis {
                            text: Some("x".to_owned()),
                            safety_status: status,
                            unknown_fields,
                            raw: hypothesis.into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        Case {
            name,
            raw,
            expected,
            marker_count,
            decode_name: format!("decode_safety_{name}"),
            retain_name: format!("decode_safety_retain128_{name}"),
        }
    }

    // These helpers deliberately do not use production wire encoders/parsers.
    assert_eq!(varint(30, 0), [0xf0, 0x01, 0x00]);
    assert_eq!(varint(30, 1), [0xf0, 0x01, 0x01]);
    assert_eq!(
        blob(rambler_protocol::RECOGNITION_RESPONSE, &[]),
        [0xca, 0x8f, 0xe4, 0x04, 0x00]
    );
    let alternating = [varint(30, 0), varint(30, 1)].concat().repeat(8);
    // Both the key and recognized value use overlong, valid varint encodings.
    let noncanonical0 = [0xf0, 0x81, 0x00, 0x80, 0x00];
    let noncanonical1 = [0xf0, 0x81, 0x00, 0x81, 0x00];
    let mixed = [
        varint(30, 0),
        varint(30, 42),
        varint(30, 1),
        // A recognized-looking sequence inside wrong-wire bytes is not a marker.
        blob(30, &[0xf0, 0x01, 0x01]),
        varint(30, 0),
        // An unknown enum after the last recognized marker must not clear it.
        vec![0xf0, 0x01, 0xaa, 0x00],
    ]
    .concat();
    let mixed_unknown = vec![
        rambler_protocol::wire::Field {
            number: 30,
            wire_type: 0,
            value: rambler_protocol::wire::Value::Varint(42),
            raw: vec![0xf0, 0x01, 0x2a],
        },
        rambler_protocol::wire::Field {
            number: 30,
            wire_type: 2,
            value: rambler_protocol::wire::Value::Bytes(vec![0xf0, 0x01, 0x01]),
            raw: vec![0xf2, 0x01, 0x03, 0xf0, 0x01, 0x01],
        },
        rambler_protocol::wire::Field {
            number: 30,
            wire_type: 0,
            value: rambler_protocol::wire::Value::Varint(42),
            raw: vec![0xf0, 0x01, 0xaa, 0x00],
        },
    ];
    let cases = [
        fixture("absent", &[], 0, None, Vec::new()),
        fixture("one0", &varint(30, 0), 1, Some(0), Vec::new()),
        fixture("one1", &varint(30, 1), 1, Some(1), Vec::new()),
        fixture("alternating16", &alternating, 16, Some(1), Vec::new()),
        fixture("noncanonical0", &noncanonical0, 1, Some(0), Vec::new()),
        fixture("noncanonical1", &noncanonical1, 1, Some(1), Vec::new()),
        fixture(
            "mixed_unknown42_wrongwire30",
            &mixed,
            3,
            Some(0),
            mixed_unknown,
        ),
    ];

    // Validate ALL full models, then allocate ALL census JSON before any bench.
    // Capacity is observed, not prescribed: the baseline may retain spare slots.
    let mut census = Vec::with_capacity(cases.len());
    for case in &cases {
        let decoded = decode_response(&case.raw).unwrap();
        assert_eq!(&decoded, &case.expected, "safety oracle {}", case.name);
        let hypothesis = &decoded.hypotheses()[0];
        let field_size = std::mem::size_of::<rambler_protocol::wire::Field>();
        census.push(
            serde_json::json!({"kind":"safety_census", "case":case.name,
                "decode_case":case.decode_name, "retain_case":case.retain_name,
                "wire_bytes":case.raw.len(), "marker_count":case.marker_count,
                "expected_status":case.expected.hypotheses()[0].safety_status,
                "genuine_unknown_count":case.expected.hypotheses()[0].unknown_fields.len(),
                "unknown_capacity":hypothesis.unknown_fields.capacity(),
                "field_size":field_size,
                "logical_retained_unknown_vector_bytes":hypothesis.unknown_fields.capacity() * field_size})
            .to_string(),
        );
    }
    for line in &census {
        println!("{line}");
    }
    for case in &cases {
        bench(&case.decode_name, iterations, repeats, 1, || {
            let response = decode_response(black_box(&case.raw)).unwrap();
            black_box(&response);
            response.raw.len()
        });
        // Same actual move-retain128 workload and LIVE/RETAINED sampling as below.
        bench(
            &case.retain_name,
            (iterations / 128).max(1),
            repeats,
            128,
            || {
                let mut updates = Vec::new();
                let mut bytes = 0;
                for _ in 0..128 {
                    let response = decode_response(black_box(&case.raw)).unwrap();
                    bytes += response.raw.len();
                    assert!(bytes <= 8 * 1024 * 1024);
                    updates.push(response);
                }
                black_box(&updates);
                if ENABLED.load(Relaxed) {
                    RETAINED.fetch_max(LIVE.load(Relaxed), Relaxed);
                }
                bytes
            },
        );
    }
}

// Identical source is intended to be copied to both revisions. All fixtures and
// independent public-model expectations are constructed and checked before timing.
fn decode_oneof(repeats: usize, check_only: bool) {
    use rambler_protocol::{
        EditingContext, Error, Hypothesis, HypothesisResult, RECOGNITION_RESPONSE,
        RecognitionResponse, RecognitionResult, S3Response, SpeechEventResult,
        VOICE_EDITING_RESULT, VoiceEditResult,
        wire::{Field, Value},
    };
    fn vint(out: &mut Vec<u8>, mut n: u64) {
        loop {
            let b = (n & 127) as u8;
            n >>= 7;
            out.push(b | if n == 0 { 0 } else { 128 });
            if n == 0 {
                break;
            }
        }
    }
    fn v(n: u32, x: u64) -> Vec<u8> {
        let mut r = Vec::new();
        vint(&mut r, u64::from(n) << 3);
        vint(&mut r, x);
        r
    }
    fn b(n: u32, x: &[u8]) -> Vec<u8> {
        let mut r = Vec::new();
        vint(&mut r, (u64::from(n) << 3) | 2);
        vint(&mut r, x.len() as u64);
        r.extend_from_slice(x);
        r
    }
    fn unknown_var(n: u32, x: u64) -> Field {
        Field {
            number: n,
            wire_type: 0,
            value: Value::Varint(x),
            raw: v(n, x),
        }
    }
    fn unknown_blob(n: u32, x: &[u8]) -> Field {
        Field {
            number: n,
            wire_type: 2,
            value: Value::Bytes(x.to_vec()),
            raw: b(n, x),
        }
    }
    fn root(body: Vec<u8>, mut result: RecognitionResponse) -> (Vec<u8>, S3Response) {
        let raw = b(RECOGNITION_RESPONSE, &body);
        result.raw = body.into();
        (
            raw.clone(),
            S3Response {
                raw: raw.into(),
                result: Some(result),
                ..Default::default()
            },
        )
    }
    fn recognition(parts: &[Vec<u8>], state: Option<i32>) -> RecognitionResponse {
        let raw = parts.concat();
        RecognitionResponse {
            active_field: Some(1),
            active_raw: Some(raw.clone().into()),
            recognition: Some(RecognitionResult {
                raw: raw.into(),
                state,
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    fn nested(edit: Vec<u8>, edit_model: VoiceEditResult) -> (Vec<u8>, S3Response) {
        let hypothesis = b(VOICE_EDITING_RESULT, &edit);
        let list = b(3, &hypothesis);
        let rec = b(3, &list);
        let mut result = recognition(std::slice::from_ref(&rec), None);
        result.recognition.as_mut().unwrap().hypothesis_result = Some(HypothesisResult {
            raw: list.into(),
            hypotheses: vec![Hypothesis {
                raw: hypothesis.into(),
                voice_edit: Some(VoiceEditResult {
                    raw: edit.into(),
                    ..edit_model
                }),
                ..Default::default()
            }],
            ..Default::default()
        });
        root(b(1, &rec), result)
    }
    struct Case {
        name: String,
        raw: Vec<u8>,
        expected: Result<S3Response, Error>,
        iterations: usize,
        retain: bool,
    }
    let mut cases = Vec::new();
    let mut add = |name: &str, fixture: (Vec<u8>, S3Response), iterations, retain| {
        cases.push(Case {
            name: name.into(),
            raw: fixture.0,
            expected: Ok(fixture.1),
            iterations,
            retain,
        });
    };
    add("empty", (vec![], S3Response::default()), 20_000, false);
    add(
        "status",
        (
            v(1, 0),
            S3Response {
                raw: v(1, 0).into(),
                status: Some(0),
                ..Default::default()
            },
        ),
        20_000,
        false,
    );
    let noise = [b(3, b"en"), v(1, 9)].concat();
    add(
        "language_wrongwire",
        root(
            noise,
            RecognitionResponse {
                language: Some("en".into()),
                unknown_fields: vec![unknown_var(1, 9)],
                ..Default::default()
            },
        ),
        20_000,
        false,
    );
    for (name, payload, state) in [
        ("recognition", v(1, 1), Some(1)),
        ("recognition_empty", vec![], None),
    ] {
        add(
            name,
            root(b(1, &payload), recognition(&[payload], state)),
            20_000,
            false,
        );
    }
    for (name, payload, event) in [("speech", v(1, 3), Some(3)), ("speech_empty", vec![], None)] {
        add(
            name,
            root(
                b(2, &payload),
                RecognitionResponse {
                    active_field: Some(2),
                    active_raw: Some(payload.clone().into()),
                    speech: Some(SpeechEventResult {
                        raw: payload.into(),
                        event,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            20_000,
            false,
        );
    }
    let opaque = [0xff, 8];
    add(
        "opaque",
        root(
            b(5, &opaque),
            RecognitionResponse {
                active_field: Some(5),
                active_raw: Some(opaque.to_vec().into()),
                unknown_fields: vec![unknown_blob(5, &opaque)],
                ..Default::default()
            },
        ),
        20_000,
        false,
    );
    for (field, size) in [(2, 128), (3, 4096)] {
        let text = "x".repeat(size);
        let edit = b(field, text.as_bytes());
        let model = VoiceEditResult {
            active_field: Some(field),
            alternative_text: (field == 2).then(|| text.clone()),
            edited_text: (field == 3).then(|| text.clone()),
            ..Default::default()
        };
        add(
            &format!("nested_{field}_{size}"),
            nested(edit, model),
            if size == 4096 { 256 } else { 20_000 },
            field == 2,
        );
    }
    let c1 = b(1, b"before");
    let c2 = [b(2, b"selected"), b(3, b"after")].concat();
    for count in [1, 2] {
        let parts = if count == 1 {
            vec![c1.clone()]
        } else {
            vec![c1.clone(), c2.clone()]
        };
        let edit: Vec<u8> = parts.iter().flat_map(|p| b(7, p)).collect();
        let context = EditingContext {
            before: "before".into(),
            selected: if count == 2 { "selected" } else { "" }.into(),
            after: if count == 2 { "after" } else { "" }.into(),
            cursor_aware: true,
            raw: parts.concat().into(),
            ..Default::default()
        };
        add(
            &format!("context_{count}"),
            nested(
                edit,
                VoiceEditResult {
                    active_field: Some(7),
                    edited_context: Some(context),
                    ..Default::default()
                },
            ),
            20_000,
            false,
        );
    }
    add(
        "repeated_string",
        nested(
            [b(3, b"first"), b(3, b"last")].concat(),
            VoiceEditResult {
                active_field: Some(3),
                edited_text: Some("last".into()),
                ..Default::default()
            },
        ),
        20_000,
        false,
    );
    for count in [2, 16] {
        let parts: Vec<_> = (0..count).map(|_| v(1, 1)).collect();
        let body = parts.iter().flat_map(|p| b(1, p)).collect();
        add(
            &format!("recognition_merge_{count}"),
            root(body, recognition(&parts, Some(1))),
            20_000,
            count == 2,
        );
    }
    let typed = [b(1, &v(1, 1)), b(2, &v(1, 3))].concat().repeat(8);
    add(
        "switch_typed16",
        root(
            typed,
            RecognitionResponse {
                active_field: Some(2),
                active_raw: Some(v(1, 3).into()),
                speech: Some(SpeechEventResult {
                    event: Some(3),
                    raw: v(1, 3).into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        20_000,
        false,
    );
    let mixed = [b(1, &v(1, 1)), b(5, &opaque)].concat().repeat(8);
    add(
        "switch_opaque16",
        root(
            mixed,
            RecognitionResponse {
                active_field: Some(5),
                active_raw: Some(opaque.to_vec().into()),
                unknown_fields: vec![unknown_blob(5, &opaque); 8],
                ..Default::default()
            },
        ),
        20_000,
        false,
    );
    for count in [0, 1, 2] {
        let part = v(1, 1);
        let mut body = if count > 0 { b(1, &part) } else { vec![] };
        body.extend(v(99, 0).repeat(20_000));
        if count == 2 {
            body.extend(b(1, &part));
        }
        let mut expected = if count == 0 {
            RecognitionResponse::default()
        } else {
            recognition(&vec![part; count], Some(1))
        };
        expected.unknown_fields = vec![unknown_var(99, 0); 20_000];
        add(
            &format!("sparse_result_{count}"),
            root(body, expected),
            256,
            false,
        );
    }
    for (name, body, error) in [
        (
            "error_runs16",
            [
                b(1, &[8]),
                [b(2, &v(1, 3)), b(1, &v(1, 1))].concat().repeat(8),
            ]
            .concat(),
            Error::Truncated,
        ),
        (
            "error_suffix20000",
            [b(1, &[8]), v(99, 0).repeat(20_000)].concat(),
            Error::Truncated,
        ),
    ] {
        cases.push(Case {
            name: name.into(),
            raw: b(RECOGNITION_RESPONSE, &body),
            expected: Err(error),
            iterations: 256,
            retain: false,
        });
    }
    let (raw, _) = nested(
        [b(2, &[0xff]), b(3, b"valid")].concat(),
        VoiceEditResult::default(),
    );
    cases.push(Case {
        name: "error_utf8_overwritten".into(),
        raw,
        expected: Err(Error::Utf8),
        iterations: 256,
        retain: false,
    });

    // Equality excludes EditingContext.raw, so assert that byte sequence separately.
    for case in &cases {
        let actual = decode_response(&case.raw);
        assert_eq!(actual, case.expected, "oneof oracle {}", case.name);
        if let (Ok(actual), Ok(expected)) = (&actual, &case.expected) {
            for (a, e) in actual.hypotheses().iter().zip(expected.hypotheses()) {
                let context = |h: &Hypothesis| {
                    h.voice_edit
                        .as_ref()
                        .and_then(|v| v.edited_context.as_ref())
                        .map(|c| c.raw.clone())
                };
                assert_eq!(context(a), context(e), "context raw {}", case.name);
            }
        }
    }
    if check_only {
        let mut inventory = Vec::new();
        for case in &cases {
            inventory.push(serde_json::json!({"case": format!("oneof_{}", case.name), "protobuf_bytes_per_unit": case.raw.len()}));
            if case.retain {
                inventory.push(serde_json::json!({"case": format!("oneof_retain128_{}", case.name), "protobuf_bytes_per_unit": case.raw.len()}));
            }
        }
        println!(
            "{}",
            serde_json::json!({"schema": "oneof-oracles-only-v1", "cases": inventory, "benchmarks_executed": false})
        );
        return;
    }
    for case in &cases {
        bench(
            &format!("oneof_{}", case.name),
            case.iterations,
            repeats,
            1,
            || {
                let decoded = decode_response(black_box(&case.raw));
                black_box(&decoded);
                case.raw.len()
            },
        );
        if case.retain {
            bench(
                &format!("oneof_retain128_{}", case.name),
                156,
                repeats,
                128,
                || {
                    let mut retained = Vec::new();
                    let mut bytes = 0;
                    for _ in 0..128 {
                        let decoded = decode_response(black_box(&case.raw)).unwrap();
                        bytes += decoded.raw.len();
                        retained.push(decoded);
                    }
                    black_box(&retained);
                    if ENABLED.load(Relaxed) {
                        RETAINED.fetch_max(LIVE.load(Relaxed), Relaxed);
                    }
                    bytes
                },
            );
        }
    }
}

// Runtime rejection keeps debug test builds possible; timing requires release.
#[allow(clippy::assertions_on_constants)]
fn main() {
    assert!(!cfg!(debug_assertions), "use --release");
    let iterations = profile_support::arg("--iterations", 20_000);
    let repeats = profile_support::arg("--repeats", 5);
    let decode_only = std::env::args().any(|arg| arg == "--decode-only");
    if std::env::args().any(|arg| arg == "--decode-oneof") {
        decode_oneof(repeats, std::env::args().any(|arg| arg == "--check-only"));
        return;
    }
    if std::env::args().any(|arg| arg == "--decode-safety") {
        decode_safety(iterations, repeats);
        return;
    }
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
