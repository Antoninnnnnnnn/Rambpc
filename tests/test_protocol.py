"""Offline vectors from the static schema; no network, credentials or APK code.

The tiny reference encoder below is independent of rambler.wire and is used only
for synthetic response fixtures. Exact request vectors additionally use literals.
"""

from dataclasses import FrozenInstanceError
import struct

import pytest

from rambler.protocol import (
    MAX_FIELDS,
    MAX_MESSAGE_BYTES,
    EditingContext,
    ProtocolConfig,
    S3Response,
    S3Status,
    SpeechEvent,
    audio_request,
    context_request,
    decode_response,
    end_request,
    initial_request,
)
from rambler.wire import Field, parse_fields


# Independent, deliberately minimal protobuf encoder for the test corpus.
def uint(value):
    result = bytearray()
    while value > 127:
        result.append((value % 128) + 128)
        value //= 128
    result.append(value)
    return bytes(result)


def scalar(number, value):
    return uint(number * 8) + uint(value)


def blob(number, value):
    if isinstance(value, str):
        value = value.encode("utf-8")
    return uint(number * 8 + 2) + uint(len(value)) + value


def fixed(number, value, *, double=False):
    return uint(number * 8 + (1 if double else 5)) + struct.pack("<d" if double else "<f", value)


def response(recognition):
    return blob(1253625, blob(1, recognition))


def with_hypothesis(hypothesis):
    return response(blob(3, blob(3, hypothesis)))


def with_edit(edit):
    return with_hypothesis(blob(74956224, edit))


def fields(payload):
    return {(item.number, item.wire_type): item.value for item in parse_fields(payload)}


def test_default_init_exact_static_vector():
    # Keys derived as (extension_number << 3) | 2. 16000f=0x467a0000.
    # No unknown settings, model/build, experiment ids or user tokens are emitted.
    expected = bytes.fromhex(
        "0a15" "6265796f6e642d6132612d7265636f676e697a6572" "1001"
        "c2888f01" "09" "12070a0566722d4652"
        "e28e8f01" "0b" "1500007a46180a20014800"
        "82c78f01" "13" "120667626f6172644206507974686f6ed00101"
        "a2e68f01" "3a" "0a02200028016801a00101c80201920329"
        "67626f6172645f67656d696e695f76335f73747265616d696e675f766f6963655f656469745f6d756c"
        "a2bfb771" "03" "08f403"
    )
    assert initial_request(ProtocolConfig()) == expected


def test_init_nested_fields_and_custom_honest_client():
    config = ProtocolConfig(language="en-GB", application_id="my-native-client", platform="Linux")
    request = fields(initial_request(config, elapsed_ms=12345))
    assert request[1, 2] == b"beyond-a2a-recognizer"
    assert request[2, 0] == 1
    assert set(number for number, wire in request) == {1, 2, 293000, 293100, 294000, 294500, 29735924}
    audio = fields(request[293100, 2])
    assert audio == {(2, 5): struct.pack("<f", 16000), (3, 0): 10, (4, 0): 1, (9, 0): 12345}
    recognizer = fields(request[294500, 2])
    assert fields(recognizer[1, 2]) == {(4, 0): 0}
    assert {number: recognizer[number, 0] for number in (5, 13, 20, 41)} == dict.fromkeys((5, 13, 20, 41), 1)
    assert recognizer[50, 2] == b"gboard_gemini_v3_streaming_voice_edit_mul"
    assert all(number not in (7, 40, 64, 91) for number, wire in recognizer)
    assert fields(request[294000, 2]) == {(2, 2): b"my-native-client", (8, 2): b"Linux", (26, 0): 1}
    assert fields(fields(request[293000, 2])[2, 2]) == {(1, 2): b"en-GB"}
    assert fields(request[29735924, 2]) == {(1, 0): 500}


def test_explicit_audio_values_are_not_relabelled_as_backend_supported():
    config = ProtocolConfig(sample_rate=48000, encoding=42, channels=2, routing="caller-routing")
    request = fields(initial_request(config))
    assert fields(request[293100, 2])[2, 5] == struct.pack("<f", 48000)
    assert fields(request[293100, 2])[3, 0] == 42
    assert fields(request[293100, 2])[4, 0] == 2
    assert fields(request[294500, 2])[50, 2] == b"caller-routing"


def test_audio_exact_vector_and_unmodified_opaque_bytes():
    assert audio_request(b"abc", elapsed_ms=150) == bytes.fromhex("e28e8f0103489601ea8e8f01050a03616263")
    chunk = b"OggS\x00\xff\x80\x00not-a-packet-boundary"
    parsed = fields(audio_request(chunk, elapsed_ms=2**40))
    assert fields(parsed[293100, 2]) == {(9, 0): 2**40}
    assert fields(parsed[293101, 2]) == {(1, 2): chunk}
    assert fields(fields(audio_request(b""))[293101, 2]) == {(1, 2): b""}


