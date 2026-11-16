//! Synthetic, bounded, same-binary wire-encoding comparison; not an engine benchmark.
//! Silent unless built in release mode and passed exactly --benchmark-wire-encoding.
//! Only std and existing crate/dev dependencies are used; no I/O except summary JSON.
use rambler_protocol::{EditingContext, Error, MAX_MESSAGE_BYTES, ProtocolConfig, wire};
use serde::Serialize;
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::OsStr;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

#[allow(dead_code)]
#[rustfmt::skip]
#[path = "support/wire_encoding_baseline.rs"]
mod baseline;

#[rustfmt::skip]
#[path = "support/request_encoding_baseline.rs"]
mod baseline_requests;

const BASELINE_COMMIT: &str = "d9af4f5c0767361fb9b2a13043e228f924e720bf";
const BASELINE_REQUEST_SHA256: &str =
    "1f7a0b3330c3dd34a23e617912567e68bfa1c0f5b36b4ba90ede6fd169af5870";
const BASELINE_SHA256: &str = "c9a6fac56e0497eb12d6104ee418e47b7e9f3c011c9b5fc5f52ae0c80edd2a6b";
const FLAG: &str = "--benchmark-wire-encoding";
const REPETITIONS: usize = 6;
const WARMUP_ITERATIONS: usize = 8;
const ALLOCATION_ITERATIONS: usize = 128;
const FIELD_NUMBERS: [u32; 10] = [
    1,
    15,
    16,
    2047,
    2048,
    262143,
    262144,
    33554431,
    33554432,
    0x1fff_ffff,
];
const PAYLOAD_LENGTHS: [usize; 6] = [0, 7, 127, 128, 1024, 65536];

struct CountingSystem;

#[global_allocator]
static ALLOCATOR: CountingSystem = CountingSystem;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOC_ZEROED_CALLS: AtomicU64 = AtomicU64::new(0);
static REALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static DEALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOC_REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);
static REALLOC_REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: every method forwards the caller's exact pointer/layout/new_size to
// System and returns its result unchanged. Counter work is atomic only: it never
// allocates, logs, panics, dereferences pointers, or modifies allocation metadata.
// Scopes are single-threaded and only cover 128 valid encodes/builds. Fixed
// cases retain at most 65536 bytes of text plus small hints/wrappers, with each
// allocation below 262144 bytes and fewer than 512 calls per operation. This
// conservative requested-byte bound is < 2^35 per scope. Counters reset only
// while disabled, so u64 overflow is impossible for these fixed cases.
// Unsafe is confined to this example allocator, never the library encoders.
unsafe impl GlobalAlloc for CountingSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Relaxed) {
            ALLOC_CALLS.fetch_add(1, Relaxed);
            ALLOC_REQUESTED_BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        // SAFETY: GlobalAlloc's caller supplies a valid layout; forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Relaxed) {
            ALLOC_ZEROED_CALLS.fetch_add(1, Relaxed);
            ALLOC_REQUESTED_BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        // SAFETY: same layout contract as System.alloc_zeroed.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Relaxed) {
            REALLOC_CALLS.fetch_add(1, Relaxed);
            // Count the requested NEW size, not a delta or a live/peak estimate.
            REALLOC_REQUESTED_BYTES.fetch_add(new_size as u64, Relaxed);
        }
        // SAFETY: pointer, old layout, and new size retain the caller's contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNTING.load(Relaxed) {
            DEALLOC_CALLS.fetch_add(1, Relaxed);
        }
        // SAFETY: pointer and allocation layout are forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
struct CounterTotals {
    alloc_calls: u64,
    alloc_zeroed_calls: u64,
    realloc_calls: u64,
    dealloc_calls: u64,
    // alloc + alloc_zeroed requested sizes, including any failed calls.
    alloc_requested_bytes: u64,
    // Sum of realloc new_size arguments, including any failed calls.
    realloc_requested_bytes: u64,
}

impl CounterTotals {
    fn requested_bytes(self) -> u64 {
        self.alloc_requested_bytes + self.realloc_requested_bytes
    }

