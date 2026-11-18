"""Explicit Windows-job synthetic selftests; deliberately not unittest-discovery named.

Run: python -I tools/opus_vs_ci_selftest.py
No entry point, native library, Windows API or child process is exercised here.
"""
from __future__ import annotations

import ast
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch


def load_sibling_checker():
    path = Path(__file__).resolve().with_name("qualify_opus_vs_ci.py")
    raw = path.read_bytes()
    name = "_rambler_ci_synthetic_checker"
    module = types.ModuleType(name)
    module.__file__ = str(path)
    sys.modules[name] = module
    exec(compile(raw, "<synthetic-checker>", "exec"), module.__dict__)
    return module


CHECKER = load_sibling_checker()
HELPER = CHECKER.load_owned_helper()
TARGET = Path("C:/fabricated/repo/target/vs17-functional")
NATIVE_OUT = TARGET / "release/build/opusic-sys/0123456789abcdef/out"


def build_message(out=NATIVE_OUT):
    return {"reason": "build-script-executed", "package_id": CHECKER.OPUS_PACKAGE_ID,
            "out_dir": str(out), "linked_libs": ["static=opus"],
            "linked_paths": ["native=" + str(out / "lib")]}


def options():
    return (["/c", "/O2", "/Ob2", "/MD", "/fp:precise", "/GS", "-Brepro"] +
            ["/D" + name for name in sorted(CHECKER.REQUIRED_DEFINES)] +
            ['/DCMAKE_INTDIR="Release"'])


def records_fixture():
    avx = sorted(CHECKER.AVX2_SOURCES)
    relative = avx + ["src/fabricated_%03d.c" % index for index in range(149)]
    lines, token_map = [], {}
    for index, name in enumerate(relative):
        source = "C:/fabricated/cache/opusic-sys-0.7.7/opus/" + name
        command = "fabricated-command-%03d" % index
        tokens = options() + (["/arch:AVX2"] if index < 3 else []) + [source]
        lines += ["^" + source.upper().replace("/", "\\"), command]
        token_map[command] = tokens
    return lines, token_map


HARNESS_NAMES = (
    "ingress::tests::partitions_match_the_same_clip_and_cross_sample_and_frame_seams",
    "tests::codec_defaults_and_explicit_bounds",
    "tests::codec_rejects_zero_overflow_unknown_missing_duplicate_and_bad_numbers",
    "tests::checked_padding_covers_boundaries_without_extra_frames",
    "tests::conversion_preserves_le_samples_tail_and_lookahead_zeros",
    "tests::synthetic_inputs_retain_published_integer_only_fingerprints",
    "native_ogg::tests::maximum_packets_and_255_byte_multiples_keep_valid_lacing_and_crc",
    "native_ogg::tests::pending_output_has_hard_limit_and_bad_packet_sizes_fail",
    "native_ogg::tests::sequence_overflow_is_an_error_not_a_panic",
)


def harness_output():
    return ("   Compiling fabricated v0.0.0\nnoise without harness markers\nrunning 9 tests\n" +
            "\n".join("test " + name + " ... ok" for name in HARNESS_NAMES) +
            "\n\ntest result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n")


def instance_fixture():
    return [{"installationVersion": "17.14.37906.3", "installationPath": "C:/Tools/Visual Studio/2022",
             "isComplete": True, "isLaunchable": True}]


def cache_fixture():
    return "\n".join((
        "// synthetic, never read from a compiler",
        "CMAKE_GENERATOR:INTERNAL=Visual Studio 17 2022",
        "CMAKE_GENERATOR_PLATFORM:INTERNAL=x64",
        "CMAKE_GENERATOR_TOOLSET:INTERNAL=host=x64",
        "CMAKE_GENERATOR_INSTANCE:INTERNAL=C:/Tools/Visual Studio/2022",
        "CMAKE_COMMAND:INTERNAL=C:/Tools/CMake/bin/cmake.exe",
        "CMAKE_BUILD_TYPE:UNINITIALIZED=Release",
        "CMAKE_C_FLAGS_RELEASE:STRING=/O2 /Ob2 /DNDEBUG",
        "CMAKE_INSTALL_PREFIX:PATH=" + str(NATIVE_OUT),
        "CMAKE_CACHEFILE_DIR:INTERNAL=" + str(NATIVE_OUT / "build"),
        "CMAKE_HOME_DIRECTORY:INTERNAL=C:/fabricated/cache/opusic-sys-0.7.7/opus",
        "OPUS_HARDENING:BOOL=ON", "OPUS_DISABLE_INTRINSICS:BOOL=OFF",
        "OPUS_BUILD_SHARED_LIBRARY:BOOL=OFF",
    ))


