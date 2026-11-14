//! Capacity is an implementation regression check, not a public API guarantee.
//! Independent literal encoder: no production wire helpers.
use rambler_protocol::{Error, MAX_FIELDS, RECOGNITION_RESPONSE, decode_response};

fn uint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    while value >= 128 {
        bytes.push(value as u8 | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}
fn blob(number: u32, body: &[u8]) -> Vec<u8> {
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

fn expected_capacity(count: usize) -> usize {
    if count <= 4 {
        count
    } else {
        count.next_power_of_two()
    }
}

#[test]
fn repeated_vectors_bound_initial_capacity_preserving_duplicates_order_and_unknowns() {
    for count in [0, 1, 2, 3, 4, 5, 16, 257] {
        for container in [3, 5, 7] {
            let first: Vec<_> = (0..count)
                .flat_map(|i| blob(3, &blob(1, if i % 3 == 0 { b"same" } else { b"other" })))
                .collect();
            // Same field number but wrong wire must not inflate the item count.
            let list = [first.clone(), vec![24, 1], blob(99, b"opaque")].concat();
            let root = response(&blob(container, &list));
            let decoded = decode_response(&root).unwrap();
            let recognition = decoded
                .result
                .as_ref()
                .unwrap()
                .recognition
                .as_ref()
                .unwrap();
            let result = match container {
                3 => recognition.hypothesis_result.as_ref(),
                5 => recognition.hypothesis_result_field5.as_ref(),
                _ => recognition.hypothesis_result_field7.as_ref(),
            }
            .unwrap();
            assert_eq!(result.hypotheses.len(), count);
            assert_eq!(result.hypotheses.capacity(), expected_capacity(count));
            assert_eq!(result.unknown_fields.len(), 2);
            assert_eq!(result.unknown_fields[0].raw, [24, 1]);
            assert_eq!(result.unknown_fields[1].raw, blob(99, b"opaque"));
            assert_eq!(result.raw, list);
            assert_eq!(decoded.raw, root);
            for (i, item) in result.hypotheses.iter().enumerate() {
                assert_eq!(
                    item.text.as_deref(),
                    Some(if i % 3 == 0 { "same" } else { "other" })
                );
            }
        }
        let item = blob(1, &blob(1, b"partial"));
        let list = [item.repeat(count), vec![8, 0]].concat();
        let decoded = decode_response(&response(&blob(4, &list))).unwrap();
        let result = decoded
            .result
            .as_ref()
            .unwrap()
            .recognition
            .as_ref()
            .unwrap()
            .partial_result
            .as_ref()
            .unwrap();
        assert_eq!(result.partials.len(), count);
        assert_eq!(result.partials.capacity(), expected_capacity(count));
        assert_eq!(result.unknown_fields[0].raw, [8, 0]);
        assert!(
            result
                .partials
                .iter()
                .all(|p| p.text.as_deref() == Some("partial"))
        );
    }
}

#[test]
fn merged_occurrences_count_only_matching_children_and_keep_empty_presence() {
    let first = blob(3, &blob(1, b"first"));
    let last = blob(3, &blob(1, b"last"));
    let recognition = [blob(3, &first), blob(3, b""), blob(3, &last), blob(4, b"")].concat();
    let decoded = decode_response(&response(&recognition)).unwrap();
    let result = decoded
        .result
        .as_ref()
        .unwrap()
        .recognition
        .as_ref()
        .unwrap();
    let hypotheses = result.hypothesis_result.as_ref().unwrap();
    assert_eq!(hypotheses.hypotheses.capacity(), 2);
    assert_eq!(hypotheses.raw, [first, last].concat());
    assert_eq!(
        hypotheses
            .hypotheses
            .iter()
            .map(|h| h.text.as_deref())
            .collect::<Vec<_>>(),
        [Some("first"), Some("last")]
    );
    let partials = result.partial_result.as_ref().unwrap();
    assert!(partials.partials.is_empty());
    assert_eq!(partials.partials.capacity(), 0);
}

#[test]
fn large_unvalidated_repeated_lists_keep_the_first_error_with_or_without_valid_prefix() {
    for (container, child) in [(3, 3), (4, 1)] {
        for prefix in [0, 1] {
            let list = [
                blob(child, b"").repeat(prefix),
                blob(child, &[0]).repeat(90_000 - prefix),
            ]
            .concat();
            let raw = response(&blob(container, &list));
            assert_eq!(decode_response(&raw), Err(Error::FieldNumber));
        }
    }
}

#[test]
fn sparse_candidates_preserve_unknowns_without_requiring_a_full_count_hint() {
    for (container, child) in [(3, 3), (4, 1)] {
        for count in [1, 2] {
            let item = blob(child, &blob(1, b"candidate"));
            let list = [
                item.clone(),
                [0x98, 0x06, 0].repeat(20_000),
                item.repeat(count - 1),
            ]
            .concat();
            let decoded = decode_response(&response(&blob(container, &list))).unwrap();
            let result = decoded
                .result
                .as_ref()
                .unwrap()
                .recognition
                .as_ref()
                .unwrap();
            let (len, capacity, unknown) = if container == 3 {
                let list = result.hypothesis_result.as_ref().unwrap();
                (
                    list.hypotheses.len(),
                    list.hypotheses.capacity(),
                    &list.unknown_fields,
                )
            } else {
                let list = result.partial_result.as_ref().unwrap();
                (
                    list.partials.len(),
                    list.partials.capacity(),
                    &list.unknown_fields,
                )
            };
            assert_eq!(len, count);
            assert_eq!(capacity, if count == 1 { 1 } else { 4 });
            assert_eq!(unknown.len(), 20_000);
            assert!(
                unknown
                    .iter()
                    .all(|f| f.number == 99 && f.raw == [0x98, 0x06, 0])
            );
        }
    }
}

#[test]
fn capacity_count_does_not_reparse_or_double_charge_the_global_budget() {
    // root + result + recognition = three wrappers; each empty repeated child
    // is one field and has no fields of its own.
    let count = MAX_FIELDS - 3;
    let raw = response(&blob(3, &blob(3, b"").repeat(count)));
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
            .hypotheses
            .capacity(),
        expected_capacity(count)
    );
    drop(decoded);
    let over = [raw, vec![8, 0]].concat();
    assert_eq!(decode_response(&over), Err(Error::FieldLimit));
}