    fn snapshot() -> Self {
        Self {
            alloc_calls: ALLOC_CALLS.load(Relaxed),
            alloc_zeroed_calls: ALLOC_ZEROED_CALLS.load(Relaxed),
            realloc_calls: REALLOC_CALLS.load(Relaxed),
            dealloc_calls: DEALLOC_CALLS.load(Relaxed),
            alloc_requested_bytes: ALLOC_REQUESTED_BYTES.load(Relaxed),
            realloc_requested_bytes: REALLOC_REQUESTED_BYTES.load(Relaxed),
        }
    }
}

struct CountScope;

impl CountScope {
    fn start() -> Self {
        // No nesting and no other threads exist in the benchmark executable.
        assert!(!COUNTING.load(Relaxed));
        for counter in [
            &ALLOC_CALLS,
            &ALLOC_ZEROED_CALLS,
            &REALLOC_CALLS,
            &DEALLOC_CALLS,
            &ALLOC_REQUESTED_BYTES,
            &REALLOC_REQUESTED_BYTES,
        ] {
            counter.store(0, Relaxed);
        }
        COUNTING.store(true, Relaxed);
        Self
    }
}

impl Drop for CountScope {
    fn drop(&mut self) {
        COUNTING.store(false, Relaxed);
    }
}

// Verbatim lib.rs bound used by the frozen helper bodies.
fn bounded(len: usize) -> Result<(), Error> {
    if len > MAX_MESSAGE_BYTES {
        Err(Error::SizeLimit)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "helper", rename_all = "snake_case")]
enum Workload {
    EncodeVarint { value: u64 },
    VarintField { number: u32, value: u64 },
    BytesField { number: u32, payload_len: usize },
    InitialRequest { hint_len: usize },
    ContextRequest { cursor_aware: bool, text_len: usize },
}

struct PreparedCase {
    workload: Workload,
    payload: Vec<u8>,
    config: Option<ProtocolConfig>,
    context: Option<EditingContext>,
}

fn varint_values() -> Vec<u64> {
    let mut values = vec![0, 1];
    // Immediately below/at/above every possible u64 varint width transition.
    for shift in (7..=63).step_by(7) {
        let boundary = 1_u64 << shift;
        values.extend([boundary - 1, boundary, boundary + 1]);
    }
    values.push(u64::MAX);
    values
}

fn workloads() -> Vec<Workload> {
    let mut cases = Vec::new();
    for value in varint_values() {
        cases.push(Workload::EncodeVarint { value });
        cases.push(Workload::VarintField { number: 1, value });
    }
    // Both sides of every key-width transition, plus the maximum valid key.
    for number in FIELD_NUMBERS {
        if number != 1 {
            for value in [0, u64::MAX] {
                cases.push(Workload::VarintField { number, value });
            }
        }
        for payload_len in PAYLOAD_LENGTHS {
            cases.push(Workload::BytesField {
                number,
                payload_len,
            });
        }
    }
    for hint_len in [0, 128, 1024] {
        cases.push(Workload::InitialRequest { hint_len });
    }
    for cursor_aware in [false, true] {
        for text_len in [0, 128, 1024, 65536] {
            cases.push(Workload::ContextRequest {
                cursor_aware,
                text_len,
            });
        }
    }
    cases
}

fn prepare(workload: Workload) -> PreparedCase {
    let payload_len = match workload {
        Workload::BytesField { payload_len, .. } => payload_len,
        _ => 0,
    };
    let payload = (0..payload_len)
        .map(|index| index.wrapping_mul(17).wrapping_add(3) as u8)
        .collect();
    let config = match workload {
        Workload::InitialRequest { hint_len } => {
            let mut config = ProtocolConfig::default();
            if hint_len != 0 {
                config.bias_terms = vec!["b".repeat(hint_len), "é𝄞".into()];
                config.preferences = vec!["p".repeat(hint_len), "\0".into()];
                config.secondary_languages = vec!["en-US".into(), "de-DE".into()];
                config.auto_punctuation = Some(true);
                config.block_offensive_words = Some(false);
                config.secondary_routing = Some("synthetic-route".into());
            }
            Some(config)
        }
        _ => None,
    };
    let context = match workload {
        Workload::ContextRequest {
            cursor_aware,
            text_len,
        } => Some(EditingContext {
            before: "x".repeat(text_len),
            selected: if cursor_aware {
                "é𝄞".into()
            } else {
                String::new()
            },
            after: if cursor_aware {
                "\0".into()
            } else {
                String::new()
            },
            cursor_aware,
            ..Default::default()
        }),
        _ => None,
    };
    PreparedCase {
        workload,
        payload,
        config,
        context,
    }
}

