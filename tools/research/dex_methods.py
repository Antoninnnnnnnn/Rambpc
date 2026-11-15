"""Bounded, read-only extraction of three lifecycle methods from one pinned APK.

This is deliberately not a general disassembler.  It validates DEX structure,
resolves declaring DEX files, and emits only descriptors, structural operands,
and references for the requested methods.  String literals and array payload
contents are never decoded into output.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
import zipfile
import zlib
from pathlib import Path

EXPECTED_APK_SHA256 = "a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8"
EXPECTED_APK_SIZE = 127_650_866
MAX_DEX_SIZE = 128 * 1024 * 1024
MAX_DEX_FILES = 256
MAX_CLASS_MEMBERS = 1_000_000
MAX_TARGET_DECLARATIONS = 16
NO_INDEX = 0xFFFFFFFF
ENDIAN_CONSTANT = 0x12345678
SUPPORTED_VERSIONS = frozenset({"035", "036", "037", "038", "039", "040", "041"})
TARGETS = (("Lklb;", "g", "(Loak;)Z"), ("Lkjo;", "run", "()V"))
NAME_ONLY_TARGET = ("Lkml;", "a")


class DexError(ValueError):
    """A fixed-diagnostic structural or bounds failure."""


def _fail(message: str) -> None:
    raise DexError(message)


def _range(data: bytes, offset: int, size: int, label: str) -> tuple[int, int]:
    if offset < 0 or size < 0 or offset > len(data) or size > len(data) - offset:
        _fail(f"{label} outside DEX bounds")
    return offset, offset + size


def _u16(data: bytes, offset: int, label: str = "u16") -> int:
    _range(data, offset, 2, label)
    return struct.unpack_from("<H", data, offset)[0]


def _u32(data: bytes, offset: int, label: str = "u32") -> int:
    _range(data, offset, 4, label)
    return struct.unpack_from("<I", data, offset)[0]


def _s16(data: bytes, offset: int) -> int:
    return struct.unpack_from("<h", data, offset)[0]


def _s32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<i", data, offset)[0]


def _uleb(data: bytes, offset: int, limit: int | None = None) -> tuple[int, int]:
    """Read an unsigned, at-most-five-byte DEX ULEB128 value."""
    end = len(data) if limit is None else min(limit, len(data))
    value = 0
    for index in range(5):
        if offset + index >= end:
            _fail("truncated ULEB128")
        byte = data[offset + index]
        if index == 4 and byte > 0x0F:
            _fail("ULEB128 exceeds 32 bits")
        value |= (byte & 0x7F) << (7 * index)
        if byte < 0x80:
            return value, offset + index + 1
    _fail("unterminated ULEB128")


def _sleb(data: bytes, offset: int, limit: int | None = None) -> tuple[int, int]:
    """Read a signed, at-most-five-byte DEX SLEB128 value."""
    end = len(data) if limit is None else min(limit, len(data))
    value = 0
    for index in range(5):
        if offset + index >= end:
            _fail("truncated SLEB128")
        byte = data[offset + index]
        value |= (byte & 0x7F) << (7 * index)
        if byte < 0x80:
            shift = 7 * (index + 1)
            if byte & 0x40:
                value -= 1 << shift
            if not -(1 << 31) <= value < (1 << 31):
                _fail("SLEB128 exceeds 32 bits")
            return value, offset + index + 1
    _fail("unterminated SLEB128")


def _aligned(offset: int, alignment: int, label: str) -> None:
    if offset % alignment:
        _fail(f"unaligned {label}")


def _signed(value: int, bits: int) -> int:
    sign = 1 << (bits - 1)
    return (value ^ sign) - sign


def _mutf8(data: bytes, offset: int) -> tuple[str, int]:
    """Decode a bounded DEX MUTF-8 identifier without replacement characters."""
    expected_units, cursor = _uleb(data, offset)
    start = cursor
    units: list[int] = []
    while True:
        _range(data, cursor, 1, "string data")
        if cursor - start > 16 * 1024 * 1024:
            _fail("string data exceeds bound")
        byte = data[cursor]
        if byte == 0:
            cursor += 1
            break
        if byte < 0x80:
            units.append(byte)
            cursor += 1
        elif byte & 0xE0 == 0xC0:
            _range(data, cursor, 2, "truncated MUTF-8")
            second = data[cursor + 1]
            if second & 0xC0 != 0x80:
                _fail("invalid MUTF-8 continuation")
            unit = ((byte & 0x1F) << 6) | (second & 0x3F)
            if unit == 0 and (byte, second) != (0xC0, 0x80):
                _fail("invalid MUTF-8 zero")
            units.append(unit)
            cursor += 2
        elif byte & 0xF0 == 0xE0:
            _range(data, cursor, 3, "truncated MUTF-8")
            second, third = data[cursor + 1], data[cursor + 2]
            if second & 0xC0 != 0x80 or third & 0xC0 != 0x80:
                _fail("invalid MUTF-8 continuation")
            units.append(((byte & 0x0F) << 12) | ((second & 0x3F) << 6) | (third & 0x3F))
            cursor += 3
        else:
            _fail("unsupported MUTF-8 sequence")
    if len(units) != expected_units:
        _fail("MUTF-8 UTF-16 length mismatch")
    chars: list[str] = []
    index = 0
    while index < len(units):
        unit = units[index]
        if 0xD800 <= unit <= 0xDBFF:
            if index + 1 >= len(units) or not 0xDC00 <= units[index + 1] <= 0xDFFF:
                _fail("unpaired MUTF-8 surrogate")
            codepoint = 0x10000 + ((unit - 0xD800) << 10) + (units[index + 1] - 0xDC00)
            chars.append(chr(codepoint))
            index += 2
        elif 0xDC00 <= unit <= 0xDFFF:
            _fail("unpaired MUTF-8 surrogate")
        else:
            chars.append(chr(unit))
            index += 1
    return "".join(chars), cursor


_NAMES = {
    0x00: "nop", 0x01: "move", 0x02: "move/from16", 0x03: "move/16",
    0x04: "move-wide", 0x05: "move-wide/from16", 0x06: "move-wide/16",
    0x07: "move-object", 0x08: "move-object/from16", 0x09: "move-object/16",
    0x0A: "move-result", 0x0B: "move-result-wide", 0x0C: "move-result-object",
    0x0D: "move-exception", 0x0E: "return-void", 0x0F: "return",
    0x10: "return-wide", 0x11: "return-object", 0x12: "const/4",
    0x13: "const/16", 0x14: "const", 0x15: "const/high16",
    0x16: "const-wide/16", 0x17: "const-wide/32", 0x18: "const-wide",
    0x19: "const-wide/high16", 0x1A: "const-string", 0x1B: "const-string/jumbo",
    0x1C: "const-class", 0x1D: "monitor-enter", 0x1E: "monitor-exit",
    0x1F: "check-cast", 0x20: "instance-of", 0x21: "array-length",
    0x22: "new-instance", 0x23: "new-array", 0x24: "filled-new-array",
    0x25: "filled-new-array/range", 0x26: "fill-array-data", 0x27: "throw",
    0x28: "goto", 0x29: "goto/16", 0x2A: "goto/32",
    0x2B: "packed-switch", 0x2C: "sparse-switch",
    0x2D: "cmpl-float", 0x2E: "cmpg-float", 0x2F: "cmpl-double",
    0x30: "cmpg-double", 0x31: "cmp-long", 0x32: "if-eq", 0x33: "if-ne",
    0x34: "if-lt", 0x35: "if-ge", 0x36: "if-gt", 0x37: "if-le",
    0x38: "if-eqz", 0x39: "if-nez", 0x3A: "if-ltz", 0x3B: "if-gez",
    0x3C: "if-gtz", 0x3D: "if-lez",
    0xFA: "invoke-polymorphic", 0xFB: "invoke-polymorphic/range",
    0xFC: "invoke-custom", 0xFD: "invoke-custom/range",
    0xFE: "const-method-handle", 0xFF: "const-method-type",
}
_ARRAY_PREFIXES = ("aget", "aput")
_FIELD_PREFIXES = ("iget", "iput", "sget", "sput")
for _opcode, _suffix in enumerate(("", "-wide", "-object", "-boolean", "-byte", "-char", "-short")):
    for _base, _start in (("aget", 0x44), ("aput", 0x4B), ("iget", 0x52),
                          ("iput", 0x59), ("sget", 0x60), ("sput", 0x67)):
        _NAMES[_start + _opcode] = _base + _suffix
for _opcode, _name in enumerate(("invoke-virtual", "invoke-super", "invoke-direct", "invoke-static", "invoke-interface"), 0x6E):
    _NAMES[_opcode] = _name
    _NAMES[_opcode + 6] = _name + "/range"
_UNARY = ("neg-int", "not-int", "neg-long", "not-long", "neg-float", "neg-double",
          "int-to-long", "int-to-float", "int-to-double", "long-to-int", "long-to-float",
          "long-to-double", "float-to-int", "float-to-long", "float-to-double",
          "double-to-int", "double-to-long", "double-to-float", "int-to-byte", "int-to-char", "int-to-short")
for _opcode, _name in enumerate(_UNARY, 0x7B):
    _NAMES[_opcode] = _name
_BINARY = ("add-int", "sub-int", "mul-int", "div-int", "rem-int", "and-int", "or-int", "xor-int",
           "shl-int", "shr-int", "ushr-int", "add-long", "sub-long", "mul-long", "div-long",
           "rem-long", "and-long", "or-long", "xor-long", "shl-long", "shr-long", "ushr-long",
           "add-float", "sub-float", "mul-float", "div-float", "rem-float",
           "add-double", "sub-double", "mul-double", "div-double", "rem-double")
for _opcode, _name in enumerate(_BINARY, 0x90):
    _NAMES[_opcode] = _name
    _NAMES[_opcode + 0x20] = _name + "/2addr"
for _opcode, _name in enumerate(("add-int/lit16", "rsub-int", "mul-int/lit16", "div-int/lit16",
                                  "rem-int/lit16", "and-int/lit16", "or-int/lit16", "xor-int/lit16"), 0xD0):
    _NAMES[_opcode] = _name
for _opcode, _name in enumerate(("add-int/lit8", "rsub-int/lit8", "mul-int/lit8", "div-int/lit8",
                                  "rem-int/lit8", "and-int/lit8", "or-int/lit8", "xor-int/lit8",
                                  "shl-int/lit8", "shr-int/lit8", "ushr-int/lit8"), 0xD8):
    _NAMES[_opcode] = _name


def _format(opcode: int) -> str:
    if opcode == 0x00 or opcode == 0x0E or opcode == 0x28:
        return "10x" if opcode != 0x28 else "10t"
    if opcode in (0x01, 0x04, 0x07, 0x21) or 0x7B <= opcode <= 0x8F or 0xB0 <= opcode <= 0xCF:
        return "12x"
    if opcode in (0x02, 0x05, 0x08):
        return "22x"
    if opcode in (0x03, 0x06, 0x09):
        return "32x"
    if 0x0A <= opcode <= 0x11 or opcode in (0x1D, 0x1E, 0x27):
        return "11x"
    if opcode == 0x12:
        return "11n"
    if opcode in (0x13, 0x16):
        return "21s"
    if opcode in (0x14, 0x17):
        return "31i"
    if opcode in (0x15, 0x19):
        return "21h"
    if opcode == 0x18:
        return "51l"
    if opcode in (0x1A, 0x1C, 0x1F, 0x22) or 0x60 <= opcode <= 0x6D or opcode in (0xFE, 0xFF):
        return "21c"
    if opcode == 0x1B:
        return "31c"
    if opcode in (0x20, 0x23) or 0x52 <= opcode <= 0x5F:
        return "22c"
    if opcode == 0x24 or 0x6E <= opcode <= 0x72 or opcode == 0xFC:
        return "35c"
    if opcode == 0x25 or 0x74 <= opcode <= 0x78 or opcode == 0xFD:
        return "3rc"
    if opcode in (0x26, 0x2B, 0x2C):
        return "31t"
    if opcode == 0x29:
        return "16t"
    if opcode == 0x2A:
        return "30t"
    if 0x2D <= opcode <= 0x31 or 0x44 <= opcode <= 0x51 or 0x90 <= opcode <= 0xAF:
        return "23x"
    if 0x32 <= opcode <= 0x37:
        return "22t"
    if 0x38 <= opcode <= 0x3D:
        return "21t"
    if 0xD0 <= opcode <= 0xD7:
        return "22s"
    if 0xD8 <= opcode <= 0xE2:
        return "22b"
    if opcode == 0xFA:
        return "45cc"
    if opcode == 0xFB:
        return "4rcc"
    _fail(f"unsupported opcode 0x{opcode:02x}")


_WIDTHS = {"10x": 1, "12x": 1, "11x": 1, "11n": 1, "10t": 1, "20t": 2,
           "22x": 2, "21s": 2, "21h": 2, "21c": 2, "21t": 2, "16t": 2,
           "22c": 2, "22t": 2, "22s": 2, "22b": 2, "23x": 2,
           "32x": 3, "31i": 3, "31c": 3, "31t": 3, "30t": 3, "35c": 3, "3rc": 3,
           "51l": 5, "45cc": 4, "4rcc": 4}


class DexFile:
    def __init__(self, data: bytes, entry: str = "classes.dex") -> None:
        self.data = data
        self.entry = entry
        self._strings: dict[int, str] = {}
        self._protos: dict[int, str] = {}
        self._parse_header()
        self._validate_tables()

    def _parse_header(self) -> None:
        d = self.data
        _range(d, 0, 112, "DEX header")
        if len(d) > MAX_DEX_SIZE:
            _fail("DEX exceeds size bound")
        if not re.fullmatch(b"dex\n[0-9]{3}\x00", d[:8]):
            _fail("unsupported DEX magic")
        self.version = d[4:7].decode("ascii")
        if self.version not in SUPPORTED_VERSIONS:
            _fail("unsupported DEX version")
        if _u32(d, 40, "endian tag") != ENDIAN_CONSTANT:
            _fail("unsupported DEX endian tag")
        if _u32(d, 32, "file size") != len(d) or _u32(d, 36, "header size") != 112:
            _fail("inconsistent DEX file/header size")
        if zlib.adler32(d[12:]) & 0xFFFFFFFF != _u32(d, 8, "checksum"):
            _fail("DEX checksum mismatch")
        if hashlib.sha1(d[32:]).digest() != d[12:32]:
            _fail("DEX signature mismatch")
        self.data_size, self.data_off = struct.unpack_from("<II", d, 104)
        if self.data_off < 112 or self.data_size != len(d) - self.data_off:
            _fail("inconsistent DEX data section")
        map_off = _u32(d, 52, "map offset")
        if map_off == 0:
            _fail("missing DEX map")
        self._data_offset(map_off, 4, "map offset")
        _aligned(map_off, 4, "map offset")
        map_size = _u32(d, map_off, "map size")
        if map_size > self.data_size // 12:
            _fail("map size exceeds bound")
        _range(d, map_off + 4, map_size * 12, "map entries")
        link_size, link_off = struct.unpack_from("<II", d, 44)
        self._table(link_size, link_off, 1, "link", allow_unaligned=True)
        specs = (("string_ids", 56, 4), ("type_ids", 64, 4), ("proto_ids", 72, 12),
                 ("field_ids", 80, 8), ("method_ids", 88, 8), ("class_defs", 96, 32))
        self.tables: dict[str, tuple[int, int]] = {}
        occupied: list[tuple[int, int, str]] = []
        for name, header_offset, width in specs:
            size, offset = struct.unpack_from("<II", d, header_offset)
            self._table(size, offset, width, name)
            if size and offset + size * width > self.data_off:
                _fail(f"{name} overlaps data section")
            self.tables[name] = (size, offset)
            if size:
                occupied.append((offset, offset + size * width, name))
        for index, (start, end, name) in enumerate(occupied):
            for other_start, other_end, other_name in occupied[index + 1:]:
                if start < other_end and other_start < end:
                    _fail(f"overlapping DEX tables {name}/{other_name}")

    def _table(self, size: int, offset: int, width: int, name: str, allow_unaligned: bool = False) -> None:
        if size == 0:
            if offset != 0:
                _fail(f"nonzero offset for empty {name}")
            return
        if offset == 0 or size > len(self.data) // width:
            _fail(f"invalid {name} table")
        if not allow_unaligned:
            _aligned(offset, 4, name)
        _range(self.data, offset, size * width, name)

    def _data_offset(self, offset: int, size: int, label: str) -> None:
        if offset < self.data_off or offset > len(self.data) or size > len(self.data) - offset:
            _fail(f"{label} outside data section")
        _range(self.data, offset, size, label)

    def _validate_tables(self) -> None:
        d = self.data
        string_count, string_off = self.tables["string_ids"]
        for index in range(string_count):
            self._data_offset(_u32(d, string_off + 4 * index), 1, "string data offset")
        type_count, type_off = self.tables["type_ids"]
        for index in range(type_count):
            if _u32(d, type_off + 4 * index) >= string_count:
                _fail("type descriptor index outside string table")
        proto_count, proto_off = self.tables["proto_ids"]
        for index in range(proto_count):
            base = proto_off + 12 * index
            shorty, return_type, parameters = struct.unpack_from("<III", d, base)
            if shorty >= string_count or return_type >= type_count:
                _fail("prototype index outside table")
            if parameters:
                self._data_offset(parameters, 4, "parameter list")
                _aligned(parameters, 4, "parameter list")
                count = _u32(d, parameters, "parameter count")
                if count > self.data_size // 2:
                    _fail("parameter list exceeds bound")
                self._data_offset(parameters + 4, count * 2, "parameter entries")
                for item in range(count):
                    if _u16(d, parameters + 4 + 2 * item) >= type_count:
                        _fail("parameter type outside table")
        field_count, field_off = self.tables["field_ids"]
        for index in range(field_count):
            class_idx, type_idx, name_idx = struct.unpack_from("<HHI", d, field_off + 8 * index)
            if class_idx >= type_count or type_idx >= type_count or name_idx >= string_count:
                _fail("field index outside table")
        method_count, method_off = self.tables["method_ids"]
        for index in range(method_count):
            class_idx, proto_idx, name_idx = struct.unpack_from("<HHI", d, method_off + 8 * index)
            if class_idx >= type_count or proto_idx >= proto_count or name_idx >= string_count:
                _fail("method index outside table")
        class_count, class_off = self.tables["class_defs"]
        for index in range(class_count):
            base = class_off + 32 * index
            values = struct.unpack_from("<8I", d, base)
            if values[0] >= type_count or (values[2] != NO_INDEX and values[2] >= type_count):
                _fail("class type outside table")
            if values[4] != NO_INDEX and values[4] >= string_count:
                _fail("class source index outside table")
            interfaces, annotations, class_data, static_values = values[3], values[5], values[6], values[7]
            for offset, label in ((interfaces, "interfaces"), (annotations, "annotations"),
                                  (class_data, "class data"), (static_values, "static values")):
                if offset:
                    self._data_offset(offset, 1, label)
            if interfaces:
                _aligned(interfaces, 4, "interfaces")
                count = _u32(d, interfaces, "interface count")
                if count > self.data_size // 2:
                    _fail("interface list exceeds bound")
                self._data_offset(interfaces + 4, count * 2, "interface entries")
            if class_data:
                _aligned(class_data, 1, "class data")

    def string(self, index: int) -> str:
        if index >= self.tables["string_ids"][0]:
            _fail("string index outside table")
        if index not in self._strings:
            offset = _u32(self.data, self.tables["string_ids"][1] + 4 * index)
            value, _ = _mutf8(self.data, offset)
            self._strings[index] = value
        return self._strings[index]

    def type_descriptor(self, index: int) -> str:
        if index >= self.tables["type_ids"][0]:
            _fail("type index outside table")
        return self.string(_u32(self.data, self.tables["type_ids"][1] + 4 * index))

    def proto(self, index: int) -> str:
        if index >= self.tables["proto_ids"][0]:
            _fail("prototype index outside table")
        if index not in self._protos:
            base = self.tables["proto_ids"][1] + 12 * index
            return_type = _u32(self.data, base + 4)
            parameters_off = _u32(self.data, base + 8)
            parameters: list[str] = []
            if parameters_off:
                count = _u32(self.data, parameters_off)
                parameters = [self.type_descriptor(_u16(self.data, parameters_off + 4 + 2 * item))
                              for item in range(count)]
            self._protos[index] = "(" + "".join(parameters) + ")" + self.type_descriptor(return_type)
        return self._protos[index]

    def method_ref(self, index: int) -> dict[str, object]:
        if index >= self.tables["method_ids"][0]:
            _fail("method reference outside table")
        class_idx, proto_idx, name_idx = struct.unpack_from("<HHI", self.data, self.tables["method_ids"][1] + 8 * index)
        return {"index": index, "class": self.type_descriptor(class_idx), "name": self.string(name_idx),
                "descriptor": self.proto(proto_idx)}

    def field_ref(self, index: int) -> dict[str, object]:
        if index >= self.tables["field_ids"][0]:
            _fail("field reference outside table")
        class_idx, type_idx, name_idx = struct.unpack_from("<HHI", self.data, self.tables["field_ids"][1] + 8 * index)
        return {"index": index, "class": self.type_descriptor(class_idx), "name": self.string(name_idx),
                "type": self.type_descriptor(type_idx)}

    def class_descriptors(self) -> dict[str, list[int]]:
        result: dict[str, list[int]] = {}
        count, offset = self.tables["class_defs"]
        for index in range(count):
            descriptor = self.type_descriptor(_u32(self.data, offset + 32 * index))
            result.setdefault(descriptor, []).append(index)
        return result

    def methods_named(self, class_descriptor: str, name: str) -> list[dict[str, object]]:
        count, offset = self.tables["method_ids"]
        matches = []
        for index in range(count):
            class_idx, proto_idx, name_idx = struct.unpack_from("<HHI", self.data, offset + 8 * index)
            if self.type_descriptor(class_idx) == class_descriptor and self.string(name_idx) == name:
                matches.append(self.method_ref(index))
        return matches

    def declarations(self, class_index: int, wanted_name: str | None = None,
                     wanted_proto: str | None = None) -> list[dict[str, object]]:
        base = self.tables["class_defs"][1] + 32 * class_index
        class_data_off = _u32(self.data, base + 24)
        if class_data_off == 0:
            return []
        cursor = class_data_off
        sizes = []
        for _ in range(4):
            if cursor >= len(self.data):
                _fail("truncated class data sizes")
            value, cursor = _uleb(self.data, cursor)
            if value > MAX_CLASS_MEMBERS:
                _fail("class member count exceeds bound")
            sizes.append(value)
        static_fields, instance_fields, direct_methods, virtual_methods = sizes
        for count in (static_fields, instance_fields):
            index = 0
            for _ in range(count):
                delta, cursor = _uleb(self.data, cursor)
                _, cursor = _uleb(self.data, cursor)
                if delta > 0xFFFFFFFF - index:
                    _fail("field index delta overflow")
                index += delta
                if index >= self.tables["field_ids"][0]:
                    _fail("encoded field outside table")
        declarations = []
        seen: set[int] = set()
        for kind, count in (("direct", direct_methods), ("virtual", virtual_methods)):
            index = 0
            previous = -1
            for _ in range(count):
                start = cursor
                delta, cursor = _uleb(self.data, cursor)
                access, cursor = _uleb(self.data, cursor)
                code_off, cursor = _uleb(self.data, cursor)
                if delta > 0xFFFFFFFF - index:
                    _fail("method index delta overflow")
                index += delta
                if index >= self.tables["method_ids"][0]:
                    _fail("method index delta outside table")
                if index <= previous or index in seen:
                    _fail("non-increasing or duplicate method index delta")
                previous = index
                seen.add(index)
                ref = self.method_ref(index)
                if wanted_name is not None and (ref["name"] != wanted_name or
                                                wanted_proto is not None and ref["descriptor"] != wanted_proto):
                    continue
                if code_off:
                    self._data_offset(code_off, 16, "code item")
                    _aligned(code_off, 4, "code item")
                declarations.append({"class": ref["class"], "name": ref["name"],
                                     "descriptor": ref["descriptor"], "method_index": index,
                                     "kind": kind, "access_flags": access, "code_offset": code_off,
                                     "encoded_method_sha256": hashlib.sha256(self.data[start:cursor]).hexdigest()})
                if len(declarations) > MAX_TARGET_DECLARATIONS:
                    _fail("target declaration count exceeds bound")
        return declarations

    def decode_code(self, declaration: dict[str, object]) -> dict[str, object]:
        offset = int(declaration["code_offset"])
        if offset == 0:
            return {"code_item_sha256": None, "status": "no code item (native or abstract)"}
        d = self.data
        registers, ins, outs, tries_count = struct.unpack_from("<4H", d, offset)
        debug_off = _u32(d, offset + 8)
        insns_size = _u32(d, offset + 12)
        if insns_size > self.data_size // 2:
            _fail("instruction array exceeds bound")
        self._data_offset(offset + 16, insns_size * 2, "instruction array")
        if debug_off:
            self._data_offset(debug_off, 1, "debug info")
        insns = [struct.unpack_from("<H", d, offset + 16 + 2 * index)[0] for index in range(insns_size)]
        records, payloads, boundaries = self._decode_instructions(insns)
        self._active_code_offset = offset
        self._active_insns_size = insns_size
        self._active_boundaries = boundaries
        try:
            self._validate_branches(records, payloads, boundaries, insns_size)
            end = offset + 16 + insns_size * 2
            if tries_count:
                if insns_size % 2:
                    self._data_offset(end, 2, "try padding")
                    end += 2
                self._data_offset(end, tries_count * 8, "try items")
                tries_start = end
                try_items = []
                for index in range(tries_count):
                    start_addr, insn_count, handler_off = struct.unpack_from("<IHH", d, tries_start + 8 * index)
                    if start_addr > insns_size or insn_count > insns_size - start_addr:
                        _fail("try range outside instructions")
                    try_items.append(handler_off)
                handlers = tries_start + tries_count * 8
                list_size, cursor = _uleb(d, handlers)
                if list_size > MAX_CLASS_MEMBERS:
                    _fail("catch handler list exceeds bound")
                handler_offsets = set()
                for _ in range(list_size):
                    handler_offsets.add(cursor - handlers)
                    count, cursor = _sleb(d, cursor)
                    for _ in range(abs(count)):
                        type_idx, cursor = _uleb(d, cursor)
                        address, cursor = _uleb(d, cursor)
                        if type_idx >= self.tables["type_ids"][0] or address not in boundaries:
                            _fail("invalid typed catch handler")
                    if count <= 0:
                        address, cursor = _uleb(d, cursor)
                        if address not in boundaries:
                            _fail("invalid catch-all handler")
                if any(handler_off not in handler_offsets for handler_off in try_items):
                    _fail("catch handler offset outside handler list")
                end = cursor
        finally:
            del self._active_code_offset, self._active_insns_size, self._active_boundaries
        return {"code_item_sha256": hashlib.sha256(d[offset:end]).hexdigest(),
                "insns_sha256": hashlib.sha256(d[offset + 16:offset + 16 + insns_size * 2]).hexdigest(),
                "registers": registers, "ins": ins, "outs": outs, "code_units": insns_size,
                "instructions": records, "payloads": payloads}

    def _decode_instructions(self, units: list[int]) -> tuple[list[dict[str, object]], list[dict[str, object]], set[int]]:
        records: list[dict[str, object]] = []
        payloads: list[dict[str, object]] = []
        boundaries: set[int] = set()
        cursor = 0
        while cursor < len(units):
            first = units[cursor]
            opcode = first & 0xFF
            if opcode == 0 and first != 0:
                record, width = self._payload(units, cursor)
                payloads.append(record)
                cursor += width
                continue
            # Only executable instruction starts are valid control-flow targets.
            boundaries.add(cursor)
            name = _NAMES.get(opcode)
            if name is None:
                _fail(f"unsupported opcode 0x{opcode:02x}")
            if opcode >= 0xFA and self.version < "038":
                _fail("DEX 038 opcode in older DEX")
            fmt = _format(opcode)
            width = _WIDTHS[fmt]
            if width > len(units) - cursor:
                _fail("truncated instruction")
            words = units[cursor:cursor + width]
            record: dict[str, object] = {"offset": cursor, "opcode": f"0x{opcode:02x}", "name": name, "width": width}
            self._operands(record, fmt, words)
            records.append(record)
            cursor += width
        return records, payloads, boundaries

    def _payload(self, units: list[int], cursor: int) -> tuple[dict[str, object], int]:
        ident = units[cursor]
        if ident == 0x0100:
            if cursor % 2:
                _fail("unaligned packed-switch payload")
            if cursor + 4 > len(units):
                _fail("truncated packed-switch payload")
            size = units[cursor + 1]
            width = 4 + 2 * size
            if width > len(units) - cursor:
                _fail("truncated packed-switch payload entries")
            first_key = _s32(struct.pack("<HH", units[cursor + 2], units[cursor + 3]), 0)
            return {"offset": cursor, "kind": "packed-switch", "size": size, "first_key": first_key}, width
        if ident == 0x0200:
            if cursor % 2:
                _fail("unaligned sparse-switch payload")
            if cursor + 2 > len(units):
                _fail("truncated sparse-switch payload")
            size = units[cursor + 1]
            width = 2 + 4 * size
            if width > len(units) - cursor:
                _fail("truncated sparse-switch payload entries")
            keys = [_s32(struct.pack("<HH", units[cursor + 2 + 2 * i], units[cursor + 3 + 2 * i]), 0)
                    for i in range(size)]
            if any(keys[i] >= keys[i + 1] for i in range(size - 1)):
                _fail("unsorted sparse-switch keys")
            return {"offset": cursor, "kind": "sparse-switch", "size": size, "keys": keys}, width
        if ident == 0x0300:
            if cursor % 2:
                _fail("unaligned array payload")
            if cursor + 4 > len(units):
                _fail("truncated array payload")
            width_bytes = units[cursor + 1]
            size = units[cursor + 2] | units[cursor + 3] << 16
            if width_bytes == 0 or size > 0xFFFFFFFF // width_bytes:
                _fail("invalid array payload dimensions")
            data_units = (width_bytes * size + 1) // 2
            width = 4 + data_units
            if width > len(units) - cursor:
                _fail("truncated array payload data")
            return {"offset": cursor, "kind": "array", "element_width": width_bytes,
                    "size": size, "contents": "<redacted>"}, width
        _fail("unsupported payload identifier")

    def _operands(self, record: dict[str, object], fmt: str, w: list[int]) -> None:
        opcode = int(str(record["opcode"]), 16)
        aa = w[0] >> 8
        if fmt in ("12x", "11n"):
            a, b = (w[0] >> 8) & 0xF, (w[0] >> 12) & 0xF
            record["registers"] = [a] if fmt == "11n" else [a, b]
            if fmt == "11n":
                record["literal"] = _signed(b, 4)
        elif fmt == "11x":
            record["registers"] = [aa]
        elif fmt == "22x":
            record["registers"] = [aa, w[1]]
        elif fmt == "32x":
            record["registers"] = [w[1], w[2]]
        elif fmt in ("21s", "21h", "31i", "51l"):
            record["registers"] = [aa]
            if fmt == "21s":
                record["literal"] = _signed(w[1], 16)
            elif fmt == "21h":
                record["literal"] = _signed(w[1] << (48 if opcode == 0x19 else 16), 64 if opcode == 0x19 else 32)
            elif fmt == "31i":
                record["literal"] = _signed(w[1] | w[2] << 16, 32)
            else:
                record["literal"] = _signed(sum(w[i] << (16 * (i - 1)) for i in range(1, 5)), 64)
        elif fmt in ("21c", "31c"):
            record["registers"] = [aa]
            index = w[1] if fmt == "21c" else w[1] | w[2] << 16
            self._reference(record, opcode, index)
        elif fmt in ("22c",):
            record["registers"] = [(w[0] >> 8) & 0xF, (w[0] >> 12) & 0xF]
            self._reference(record, opcode, w[1])
        elif fmt in ("35c", "45cc"):
            count, g = (w[0] >> 12) & 0xF, (w[0] >> 8) & 0xF
            values = [(w[2] >> shift) & 0xF for shift in (0, 4, 8, 12)] + [g]
            if count > 5:
                _fail("invoke register count exceeds format")
            record["registers"] = values[:count]
            self._reference(record, opcode, w[1])
            if fmt == "45cc":
                record["prototype"] = self.proto(w[3])
        elif fmt in ("3rc", "4rcc"):
            count, start = aa, w[2]
            if count and start + count > 0x10000:
                _fail("range register overflow")
            record["registers"] = {"start": start, "count": count}
            self._reference(record, opcode, w[1])
            if fmt == "4rcc":
                record["prototype"] = self.proto(w[3])
        elif fmt == "23x":
            record["registers"] = [aa, w[1] & 0xFF, w[1] >> 8]
        elif fmt == "22b":
            record["registers"] = [aa, w[1] & 0xFF]
            record["literal"] = _signed(w[1] >> 8, 8)
        elif fmt == "22s":
            record["registers"] = [(w[0] >> 8) & 0xF, (w[0] >> 12) & 0xF]
            record["literal"] = _signed(w[1], 16)
        elif fmt in ("10t", "16t", "30t"):
            displacement = _signed(aa, 8) if fmt == "10t" else _signed(w[1], 16) if fmt == "16t" else _signed(w[1] | w[2] << 16, 32)
            record["branch_target"] = int(record["offset"]) + displacement
        elif fmt in ("21t", "22t", "31t"):
            if fmt == "22t":
                record["registers"] = [(w[0] >> 8) & 0xF, (w[0] >> 12) & 0xF]
            else:
                record["registers"] = [aa]
            displacement = _signed(w[1], 16) if fmt != "31t" else _signed(w[1] | w[2] << 16, 32)
            key = "payload_target" if fmt == "31t" else "branch_target"
            record[key] = int(record["offset"]) + displacement

    def _reference(self, record: dict[str, object], opcode: int, index: int) -> None:
        if opcode in (0x1A, 0x1B):
            if index >= self.tables["string_ids"][0]:
                _fail("const-string index outside table")
            record["string_index"] = index
            record["literal"] = "<redacted>"
        elif opcode in (0x1C, 0x1F, 0x20, 0x22, 0x23, 0x24, 0x25):
            record["type"] = self.type_descriptor(index)
        elif 0x52 <= opcode <= 0x6D:
            record["field"] = self.field_ref(index)
        elif 0x6E <= opcode <= 0x78 or opcode in (0xFA, 0xFB):
            record["method"] = self.method_ref(index)
        elif opcode in (0xFC, 0xFD):
            record["call_site_index"] = index
        elif opcode == 0xFE:
            record["method_handle_index"] = index
        elif opcode == 0xFF:
            record["prototype"] = self.proto(index)

    def _validate_branches(self, records: list[dict[str, object]], payloads: list[dict[str, object]],
                           boundaries: set[int], insns_size: int) -> None:
        by_offset = {int(item["offset"]): item for item in payloads}
        for record in records:
            offset = int(record["offset"])
            for key in ("branch_target", "payload_target"):
                target = int(record[key]) if key in record else None
                target_boundaries = by_offset if key == "payload_target" else boundaries
                if target is not None and (target < 0 or target >= insns_size or target not in target_boundaries):
                    _fail("branch target outside instruction boundaries")
            if "payload_target" in record:
                target = int(record["payload_target"])
                payload = by_offset.get(target)
                expected = "array" if record["name"] == "fill-array-data" else str(record["name"])
                if payload is None or payload["kind"] != expected:
                    _fail("instruction references missing or wrong payload")
                if record["name"] in ("packed-switch", "sparse-switch"):
                    raw = self._switch_cases(record, payload)
                    record["switch_cases_2_8"] = {str(key): raw.get(key) for key in (2, 8)}
                    record["discriminator_provenance"] = "unknown; structural targets only"

    def _switch_cases(self, switch: dict[str, object], payload: dict[str, object]) -> dict[int, int]:
        offset = int(payload["offset"])
        # Re-read only the already validated code item units through instruction offsets.
        code_offset = int(self._active_code_offset)
        base = code_offset + 16 + 2 * offset
        size = int(payload["size"])
        result: dict[int, int] = {}
        if payload["kind"] == "packed-switch":
            first = int(payload["first_key"])
            for index in range(size):
                target = _s32(self.data, base + 8 + 4 * index)
                result[first + index] = int(switch["offset"]) + target
        else:
            for index, key in enumerate(payload["keys"]):
                target_base = base + 4 + 4 * size + 4 * index
                result[int(key)] = int(switch["offset"]) + _s32(self.data, target_base)
        for target in result.values():
            if target < 0 or target >= int(self._active_insns_size) or target not in self._active_boundaries:
                _fail("switch case target outside instruction boundaries")
        return result

    def decode(self, declaration: dict[str, object]) -> dict[str, object]:
        return self.decode_code(declaration)


def extract_dex(data: bytes, entry: str) -> dict[str, object]:
    dex = DexFile(data, entry)
    classes = dex.class_descriptors()
    result: dict[str, object] = {"entry": entry, "sha256": hashlib.sha256(data).hexdigest(),
                                 "version": dex.version, "targets": {}}
    for class_name, method_name, proto in TARGETS:
        key = class_name + "." + method_name + proto
        declarations = []
        for class_index in classes.get(class_name, []):
            declarations.extend(dex.declarations(class_index, method_name, proto))
        decoded = []
        for declaration in declarations:
            item = dict(declaration)
            item["code"] = dex.decode(declaration)
            decoded.append(item)
        result["targets"][key] = {"class_present": class_name in classes, "declarations": decoded}
    class_name, method_name = NAME_ONLY_TARGET
    declarations = []
    for class_index in classes.get(class_name, []):
        declarations.extend(dex.declarations(class_index, method_name))
    result["targets"][class_name + "." + method_name] = {
        "class_present": class_name in classes, "declarations": declarations,
        "method_id_references": dex.methods_named(class_name, method_name)}
    return result


def _apk_digest(path: Path) -> tuple[int, str]:
    size = path.stat().st_size
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while True:
            block = source.read(1024 * 1024)
            if not block:
                break
            digest.update(block)
    return size, digest.hexdigest()


def extract_apk(apk: Path) -> dict[str, object]:
    size, digest = _apk_digest(apk)
    if size != EXPECTED_APK_SIZE or digest != EXPECTED_APK_SHA256:
        _fail("APK identity mismatch")
    entries: list[dict[str, object]] = []
    with zipfile.ZipFile(apk) as archive:
        infos = [info for info in archive.infolist()
                 if re.fullmatch(r"classes(?:[1-9][0-9]*)?\.dex", info.filename)]
        if not infos or len(infos) > MAX_DEX_FILES:
            _fail("APK DEX entry count outside bounds")
        names = [info.filename for info in infos]
        if len(set(names)) != len(names):
            _fail("duplicate APK DEX entry")
        infos.sort(key=lambda info: (0 if info.filename == "classes.dex" else int(re.search(r"[0-9]+", info.filename).group()), info.filename))
        for info in infos:
            if info.file_size > MAX_DEX_SIZE or info.flag_bits & 1:
                _fail("DEX entry exceeds bound or is encrypted")
            with archive.open(info) as source:
                data = source.read(MAX_DEX_SIZE + 1)
            if len(data) != info.file_size or len(data) > MAX_DEX_SIZE:
                _fail("DEX entry size mismatch")
            entries.append(extract_dex(data, info.filename))
    return {"schema_version": 1, "source": {"bytes": size, "sha256": digest},
            "literal_policy": "const-string literals and array payload bytes redacted",
            "analysis_limit": "structural operands and direct branch targets only; no full control-flow or discriminator recovery",
            "dex_files": entries}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apk", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args(argv)
    if args.output_dir.exists():
        _fail("output directory must be new")
    if not args.output_dir.parent.is_dir():
        _fail("output parent directory does not exist")
    result = extract_apk(args.apk)
    args.output_dir.mkdir()
    output = args.output_dir / "lifecycle-methods.json"
    with output.open("x", encoding="utf-8", newline="\n") as target:
        json.dump(result, target, ensure_ascii=False, indent=2, sort_keys=True)
        target.write("\n")
    print(str(output))
    return 0


if __name__ == "__main__":
    main()
