"""Schema-free protobuf wire-format helpers for diagnostics."""

from dataclasses import dataclass

_MAX_UINT64 = (1 << 64) - 1
_MAX_FIELD_NUMBER = (1 << 29) - 1


@dataclass(frozen=True)
class Field:
    number: int
    wire_type: int
    value: int | bytes


def _uint64(value: int) -> None:
    if not isinstance(value, int) or isinstance(value, bool) or not 0 <= value <= _MAX_UINT64:
        raise ValueError("value must be an unsigned 64-bit integer")


def encode_varint(value: int) -> bytes:
    """Encode an unsigned 64-bit integer as a protobuf varint."""
    _uint64(value)
    out = bytearray()
    while value >= 0x80:
        out.append((value & 0x7F) | 0x80)
        value >>= 7
    out.append(value)
    return bytes(out)


def _field_number(number: int) -> None:
    if (not isinstance(number, int) or isinstance(number, bool)
            or not 1 <= number <= _MAX_FIELD_NUMBER):
        raise ValueError("field number must be in 1..2^29-1")


def varint_field(number: int, value: int) -> bytes:
    _field_number(number)
    return encode_varint((number << 3) | 0) + encode_varint(value)


def bytes_field(number: int, value: bytes) -> bytes:
    _field_number(number)
    if not isinstance(value, bytes):
        raise ValueError("value must be bytes")
    return encode_varint((number << 3) | 2) + encode_varint(len(value)) + value


def string_field(number: int, value: str) -> bytes:
    if not isinstance(value, str):
        raise ValueError("value must be str")
    try:
        encoded = value.encode("utf-8")
    except UnicodeEncodeError as exc:
        raise ValueError("string must be valid UTF-8") from exc
    return bytes_field(number, encoded)


def _read_varint(data: bytes, offset: int) -> tuple[int, int]:
    value = 0
    for index in range(10):
        if offset >= len(data):
            raise ValueError("truncated varint")
        byte = data[offset]
        offset += 1
        if index == 9 and byte > 1:
            raise ValueError("varint overflows unsigned 64-bit integer")
        value |= (byte & 0x7F) << (7 * index)
        if byte < 0x80:
            return value, offset
    raise ValueError("varint exceeds 10 bytes")


def parse_fields(
    data: bytes, *, max_size: int = 8 * 1024 * 1024, max_fields: int = 100000
) -> list[Field]:
    """Parse top-level protobuf fields without interpreting schemas or groups."""
    if not isinstance(data, bytes):
        raise ValueError("data must be bytes")
    if (not isinstance(max_size, int) or isinstance(max_size, bool) or max_size < 0
            or not isinstance(max_fields, int) or isinstance(max_fields, bool) or max_fields < 0):
        raise ValueError("limits must be non-negative integers")
    if len(data) > max_size:
        raise ValueError("input exceeds max_size")

    fields: list[Field] = []
    offset = 0
    while offset < len(data):
        if len(fields) >= max_fields:
            raise ValueError("input exceeds max_fields")
        key, offset = _read_varint(data, offset)
        number, wire_type = key >> 3, key & 7
        _field_number(number)
        if wire_type == 0:
            value, offset = _read_varint(data, offset)
        elif wire_type == 1:
            end = offset + 8
            if end > len(data):
                raise ValueError("truncated fixed64 field")
            value = data[offset:end]
            offset = end
        elif wire_type == 2:
            size, offset = _read_varint(data, offset)
            end = offset + size
            if end > len(data):
                raise ValueError("truncated length-delimited field")
            value = data[offset:end]
            offset = end
        elif wire_type == 5:
            end = offset + 4
            if end > len(data):
                raise ValueError("truncated fixed32 field")
            value = data[offset:end]
            offset = end
        elif wire_type in (3, 4):
            raise ValueError("protobuf groups are not supported")
        else:
            raise ValueError(f"invalid protobuf wire type: {wire_type}")
        fields.append(Field(number, wire_type, value))
    return fields