class OptionTests(unittest.TestCase):
    def rejected(self, tokens):
        with self.assertRaises(CHECKER.QualificationError):
            CHECKER.validate_options(tokens)

    def test_positive_bare_defines_and_mixed_option_case(self):
        self.assertFalse(CHECKER.validate_options(options()))
        self.assertTrue(CHECKER.validate_options(options() + ["-arch:AVX2"]))
        tokens = [t.replace("/O2", "-o2").replace("/Ob2", "-oB2") for t in options()]
        self.assertFalse(CHECKER.validate_options(tokens))

    def test_separate_define_forms_preserve_macro_case(self):
        tokens = []
        for token in options():
            tokens.extend(["-D", token[2:]] if token.startswith("/D") else [token])
        self.assertEqual(CHECKER.join_defines(["/D", "MixedCase=1", "-DNDEBUG"]),
                         ["/DMixedCase=1", "-DNDEBUG"])
        self.assertFalse(CHECKER.validate_options(tokens))
        self.rejected([t.replace("NDEBUG", "ndebug") for t in options()])

    def test_real_shaped_options_keep_lowercase_diagnostics_separate(self):
        noise = ["/IC:/fab/include", "/nologo", "/W0", "/WX-", "/diagnostics:column",
                 "/D", "_MBCS", "/D", "HAVE_LRINT", "/D", "HAVE_LRINTF",
                 "/D", "HAVE_CONFIG_H", "/D", "_CRT_SECURE_NO_WARNINGS",
                 "/Zc:wchar_t", "/Zc:forScope", "/Zc:inline", "/std:c11",
                 "/FoOPUS.DIR\\RELEASE\\", "/FdC:/fab/x.pdb", "/external:W0", "/Gd", "/TC",
                 "/w44244", "/w44305", "/w44267"]
        self.assertFalse(CHECKER.validate_options(options() + noise))
        self.assertEqual(CHECKER.join_defines(["/diagnostics:column", "/d1foo", "/d2bar"]),
                         ["/diagnostics:column", "/d1foo", "/d2bar"])
        self.rejected(options() + ["/D", "/diagnostics:column"])
        self.rejected([t for t in options() if t != "/DNDEBUG"] + ["/d", "NDEBUG"])

    def test_conflicts_and_duplicate_families(self):
        for override in ("/O1", "/Od", "/Ox", "/Os", "/Ot", "/O2", "/Ob0", "/Ob1", "/Ob2",
                         "/Ob3", "/MDd", "/MT", "/MTd", "/MD", "/fp:fast", "/fp:strict",
                         "/fp:except", "/fp:precise", "/GS-", "/GS", "-Brepro", "/Brepro-",
                         "/arch:SSE2", "/RTC1", "/RTCsu", "/GL", "/GL-", "/U", "/UNDEBUG",
                         "@fabricated.rsp", "/link"):
            with self.subTest(option=override):
                self.rejected(options() + [override])

    def test_missing_families_and_path_substring_impostors(self):
        for wanted in ("/O2", "/Ob2", "/MD", "/fp:precise", "/GS", "-Brepro"):
            with self.subTest(option=wanted):
                self.rejected([t for t in options() if t != wanted] + ["/IC:/fake/" + wanted[1:]])
        self.assertFalse(CHECKER.validate_options(options() + ["/IC:/RTC1/GL/NDEBUG", "/FoC:/fake/Ob0.obj"]))

    def test_defines_missing_assigned_duplicate_and_debug_poison(self):
        for name in sorted(CHECKER.REQUIRED_DEFINES):
            with self.subTest(macro=name):
                base = [t for t in options() if t != "/D" + name]
                self.rejected(base)
                self.rejected(base + ["/D" + name + "=1"])
                self.rejected(options() + ["/D", name])
                self.rejected(base + ["/IC:/fake/" + name])
        for poison in ("/D_DEBUG", "/D_DEBUG=0", "/D", "/D!bad", "/DNAME=1", "/DNAME=2"):
            if poison.startswith("/DNAME"):
                self.rejected(options() + ["/DNAME=0", poison])
            else:
                self.rejected(options() + [poison])

    def test_release_define_and_arch_duplicates(self):
        self.rejected([t.replace('CMAKE_INTDIR="Release"', 'CMAKE_INTDIR="Debug"') for t in options()])
        self.rejected(options() + ["/arch:AVX2", "-arch:avx2"])
        self.rejected(options() + ["\x00"])


