"""Original response bytes from controlled synthetic live sessions; offline only.

The corpus was independently decoded and recursively inspected for credentials
before inclusion. No network, APK, audio runtime, or credentials are required.
Do not regenerate these bytes with the production encoder: original hashes are
part of the evidence. A final result does not itself assert terminal S3 status.
"""

import hashlib
import json
from pathlib import Path

import pytest

from rambler.protocol import decode_response
from rambler.wire import Field


CORPUS = json.loads(
    (Path(__file__).parent / "fixtures" / "live_synthetic_responses.json").read_text(
        encoding="utf-8"
    )
)


@pytest.mark.parametrize("case", CORPUS["cases"], ids=lambda case: case["id"])
def test_original_live_synthetic_response_decodes_offline(case):
    assert CORPUS["fixture_kind"] == "original_unmodified_server_response"
    assert CORPUS["privacy_review"]["payload_modified"] is False
    payload = bytes.fromhex(case["payload_hex"])
    assert len(payload) == case["payload_bytes"]
    assert hashlib.sha256(payload).hexdigest() == case["payload_sha256"]
    assert len(bytes.fromhex(case["input_audio_sha256"])) == 32

    response = decode_response(payload)
    expected = case["expected"]
    assert response.raw == payload
    assert response.status is None  # Absent, not the separate terminal message.
    assert response.error_code is None and response.error_message is None
    assert response.recognition_state == expected["recognition_state"] == 1
    assert response.speech_event is None and response.partials == ()
    assert len(response.hypotheses) == 1
    hypothesis = response.hypotheses[0]
    assert hypothesis.text == expected["text"]
    assert hypothesis.confidence is None
    edit = hypothesis.voice_edit
    assert edit is not None
    assert edit.intent == expected["intent"] == 2
    assert edit.transcript == expected["transcript"]
    assert edit.edited_text == expected["edited_text"]
    assert edit.active_field == expected["voice_edit_active_field"] == 3
    assert edit.alternative_text is None and edit.edited_context is None
    # Unknown envelope data observed in all three real results stays available.
    assert response.unknown_fields == (
        Field(5, 0, expected["top_level_unknown_varint_field5"]),
    )
