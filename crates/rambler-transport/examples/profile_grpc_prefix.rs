//! Opt-in, bounded constructor microbenchmark; no transport, payloads or threads.
//! Silent unless built in release mode and given exactly --benchmark-grpc-prefix.
//! Timing includes dispatch, Bytes destruction and the disabled allocator gate.
//! That gate's overhead does NOT cancel when allocator-call counts differ; timings
//! describe this instrumented binary only, not transport or real-dictation speed.

#[path = "../src/grpc_prefix.rs"]
mod grpc_prefix;

use bytes::Bytes;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    ffi::OsStr,
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};

const FLAG: &str = "--benchmark-grpc-prefix";
const WARMUP_ITERATIONS: u32 = 1024;
const COUNT_ITERATIONS: u32 = 4096;
const TIMING_ITERATIONS: u32 = 32768;
const REPETITIONS: usize = 8;
// Literal independent oracles: no to_be_bytes or candidate-table construction.
const CASES: [(u32, [u8; 5]); 16] = [
    (0, [0, 0, 0, 0, 0]),
    (1, [0, 0, 0, 0, 1]),
    (4, [0, 0, 0, 0, 4]),
    (32, [0, 0, 0, 0, 32]),
    (64, [0, 0, 0, 0, 64]),
    (96, [0, 0, 0, 0, 96]),
    (127, [0, 0, 0, 0, 127]),
    (128, [0, 0, 0, 0, 128]),
    (160, [0, 0, 0, 0, 160]),
    (192, [0, 0, 0, 0, 192]),
    (255, [0, 0, 0, 0, 255]),
    (256, [0, 0, 0, 1, 0]),
    (1024, [0, 0, 0, 4, 0]),
    (65535, [0, 0, 0, 255, 255]),
    (65536, [0, 0, 1, 0, 0]),
    (u32::MAX, [0, 255, 255, 255, 255]),
];

type Constructor = fn(u32) -> Bytes;

// Exact constructor statements from wire.rs upload() at 7a03f38, with only
// send_bytes's Bytes argument returned instead of passed into the async writer.
#[inline(never)]
fn baseline(len: u32) -> Bytes {
    let mut header = [0u8; 5];
    header[1..].copy_from_slice(&len.to_be_bytes());
    Bytes::copy_from_slice(&header)
}

#[inline(never)]
fn candidate(len: u32) -> Bytes {
    grpc_prefix::grpc_prefix(len)
}

struct CountingAllocator;
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

// Only the single-threaded, explicitly gated benchmark enables these counters.
// Unit tests never call benchmark() or allocation_counts(). The GlobalAlloc
// implementation neither allocates nor formats, clocks, locks or panics.
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ZEROED_CALLS: AtomicU64 = AtomicU64::new(0);
static REALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static DEALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);

fn record_request(counter: &AtomicU64, size: usize) {
    if COUNTING.load(Ordering::Relaxed) {
        counter.fetch_add(1, Ordering::Relaxed);
        // Requested sizes, even on failure; realloc contributes its new size.
        REQUESTED_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    }
}

// SAFETY: Every method forwards the original pointer/layout/size unchanged to
// System. Counters do not inspect or modify allocations, and never allocate.
// All unsafe in this change is confined to this opt-in benchmark allocator.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_request(&ALLOC_CALLS, layout.size());
        // SAFETY: GlobalAlloc's caller supplies the valid allocation layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_request(&ZEROED_CALLS, layout.size());
        // SAFETY: The unchanged layout has the caller's allocation contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_request(&REALLOC_CALLS, new_size);
        // SAFETY: Original allocation and new size retain the caller's contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNTING.load(Ordering::Relaxed) {
            DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: Original pointer/layout identify the caller's live allocation.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[derive(Clone, Copy, Default)]
struct Counts {
    alloc: u64,
    zeroed: u64,
    realloc: u64,
    dealloc: u64,
    requested_bytes: u64,
}

#[derive(Clone, Copy, Default)]
struct Row {
    // Numeric schema v1: phase 0 = allocation counts, 1 = elapsed time;
    // variant 0 = baseline, 1 = candidate; all repetitions are zero-based.
    phase: u32,
    payload_len: u32,
    variant: u32,
    repetition: usize,
    order_slot: usize,
    iterations: u32,
    elapsed_ns: u128,
    counts: Counts,
}

#[inline(never)]
fn run_iterations(constructor: Constructor, len: u32, iterations: u32) {
    for _ in 0..iterations {
        let bytes = black_box(constructor)(black_box(len));
        // Destruction is inside both the timing and allocation-count scopes.
        drop(black_box(bytes));
    }
}

fn allocation_counts(constructor: Constructor, len: u32) -> Counts {
    ALLOC_CALLS.store(0, Ordering::Relaxed);
    ZEROED_CALLS.store(0, Ordering::Relaxed);
    REALLOC_CALLS.store(0, Ordering::Relaxed);
    DEALLOC_CALLS.store(0, Ordering::Relaxed);
    REQUESTED_BYTES.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    // No clock, logging, assertion, row allocation or unrelated work in scope.
    run_iterations(constructor, len, COUNT_ITERATIONS);
    COUNTING.store(false, Ordering::Relaxed);
    Counts {
        alloc: ALLOC_CALLS.load(Ordering::Relaxed),
        zeroed: ZEROED_CALLS.load(Ordering::Relaxed),
        realloc: REALLOC_CALLS.load(Ordering::Relaxed),
        dealloc: DEALLOC_CALLS.load(Ordering::Relaxed),
        requested_bytes: REQUESTED_BYTES.load(Ordering::Relaxed),
    }
}