def test_context_exact_vectors_and_unicode():
    assert context_request(EditingContext(before="abc")) == bytes.fromhex("9a4b050a03616263")
    assert context_request(EditingContext("a", "b", "c", True)) == bytes.fromhex("9a4b0b22090a01611201621a0163")
    assert context_request(EditingContext()) == bytes.fromhex("9a4b020a00")
    assert context_request(EditingContext(cursor_aware=True)) == bytes.fromhex("9a4b0822060a0012001a00")
    context = EditingContext("avant é", "𝄞", "après", True)
    nested = fields(fields(context_request(context))[1203, 2])
    assert set(nested) == {(4, 2)}
    assert fields(nested[4, 2]) == {(1, 2): "avant é".encode(), (2, 2): "𝄞".encode(), (3, 2): "après".encode()}


@pytest.mark.parametrize("context", [EditingContext(selected="selection"), EditingContext(after="after")])
def test_simple_context_never_silently_discards_selection_or_after(context):
    with pytest.raises(ValueError, match="cursor_aware"):
        context_request(context)


def test_end_is_exactly_both_verified_flags():
    assert end_request() == b"\x10\x01\x18\x01"
    assert parse_fields(end_request()) == [Field(2, 0, 1), Field(3, 0, 1)]


def test_configs_are_frozen():
    with pytest.raises(FrozenInstanceError):
        ProtocolConfig().language = "other"
    with pytest.raises(FrozenInstanceError):
        EditingContext().before = "other"
    with pytest.raises(FrozenInstanceError):
        decode_response(b"").status = 0


@pytest.mark.parametrize("kwargs", [
    {"language": ""}, {"application_id": ""}, {"routing": ""}, {"platform": ""},
    {"language": "\ud800"}, {"sample_rate": 0}, {"sample_rate": 1.5},
    {"sample_rate": True}, {"encoding": -1}, {"encoding": 2**31},
    {"channels": 0}, {"channels": False},
])
def test_invalid_config(kwargs):
    with pytest.raises(ValueError):
        ProtocolConfig(**kwargs)


@pytest.mark.parametrize("elapsed", [-1, True, 1.0, 2**63])
def test_invalid_elapsed(elapsed):
    with pytest.raises(ValueError):
        initial_request(ProtocolConfig(), elapsed_ms=elapsed)
    with pytest.raises(ValueError):
        audio_request(b"", elapsed_ms=elapsed)


@pytest.mark.parametrize("chunk", ["text", bytearray(b"bytes"), memoryview(b"bytes"), None])
def test_audio_requires_bytes(chunk):
    with pytest.raises(ValueError):
        audio_request(chunk)


def test_request_input_validation_and_size_bounds():
    with pytest.raises(ValueError):
        initial_request(None)
    with pytest.raises(ValueError):
        context_request(None)
    with pytest.raises(ValueError):
        EditingContext(before="\ud800")
    with pytest.raises(ValueError):
        EditingContext(cursor_aware=1)
    with pytest.raises(ValueError):
        audio_request(b"x" * MAX_MESSAGE_BYTES)  # Wrapper also counts toward limit.
    with pytest.raises(ValueError):
        context_request(EditingContext(before="x" * MAX_MESSAGE_BYTES))


def test_empty_response_preserves_absence_and_no_final_claim():
    decoded = decode_response(b"")
    assert isinstance(decoded, S3Response)
    assert decoded.status is None and decoded.error_code is None and decoded.error_message is None
    assert decoded.recognition_state is None and decoded.speech_event is None
    assert decoded.hypotheses == decoded.partials == ()
    assert decoded.raw == b"" and decoded.unknown_fields == ()
    assert not hasattr(decoded, "final") and not hasattr(decoded, "is_final")


def test_error_exact_vector_last_wins_and_signed_int32():
    payload = bytes.fromhex("080210071a046f6f7073")
    decoded = decode_response(payload)
    assert (decoded.status, decoded.error_code, decoded.error_message) == (S3Status.ERROR, 7, "oops")
    assert decoded.raw == payload
    decoded = decode_response(scalar(1, 0) + scalar(1, 3) + scalar(2, 1) + scalar(2, 2**64 - 1) + blob(3, "old") + blob(3, "new"))
    assert (decoded.status, decoded.error_code, decoded.error_message) == (S3Status.NOT_STARTED, -1, "new")
    assert decode_response(scalar(1, 317)).status == 317


