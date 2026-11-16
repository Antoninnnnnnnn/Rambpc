"""Synthetic, stdlib-only tests for the bounded DEX lifecycle extractor."""
import hashlib
import json
import struct
import unittest
import zlib

from tools.research import dex_methods as dm


SECRET = "synthetic-secret-never-emit-7f3c91"


def uleb(value):
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        out.append(byte | (0x80 if value else 0))
        if not value:
            return bytes(out)


def align(value, alignment=4):
    return (value + alignment - 1) // alignment * alignment


def synthetic_dex(units=None, class_data_override=None, with_handler=False, handler_off=0,
                  handler_address=0, typed_handler=False):
    """Build one tiny DEX containing only Lkjo;.run()V and no real input data."""
    if units is None:
        # const-string v0, SECRET; const/4 v0, 2; sparse-switch v0
        # case 2 -> const/4 v0, 8; case 8 -> return-void.
        units = [0x001A, 3, 0x2012, 0x002C, 5, 0, 0x8012, 0x000E,
                 0x0200, 2, 2, 0, 8, 0, 3, 0, 4, 0]
    strings = ["Lkjo;", "V", "run", SECRET]
    string_count = len(strings)
    string_ids_off = 112
    type_ids_off = string_ids_off + 4 * string_count
    proto_ids_off = type_ids_off + 8
    method_ids_off = proto_ids_off + 12
    class_defs_off = method_ids_off + 8
    data_off = class_defs_off + 32
    data = bytearray()
    string_offsets = []
    for value in strings:
        string_offsets.append(data_off + len(data))
        encoded = value.encode("utf-8")
        data += uleb(len(value)) + encoded + b"\0"
    class_data_off = data_off + len(data)
    code_off = align(class_data_off + 16)
    if class_data_override is None:
        class_data = b"\0\0\1\0" + uleb(0) + uleb(1) + uleb(code_off)
    else:
        class_data = class_data_override
    data += class_data
    data += b"\0" * (code_off - (data_off + len(data)))
    data += struct.pack("<4HII", 2, 1, 0, int(with_handler), 0, len(units))
    for unit in units:
        data += struct.pack("<H", unit)
    if with_handler:
        if len(units) % 2:
            data += b"\0\0"
        data += struct.pack("<IHH", 0, len(units), handler_off)
        # One handler: signed size 1 for typed, or 0 for catch-all.
        data += uleb(1) + (b"\1" + uleb(0) if typed_handler else b"\0")
        data += uleb(handler_address)
    map_off = align(data_off + len(data))
    data += b"\0" * (map_off - (data_off + len(data)))
    data += struct.pack("<I", 0)  # A structurally empty map is sufficient here.
    result = bytearray(112)
    result += struct.pack("<" + "I" * string_count, *string_offsets)
    result += struct.pack("<II", 0, 1)  # Lkjo; and V descriptor string indices.
    result += struct.pack("<III", 1, 1, 0)  # shorty V, return V, no parameters.
    result += struct.pack("<HHI", 0, 0, 2)  # Lkjo;, proto 0, name run.
    result += struct.pack("<8I", 0, 1, dm.NO_INDEX, 0, dm.NO_INDEX, 0, class_data_off, 0)
    assert len(result) == data_off
    result += data
    struct.pack_into("<I", result, 32, len(result))
    struct.pack_into("<I", result, 36, 112)
    struct.pack_into("<I", result, 40, dm.ENDIAN_CONSTANT)
    struct.pack_into("<II", result, 44, 0, 0)
    struct.pack_into("<I", result, 52, map_off)
    tables = ((56, string_count, string_ids_off), (64, 2, type_ids_off),
              (72, 1, proto_ids_off), (80, 0, 0), (88, 1, method_ids_off),
              (96, 1, class_defs_off), (104, len(data), data_off))
    for offset, first, second in tables:
        struct.pack_into("<II", result, offset, first, second)
    result[:8] = b"dex\n035\0"
    result[12:32] = hashlib.sha1(result[32:]).digest()
    struct.pack_into("<I", result, 8, zlib.adler32(result[12:]) & 0xFFFFFFFF)
    return bytes(result)


def declarations(dex):
    result = dm.extract_dex(dex, "synthetic.dex")
    return result["targets"]["Lkjo;.run()V"]["declarations"]