class RecordTests(unittest.TestCase):
    def validate(self, lines, token_map):
        return CHECKER.validate_records("\n".join(lines) + "\n", token_map.__getitem__)

    def test_positive_152_unique_source_bound_records(self):
        self.assertEqual(self.validate(*records_fixture()), {"compiler_records": 152, "avx2_records": 3})

    def test_trailing_log_newlines_do_not_hide_internal_pair_errors(self):
        lines, mapping = records_fixture()
        self.assertEqual(CHECKER.validate_records("\r\n".join(lines) + "\r\n\r\n", mapping.__getitem__),
                         {"compiler_records": 152, "avx2_records": 3})
        with self.assertRaises(CHECKER.QualificationError):
            CHECKER.validate_records("\n".join(lines[:2] + [""] + lines[2:]), mapping.__getitem__)

    def test_pairing_empty_truncated_extra_and_nul(self):
        lines, mapping = records_fixture()
        variants = (lines[:-1], lines + ["noise"], [""] + lines, ["^"],
                    [lines[0], "^another-source"] + lines[2:],
                    [lines[0], ""] + lines[2:], [lines[0] + "\x00"] + lines[1:])
        for bad in variants:
            with self.subTest(prefix=bad[:2]), self.assertRaises(CHECKER.QualificationError):
                self.validate(bad, mapping)

    def test_duplicate_multi_header_and_wrong_last_source(self):
        for change in ("duplicate", "multi-header", "wrong-last", "extra-source", "relative"):
            lines, mapping = records_fixture()
            if change == "duplicate":
                lines[2:4] = lines[:2]
            elif change == "multi-header":
                lines[0] += "|C:/fabricated/second.c"
            elif change == "wrong-last":
                mapping[lines[1]][-1] = "C:/fabricated/wrong.c"
            elif change == "extra-source":
                mapping[lines[1]].insert(0, "C:/fabricated/extra.c")
            else:
                lines[0] = "^relative.c"
            with self.subTest(change=change), self.assertRaises(CHECKER.QualificationError):
                self.validate(lines, mapping)

    def test_avx_set_requires_exact_components_not_substrings(self):
        for change in ("missing", "extra", "impostor", "other-tree"):
            lines, mapping = records_fixture()
            if change == "missing":
                mapping[lines[1]].remove("/arch:AVX2")
            elif change == "extra":
                mapping[lines[7]].insert(0, "/arch:AVX2")
            else:
                old = mapping[lines[1]][-1]
                new = old.replace("/opus/celt/", "/opus/notcelt/") if change == "impostor" else old.replace("/cache/", "/other/")
                mapping[lines[1]][-1] = new
                lines[0] = "^" + new
            with self.subTest(change=change), self.assertRaises(CHECKER.QualificationError):
                self.validate(lines, mapping)

    def test_records_bind_to_cache_source_home(self):
        lines, mapping = records_fixture()
        text = "\n".join(lines)
        home = "c:/fabricated/cache/opusic-sys-0.7.7/opus"
        self.assertEqual(CHECKER.validate_records(text, mapping.__getitem__, home),
                         {"compiler_records": 152, "avx2_records": 3})
        with self.assertRaises(CHECKER.QualificationError):
            CHECKER.validate_records(text, mapping.__getitem__, home.replace("/cache/", "/other/"))

    def test_tokenizer_is_injected_and_options_checked_each_record(self):
        lines, mapping = records_fixture()
        calls = []
        def tokenizer(command):
            calls.append(command)
            return mapping[command]
        CHECKER.validate_records("\n".join(lines), tokenizer)
        self.assertEqual(len(calls), 152)
        mapping[lines[-1]].insert(0, "/Od")
        with self.assertRaises(CHECKER.QualificationError):
            self.validate(lines, mapping)