def test_status_and_speech_event_enums_are_raw_compatible():
    assert [int(item) for item in S3Status] == [0, 1, 2, 3]
    assert [int(item) for item in SpeechEvent] == [0, 1, 3]
    for value in (0, 1, 3, 1234):
        decoded = decode_response(blob(1253625, blob(2, scalar(1, value))))
        assert decoded.speech_event == value
        assert type(decoded.speech_event) is int
        assert decoded.recognition_state is None and decoded.hypotheses == ()


def test_nested_hypotheses_partials_and_voice_edit_context():
    cursor = blob(1, "avant ") + blob(2, "sélection") + blob(3, " après") + scalar(77, 9)
    edit = blob(1, "transcript <cursor>brut</cursor>") + blob(7, cursor) + scalar(4, 6) + blob(99, b"unknown-edit")
    hypothesis = blob(1, "ASR candidat") + fixed(2, 0.75) + blob(74956224, edit) + scalar(80, 4)
    hypothesis2 = blob(1, "autre candidat") + fixed(2, 0.5)
    hypothesis_list = blob(3, hypothesis) + blob(3, hypothesis2) + scalar(81, 5)
    partial = blob(1, "partiel") + fixed(2, 0.95, double=True) + scalar(82, 6)
    partial2 = blob(1, " suite") + fixed(2, 0.125, double=True)
    partial_list = blob(1, partial) + blob(1, partial2) + blob(83, b"opaque")
    recognition = scalar(1, 23) + blob(3, hypothesis_list) + blob(4, partial_list) + scalar(84, 7)
    result = blob(1, recognition) + blob(3, "result-id") + blob(4, "auxiliary") + scalar(85, 8)
    payload = scalar(1, 0) + blob(1253625, result) + scalar(86, 9)
    decoded = decode_response(payload)
    assert decoded.status == 0 and decoded.recognition_state == 23
    assert decoded.speech_event is None
    assert [item.text for item in decoded.hypotheses] == ["ASR candidat", "autre candidat"]
    assert [item.confidence for item in decoded.hypotheses] == [0.75, 0.5]
    assert [item.text for item in decoded.partials] == ["partiel", " suite"]
    assert [item.confidence for item in decoded.partials] == [0.95, 0.125]
    voice = decoded.hypotheses[0].voice_edit
    assert voice.transcript == "transcript <cursor>brut</cursor>"
    assert voice.intent == 6 and voice.active_field == 7
    assert voice.alternative_text is None and voice.edited_text is None
    assert voice.edited_context == EditingContext("avant ", "sélection", " après", True, unknown_fields=(Field(77, 0, 9),))
    assert voice.edited_context.raw == cursor
    assert voice.unknown_fields == (Field(99, 2, b"unknown-edit"),)
    assert voice.raw == edit
    assert decoded.hypotheses[0].unknown_fields == (Field(80, 0, 4),)
    assert decoded.hypotheses[0].raw == hypothesis
    assert decoded.partials[0].unknown_fields == (Field(82, 0, 6),)
    assert decoded.partials[0].raw == partial
    assert decoded.result.recognition.hypothesis_result.unknown_fields == (Field(81, 0, 5),)
    assert decoded.result.recognition.partial_result.unknown_fields == (Field(83, 2, b"opaque"),)
    assert decoded.result.recognition.unknown_fields == (Field(84, 0, 7),)
    assert decoded.result.recognition.raw == recognition
    assert decoded.result.unknown_fields == (Field(85, 0, 8),)
    assert decoded.result.raw == result
    assert decoded.result.result_id == "result-id" and decoded.result.alternative_text == "auxiliary"
    assert decoded.unknown_fields == (Field(86, 0, 9),) and decoded.raw == payload


@pytest.mark.parametrize("intent", range(7))
def test_all_verified_intents_are_preserved_without_applying_edits(intent):
    decoded = decode_response(with_edit(blob(1, "raw") + blob(3, "candidate edit") + scalar(4, intent)))
    edit = decoded.hypotheses[0].voice_edit
    assert edit.intent == intent and type(edit.intent) is int
    assert edit.transcript == "raw" and edit.edited_text == "candidate edit"
    assert decoded.hypotheses[0].text is None


@pytest.mark.parametrize("last", [2, 3, 7])
def test_voice_edit_oneof_last_member_wins(last):
    alternatives = {2: blob(2, "alternative"), 3: blob(3, "edited"), 7: blob(7, blob(1, "before"))}
    sequence = [number for number in alternatives if number != last] + [last]
    voice = decode_response(with_edit(blob(1, "raw") + b"".join(alternatives[number] for number in sequence) + scalar(4, 99))).hypotheses[0].voice_edit
    assert voice.active_field == last and voice.intent == 99 and voice.transcript == "raw"
    assert (voice.alternative_text is not None) == (last == 2)
    assert (voice.edited_text is not None) == (last == 3)
    assert (voice.edited_context is not None) == (last == 7)


