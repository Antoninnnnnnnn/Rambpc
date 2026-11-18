use super::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
};

fn frame(body: &[u8]) -> Vec<u8> {
    let n = u32::try_from(body.len()).unwrap();
    let mut wire = vec![0, (n >> 24) as u8, (n >> 16) as u8, (n >> 8) as u8, n as u8];
    wire.extend(body);
    wire
}

#[test]
fn every_three_way_partition_matches_the_envelope_ends_and_original_payloads() {
    let bodies: [&[u8]; 4] = [&[8, 0], &[], &[8, 0, 0xfa, 1, 2, 0, 0xff], &[8, 1]];
    let wire: Vec<_> = bodies.iter().flat_map(|body| frame(body)).collect();
    for a in 0..=wire.len() {
        for b in a..=wire.len() {
            for end in b..=wire.len() {
                let mut decoder = Deframer::new(7);
                let mut delivered = 0;
                let mut expected_end = 0;
                for (start, stop) in [(0, a), (a, b), (b, end)] {
                    let mut next = decoder.frame(&wire[start..stop]).unwrap();
                    while delivered < bodies.len() {
                        let body = bodies[delivered];
                        if expected_end + 5 + body.len() > stop {
                            break;
                        }
                        assert_eq!(next.as_deref(), Some(body), "{a}, {b}, {end}, {stop}");
                        delivered += 1;
                        expected_end += 5 + body.len();
                        next = decoder.message().unwrap();
                    }
                    assert!(next.is_none());
                    assert_eq!(decoder.is_empty(), stop == expected_end);
                }
            }
        }
    }
}

#[test]
fn a_valid_prefix_is_delivered_before_an_error_even_if_both_are_in_one_chunk() {
    let prefix = frame(&[8, 0]);
    let limit = 7;
    for (suffix, expected) in [
        ([1, 0, 0, 0, 8], TransportError::CompressionUnsupported),
        ([2, 0, 0, 0, 8], TransportError::Protocol),
        (
            [0, 0, 0, 0, 8],
            TransportError::Grpc(Code::ResourceExhausted),
        ),
    ] {
        let wire = [prefix.as_slice(), &suffix].concat();
        for split in 0..=wire.len() {
            let mut decoder = Deframer::new(limit);
            let mut emitted = false;
            let mut found_error = false;
            for chunk in [&wire[..split], &wire[split..]] {
                let mut next = decoder.frame(chunk);
                loop {
                    match next {
                        Ok(None) => break,
                        Ok(Some(body)) => {
                            assert!(!emitted);
                            assert_eq!(body.as_ref(), &[8, 0]);
                            emitted = true;
                            next = decoder.message();
                        }
                        Err(error) => {
                            assert!(emitted);
                            assert!(
                                matches!(
                                    (&expected, &error),
                                    (
                                        TransportError::CompressionUnsupported,
                                        TransportError::CompressionUnsupported
                                    ) | (TransportError::Protocol, TransportError::Protocol)
                                        | (
                                            TransportError::Grpc(Code::ResourceExhausted),
                                            TransportError::Grpc(Code::ResourceExhausted)
                                        )
                                ),
                                "expected {expected:?}, got {error:?}"
                            );
                            found_error = true;
                            break;
                        }
                    }
                }
                if found_error {
                    break;
                }
            }
            assert!(found_error);
        }
    }
}

// Allocation accounting is local to the calling test thread. No TLS destructor,
// allocations, locking, or formatting in the allocator. No production allocator
// change (this module is only compiled into the unit-test binary).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Requests {
    alloc: usize,
    realloc: usize,
    requested: usize,
    live: usize,
    peak: usize,
}
thread_local! { static REQUESTS: Cell<Option<Requests>> = const { Cell::new(None) }; }

fn record(old: usize, new: usize, request: Option<bool>) {
    let _ = REQUESTS.try_with(|counts| {
        if let Some(mut counts_value) = counts.get() {
            match request {
                Some(false) => counts_value.alloc = counts_value.alloc.saturating_add(1),
                Some(true) => counts_value.realloc = counts_value.realloc.saturating_add(1),
                None => {}
            }
            counts_value.requested = counts_value.requested.saturating_add(new);
            counts_value.live = counts_value.live.saturating_sub(old).saturating_add(new);
            counts_value.peak = counts_value.peak.max(counts_value.live);
            counts.set(Some(counts_value));
        }
    });
}