class HarnessTests(unittest.TestCase):
    def test_exact_nine_names_with_normal_noise(self):
        self.assertEqual(CHECKER.validate_test_output(harness_output()), 9)

    def test_compiler_warning_about_a_test_is_not_a_harness_record(self):
        self.assertEqual(CHECKER.validate_test_output(
            "warning: this test helper is unused\n" + harness_output()), 9)

    def test_count_poison_duplicate_unknown_missing_and_extra(self):
        text = harness_output()
        name, other = HARNESS_NAMES[:2]
        old_eight = (text.replace("running 9 tests", "running 8 tests")
                     .replace("9 passed", "8 passed")
                     .replace("test " + name + " ... ok\n", ""))
        variants = (
            old_eight, text.replace("9 passed", "8 passed"), text.replace("0 ignored", "1 ignored"),
            text + "running 9 tests\n", text + "test result: FAILED.\n",
            text + "test injected ... ok\n", text.replace(name, other),
            text.replace(name, "unknown"), text.replace("test " + name + " ... ok\n", ""),
            text.replace(" ... ok", " ... ignored", 1), text.replace(" ... ok", " ... FAILED", 1),
            text + "test malformed\n", text.replace("running 9 tests", "running 90 tests"),
            text + "\x1b[0m", text + "  running 9 tests\n", text + "  test injected ... ok\n",
        )
        for bad in variants:
            with self.subTest(tail=bad[-100:]), self.assertRaises(CHECKER.QualificationError):
                CHECKER.validate_test_output(bad)


class CargoOutputTests(unittest.TestCase):
    def parse(self, message):
        text = json.dumps(message) + "\n" + harness_output()
        self.assertEqual(CHECKER.validate_test_output(text), 9)
        return CHECKER.opus_output_directory(text, TARGET)

    def test_out_dir_independent_of_old_new_or_future_private_layout(self):
        for suffix in ("release/build/opusic-sys-0123456789abcdef/out",
                       "release/build/opusic-sys/0123456789abcdef/out",
                       "release/build/opusic-sys/0123456789abcdef/run-custom-build/out"):
            out = TARGET / suffix
            with self.subTest(suffix=suffix):
                self.assertEqual(self.parse(build_message(out)), out)
        noise = {"reason": "compiler-artifact", "package_id": "other"}
        text = json.dumps(noise) + "\n" + json.dumps(build_message()) + "\n" + harness_output()
        self.assertEqual(CHECKER.opus_output_directory(text, TARGET), NATIVE_OUT)

    def test_missing_duplicate_wrong_version_name_and_source(self):
        good = build_message()
        bad_messages = [{}, dict(good, reason="compiler-artifact")]
        for package in (CHECKER.OPUS_PACKAGE_ID.replace("0.7.7", "0.7.8"),
                        CHECKER.OPUS_PACKAGE_ID.replace("opusic-sys@", "opusic-sys2@"),
                        "path+file:///fabricated#opusic-sys@0.7.7",
                        "sparse+https://index.crates.io/#opusic-sys@0.7.7"):
            bad_messages.append(dict(good, package_id=package))
        for bad in bad_messages:
            with self.subTest(message=bad), self.assertRaises(CHECKER.QualificationError):
                self.parse(bad)
        for text in (harness_output(), (json.dumps(good) + "\n") * 2 + harness_output()):
            with self.assertRaises(CHECKER.QualificationError):
                CHECKER.opus_output_directory(text, TARGET)

    def test_out_dir_and_native_link_binding(self):
        for out in (None, 17, "relative/out", "//host/share/out", "\\\\?\\C:\\out",
                    str(TARGET / "../escape/out"), str(TARGET) + "-other/out", str(TARGET)):
            with self.subTest(out=out), self.assertRaises(CHECKER.QualificationError):
                self.parse(dict(build_message(), out_dir=out))
        for field, value in (("linked_libs", ["opus"]), ("linked_libs", ["static=other"]),
                             ("linked_libs", ["static=opus", "static=opus"]),
                             ("linked_paths", []), ("linked_paths", ["native=C:/other/lib"]),
                             ("linked_paths", ["C:/other/lib"]),
                             ("linked_paths", build_message()["linked_paths"] * 2)):
            with self.subTest(field=field, value=value), self.assertRaises(CHECKER.QualificationError):
                self.parse(dict(build_message(), **{field: value}))
        missing = build_message()
        del missing["out_dir"]
        with self.assertRaises(CHECKER.QualificationError):
            self.parse(missing)

    def test_malformed_json_duplicate_keys_and_reason_type(self):
        for line in ("{", '{"reason":3}', '{"reason":"a","reason":"b"}'):
            with self.subTest(line=line), self.assertRaises(CHECKER.QualificationError):
                CHECKER.opus_output_directory(line + "\n" + json.dumps(build_message()), TARGET)

    def test_native_file_guard_checks_target_and_all_components(self):
        with tempfile.TemporaryDirectory(prefix="rambler-fabricated-layout-") as folder:
            base = Path(folder)
            path = base / "out/build/CMakeCache.txt"
            path.parent.mkdir(parents=True)
            path.write_text("fabricated", encoding="utf-8")
            self.assertEqual(CHECKER.plain_native_path(path, base), path)
            with self.assertRaises(CHECKER.QualificationError):
                CHECKER.plain_native_path(base.parent / "outside", base)
            for poisoned in (base, base / "out", path.parent, path):
                for mode, attributes in ((CHECKER.stat.S_IFLNK, 0), (CHECKER.stat.S_IFDIR, 0x400)):
                    real_lstat = Path.lstat
                    def fake_lstat(item, *, poisoned=poisoned, mode=mode, attributes=attributes):
                        if item == poisoned:
                            return types.SimpleNamespace(st_mode=mode, st_file_attributes=attributes)
                        return real_lstat(item)
                    with self.subTest(component=poisoned, attributes=attributes), \
                            patch.object(Path, "lstat", fake_lstat), \
                            self.assertRaises(CHECKER.QualificationError):
                        CHECKER.plain_native_path(path, base)