fn ordered_constructors(rotation: usize) -> [(u32, Constructor); 2] {
    if rotation.is_multiple_of(2) {
        [(0, baseline), (1, candidate)]
    } else {
        [(1, candidate), (0, baseline)]
    }
}

fn verify_prefixes() {
    for (len, expected) in CASES {
        // Check length AND every byte, before any measurements are taken.
        assert_eq!(baseline(len).as_ref(), expected.as_slice());
        assert_eq!(candidate(len).as_ref(), expected.as_slice());
    }
}

fn benchmark() {
    verify_prefixes();
    // Fixed-size stack storage; nothing is printed until all scopes finish.
    let mut rows = [Row::default(); CASES.len() * 2 * (REPETITIONS + 1)];
    let mut row_index = 0;

    for (case_index, (len, _)) in CASES.iter().enumerate() {
        for (_, constructor) in ordered_constructors(case_index) {
            run_iterations(constructor, *len, WARMUP_ITERATIONS);
        }
    }

    // Allocation counts are separate from elapsed measurements. No clocks are
    // read while COUNTING is true, and no counts are inferred from elapsed time.
    for (case_index, (len, _)) in CASES.iter().enumerate() {
        for (order_slot, (variant, constructor)) in
            ordered_constructors(case_index).into_iter().enumerate()
        {
            let counts = allocation_counts(constructor, *len);
            rows[row_index] = Row {
                phase: 0,
                payload_len: *len,
                variant,
                repetition: 0,
                order_slot,
                iterations: COUNT_ITERATIONS,
                elapsed_ns: 0,
                counts,
            };
            row_index += 1;
        }
    }

    // Each length alternates A/B then B/A; the first order also rotates across
    // lengths. Every one of the eight repetitions is retained, not just a best.
    for repetition in 0..REPETITIONS {
        for (case_index, (len, _)) in CASES.iter().enumerate() {
            for (order_slot, (variant, constructor)) in
                ordered_constructors(repetition + case_index)
                    .into_iter()
                    .enumerate()
            {
                let started = Instant::now();
                run_iterations(constructor, *len, TIMING_ITERATIONS);
                let elapsed_ns = started.elapsed().as_nanos();
                rows[row_index] = Row {
                    phase: 1,
                    payload_len: *len,
                    variant,
                    repetition,
                    order_slot,
                    iterations: TIMING_ITERATIONS,
                    elapsed_ns,
                    counts: Counts::default(),
                };
                row_index += 1;
            }
        }
    }

    println!(
        "schema,phase,payload_len,variant,repetition,order_slot,iterations,elapsed_ns,alloc_calls,alloc_zeroed_calls,realloc_calls,dealloc_calls,requested_bytes"
    );
    for row in rows {
        // Phase 0 elapsed_ns and phase 1 count columns are zero placeholders,
        // not observed zeros. Counts/bytes are totals, not per-iteration values.
        println!(
            "1,{},{},{},{},{},{},{},{},{},{},{},{}",
            row.phase,
            row.payload_len,
            row.variant,
            row.repetition,
            row.order_slot,
            row.iterations,
            row.elapsed_ns,
            row.counts.alloc,
            row.counts.zeroed,
            row.counts.realloc,
            row.counts.dealloc,
            row.counts.requested_bytes,
        );
    }
}

fn exact_flag(flag: Option<&OsStr>, has_extra: bool) -> bool {
    flag == Some(OsStr::new(FLAG)) && !has_extra
}

fn main() {
    if cfg!(debug_assertions) {
        return;
    }
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if !exact_flag(first.as_deref(), args.next().is_some()) {
        return;
    }
    benchmark();
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pure constructor/argument/order checks only: never enable global counters
    // in the potentially parallel test harness (including --release tests).
    #[test]
    fn fixed_cases_match_independent_literals() {
        verify_prefixes();
    }

    #[test]
    fn only_the_sole_exact_flag_is_accepted() {
        assert!(exact_flag(Some(OsStr::new(FLAG)), false));
        assert!(!exact_flag(None, false));
        assert!(!exact_flag(Some(OsStr::new(FLAG)), true));
        assert!(!exact_flag(
            Some(OsStr::new("--benchmark-grpc-prefix=1")),
            false
        ));
        assert!(!exact_flag(Some(OsStr::new("--help")), false));
    }

    #[test]
    fn both_orders_receive_equal_repetitions() {
        for case_index in 0..CASES.len() {
            let mut baseline_first = 0;
            for repetition in 0..REPETITIONS {
                let order = ordered_constructors(repetition + case_index);
                assert_ne!(order[0].0, order[1].0);
                baseline_first += usize::from(order[0].0 == 0);
            }
            assert_eq!(baseline_first * 2, REPETITIONS);
        }
    }
}