type Encoder = fn(&PreparedCase) -> Vec<u8>;

#[inline(never)]
fn legacy(case: &PreparedCase) -> Vec<u8> {
    match case.workload {
        Workload::EncodeVarint { value } => baseline::encode_varint(value),
        Workload::VarintField { number, value } => baseline::varint_field(number, value).unwrap(),
        Workload::BytesField { number, .. } => {
            baseline::bytes_field(number, &case.payload).unwrap()
        }
        Workload::InitialRequest { .. } => {
            baseline_requests::initial_request(case.config.as_ref().unwrap(), 16384).unwrap()
        }
        Workload::ContextRequest { .. } => {
            baseline_requests::context_request(case.context.as_ref().unwrap()).unwrap()
        }
    }
}

#[inline(never)]
fn candidate(case: &PreparedCase) -> Vec<u8> {
    match case.workload {
        Workload::EncodeVarint { value } => wire::encode_varint(value),
        Workload::VarintField { number, value } => wire::varint_field(number, value).unwrap(),
        Workload::BytesField { number, .. } => wire::bytes_field(number, &case.payload).unwrap(),
        Workload::InitialRequest { .. } => {
            rambler_protocol::initial_request(case.config.as_ref().unwrap(), 16384).unwrap()
        }
        Workload::ContextRequest { .. } => {
            rambler_protocol::context_request(case.context.as_ref().unwrap()).unwrap()
        }
    }
}

#[inline(never)]
fn batch(encoder: Encoder, case: &PreparedCase, iterations: usize) {
    let encoder = black_box(encoder);
    for _ in 0..iterations {
        let out = encoder(black_box(case));
        drop(black_box(out));
    }
}

fn allocation_pass(encoder: Encoder, case: &PreparedCase) -> CounterTotals {
    let scope = CountScope::start();
    batch(encoder, case, ALLOCATION_ITERATIONS);
    drop(scope);
    CounterTotals::snapshot()
}

fn elapsed_pass(encoder: Encoder, case: &PreparedCase, iterations: usize) -> u64 {
    let start = Instant::now();
    batch(encoder, case, iterations);
    u64::try_from(start.elapsed().as_nanos()).expect("bounded elapsed nanoseconds")
}

fn timing_iterations(workload: Workload) -> usize {
    match workload {
        Workload::BytesField { payload_len, .. } if payload_len >= 65536 => 64,
        Workload::BytesField { payload_len, .. } if payload_len >= 1024 => 512,
        Workload::ContextRequest { text_len, .. } if text_len >= 65536 => 64,
        Workload::ContextRequest { text_len, .. } if text_len >= 1024 => 512,
        Workload::InitialRequest { .. } => 512,
        _ => 2048,
    }
}

// All equality and error checks finish BEFORE any measured pass. Never emit raw
// payloads even on mismatch: panic messages deliberately contain no byte arrays.
fn check_equality(cases: &[PreparedCase]) -> Vec<usize> {
    let lengths = cases
        .iter()
        .map(|case| {
            let a = black_box(legacy(black_box(case)));
            let b = black_box(candidate(black_box(case)));
            assert!(a == b, "synthetic encoding mismatch");
            a.len()
        })
        .collect();
    check_bounds_and_errors();
    lengths
}

