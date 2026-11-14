//! Independent byte-level oracle for the measured request fast path.
use rambler_protocol::{AUDIO_DATA, AUDIO_INFO, Error, MAX_MESSAGE_BYTES, audio_request};

fn uint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let next = (value & 127) as u8;
        value >>= 7;
        out.push(next | if value == 0 { 0 } else { 128 });
        if value == 0 {
            return out;
        }
    }
}
fn blob(number: u32, data: &[u8]) -> Vec<u8> {
    [
        uint(u64::from(number) * 8 + 2),
        uint(data.len() as u64),
        data.to_vec(),
    ]
    .concat()
}
fn oracle(chunk: &[u8], elapsed: u64) -> Vec<u8> {
    let timestamp = [vec![0x48], uint(elapsed)].concat();
    [
        blob(AUDIO_INFO, &timestamp),
        blob(AUDIO_DATA, &blob(1, chunk)),
    ]
    .concat()
}

#[test]
fn audio_fast_path_matches_independent_oracle_at_varint_boundaries() {
    let lengths = [
        0, 1, 2, 117, 118, 119, 120, 121, 126, 127, 128, 129, 16_373, 16_374, 16_375, 16_376,
        16_383, 16_384, 16_385, 65_536,
    ];
    let elapsed_values = [
        0,
        1,
        127,
        128,
        16_383,
        16_384,
        (1 << 21) - 1,
        1 << 21,
        (1 << 35) - 1,
        1 << 35,
        i64::MAX as u64,
    ];
    for len in lengths {
        let chunk: Vec<_> = (0..len).map(|i| (i % 251) as u8).collect();
        for elapsed in elapsed_values {
            assert_eq!(
                audio_request(&chunk, elapsed).unwrap(),
                oracle(&chunk, elapsed)
            );
        }
    }
    let mut state = 0x1234_5678_9abc_def0u64;
    for _ in 0..2000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let len = (state % 8192) as usize;
        let chunk = vec![state as u8; len];
        let elapsed = state & i64::MAX as u64;
        assert_eq!(
            audio_request(&chunk, elapsed).unwrap(),
            oracle(&chunk, elapsed)
        );
    }
}

#[test]
fn audio_fast_path_accepts_exact_root_bound_and_rejects_next_byte() {
    for elapsed in [0, 127, 128, i64::MAX as u64] {
        // For these near-limit lengths all nested length prefixes are 4 bytes.
        // Two extension keys (4+4), two wrapper lengths (1+4), inner key+length
        // (1+4), and timestamp key+value (1+varint(elapsed).len()).
        let overhead = 19 + uint(elapsed).len();
        let mut chunk = vec![0xa5; MAX_MESSAGE_BYTES - overhead];
        let actual = audio_request(&chunk, elapsed).unwrap();
        assert_eq!(actual.len(), MAX_MESSAGE_BYTES);
        assert_eq!(actual, oracle(&chunk, elapsed));
        chunk.push(0xa5);
        assert_eq!(audio_request(&chunk, elapsed), Err(Error::SizeLimit));
    }
    assert_eq!(
        audio_request(&vec![0; MAX_MESSAGE_BYTES], 0),
        Err(Error::SizeLimit)
    );
    assert_eq!(
        audio_request(&vec![0; MAX_MESSAGE_BYTES + 1], 0),
        Err(Error::SizeLimit)
    );
    assert_eq!(
        audio_request(&[], 1 << 63),
        Err(Error::InvalidConfig("elapsed_ms range"))
    );
}