class UlebBoundsTests(unittest.TestCase):
    def test_truncated_and_overwide_uleb_have_fixed_diagnostics(self):
        cases = ((b"", "truncated ULEB128"), (b"\x80", "truncated ULEB128"),
                 (b"\x80\x80\x80\x80\x80", "ULEB128 exceeds 32 bits"),
                 (b"\x80\x80\x80\x80\x10", "ULEB128 exceeds 32 bits"))
        for raw, diagnostic in cases:
            with self.subTest(diagnostic=diagnostic):
                with self.assertRaisesRegex(dm.DexError, "^" + diagnostic + "$"):
                    dm._uleb(raw, 0)

    def test_uleb_limit_prevents_reading_adjacent_data(self):
        with self.assertRaisesRegex(dm.DexError, "^truncated ULEB128$"):
            dm._uleb(b"\x80\x00", 0, 1)

    def test_short_header_never_echoes_raw_bytes(self):
        raw = (SECRET.encode() + b"\0")[:20]
        with self.assertRaises(dm.DexError) as caught:
            dm.DexFile(raw)
        self.assertEqual(str(caught.exception), "DEX header outside DEX bounds")
        self.assertNotIn(SECRET, str(caught.exception))


class HeaderValidationTests(unittest.TestCase):
    def test_valid_checksum_signature_and_exact_method_hash(self):
        dex = synthetic_dex()
        found = declarations(dex)
        self.assertEqual(len(found), 1)
        self.assertEqual(found[0]["descriptor"], "()V")
        self.assertRegex(found[0]["code"]["code_item_sha256"], "^[0-9a-f]{64}$")
        self.assertRegex(found[0]["encoded_method_sha256"], "^[0-9a-f]{64}$")

    def test_checksum_mismatch_is_rejected_before_decoding(self):
        dex = bytearray(synthetic_dex())
        dex[-1] ^= 1
        with self.assertRaisesRegex(dm.DexError, "^DEX checksum mismatch$"):
            dm.DexFile(bytes(dex))

    def test_table_offset_and_reverse_endian_are_rejected(self):
        dex = bytearray(synthetic_dex())
        struct.pack_into("<I", dex, 60, align(len(dex) + 4))
        # Repair integrity so the structural error, not a stale checksum, is tested.
        dex[12:32] = hashlib.sha1(dex[32:]).digest()
        struct.pack_into("<I", dex, 8, zlib.adler32(dex[12:]) & 0xFFFFFFFF)
        with self.assertRaisesRegex(dm.DexError, "^string_ids outside DEX bounds$"):
            dm.DexFile(bytes(dex))
        dex = bytearray(synthetic_dex())
        struct.pack_into("<I", dex, 40, 0x78563412)
        dex[12:32] = hashlib.sha1(dex[32:]).digest()
        struct.pack_into("<I", dex, 8, zlib.adler32(dex[12:]) & 0xFFFFFFFF)
        with self.assertRaisesRegex(dm.DexError, "^unsupported DEX endian tag$"):
            dm.DexFile(bytes(dex))