struct MeasuringAllocator;
#[global_allocator]
static ALLOCATOR: MeasuringAllocator = MeasuringAllocator;

// SAFETY: Every operation forwards the same pointer, layout and size to System.
unsafe impl GlobalAlloc for MeasuringAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(0, layout.size(), Some(false));
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(0, layout.size(), Some(false));
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(layout.size(), size, Some(true));
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record(layout.size(), 0, None);
        unsafe { System.dealloc(ptr, layout) }
    }
}

trait Trial {
    fn frame(&mut self, chunk: &[u8]) -> Option<Bytes>;
    fn message(&mut self) -> Option<Bytes>;
    fn pending(&self) -> usize;
    fn copied(&self) -> usize;
}

// Pre-change decoder's successful-input path at 4cf9c4d (same extends,
// advances, and splits, with copied-input accounting). This measures cost;
// the positional oracle above independently checks correctness and EOF.
#[derive(Default)]
struct Before {
    buffer: BytesMut,
    copied: usize,
    limit: usize,
}
impl Trial for Before {
    fn frame(&mut self, chunk: &[u8]) -> Option<Bytes> {
        self.copied += chunk.len();
        self.buffer.extend_from_slice(chunk);
        self.message()
    }
    fn message(&mut self) -> Option<Bytes> {
        if self.buffer.len() < 5 {
            return None;
        }
        assert_eq!(self.buffer[0], 0);
        let size = u32::from_be_bytes(self.buffer[1..5].try_into().unwrap()) as usize;
        assert!(size <= self.limit);
        if self.buffer.len() - 5 < size {
            return None;
        }
        self.buffer.advance(5);
        Some(self.buffer.split_to(size).freeze())
    }
    fn pending(&self) -> usize {
        self.buffer.len()
    }
    fn copied(&self) -> usize {
        self.copied
    }
}
impl Trial for Deframer {
    fn frame(&mut self, chunk: &[u8]) -> Option<Bytes> {
        self.frame(chunk).unwrap()
    }
    fn message(&mut self) -> Option<Bytes> {
        self.message().unwrap()
    }
    fn pending(&self) -> usize {
        self.buffer.as_ref().map_or(0, BytesMut::len)
    }
    fn copied(&self) -> usize {
        self.copied_input
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Cost {
    copied: usize,
    pending_peak: usize,
    heap: Requests,
}

fn measure(mut decoder: impl Trial, frames: &[&[u8]], bodies: &[Vec<u8>], hold: usize) -> Cost {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            REQUESTS.set(None);
        }
    }
    let _guard = Reset;
    let mut held = [None, None];
    let mut emitted = 0;
    let mut pending_peak = 0;
    REQUESTS.set(Some(Requests::default()));
    for &chunk in frames {
        let mut next = decoder.frame(black_box(chunk));
        while let Some(bytes) = next {
            assert_eq!(bytes.as_ref(), bodies[emitted]);
            if hold == 0 {
                drop(black_box(bytes));
            } else {
                held[emitted % hold] = Some(black_box(bytes));
            }
            emitted += 1;
            next = decoder.message();
        }
        pending_peak = pending_peak.max(decoder.pending());
    }
    assert_eq!(emitted, bodies.len());
    assert_eq!(decoder.pending(), 0);
    let copied = decoder.copied();
    drop(held);
    drop(decoder);
    let heap = REQUESTS.take().unwrap();
    assert_eq!(heap.live, 0);
    Cost {
        copied,
        pending_peak,
        heap,
    }
}

fn compare(
    label: &str,
    limit: usize,
    frames: &[&[u8]],
    bodies: &[Vec<u8>],
    hold: usize,
) -> (Cost, Cost) {
    let before = measure(
        Before {
            limit,
            ..Before::default()
        },
        frames,
        bodies,
        hold,
    );
    let after = measure(Deframer::new(limit), frames, bodies, hold);
    for (variant, cost) in [("base", &before), ("candidate", &after)] {
        println!(
            "case={label} variant={variant} frames={} messages={} hold={hold} copied={} alloc={} realloc={} requested={} peak_live={} pending_peak={}",
            frames.len(),
            bodies.len(),
            cost.copied,
            cost.heap.alloc,
            cost.heap.realloc,
            cost.heap.requested,
            cost.heap.peak,
            cost.pending_peak,
        );
    }
    assert!(after.pending_peak <= before.pending_peak);
    (before, after)
}

