//! Static client-helper compatibility, not execution of Android or a live service.
use rambler_engine::*;
use rambler_protocol::{
    RECOGNITION_RESPONSE, VOICE_EDITING_RESULT, decode_response,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};

const CLIENT: RenderPolicy = RenderPolicy::S3ClientCompatible;
const PROFILE: CommandProfile = CommandProfile::S3DefaultPatternsV1;
const POLICIES: [RenderPolicy; 3] = [RenderPolicy::Strict, RenderPolicy::AsrOnly, CLIENT];

fn hypothesis(asr: Option<&str>, edit: Option<Vec<u8>>, safety: &[u8]) -> Vec<u8> {
    let mut wire = asr.map_or_else(Vec::new, |text| s(1, text).unwrap());
    if let Some(edit) = edit {
        wire.extend(b(VOICE_EDITING_RESULT, &edit).unwrap());
    }
    wire.extend_from_slice(safety);
    wire
}
fn update(hypotheses: &[Vec<u8>]) -> S3Response {
    let hypotheses: Vec<u8> = hypotheses.iter().flat_map(|h| b(3, h).unwrap()).collect();
    let recognition = [v(1, 1).unwrap(), b(3, &hypotheses).unwrap()].concat();
    decode_response(&b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap()).unwrap()
}
fn edit(intent: Option<u64>, field: Option<u32>, text: &str) -> Vec<u8> {
    let mut wire = s(1, "TRANSCRIPT MUST NEVER BE USED").unwrap();
    if let Some(intent) = intent {
        wire.extend(v(4, intent).unwrap());
    }
    match field {
        Some(7) => wire.extend(b(7, &s(1, text).unwrap()).unwrap()),
        Some(field) => wire.extend(s(field, text).unwrap()),
        None => {}
    }
    wire
}
fn result(asr: Option<&str>, edit: Option<Vec<u8>>) -> DictationResult {
    DictationResult {
        final_updates: vec![update(&[hypothesis(asr, edit, &[])])],
        context: Some(EditingContext {
            before: "captured BEFORE ".into(),
            selected: "captured SELECTION".into(),
            after: " captured AFTER".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn client_accepts_both_intents_and_members_without_changing_strict_contracts() {
    for intent in [2, 3] {
        for field in [3, 7] {
            for text in ["", "édité 🦀", "send"] {
                let result = result(Some("ASR"), Some(edit(Some(intent), Some(field), text)));
                let rendered = result.render(CLIENT).unwrap();
                assert_eq!(rendered.text, text);
                assert_eq!(rendered.intent, Some(intent as i32));
                assert_eq!(rendered.fallback_reason, None);
                assert_eq!(
                    rendered.provenance,
                    if field == 3 {
                        TextProvenance::VoiceEditText
                    } else {
                        TextProvenance::VoiceEditContext
                    }
                );
                assert_eq!(result.render(RenderPolicy::AsrOnly).unwrap().text, "ASR");
                if intent == 3 && field == 7 {
                    assert!(matches!(result.text(), Err(EngineError::AmbiguousResult)));
                    assert!(matches!(
                        result.render(RenderPolicy::Strict),
                        Err(EngineError::AmbiguousResult)
                    ));
                } else {
                    assert_eq!(result.text().unwrap(), text);
                    assert_eq!(result.render(RenderPolicy::Strict).unwrap().text, text);
                }
                if intent == 3 && field == 3 {
                    assert!(matches!(
                        replacement_proposal_with_policy(&result, None, CLIENT),
                        Err(EngineError::UnsupportedEdit)
                    ));
                } else {
                    assert_eq!(
                        replacement_proposal_with_policy(&result, None, CLIENT)
                            .unwrap()
                            .replacement_text,
                        text
                    );
                }
                if intent == 3 {
                    assert!(matches!(
                        replacement_proposal(&result, None),
                        Err(EngineError::UnsupportedEdit)
                    ));
                    assert!(matches!(
                        replacement_proposal_with_policy(&result, None, RenderPolicy::Strict),
                        Err(EngineError::UnsupportedEdit)
                    ));
                }
                assert!(matches!(
                    replacement_proposal_with_policy(&result, None, RenderPolicy::AsrOnly),
                    Err(EngineError::UnsupportedEdit)
                ));
            }
        }
    }
}

#[test]
fn fallback_is_explicit_and_only_uses_hypothesis_text_including_java_default() {
    for (edit, reason) in [
        (None, FallbackReason::VoiceEditAbsent),
        (
            Some(edit(Some(0), Some(3), "REJECTED")),
            FallbackReason::UnsupportedIntent,
        ),
        (
            Some(edit(Some(1), Some(7), "REJECTED")),
            FallbackReason::UnsupportedIntent,
        ),
        (
            Some(edit(Some(99), Some(3), "REJECTED")),
            FallbackReason::UnsupportedIntent,
        ),
        (
            Some(edit(None, Some(3), "REJECTED")),
            FallbackReason::UnsupportedIntent,
        ),
        (
            Some(edit(Some(2), Some(2), "ALTERNATIVE")),
            FallbackReason::UnsupportedMember,
        ),
        (
            Some(edit(Some(3), None, "")),
            FallbackReason::UnsupportedMember,
        ),
    ] {
        for asr in [Some("ASR"), Some(""), None] {
            let result = result(asr, edit.clone());
            let rendered = result.render(CLIENT).unwrap();
            assert_eq!(rendered.text, asr.unwrap_or_default());
            assert_eq!(rendered.provenance, TextProvenance::HypothesisText);
            assert_eq!(rendered.fallback_reason, Some(reason));
            assert!(matches!(
                replacement_proposal_with_policy(&result, None, CLIENT),
                Err(EngineError::UnsupportedEdit)
            ));
            assert!(matches!(
                result.classify_command(PROFILE, Some(4)),
                Err(EngineError::UnsupportedEdit)
            ));
            if edit.is_some() {
                assert!(matches!(result.text(), Err(EngineError::AmbiguousResult)));
            }
            if asr.is_none() {
                assert!(matches!(
                    result.render(RenderPolicy::AsrOnly),
                    Err(EngineError::AmbiguousResult)
                ));
            }
        }
    }
}

#[test]
fn proposals_preserve_full_window_snapshot_revision_and_utf16_selection() {
    let context = [
        s(1, "a🦀").unwrap(),
        s(2, "é🦀").unwrap(),
        s(3, "後").unwrap(),
    ]
    .concat();
    for intent in [2, 3] {
        let result = result(
            Some("ASR"),
            Some([v(4, intent).unwrap(), b(7, &context).unwrap()].concat()),
        );
        let snapshot = result.context.clone().unwrap();
        let revision =
            Revision::Integer(DecimalRevision::new("1844674407370955161600000000000001").unwrap());
        let proposal =
            replacement_proposal_with_policy(&result, Some(revision.clone()), CLIENT).unwrap();
        assert_eq!(proposal.replacement_text, "a🦀é🦀後");
        assert_eq!(proposal.selection_utf16_range(), Some((3, 6)));
        assert_eq!(proposal.snapshot, snapshot);
        proposal
            .validate_current(&snapshot, Some(&revision))
            .unwrap();
        for changed in [
            Revision::from("1844674407370955161600000000000001"),
            Revision::from(1i64),
        ] {
            assert!(matches!(
                proposal.validate_current(&snapshot, Some(&changed)),
                Err(EngineError::ContextConflict)
            ));
        }
        assert!(matches!(
            proposal.validate_current(&snapshot, None),
            Err(EngineError::ContextConflict)
        ));
        for member in 0..3 {
            let mut current = snapshot.clone();
            match member {
                0 => current.before.push('x'),
                1 => current.selected.push('x'),
                _ => current.after.push('x'),
            }
            assert!(matches!(
                proposal.validate_current(&current, Some(&revision)),
                Err(EngineError::ContextConflict)
            ));
        }
        assert_eq!(result.context.as_ref(), Some(&snapshot)); // No editor mutation.
        let mut absent = result.clone();
        absent.context = None;
        assert!(matches!(
            replacement_proposal_with_policy(&absent, None, CLIENT),
            Err(EngineError::UnsupportedEdit)
        ));
    }
}

#[test]
fn every_policy_keeps_global_safety_and_ambiguity_before_fallback_or_commands() {
    let accepted = hypothesis(Some("ASR"), Some(edit(Some(3), Some(7), "send")), &[]);
    for count in [0, 2] {
        let mut result = result(Some("ASR"), None);
        result.final_updates = vec![update(&vec![accepted.clone(); count])];
        assert_ambiguous(&result);
    }
    let mut multiple = result(Some("ASR"), Some(edit(Some(3), Some(7), "send")));
    multiple
        .final_updates
        .push(multiple.final_updates[0].clone());
    assert_ambiguous(&multiple);
    multiple.final_updates.clear();
    assert_ambiguous(&multiple);
    for (safety, blocked) in [
        (v(30, 1).unwrap(), true),
        (v(30, 2).unwrap(), false),
        (b(30, b"unknown").unwrap(), false),
    ] {
        let unsafe_candidate = hypothesis(Some("ASR"), None, &safety);
        for hypotheses in [
            vec![unsafe_candidate.clone()],
            vec![accepted.clone(), unsafe_candidate.clone()],
        ] {
            let mut result = result(Some("ASR"), None);
            result.final_updates = vec![update(&hypotheses)];
            for policy in POLICIES {
                assert_safety(result.render(policy), blocked);
                assert_safety(
                    replacement_proposal_with_policy(&result, None, policy),
                    blocked,
                );
            }
            assert_safety(result.classify_command(PROFILE, Some(4)), blocked);
            result
                .final_updates
                .push(update(std::slice::from_ref(&accepted)));
            for policy in POLICIES {
                assert_safety(result.render(policy), blocked);
            }
        }
    }
}
fn assert_safety<T>(result: Result<T, EngineError>, blocked: bool) {
    assert!(if blocked {
        matches!(result, Err(EngineError::SafetyBlocked))
    } else {
        matches!(result, Err(EngineError::UnrecognizedSafety))
    });
}
fn assert_ambiguous(result: &DictationResult) {
    for policy in POLICIES {
        assert!(matches!(
            result.render(policy),
            Err(EngineError::AmbiguousResult)
        ));
        assert!(matches!(
            replacement_proposal_with_policy(result, None, policy),
            Err(EngineError::AmbiguousResult)
        ));
    }
    assert!(matches!(
        result.classify_command(PROFILE, Some(4)),
        Err(EngineError::AmbiguousResult)
    ));
}

#[test]
fn oneof_wire_order_not_populated_old_members_controls_render_and_proposal() {
    for fields in [[3, 7], [7, 3], [3, 2], [2, 7]] {
        let wire = [
            edit(Some(3), Some(fields[0]), "FIRST"),
            edit(Some(3), Some(fields[1]), "LAST"),
        ]
        .concat();
        let result = result(Some("ASR"), Some(wire));
        let rendered = result.render(CLIENT).unwrap();
        if fields[1] == 2 {
            assert_eq!(rendered.text, "ASR");
            assert_eq!(
                rendered.fallback_reason,
                Some(FallbackReason::UnsupportedMember)
            );
            assert!(matches!(
                replacement_proposal_with_policy(&result, None, CLIENT),
                Err(EngineError::UnsupportedEdit)
            ));
        } else {
            assert_eq!(rendered.text, "LAST");
            if fields[1] == 3 {
                assert!(matches!(
                    replacement_proposal_with_policy(&result, None, CLIENT),
                    Err(EngineError::UnsupportedEdit)
                ));
            } else {
                assert_eq!(
                    replacement_proposal_with_policy(&result, None, CLIENT)
                        .unwrap()
                        .replacement_text,
                    "LAST"
                );
            }
        }
    }
}

#[test]
fn default_command_patterns_are_ascii_exact_and_java_trimmed_without_locale_gates() {
    use CommandKind::*;
    for (text, expected) in [
        ("send", Some(Send)),
        ("SeNt", Some(Send)),
        ("!SEND?", Some(Send)),
        ("éSendé", Some(Send)),
        ("🦀sent🦀", Some(Send)),
        ("\u{a0}send\u{a0}", Some(Send)),
        ("ſend", None),
        ("ｓｅｎｄ", None),
        ("send1", None),
        ("1send", None),
        ("send send", None),
        ("sénd", None),
        ("", None),
        (" ", None),
        ("!!!", None),
        ("clear", Some(DeleteLastSentence)),
        ("ClEaR!", Some(DeleteLastSentence)),
        ("clearé", Some(DeleteLastSentence)),
        ("clearſ", Some(DeleteLastSentence)),
        ("\0\u{1f}clear\u{1e}\0", Some(DeleteLastSentence)),
        ("\u{a0}clear", None),
        ("\u{85}clear", None),
        ("\u{2003}clear", None),
        ("éclear", None),
        ("!clear", None),
        ("clear1", None),
        ("clear all", Some(DeleteAll)),
        ("CLEAR\tALL!", Some(DeleteAll)),
        ("clear\nall", Some(DeleteAll)),
        ("clear\u{b}all", Some(DeleteAll)),
        ("clear\u{c}all", Some(DeleteAll)),
        ("clear\rall", Some(DeleteAll)),
        ("clear  all", None),
        ("clear\r\nall", None),
        ("clear\0all", None),
        ("clear\u{1f}all", None),
        ("clear\u{a0}all", None),
        ("clear\u{2003}all", None),
        ("clearall", None),
        ("clear all1", None),
        ("clear all🦀", Some(DeleteAll)),
        ("clear all clear", None),
        ("İSEND", Some(Send)),
        ("cleaK", None),
    ] {
        let result = result(Some("clear all"), Some(edit(Some(3), Some(3), text)));
        let candidate = result.classify_command(PROFILE, Some(4)).unwrap();
        assert_eq!(
            candidate.map(|candidate| candidate.command),
            expected,
            "{text:?}"
        );
        if let Some(candidate) = candidate {
            assert!(!candidate.applied);
            assert_eq!(candidate.intent, Some(3));
            assert_eq!(candidate.provenance, TextProvenance::VoiceEditText);
            assert_eq!(candidate.profile, PROFILE);
            assert_eq!(
                candidate.disposition,
                if expected == Some(Send) {
                    CommandDisposition::EligibleForExplicitSend
                } else {
                    CommandDisposition::RejectedByClientBranch
                }
            );
        }
    }
}

#[test]
fn send_requires_exact_ime4_and_context_render_is_full_window_not_selected_only() {
    for action in [None, Some(0), Some(3), Some(4), Some(260)] {
        let result = result(Some("ASR"), Some(edit(Some(2), Some(7), "send")));
        let candidate = result.classify_command(PROFILE, action).unwrap().unwrap();
        assert_eq!(candidate.command, CommandKind::Send);
        assert_eq!(candidate.preconditions.required_ime_action, Some(4));
        assert_eq!(candidate.preconditions.observed_ime_action, action);
        assert_eq!(
            candidate.disposition,
            if action == Some(4) {
                CommandDisposition::EligibleForExplicitSend
            } else {
                CommandDisposition::RequiresImeAction4
            }
        );
        assert!(!candidate.applied);
    }
    let context = [
        s(1, "existing ").unwrap(),
        s(2, "send").unwrap(),
        s(3, " text").unwrap(),
    ]
    .concat();
    let result = result(
        Some("send"),
        Some([v(4, 2).unwrap(), b(7, &context).unwrap()].concat()),
    );
    assert_eq!(result.classify_command(PROFILE, Some(4)).unwrap(), None);
}

#[test]
fn asr_transcript_and_rejected_edits_never_become_command_candidates() {
    for edit in [
        None,
        Some(edit(Some(99), Some(3), "send")),
        Some(edit(Some(2), Some(2), "send")),
    ] {
        let result = result(Some("send"), edit);
        assert!(matches!(
            result.classify_command(PROFILE, Some(4)),
            Err(EngineError::UnsupportedEdit)
        ));
    }
    for field in [3, 7] {
        let wire = [
            s(1, "send").unwrap(),
            v(4, 2).unwrap(),
            if field == 3 {
                s(3, "").unwrap()
            } else {
                b(7, &[]).unwrap()
            },
        ]
        .concat();
        let result = result(Some("send"), Some(wire));
        assert_eq!(result.classify_command(PROFILE, Some(4)).unwrap(), None);
    }
}