class MethodDataTests(unittest.TestCase):
    def test_non_increasing_method_index_delta_is_rejected(self):
        # Two direct methods, both with index delta zero: the second is invalid.
        class_data = b"\0\0\2\0" + uleb(0) + uleb(1) + uleb(0) + uleb(0) + uleb(1) + uleb(0)
        with self.assertRaisesRegex(dm.DexError, "^non-increasing or duplicate method index delta$"):
            declarations(synthetic_dex(class_data_override=class_data))

    def test_method_index_delta_outside_table_is_rejected(self):
        class_data = b"\0\0\1\0" + uleb(2) + uleb(1) + uleb(0)
        with self.assertRaisesRegex(dm.DexError, "^method index delta outside table$"):
            declarations(synthetic_dex(class_data_override=class_data))

    def test_catch_handler_offset_is_relative_to_handler_list(self):
        found = declarations(synthetic_dex([0x000E], with_handler=True, handler_off=1))
        self.assertRegex(found[0]["code"]["code_item_sha256"], "^[0-9a-f]{64}$")
        with self.assertRaisesRegex(dm.DexError, "^catch handler offset outside handler list$"):
            declarations(synthetic_dex([0x000E], with_handler=True, handler_off=0))

    def test_invoke_35c_uses_high_nibble_argument_count(self):
        code = declarations(synthetic_dex([0x206E, 0, 0x0010, 0x000E]))[0]["code"]
        invoke = code["instructions"][0]
        self.assertEqual(invoke["name"], "invoke-virtual")
        self.assertEqual(invoke["registers"], [0, 1])
        self.assertEqual(invoke["method"]["class"], "Lkjo;")
        self.assertEqual(invoke["method"]["name"], "run")

    def test_truncated_instruction_and_unknown_opcode_are_rejected(self):
        with self.assertRaisesRegex(dm.DexError, "^truncated instruction$"):
            declarations(synthetic_dex([0x0018]))
        with self.assertRaisesRegex(dm.DexError, "^unsupported opcode 0x73$"):
            declarations(synthetic_dex([0x0073]))

    def test_const_string_and_array_payload_contents_are_redacted(self):
        found = declarations(synthetic_dex())[0]
        serialized = json.dumps(found)
        self.assertNotIn(SECRET, serialized)
        const_string = found["code"]["instructions"][0]
        self.assertEqual(const_string["name"], "const-string")
        self.assertEqual(const_string["literal"], "<redacted>")
        self.assertNotIn(SECRET, str(found["code"]["payloads"]))
        dex = dm.DexFile(synthetic_dex())
        array, width = dex._payload([0x0300, 2, 2, 0, 0xBEEF, 0xCAFE], 0)
        self.assertEqual(width, 6)
        self.assertEqual(array["contents"], "<redacted>")
        self.assertNotIn("beef", json.dumps(array).lower())
        self.assertNotIn("cafe", json.dumps(array).lower())


class SwitchAndPayloadTests(unittest.TestCase):
    def test_switch_cases_two_and_eight_are_structural_not_provenanced(self):
        code = declarations(synthetic_dex())[0]["code"]
        switch = next(item for item in code["instructions"] if item["name"] == "sparse-switch")
        self.assertEqual(switch["switch_cases_2_8"], {"2": 6, "8": 7})
        self.assertIn("unknown", switch["discriminator_provenance"])

    def test_switch_payload_must_land_on_an_instruction_boundary(self):
        units = [0x002B, 2, 0, 0x000E, 0x0100, 0, 0, 0]
        with self.assertRaisesRegex(dm.DexError, "^branch target outside instruction boundaries$"):
            declarations(synthetic_dex(units))

    def test_array_payload_alignment_and_length_are_explicit(self):
        dex = dm.DexFile(synthetic_dex())
        with self.assertRaisesRegex(dm.DexError, "^unaligned array payload$"):
            dex._payload([0, 0x0300, 1, 1, 0], 1)
        with self.assertRaisesRegex(dm.DexError, "^truncated array payload data$"):
            dex._payload([0x0300, 4, 2, 0, 0xFFFF], 0)
        with self.assertRaisesRegex(dm.DexError, "^truncated packed-switch payload entries$"):
            dex._payload([0x0100, 1, 0, 0, 0], 0)

    def test_sparse_switch_keys_must_be_strictly_ordered(self):
        dex = dm.DexFile(synthetic_dex())
        units = [0x0200, 2, 2, 0, 2, 0, 0, 0, 0, 0, 0, 0]
        with self.assertRaisesRegex(dm.DexError, "^unsorted sparse-switch keys$"):
            dex._payload(units, 0)


