import pytest

from src.rambler.wire import Field, bytes_field, encode_varint, parse_fields, string_field, varint_field


@pytest.mark.parametrize(
    ("value", "expected"),
    [(0, b"\x00"), (1, b"\x01"), (127, b"\x7f"), (128, b"\x80\x01"),
     (300, b"\xac\x02"), ((1 << 64) - 1, b"\xff" * 9 + b"\x01")],
)
def test_encode_varint_vectors(value, expected):
    assert encode_varint(value) == expected


def test_exact_field_vectors_and_parse():
    message = (varint_field(1, 150) + bytes_field(2, b"abc")
               + string_field(3, "é") + bytes.fromhex("210102030405060708")
               + bytes.fromhex("2d01020304"))
    assert message == bytes.fromhex("08960112036162631a02c3a92101020304050607082d01020304")
    assert parse_fields(message) == [
        Field(1, 0, 150), Field(2, 2, b"abc"), Field(3, 2, "é".encode()),
        Field(4, 1, bytes.fromhex("0102030405060708")),
        Field(5, 5, bytes.fromhex("01020304")),
    ]


def test_empty_and_unicode():
    assert parse_fields(b"") == []
    assert parse_fields(string_field(1, "Rambler 東京")) == [
        Field(1, 2, "Rambler 東京".encode("utf-8"))
    ]


@pytest.mark.parametrize("value", [-1, 1 << 64, True, 1.0])
def test_varint_rejects_invalid_values(value):
    with pytest.raises(ValueError):
        encode_varint(value)


@pytest.mark.parametrize("number", [-1, 0, (1 << 29), True])
def test_fields_reject_invalid_numbers(number):
    with pytest.raises(ValueError):
        varint_field(number, 0)
    with pytest.raises(ValueError):
        bytes_field(number, b"")


def test_truncated_varints_and_values():
    for data in (b"\x80", b"\x08\x80", b"\x0a\x02x", b"\x09\x00", b"\x0d\x00"):
        with pytest.raises(ValueError):
            parse_fields(data)


def test_varint_overflow():
    with pytest.raises(ValueError, match="overflows"):
        parse_fields(b"\x08" + b"\x80" * 9 + b"\x02")
    with pytest.raises(ValueError, match="overflows"):
        parse_fields(b"\x80" * 9 + b"\x02")


@pytest.mark.parametrize("data", [b"\x0b", b"\x0c", b"\x0e", b"\x0f"])
def test_groups_and_invalid_wire_types_are_rejected(data):
    with pytest.raises(ValueError):
        parse_fields(data)


@pytest.mark.parametrize("data", [b"\x00", b"\x80\x80\x80\x80\x10\x00"])
def test_invalid_field_numbers_are_rejected(data):
    with pytest.raises(ValueError):
        parse_fields(data)


def test_size_and_field_count_limits():
    with pytest.raises(ValueError, match="max_size"):
        parse_fields(b"\x08\x01", max_size=1)
    with pytest.raises(ValueError, match="max_fields"):
        parse_fields(b"\x08\x01\x10\x02", max_fields=1)
    assert parse_fields(b"", max_fields=0) == []
    with pytest.raises(ValueError):
        parse_fields(b"", max_size=-1)
    with pytest.raises(ValueError):
        parse_fields(b"", max_fields=-1)
