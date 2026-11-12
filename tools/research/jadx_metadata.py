"""Inspect protobuf-lite RawMessageInfo in JADX Java output; never execute it.

Run from a trusted directory with isolation:
  python -I tools/research/jadx_metadata.py ABSOLUTE_GENERATED_JAVA_PATH [...]

Outputs field numbers/types/presence and the original object table. Descriptive
field names and object-table associations still require checking the class.
Only the generated new aafx(message, literal, objects) shape is supported.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re


TYPES = (
    "double", "float", "int64", "uint64", "int32", "fixed64", "fixed32",
    "bool", "string", "message", "bytes", "uint32", "enum", "sfixed32",
    "sfixed64", "sint32", "sint64", "group",
)
ESCAPES = {"b": "\b", "t": "\t", "n": "\n", "f": "\f", "r": "\r",
           '"': '"', "'": "'", "\\": "\\"}
LITERAL = r'"(?:\\.|[^"\\])*"'
INFO = re.compile(r'new aafx\(\w+,\s*(' + LITERAL + r'),\s*(.*?)\);')


def java_literal(text: str) -> str:
    def replace(match: re.Match[str]) -> str:
        value = match.group(1)
        if value.startswith("u"):
            return chr(int(value.lstrip("u"), 16))
        if value[0].isdigit():
            return chr(int(value, 8))
        return ESCAPES[value]
    return re.sub(r"\\(u+[0-9a-fA-F]{4}|[0-3][0-7]{0,2}|[4-7][0-7]?|.)",
                  replace, text[1:-1])


def info_integers(text: str) -> list[int]:
    values: list[int] = []
    accumulator = shift = 0
    for char in text:
        value = ord(char)
        if value >= 0xD800:
            accumulator |= (value & 0x1FFF) << shift
            shift += 13
        else:
            values.append(accumulator | (value << shift))
            accumulator = shift = 0
    if shift:
        raise ValueError("truncated RawMessageInfo integer")
    return values


def type_name(value: int) -> str:
    if value >= 51:
        return "oneof " + TYPES[value - 51]
    if value == 50:
        return "map"
    if value == 49:
        return "repeated group"
    if value >= 35:
        packed = ("double", "float", "int64", "uint64", "int32", "fixed64",
                  "fixed32", "bool", "uint32", "enum", "sfixed32",
                  "sfixed64", "sint32", "sint64")
        return "packed " + packed[value - 35]
    if value >= 18:
        return "repeated " + TYPES[value - 18]
    return TYPES[value]


def inspect(path: Path) -> dict:
    source = path.read_text(encoding="utf-8")
    match = INFO.search(source)
    if match is None:
        raise ValueError("no supported aafx metadata constructor")
    values = info_integers(java_literal(match.group(1)))
    if len(values) < 2:
        raise ValueError("missing metadata header")
    flags, count = values[:2]
    result = {"path": str(path), "line": source.count("\n", 0, match.start()) + 1,
              "flags": flags, "field_count": count, "fields": [],
              "object_table": match.group(2)}
    if count == 0:
        if len(values) != 2:
            raise ValueError("unexpected metadata after empty message")
        return result
    if len(values) < 10:
        raise ValueError("incomplete metadata header")
    result["header"] = dict(zip(
        ("oneofs", "hasbits_words", "minimum", "maximum", "entries", "maps",
         "repeated", "check_initialized"), values[2:10]))
    index = 10
    for _ in range(count):
        number, descriptor = values[index:index + 2]
        index += 2
        kind = descriptor & 0xFF
        field = {"number": number, "type": type_name(kind),
                 "descriptor": descriptor}
        if kind >= 51:
            field["oneof_index"] = values[index]
            index += 1
        elif descriptor & 0x1000:
            field["presence_bit"] = values[index]
            index += 1
        result["fields"].append(field)
    if index != len(values):
        raise ValueError(f"unparsed metadata integers: {values[index:]}")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", type=Path)
    args = parser.parse_args()
    for path in args.paths:
        print(json.dumps(inspect(path), ensure_ascii=True))


if __name__ == "__main__":
    main()