class OpcodeGoldenTests(unittest.TestCase):
    """Literal goldens transcribed from the DEX bytecode opcode/format table.

    Expected sequences deliberately do not use the parser's family tables or
    derive one family's expected names from another family.
    """

    def assert_family(self, start, names, formats):
        expected_names = names.split()
        expected_formats = formats.split()
        self.assertEqual(len(expected_names), len(expected_formats))
        for opcode, name, fmt in zip(range(start, start + len(expected_names)),
                                     expected_names, expected_formats):
            with self.subTest(opcode=hex(opcode)):
                self.assertEqual(dm._NAMES[opcode], name)
                self.assertEqual(dm._format(opcode), fmt)

    def test_low_opcode_family_literal_goldens(self):
        self.assert_family(0x00, """
            nop move move/from16 move/16 move-wide move-wide/from16 move-wide/16
            move-object move-object/from16 move-object/16 move-result move-result-wide
            move-result-object move-exception return-void return return-wide return-object
            const/4 const/16 const const/high16 const-wide/16 const-wide/32 const-wide
            const-wide/high16 const-string const-string/jumbo const-class monitor-enter
            monitor-exit check-cast instance-of array-length new-instance new-array
            filled-new-array filled-new-array/range fill-array-data throw goto goto/16
            goto/32 packed-switch sparse-switch cmpl-float cmpg-float cmpl-double
            cmpg-double cmp-long if-eq if-ne if-lt if-ge if-gt if-le
            if-eqz if-nez if-ltz if-gez if-gtz if-lez
        """, """
            10x 12x 22x 32x 12x 22x 32x 12x 22x 32x 11x 11x 11x 11x 10x 11x 11x 11x
            11n 21s 31i 21h 21s 31i 51l 21h 21c 31c 21c 11x 11x 21c 22c 12x 21c 22c
            35c 3rc 31t 11x 10t 16t 30t 31t 31t 23x 23x 23x 23x 23x
            22t 22t 22t 22t 22t 22t 21t 21t 21t 21t 21t 21t
        """)

    def test_array_and_field_family_literal_goldens(self):
        self.assert_family(0x44, """
            aget aget-wide aget-object aget-boolean aget-byte aget-char aget-short
            aput aput-wide aput-object aput-boolean aput-byte aput-char aput-short
            iget iget-wide iget-object iget-boolean iget-byte iget-char iget-short
            iput iput-wide iput-object iput-boolean iput-byte iput-char iput-short
            sget sget-wide sget-object sget-boolean sget-byte sget-char sget-short
            sput sput-wide sput-object sput-boolean sput-byte sput-char sput-short
        """, """
            23x 23x 23x 23x 23x 23x 23x 23x 23x 23x 23x 23x 23x 23x
            22c 22c 22c 22c 22c 22c 22c 22c 22c 22c 22c 22c 22c 22c
            21c 21c 21c 21c 21c 21c 21c 21c 21c 21c 21c 21c 21c 21c
        """)

    def test_invoke_family_literal_goldens(self):
        self.assert_family(0x6E, """
            invoke-virtual invoke-super invoke-direct invoke-static invoke-interface
        """, "35c 35c 35c 35c 35c")
        self.assert_family(0x74, """
            invoke-virtual/range invoke-super/range invoke-direct/range
            invoke-static/range invoke-interface/range
        """, "3rc 3rc 3rc 3rc 3rc")

    def test_unary_family_literal_golden_sequence(self):
        code = declarations(synthetic_dex(list(range(0x7B, 0x90)) + [0x000E]))[0]["code"]
        self.assertEqual([item["name"] for item in code["instructions"][:-1]], """
            neg-int not-int neg-long not-long neg-float neg-double
            int-to-long int-to-float int-to-double long-to-int long-to-float long-to-double
            float-to-int float-to-long float-to-double double-to-int double-to-long
            double-to-float int-to-byte int-to-char int-to-short
        """.split())
        self.assertTrue(all(item["width"] == 1 for item in code["instructions"][:-1]))

    def test_binary_three_register_family_literal_golden_sequence(self):
        units = [word for opcode in range(0x90, 0xB0) for word in (0x0100 | opcode, 0x0302)]
        records = declarations(synthetic_dex(units + [0x000E]))[0]["code"]["instructions"][:-1]
        self.assertEqual([item["name"] for item in records], """
            add-int sub-int mul-int div-int rem-int and-int or-int xor-int shl-int shr-int ushr-int
            add-long sub-long mul-long div-long rem-long and-long or-long xor-long shl-long shr-long ushr-long
            add-float sub-float mul-float div-float rem-float
            add-double sub-double mul-double div-double rem-double
        """.split())
        self.assertTrue(all(item["width"] == 2 and item["registers"] == [1, 2, 3] for item in records))

    def test_binary_two_address_family_literal_golden_sequence(self):
        units = [0x2100 | opcode for opcode in range(0xB0, 0xD0)]
        records = declarations(synthetic_dex(units + [0x000E]))[0]["code"]["instructions"][:-1]
        self.assertEqual([item["name"] for item in records], """
            add-int/2addr sub-int/2addr mul-int/2addr div-int/2addr rem-int/2addr
            and-int/2addr or-int/2addr xor-int/2addr shl-int/2addr shr-int/2addr ushr-int/2addr
            add-long/2addr sub-long/2addr mul-long/2addr div-long/2addr rem-long/2addr
            and-long/2addr or-long/2addr xor-long/2addr shl-long/2addr shr-long/2addr ushr-long/2addr
            add-float/2addr sub-float/2addr mul-float/2addr div-float/2addr rem-float/2addr
            add-double/2addr sub-double/2addr mul-double/2addr div-double/2addr rem-double/2addr
        """.split())
        self.assertTrue(all(item["width"] == 1 and item["registers"] == [1, 2] for item in records))

    def test_representative_arithmetic_opcodes_have_literal_names(self):
        for opcode, expected in ((0x9B, "add-long"), (0xA6, "add-float"), (0xAB, "add-double"),
                                 (0xBB, "add-long/2addr"), (0xC6, "add-float/2addr"),
                                 (0xCB, "add-double/2addr")):
            with self.subTest(opcode=hex(opcode)):
                units = [opcode, 0] if opcode < 0xB0 else [opcode]
                record = declarations(synthetic_dex(units + [0x000E]))[0]["code"]["instructions"][0]
                self.assertEqual(record["name"], expected)

    def test_literal_arithmetic_family_literal_goldens(self):
        self.assert_family(0xD0, """
            add-int/lit16 rsub-int mul-int/lit16 div-int/lit16 rem-int/lit16
            and-int/lit16 or-int/lit16 xor-int/lit16
            add-int/lit8 rsub-int/lit8 mul-int/lit8 div-int/lit8 rem-int/lit8
            and-int/lit8 or-int/lit8 xor-int/lit8 shl-int/lit8 shr-int/lit8 ushr-int/lit8
        """, "22s 22s 22s 22s 22s 22s 22s 22s 22b 22b 22b 22b 22b 22b 22b 22b 22b 22b 22b")

    def test_dex038_family_literal_goldens(self):
        self.assert_family(0xFA, """
            invoke-polymorphic invoke-polymorphic/range invoke-custom invoke-custom/range
            const-method-handle const-method-type
        """, "45cc 4rcc 35c 3rc 21c 21c")