fn check_bounds_and_errors() {
    // A single synthetic buffer checks over-limit payloads, envelope-only
    // overflows, exact complete-message limits, and mixed-invalid precedence.
    let data = vec![0x5a; MAX_MESSAGE_BYTES + 1];
    for number in [0, 1, 15, 16, 0x1fff_ffff, 0x2000_0000, u32::MAX] {
        for len in [
            0,
            MAX_MESSAGE_BYTES - 10,
            MAX_MESSAGE_BYTES - 9,
            MAX_MESSAGE_BYTES - 6,
            MAX_MESSAGE_BYTES - 5,
            MAX_MESSAGE_BYTES - 4,
            MAX_MESSAGE_BYTES,
            MAX_MESSAGE_BYTES + 1,
        ] {
            let a = black_box(baseline::bytes_field(number, &data[..len]));
            let b = black_box(wire::bytes_field(number, &data[..len]));
            assert!(a == b, "synthetic boundary/error mismatch");
        }
    }
    // Independent assertions make equality to a frozen bug insufficient.
    assert!(
        wire::bytes_field(0, &data) == Err(Error::SizeLimit),
        "oversized payload must win over invalid field"
    );
    assert!(
        wire::bytes_field(0, &data[..MAX_MESSAGE_BYTES]) == Err(Error::FieldNumber),
        "invalid field must win over oversized envelope"
    );
    assert!(
        wire::bytes_field(1, &data[..MAX_MESSAGE_BYTES]) == Err(Error::SizeLimit),
        "oversized envelope must be rejected"
    );
    for number in [0, 0x2000_0000, u32::MAX] {
        for value in [0, u64::MAX] {
            let a = black_box(baseline::varint_field(number, value));
            let b = black_box(wire::varint_field(number, value));
            assert!(
                a == b && b == Err(Error::FieldNumber),
                "invalid field mismatch"
            );
        }
    }
    for number in FIELD_NUMBERS {
        let a = black_box(baseline::string_field(number, "synthetic-é-𝄞"));
        let b = black_box(wire::string_field(number, "synthetic-é-𝄞"));
        assert!(a == b, "synthetic string mismatch");
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct TimingPair {
    baseline_first: bool,
    baseline_elapsed_ns: u64,
    candidate_elapsed_ns: u64,
}

#[derive(Serialize)]
struct CaseSummary {
    workload: Workload,
    equal_bytes: bool,
    output_len: usize,
    allocation_iterations_per_implementation: usize,
    baseline_counters: CounterTotals,
    candidate_counters: CounterTotals,
    baseline_total_requested_bytes: u64,
    candidate_total_requested_bytes: u64,
    timing_iterations_per_implementation_per_repetition: usize,
    timings: [TimingPair; REPETITIONS],
}

#[derive(Serialize)]
struct Summary {
    schema: &'static str,
    baseline_commit: &'static str,
    baseline_wire_git_lf_sha256: &'static str,
    baseline_request_git_lf_sha256: &'static str,
    synthetic_only: bool,
    release_only: bool,
    repetitions: usize,
    warmup_iterations_per_implementation_per_repetition: usize,
    timing_scope: &'static str,
    counter_scope: &'static str,
    cases: Vec<CaseSummary>,
}

fn benchmark() {
    let cases: Vec<_> = workloads().into_iter().map(prepare).collect();
    let output_lengths = check_equality(&cases);
    let mut rows = Vec::with_capacity(cases.len());

    // Allocation-only phase: no clocks, summary serialization, or row allocations
    // while counting is enabled. Alternate order across cases.
    for (index, (case, output_len)) in cases.iter().zip(output_lengths).enumerate() {
        batch(legacy, case, WARMUP_ITERATIONS);
        batch(candidate, case, WARMUP_ITERATIONS);
        let (a, b) = if index % 2 == 0 {
            let a = allocation_pass(legacy, case);
            let b = allocation_pass(candidate, case);
            (a, b)
        } else {
            let b = allocation_pass(candidate, case);
            let a = allocation_pass(legacy, case);
            (a, b)
        };
        rows.push(CaseSummary {
            workload: case.workload,
            equal_bytes: true,
            output_len,
            allocation_iterations_per_implementation: ALLOCATION_ITERATIONS,
            baseline_counters: a,
            candidate_counters: b,
            baseline_total_requested_bytes: a.requested_bytes(),
            candidate_total_requested_bytes: b.requested_bytes(),
            timing_iterations_per_implementation_per_repetition: timing_iterations(case.workload),
            timings: [TimingPair::default(); REPETITIONS],
        });
    }

    // Elapsed-only phase: counters remain disabled. The allocator still performs
    // its disabled atomic gate load; this instrumentation cost affects both paths.
    // Rotate A/B vs B/A across repetitions AND cases; no sorting/cherry-picking.
    for repetition in 0..REPETITIONS {
        for (index, (case, row)) in cases.iter().zip(&mut rows).enumerate() {
            let iterations = row.timing_iterations_per_implementation_per_repetition;
            let baseline_first = (repetition + index) % 2 == 0;
            let (a, b) = if baseline_first {
                batch(legacy, case, WARMUP_ITERATIONS);
                batch(candidate, case, WARMUP_ITERATIONS);
                let a = elapsed_pass(legacy, case, iterations);
                let b = elapsed_pass(candidate, case, iterations);
                (a, b)
            } else {
                batch(candidate, case, WARMUP_ITERATIONS);
                batch(legacy, case, WARMUP_ITERATIONS);
                let b = elapsed_pass(candidate, case, iterations);
                let a = elapsed_pass(legacy, case, iterations);
                (a, b)
            };
            row.timings[repetition] = TimingPair {
                baseline_first,
                baseline_elapsed_ns: a,
                candidate_elapsed_ns: b,
            };
        }
    }

    let summary = Summary {
        schema: "rambler-wire-encoding-v2",
        baseline_commit: BASELINE_COMMIT,
        baseline_wire_git_lf_sha256: BASELINE_SHA256,
        baseline_request_git_lf_sha256: BASELINE_REQUEST_SHA256,
        synthetic_only: true,
        release_only: true,
        repetitions: REPETITIONS,
        warmup_iterations_per_implementation_per_repetition: WARMUP_ITERATIONS,
        timing_scope: "batch elapsed ns: helper or complete request assembly + opaque dispatch + Vec drop; counters disabled; fixture/config preparation excluded",
        counter_scope: "totals per implementation/case over allocation_iterations; calls including failures; alloc_requested_bytes includes alloc_zeroed; realloc bytes are full new_size, not deltas; no live/peak/RSS estimate",
        cases: rows,
    };
    println!(
        "{}",
        serde_json::to_string(&summary).expect("fixed summary JSON")
    );
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(FLAG)) || args.next().is_some() {
        return;
    }
    if cfg!(debug_assertions) {
        return;
    }
    benchmark();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_is_fixed_and_covers_width_boundaries() {
        let values = varint_values();
        assert_eq!(values.len(), 30);
        for shift in (7..=63).step_by(7) {
            let boundary = 1_u64 << shift;
            assert!(values.contains(&(boundary - 1)));
            assert!(values.contains(&boundary));
            assert!(values.contains(&(boundary + 1)));
        }
        let cases = workloads();
        assert_eq!(cases.len(), 149);
        for (index, workload) in cases.iter().enumerate() {
            assert!(!cases[..index].contains(workload));
        }
        for number in FIELD_NUMBERS {
            for payload_len in PAYLOAD_LENGTHS {
                assert!(cases.contains(&Workload::BytesField {
                    number,
                    payload_len,
                }));
            }
        }
        for workload in cases {
            assert!((64..=2048).contains(&timing_iterations(workload)));
        }
    }

    #[test]
    fn requested_bytes_sum_new_sizes_not_growth_deltas() {
        let totals = CounterTotals {
            alloc_calls: 2,
            alloc_zeroed_calls: 1,
            realloc_calls: 2,
            dealloc_calls: 3,
            alloc_requested_bytes: 10 + 20 + 30,
            realloc_requested_bytes: 40 + 80,
        };
        assert_eq!(totals.requested_bytes(), 180);
        assert_eq!(CounterTotals::default().requested_bytes(), 0);
        let conservative_scope_bytes = ALLOCATION_ITERATIONS as u64 * 512 * 262144;
        assert!(conservative_scope_bytes < (1_u64 << 35));
    }

    #[test]
    fn frozen_and_candidate_encoding_and_errors_match() {
        let cases: Vec<_> = workloads().into_iter().map(prepare).collect();
        assert_eq!(check_equality(&cases).len(), 149);
    }
}