// A synthetic S3 NORMAL response with an ignorable, varied field 31 (bytes).
// Payload length includes status, tag and length varint. No recorded data.
fn response(data_len: usize, seed: usize) -> Vec<u8> {
    let mut body = vec![8, 0, 0xfa, 1];
    let mut rest = data_len;
    while rest >= 128 {
        body.push((rest as u8 & 127) | 128);
        rest >>= 7;
    }
    body.push(rest as u8);
    body.extend((0..data_len).map(|offset| (offset * 13 + seed * 17) as u8));
    body
}

#[test]
fn receive_cost_profile() {
    const N: usize = 64;
    const LIMIT: usize = 64 * 1024;
    const DATA: usize = 16 * 1024;
    for (label, hold, data_len, n) in [
        ("cold-single", 0, 91, 1),
        ("exact-drop", 0, 91, N),
        ("exact-held-tiny", 2, 0, N),
        ("exact-held-96", 2, 91, N),
        ("exact-held-512", 2, 506, N),
        ("exact-held-4096", 2, 4090, N),
    ] {
        let bodies: Vec<_> = (0..n).map(|seed| response(data_len, seed)).collect();
        let frames: Vec<_> = bodies.iter().map(|body| frame(body)).collect();
        let frames: Vec<_> = frames.iter().map(Vec::as_slice).collect();
        let (before, after) = compare(label, LIMIT, &frames, &bodies, hold);
        if hold == 0 {
            assert_eq!(before, after);
        } else {
            assert_eq!(before.copied - after.copied, 5 * (n / 2));
            assert!(after.heap.alloc <= 3 * before.heap.alloc / 4);
            assert_eq!(after.heap.realloc, 0);
            assert!(after.heap.peak <= before.heap.peak);
        }
    }

    let bodies: Vec<_> = [11, 59, 250, 1018, 4090]
        .into_iter()
        .map(|n| response(n, n))
        .collect();
    let frames: Vec<_> = bodies.iter().map(|body| frame(body)).collect();
    let frames: Vec<_> = frames.iter().map(Vec::as_slice).collect();
    let (before, after) = compare("exact-growing-drop", LIMIT, &frames, &bodies, 0);
    assert_eq!(after.copied, before.copied);
    assert!(after.heap.alloc + after.heap.realloc <= before.heap.alloc + before.heap.realloc);

    let bodies = vec![Vec::new(); N];
    let frames = vec![&b"\0\0\0\0\0"[..]; N];
    let (before, after) = compare("exact-zero", 0, &frames, &bodies, 2);
    assert!(before.heap.alloc > 0);
    assert_eq!(
        after,
        Cost {
            copied: 0,
            pending_peak: 0,
            heap: Requests::default()
        }
    );

    let bodies: Vec<_> = (0..N).map(|seed| response(91, seed)).collect();
    let stream: Vec<_> = bodies.iter().flat_map(|body| frame(body)).collect();
    let fragment_frames: Vec<_> = stream
        .chunks(101)
        .flat_map(|one| [&one[..3], &one[3..]])
        .collect();
    let (before, after) = compare("fragmented", LIMIT, &fragment_frames, &bodies, 2);
    assert_eq!(before, after);
    let coalesced_frames: Vec<_> = stream.chunks(202).collect();
    let (before, after) = compare("coalesced", LIMIT, &coalesced_frames, &bodies, 2);
    assert_eq!(before, after);

    let bodies: Vec<_> = (0..4).map(|seed| response(LIMIT - 7, seed)).collect();
    assert!(bodies.iter().all(|b| b.len() == LIMIT));
    let wire: Vec<_> = bodies.iter().map(|body| frame(body)).collect();
    let frames: Vec<_> = wire.iter().flat_map(|wire| wire.chunks(DATA)).collect();
    let (before, after) = compare("limit-fragmented", LIMIT, &frames, &bodies, 2);
    assert_eq!(before, after);
    assert!(after.pending_peak <= LIMIT + 4);
    assert!(after.heap.peak <= 8 * (LIMIT + DATA));
}