@pytest.mark.parametrize("last", [1, 2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 18, 19, 20, 21])
def test_response_payload_oneof_and_independent_strings(last):
    recognition = blob(1, scalar(1, 17))
    speech = blob(2, scalar(1, 3))
    alternative = blob(last, scalar(1, 44))
    result = recognition + speech + alternative + blob(3, "id") + blob(4, "independent")
    decoded = decode_response(blob(1253625, result))
    assert decoded.result.active_field == last
    assert decoded.result.active_raw == scalar(1, 44)
    assert decoded.result.result_id == "id" and decoded.result.alternative_text == "independent"
    assert decoded.recognition_state == (44 if last == 1 else None)
    assert decoded.speech_event == (44 if last == 2 else None)
    if last not in (1, 2):
        assert decoded.result.unknown_fields == (Field(last, 2, scalar(1, 44)),)


def test_unknown_non_oneof_field_and_wrong_wire_type_do_not_clear_result():
    result = blob(1, scalar(1, 9)) + blob(14, b"future") + scalar(2, 22)
    decoded = decode_response(blob(1253625, result))
    assert decoded.recognition_state == 9
    assert decoded.result.unknown_fields == (Field(14, 2, b"future"), Field(2, 0, 22))


def test_singular_last_wins_repeated_items_preserve_order():
    hypothesis = blob(1, "old") + blob(1, "new") + fixed(2, 0.25) + fixed(2, 0.5)
    partial = blob(1, "old") + blob(1, "new") + fixed(2, 0.25, double=True) + fixed(2, 0.75, double=True)
    old_list = blob(3, blob(1, "discarded"))
    new_list = blob(3, hypothesis) + blob(3, blob(1, "second"))
    recognition = scalar(1, 0) + scalar(1, 77) + blob(3, old_list) + blob(3, new_list) + blob(4, blob(1, partial))
    decoded = decode_response(response(scalar(1, 0)) + response(recognition))
    assert decoded.recognition_state == 77
    assert [item.text for item in decoded.hypotheses] == ["new", "second"]
    assert decoded.hypotheses[0].confidence == 0.5
    assert decoded.partials[0].text == "new" and decoded.partials[0].confidence == 0.75


def test_unknown_fields_order_wrong_wire_type_and_original_noncanonical_bytes():
    payload = b"\x08\x80\x00" + blob(1, b"not-status") + scalar(100, 7) + blob(101, b"\xff\x00") + scalar(100, 8)
    decoded = decode_response(payload)
    assert decoded.status == 0
    assert decoded.unknown_fields == (Field(1, 2, b"not-status"), Field(100, 0, 7), Field(101, 2, b"\xff\x00"), Field(100, 0, 8))
    assert decoded.raw == payload


@pytest.mark.parametrize("payload", [
    b"\x00", b"\x08", b"\x80", b"\x0a\x05x", b"\x0f", b"\x0b", b"\x0c",
    b"\x09\x00", b"\x0d\x00", b"\x08" + b"\xff" * 10 + b"\x01",
    blob(1253625, b"\x0a\x02\x08"), with_hypothesis(b"\x15\x00"),
    response(blob(4, blob(1, b"\x11\x00"))), with_edit(blob(7, b"\x0a\x08bad")),
])
def test_malformed_protobuf_raises_value_error(payload):
    with pytest.raises(ValueError):
        decode_response(payload)


@pytest.mark.parametrize("payload", [
    blob(3, b"\xff"), blob(3, b"\xff") + blob(3, "valid"),
    with_hypothesis(blob(1, b"\xff")), with_edit(blob(1, b"\xff")),
    with_edit(blob(2, b"\xff") + blob(3, "valid")),
    with_edit(blob(7, blob(1, b"\xff"))),
    response(blob(4, blob(1, blob(1, b"\xff")))),
    blob(1253625, blob(3, b"\xff")),
])
def test_known_strings_are_strict_utf8_even_if_overwritten(payload):
    with pytest.raises(ValueError):
        decode_response(payload)


@pytest.mark.parametrize("payload", [None, "text", bytearray(b""), memoryview(b"")])
def test_decode_requires_bytes(payload):
    with pytest.raises(ValueError):
        decode_response(payload)


def test_response_size_and_total_nested_field_budget():
    with pytest.raises(ValueError):
        decode_response(b"x" * (MAX_MESSAGE_BYTES + 1))
    with pytest.raises(ValueError):
        decode_response(scalar(1, 0) * (MAX_FIELDS + 1))
    # Root has exactly MAX_FIELDS fields; its one nested field exceeds the global
    # budget, although each individual message is under the per-message budget.
    with pytest.raises(ValueError):
        decode_response(scalar(1, 0) * (MAX_FIELDS - 1) + response(scalar(1, 0)))
