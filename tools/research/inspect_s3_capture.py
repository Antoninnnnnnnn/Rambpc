"""Independently inspect S3 response protobuf captures (no app decoder imports).

This is an analysis tool, not a streaming client or a complete protobuf runtime.
Run with python -I from a trusted directory. Inputs are response-only .pb files.
Unknown fields are listed and retained; unsupported groups fail explicitly.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import struct


def varint(data: bytes, pos: int) -> tuple[int, int]:
    value = 0
    for index in range(10):
        if pos >= len(data):
            raise ValueError("truncated varint")
        byte = data[pos]
        pos += 1
        if index == 9 and byte > 1:
            raise ValueError("uint64 overflow")
        value |= (byte & 127) << (index * 7)
        if byte < 128:
            return value, pos
    raise ValueError("unterminated varint")


def fields(data: bytes) -> list[tuple[int, int, object]]:
    result = []
    pos = 0
    while pos < len(data):
        tag, pos = varint(data, pos)
        number, wire = tag >> 3, tag & 7
        if not 0 < number <= 536870911:
            raise ValueError("invalid field number")
        if wire == 0:
            value, pos = varint(data, pos)
        elif wire in (1, 5):
            size = 8 if wire == 1 else 4
            if pos + size > len(data):
                raise ValueError("truncated fixed-width field")
            value = data[pos:pos + size]
            pos += size
        elif wire == 2:
            size, pos = varint(data, pos)
            if size > len(data) - pos:
                raise ValueError("truncated length-delimited field")
            value = data[pos:pos + size]
            pos += size
        else:
            raise ValueError(f"unsupported wire type {wire}")
        result.append((number, wire, value))
    return result


def values(parsed: list, number: int, wire: int) -> list:
    found = [value for n, w, value in parsed if n == number and w == wire]
    if any(n == number and w != wire for n, w, _ in parsed):
        raise ValueError(f"unexpected wire type for known field {number}")
    return found


def scalar(parsed: list, number: int, wire: int = 0):
    found = values(parsed, number, wire)
    return found[-1] if found else None


def text(parsed: list, number: int):
    value = scalar(parsed, number, 2)
    return None if value is None else value.decode("utf-8")


def inspect(path: Path) -> dict:
    data = path.read_bytes()
    top = fields(data)
    record = {"path": str(path), "bytes": len(data),
              "sha256": hashlib.sha256(data).hexdigest(),
              "top_fields": [[n, w] for n, w, _ in top],
              "s3_status": scalar(top, 1), "results": []}
    for result_data in values(top, 1253625, 2):
        result = fields(result_data)
        entry = {"fields": [[n, w] for n, w, _ in result]}
        for speech_data in values(result, 2, 2):
            entry["speech_event_1253625_2_1"] = scalar(fields(speech_data), 1)
        for recognition_data in values(result, 1, 2):
            recognition = fields(recognition_data)
            entry["recognition_state_1253625_1_1"] = scalar(recognition, 1)
            entry["hypotheses"] = []
            for final_data in values(recognition, 3, 2):
                final = fields(final_data)
                for hypothesis_data in values(final, 3, 2):
                    hypothesis = fields(hypothesis_data)
                    score = scalar(hypothesis, 2, 5)
                    item = {"path": "1253625.1.3.3", "text_field1": text(hypothesis, 1),
                            "confidence_field2": None if score is None else struct.unpack("<f", score)[0],
                            "fields": [[n, w] for n, w, _ in hypothesis], "voice_edits": []}
                    for edit_data in values(hypothesis, 74956224, 2):
                        edit = fields(edit_data)
                        detail = {"extension": 74956224, "fields": [[n, w] for n, w, _ in edit],
                                  "transcript_field1": text(edit, 1),
                                  "alternative_field2": text(edit, 2),
                                  "edited_text_field3": text(edit, 3), "intent_field4": scalar(edit, 4),
                                  "edited_context_field7": []}
                        for context_data in values(edit, 7, 2):
                            context = fields(context_data)
                            detail["edited_context_field7"].append([text(context, n) for n in (1, 2, 3)])
                        item["voice_edits"].append(detail)
                    entry["hypotheses"].append(item)
        record["results"].append(entry)
    return record


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", type=Path, nargs="+")
    args = parser.parse_args()
    for path in args.paths:
        targets = sorted(path.rglob("response-*.pb")) if path.is_dir() else [path]
        for target in targets:
            print(json.dumps(inspect(target), ensure_ascii=True))


if __name__ == "__main__":
    main()