class ExecutableTargetTests(unittest.TestCase):
    # Payload words are synthetic data, never instructions or expected opcode names.
    PAYLOADS = ((0x0100, 0, 0, 0), (0x0200, 0), (0x0300, 1, 0, 0))

    def test_packed_switch_payload_requires_four_byte_alignment(self):
        with self.assertRaisesRegex(dm.DexError, "^unaligned packed-switch payload$"):
            declarations(synthetic_dex([0x000E, 0x0100, 0, 0, 0]))

    def test_sparse_switch_payload_requires_four_byte_alignment(self):
        with self.assertRaisesRegex(dm.DexError, "^unaligned sparse-switch payload$"):
            declarations(synthetic_dex([0x000E, 0x0200, 0]))

    def test_aligned_switch_payloads_are_valid_data(self):
        for payload in self.PAYLOADS[:2]:
            with self.subTest(ident=hex(payload[0])):
                code = declarations(synthetic_dex([0x000E, 0x0000] + list(payload)))[0]["code"]
                self.assertEqual(code["payloads"][0]["offset"], 2)

    def test_goto_into_packed_payload_boundary_is_rejected(self):
        with self.assertRaisesRegex(dm.DexError, "^branch target outside instruction boundaries$"):
            declarations(synthetic_dex([0x0228, 0x000E, 0x0100, 0, 0, 0]))

    def test_all_branch_formats_exclude_each_payload_kind(self):
        branches = ([0x0428, 0x000E, 0x000E, 0x000E],
                    [0x0029, 4, 0x000E, 0x000E], [0x002A, 4, 0, 0x000E])
        branches += tuple([opcode, 4, 0x000E, 0x000E] for opcode in range(0x32, 0x3E))
        for branch in branches:
            for payload in self.PAYLOADS:
                with self.subTest(opcode=hex(branch[0] & 0xFF), ident=hex(payload[0])):
                    with self.assertRaisesRegex(dm.DexError, "^branch target outside instruction boundaries$"):
                        declarations(synthetic_dex(branch + list(payload)))

    def test_executable_branch_boundary_controls(self):
        code = declarations(synthetic_dex([0x0328, 0x0013, 0, 0x000E]))[0]["code"]
        self.assertEqual(code["instructions"][0]["branch_target"], 3)
        with self.assertRaisesRegex(dm.DexError, "^branch target outside instruction boundaries$"):
            declarations(synthetic_dex([0x0228, 0x0013, 0, 0x000E]))

    def test_switch_cases_exclude_their_own_payload_boundary(self):
        for opcode, payload in ((0x002B, [0x0100, 1, 2, 0, 4, 0]),
                                (0x002C, [0x0200, 1, 2, 0, 4, 0])):
            with self.subTest(opcode=hex(opcode)):
                with self.assertRaisesRegex(dm.DexError, "^switch case target outside instruction boundaries$"):
                    declarations(synthetic_dex([opcode, 4, 0, 0x000E] + payload))

    def test_switch_cases_exclude_other_payload_boundaries(self):
        for opcode, payload in ((0x002B, [0x0100, 1, 2, 0, 10, 0]),
                                (0x002C, [0x0200, 1, 2, 0, 10, 0])):
            for other in self.PAYLOADS:
                with self.subTest(opcode=hex(opcode), ident=hex(other[0])):
                    with self.assertRaisesRegex(dm.DexError, "^switch case target outside instruction boundaries$"):
                        declarations(synthetic_dex([opcode, 4, 0, 0x000E] + payload + list(other)))

    def test_switch_case_executable_boundary_controls(self):
        for opcode, ident in ((0x002B, 0x0100), (0x002C, 0x0200)):
            with self.subTest(opcode=hex(opcode)):
                code = declarations(synthetic_dex([opcode, 4, 0, 0x000E, ident, 1, 2, 0, 3, 0]))[0]["code"]
                self.assertEqual(code["instructions"][0]["switch_cases_2_8"], {"2": 3, "8": None})
                with self.assertRaisesRegex(dm.DexError, "^switch case target outside instruction boundaries$"):
                    declarations(synthetic_dex([opcode, 4, 0, 0x000E, ident, 1, 2, 0, 1, 0]))

    def test_typed_and_catch_all_entries_exclude_payload_boundaries(self):
        for typed in (False, True):
            for payload in self.PAYLOADS:
                with self.subTest(typed=typed, ident=hex(payload[0])):
                    diagnostic = "invalid typed catch handler" if typed else "invalid catch-all handler"
                    with self.assertRaisesRegex(dm.DexError, "^" + diagnostic + "$"):
                        declarations(synthetic_dex([0x000E, 0x0000] + list(payload), with_handler=True,
                                                   handler_off=1, handler_address=2, typed_handler=typed))

    def test_catch_entries_require_executable_instruction_boundaries(self):
        for typed in (False, True):
            for target in (1, 3):  # Inside const/16, or past the instruction array.
                with self.subTest(typed=typed, target=target):
                    diagnostic = "invalid typed catch handler" if typed else "invalid catch-all handler"
                    with self.assertRaisesRegex(dm.DexError, "^" + diagnostic + "$"):
                        declarations(synthetic_dex([0x0013, 0, 0x000E], with_handler=True,
                                                   handler_off=1, handler_address=target, typed_handler=typed))

    def test_catch_entries_can_target_executable_code_after_payloads(self):
        for typed in (False, True):
            for payload in self.PAYLOADS:
                with self.subTest(typed=typed, ident=hex(payload[0])):
                    units = [0x000E, 0x0000] + list(payload) + [0x000D, 0x000E]
                    code = declarations(synthetic_dex(units, with_handler=True, handler_off=1,
                                                      handler_address=len(units) - 2, typed_handler=typed))[0]["code"]
                    self.assertEqual(code["instructions"][-2]["name"], "move-exception")

    def test_payload_references_are_distinct_from_executable_targets(self):
        for opcode, payload in zip((0x002B, 0x002C, 0x0026), self.PAYLOADS):
            with self.subTest(opcode=hex(opcode)):
                units = [opcode, 4, 0, 0x000E] + list(payload)
                code = declarations(synthetic_dex(units))[0]["code"]
                self.assertEqual(code["instructions"][0]["payload_target"], 4)
                dex = dm.DexFile(synthetic_dex())
                _, data, executable = dex._decode_instructions(units)
                self.assertEqual(data[0]["offset"], 4)
                self.assertNotIn(4, executable)
                self.assertIn(3, executable)
        with self.assertRaisesRegex(dm.DexError, "^instruction references missing or wrong payload$"):
            declarations(synthetic_dex([0x002B, 4, 0, 0x000E, 0x0200, 0]))
        with self.assertRaises(dm.DexError):
            declarations(synthetic_dex([0x002B, 3, 0, 0x000E]))


if __name__ == "__main__":
    unittest.main()
