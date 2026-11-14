//! Ownership/storage regression tests. Wire bytes are independently assembled.
use bytes::Bytes;
use rambler_protocol::{RECOGNITION_RESPONSE, decode_response};

fn blob(number: u32, body: &[u8]) -> Vec<u8> {
    fn uint(mut n: u64) -> Vec<u8> {
        let mut out = Vec::new();
        while n >= 128 {
            out.push(n as u8 | 128);
            n >>= 7;
        }
        out.push(n as u8);
        out
    }
    [
        uint(u64::from(number) * 8 + 2),
        uint(body.len() as u64),
        body.to_vec(),
    ]
    .concat()
}
fn response(recognition: &[u8]) -> Vec<u8> {
    blob(RECOGNITION_RESPONSE, &blob(1, recognition))
}
fn inside(root: &Bytes, child: &Bytes) -> bool {
    let start = root.as_ptr() as usize;
    let child_start = child.as_ptr() as usize;
    child_start >= start && child_start + child.len() <= start + root.len()
}

#[test]
fn copies_only_exact_root_and_single_occurrences_share_its_storage() {
    let hypothesis = blob(1, "synthetic é/𝄞".as_bytes());
    let list = blob(3, &hypothesis);
    let recognition = [vec![8, 1], blob(3, &list)].concat();
    let payload = response(&recognition);
    // A tiny view from a much larger transport allocation must not be retained.
    let mut frame = vec![0xa5; 2 * 1024 * 1024];
    frame[1024..1024 + payload.len()].copy_from_slice(&payload);
    let frame = Bytes::from(frame);
    let input = frame.slice(1024..1024 + payload.len());
    let decoded = decode_response(&input).unwrap();
    assert_eq!(decoded.raw, payload);
    assert_ne!(decoded.raw.as_ptr(), input.as_ptr());
    let result = decoded.result.as_ref().unwrap();
    let rec = result.recognition.as_ref().unwrap();
    let list = rec.hypothesis_result.as_ref().unwrap();
    let hyp = &list.hypotheses[0];
    for child in [
        &result.raw,
        result.active_raw.as_ref().unwrap(),
        &rec.raw,
        &list.raw,
        &hyp.raw,
    ] {
        assert!(inside(&decoded.raw, child));
    }
    assert_eq!(
        result.active_raw.as_ref().unwrap().as_ptr(),
        rec.raw.as_ptr()
    );
    let kept = hyp.clone();
    let detached = Bytes::copy_from_slice(&hyp.raw);
    assert_eq!(detached, hyp.raw);
    assert!(!inside(&decoded.raw, &detached));
    let cloned_root = decoded.raw.clone();
    assert_eq!(cloned_root.as_ptr(), decoded.raw.as_ptr());
    drop(decoded);
    drop(cloned_root);
    drop(input);
    drop(frame);
    assert_eq!(kept.raw, hypothesis);
    assert_eq!(kept.text.as_deref(), Some("synthetic é/𝄞"));
}

#[test]
fn merged_raw_is_logical_concat_but_children_still_view_original_root() {
    let first = blob(3, &blob(3, &blob(1, b"first")));
    let second = blob(3, &blob(3, &blob(1, b"second")));
    let payload = [response(&first), response(&second)].concat();
    let decoded = decode_response(&payload).unwrap();
    let result = decoded.result.as_ref().unwrap();
    let rec = result.recognition.as_ref().unwrap();
    assert_eq!(decoded.raw, payload);
    assert_eq!(rec.raw, [first, second].concat());
    assert!(!inside(&decoded.raw, &rec.raw));
    // The active_raw alias must not allocate a second copy of the merged body.
    assert_eq!(
        result.active_raw.as_ref().unwrap().as_ptr(),
        rec.raw.as_ptr()
    );
    let list = rec.hypothesis_result.as_ref().unwrap();
    assert!(!inside(&decoded.raw, &list.raw));
    for hyp in &list.hypotheses {
        assert!(inside(&decoded.raw, &hyp.raw));
    }
    let kept = list.clone();
    drop(decoded);
    assert_eq!(kept.hypotheses[0].text.as_deref(), Some("first"));
    assert_eq!(kept.hypotheses[1].text.as_deref(), Some("second"));
}