class MetadataTests(unittest.TestCase):
    def approved(self):
        return CHECKER.validate_vswhere(json.dumps(instance_fixture()).encode("utf-8"))

    def test_vs_utf8_and_case_slash_normalized_identity(self):
        self.assertTrue(CHECKER.validate_cache(cache_fixture(), self.approved(), "c:\\TOOLS\\cmake\\bin\\CMAKE.exe", NATIVE_OUT))
        self.assertEqual(self.approved(), {"c:/tools/visual studio/2022"})
        items = instance_fixture()
        items[0]["displayName"] = "Fabricated édition"
        self.assertEqual(CHECKER.validate_vswhere(json.dumps(items, ensure_ascii=False).encode()), self.approved())

    def test_vs_major_incomplete_missing_malformed_duplicate(self):
        for key, value in (("installationVersion", "18.0.1.2"), ("installationVersion", "17.bad"),
                           ("installationPath", "relative"), ("isComplete", False), ("isLaunchable", 1)):
            items = instance_fixture()
            items[0][key] = value
            with self.subTest(key=key), self.assertRaises(CHECKER.QualificationError):
                CHECKER.validate_vswhere(json.dumps(items).encode())
        for raw in (b"\xff", b"[]", b"{}", b"null", b'[{},{}]',
                    b'[{"installationVersion":"17.0.1.2","installationVersion":"17.1.2.3"}]',
                    json.dumps(instance_fixture() * 2).encode()):
            with self.subTest(raw=raw), self.assertRaises(CHECKER.QualificationError):
                CHECKER.validate_vswhere(raw)

    def test_cache_generator_platform_host_release_cmake_instance_and_duplicates(self):
        text = cache_fixture()
        variants = (
            text.replace("Visual Studio 17 2022", "Ninja"), text.replace("=x64", "=Win32"),
            text.replace("host=x64", "host=x86"), text.replace("=Release", "=Debug"),
            text.replace("CMAKE_BUILD_TYPE:UNINITIALIZED", "CMAKE_BUILD_TYPE:STRING"),
            text.replace("/Visual Studio/2022", "/Visual Studio/other"),
            text.replace("/CMake/bin/", "/Other/bin/"), text.replace("HARDENING:BOOL=ON", "HARDENING:BOOL=OFF"),
            text.replace("INTRINSICS:BOOL=OFF", "INTRINSICS:BOOL=ON"),
            text.replace("/O2 /Ob2 /DNDEBUG", "-nologo -MD -Brepro"),
            text.replace("CMAKE_INSTALL_PREFIX:PATH=", "CMAKE_INSTALL_PREFIX:STRING="),
            text.replace("CMAKE_CACHEFILE_DIR:INTERNAL=", "CMAKE_CACHEFILE_DIR:PATH="),
            text.replace("0123456789abcdef", "fedcba9876543210"),
            text.replace("opusic-sys-0.7.7/opus", "opusic-sys-0.7.8/opus"),
            text.replace("CMAKE_HOME_DIRECTORY:INTERNAL=", "CMAKE_HOME_DIRECTORY:PATH="),
            text + "\nCMAKE_GENERATOR:INTERNAL=Visual Studio 17 2022", text + "\nmalformed",
        )
        for bad in variants:
            with self.subTest(tail=bad[-100:]), self.assertRaises(CHECKER.QualificationError):
                CHECKER.validate_cache(bad, self.approved(), "C:/Tools/CMake/bin/cmake.exe", NATIVE_OUT)

    def test_stable_host_identity(self):
        text = "rustc 1.99.1 (fabricated)\nhost: x86_64-pc-windows-msvc\nrelease: 1.99.1\n"
        self.assertEqual(CHECKER.validate_rustc(text), "1.99.1")
        for bad in (text.replace("x86_64", "i686"), text.replace("release: 1.99.1", "release: 1.99.1-nightly"),
                    text + "host: x86_64-pc-windows-msvc\n"):
            with self.assertRaises(CHECKER.QualificationError):
                CHECKER.validate_rustc(bad)

    def test_small_environment_drops_every_unapproved_override(self):
        poison = ("CL", "_CL_", "INCLUDE", "LIB", "LIBPATH", "VSINSTALLDIR", "VCToolsInstallDir",
                  "CFLAGS", "HOST_CFLAGS", "TARGET_CFLAGS", "CXX", "CC", "CMAKE_TOOLCHAIN_FILE",
                  "CMAKE_GENERATOR", "HOST_CMAKE", "TARGET_CMAKE", "RUSTFLAGS", "RUSTC_WRAPPER",
                  "RUSTC_WORKSPACE_WRAPPER", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET",
                  "CARGO_PROFILE_RELEASE_OPT_LEVEL", "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER")
        parent = {name: "POISON" for name in poison}
        parent.update({"SystemRoot": "C:/Windows", "ProgramData": "C:/ProgramData", "RUSTUP_TOOLCHAIN": "nightly",
                       "CMAKE": "POISON", "CARGO_TARGET_DIR": "POISON", "PATH": "POISON"})
        env = CHECKER.child_environment(parent, Path("C:/Cargo/bin"), Path("C:/Windows"),
                                        Path("C:/CMake/cmake.exe"), Path("C:/repo/target/vs17-functional"))
        self.assertTrue(all(name.upper() not in env for name in poison))
        self.assertEqual(env["RUSTUP_TOOLCHAIN"], "stable")
        self.assertEqual(env["MSBUILDDISABLENODEREUSE"], "1")
        self.assertEqual(env["RUSTUP_AUTO_INSTALL"], "0")
        self.assertEqual(env["CARGO_NET_OFFLINE"], "true")
        self.assertIn("PROGRAMDATA", env)
        self.assertNotIn("POISON", env.values())


class OwnedHelperContractTests(unittest.TestCase):
    def test_pin_and_canonical_digest_contract(self):
        raw = Path(__file__).resolve().with_name("windows_owned_job.py").read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), CHECKER.OWNED_JOB_SHA256)
        self.assertEqual(HELPER.canonical({"z": "é", "a": [1, True]}), b'{"a":[1,true],"z":"\\u00e9"}')
        self.assertEqual(HELPER.digest(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")

    def test_capture_ok_all_truth_controls_without_running_capture(self):
        good = dict(actual_exit=0, gate_released=True, job_empty=True,
                    timeout=False, output_limit=False, spawn_error=False, capture_error=False,
                    containment_error=False, cleanup_error=False)
        self.assertTrue(HELPER.capture_ok(good))
        for key in ("timeout", "output_limit", "spawn_error", "capture_error", "containment_error", "cleanup_error"):
            with self.subTest(key=key):
                self.assertFalse(HELPER.capture_ok(dict(good, **{key: True})))
        for key in ("gate_released", "job_empty"):
            self.assertFalse(HELPER.capture_ok(dict(good, **{key: False})))
        for status in (None, 1, -1, 0xC0000005):
            self.assertFalse(HELPER.capture_ok(dict(good, actual_exit=status)))

    def test_bootstrap_source_contract_not_executed(self):
        tree = ast.parse(HELPER.BOOTSTRAP)
        self.assertTrue(isinstance(tree, ast.Module))
        self.assertIn("sys.stdin.buffer.read(1048577)", HELPER.BOOTSTRAP)
        self.assertIn("subprocess.Popen(payload['argv'], stdin=subprocess.DEVNULL)", HELPER.BOOTSTRAP)
        self.assertLess(HELPER.BOOTSTRAP.index("sys.stdin.buffer.read"), HELPER.BOOTSTRAP.index("subprocess.Popen"))
        self.assertNotIn("shell=True", HELPER.BOOTSTRAP)


if __name__ == "__main__":
    if not sys.flags.isolated:
        raise SystemExit("Run this synthetic-only script explicitly with python -I.")
    unittest.main(verbosity=2)
