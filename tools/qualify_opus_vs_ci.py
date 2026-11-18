"""CI-only functional VS17 qualification, not a benchmark or example-main run.

Run explicitly with Python -I after promotion to tools/. Synthetic selftests
inject a tokenizer; only the Windows entry point uses CommandLineToArgvW.
"""
from __future__ import annotations

from collections import Counter
import ctypes
from ctypes import wintypes
import hashlib
import json
import ntpath
import os
from pathlib import Path
import re
import shutil
import stat
import sys
import tempfile
import time
import types

OWNED_JOB_SHA256 = "3ac53d5e3f202512b18cb3e4bafdc27c8b87add5c4d9afb70a3dd07462fd76aa"
GENERATOR = "Visual Studio 17 2022"
OPUS_PACKAGE_ID = "registry+https://github.com/rust-lang/crates.io-index#opusic-sys@0.7.7"
REQUIRED_DEFINES = frozenset((
    "NDEBUG", "OPUS_BUILD", "ENABLE_HARDENING", "DISABLE_DEBUG_FLOAT", "USE_ALLOCA",
    "OPUS_HAVE_RTCD", "OPUS_X86_MAY_HAVE_SSE", "OPUS_X86_PRESUME_SSE",
    "OPUS_X86_MAY_HAVE_SSE2", "OPUS_X86_PRESUME_SSE2", "OPUS_X86_MAY_HAVE_SSE4_1",
    "OPUS_X86_MAY_HAVE_AVX2",
))
AVX2_SOURCES = frozenset((
    "celt/x86/pitch_avx.c", "silk/x86/nsq_del_dec_avx2.c",
    "silk/float/x86/inner_product_flp_avx2.c",
))
EXPECTED_TESTS = frozenset((
    "ingress::tests::partitions_match_the_same_clip_and_cross_sample_and_frame_seams",
    "tests::codec_defaults_and_explicit_bounds",
    "tests::codec_rejects_zero_overflow_unknown_missing_duplicate_and_bad_numbers",
    "tests::checked_padding_covers_boundaries_without_extra_frames",
    "tests::conversion_preserves_le_samples_tail_and_lookahead_zeros",
    "tests::synthetic_inputs_retain_published_integer_only_fingerprints",
    "native_ogg::tests::maximum_packets_and_255_byte_multiples_keep_valid_lacing_and_crc",
    "native_ogg::tests::pending_output_has_hard_limit_and_bad_packet_sizes_fail",
    "native_ogg::tests::sequence_overflow_is_an_error_not_a_panic",
))
ENV_ALLOWLIST = frozenset(name.upper() for name in (
    "SystemRoot", "windir", "SystemDrive", "ComSpec", "PATHEXT", "TEMP", "TMP",
    "USERPROFILE", "LOCALAPPDATA", "APPDATA", "ProgramData", "ProgramFiles",
    "ProgramFiles(x86)", "ProgramW6432", "CommonProgramFiles", "CommonProgramFiles(x86)",
    "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE", "OS", "CARGO_HOME", "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
))


class QualificationError(ValueError):
    """Messages are fixed diagnostic codes, never paths or environment values."""


def require(condition, code):
    if not condition:
        raise QualificationError(code)


def one(items, code):
    items = list(items)
    require(len(items) == 1, code)
    return items[0]


def windows_path(value):
    require(isinstance(value, str) and bool(re.match(r"^[A-Za-z]:[\\/]", value)),
            "invalid_absolute_windows_path")
    require(not any(c in value for c in '\x00\r\n|\"') and
            not any(x in (".", "..") for x in value.replace("\\", "/").split("/")),
            "invalid_windows_path_component")
    return ntpath.normcase(ntpath.normpath(value)).replace("\\", "/")


def load_owned_helper():
    """Execute exactly the verified bytes, without a loader reread or sys.path edit."""
    path = Path(__file__).resolve().with_name("windows_owned_job.py")
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == OWNED_JOB_SHA256, "owned_helper_pin_mismatch")
    name = "_rambler_ci_verified_windows_owned_job"
    module = types.ModuleType(name)
    module.__file__ = str(path)
    sys.modules[name] = module
    try:
        exec(compile(raw, "<verified-windows-owned-job>", "exec"), module.__dict__)
    except BaseException:
        sys.modules.pop(name, None)
        raise
    return module


def windows_arguments(command):
    """Native quoting seam; pure validators below never call Windows APIs."""
    require(isinstance(command, str) and command and "\x00" not in command,
            "invalid_command_line")
    shell = ctypes.WinDLL("shell32", use_last_error=True)
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    shell.CommandLineToArgvW.argtypes = [wintypes.LPCWSTR, ctypes.POINTER(ctypes.c_int)]
    shell.CommandLineToArgvW.restype = ctypes.POINTER(wintypes.LPWSTR)
    kernel.LocalFree.argtypes = [ctypes.c_void_p]
    kernel.LocalFree.restype = ctypes.c_void_p
    count = ctypes.c_int()
    # The tlog omits cl.exe. A dummy argv[0] gives the first option normal rules.
    argv = shell.CommandLineToArgvW("cl.exe " + command, ctypes.byref(count))
    require(bool(argv), "windows_tokenization_failed")
    try:
        return [argv[i] for i in range(1, count.value)]
    finally:
        kernel.LocalFree(ctypes.cast(argv, ctypes.c_void_p))


def join_defines(tokens):
    """Join /D NAME and -DNAME without changing case-sensitive macro text."""
    require(isinstance(tokens, (list, tuple)) and
            all(isinstance(t, str) and t and "\x00" not in t for t in tokens),
            "invalid_token_list")
    result = []
    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token in ("/D", "-D"):
            index += 1
            require(index < len(tokens) and not tokens[index].startswith(("/", "-")),
                    "missing_define_value")
            token += tokens[index]
        result.append(token)
        index += 1
    return result


def validate_options(tokens):
    """Check actual option tokens, not substrings in include/output/source paths."""
    tokens = join_defines(tokens)
    require(not any(t.startswith("@") or t.lower() in ("/link", "-link") for t in tokens),
            "hidden_or_linker_options_not_allowed")
    families = {key: [] for key in ("opt", "inline", "runtime", "fp", "gs", "brepro", "arch")}
    definitions = {}
    for token in tokens:
        if not token.startswith(("/", "-")):
            continue
        option = token[1:]
        lower = option.lower()
        require(not lower.startswith(("rtc", "gl", "u")), "forbidden_compiler_option")
        if option.startswith("D"):
            value = option[1:]
            match = re.fullmatch(r"([A-Za-z_][A-Za-z_0-9]*)(?:=(.*))?", value)
            require(match is not None, "malformed_define")
            name = match[1]
            require(name not in definitions, "duplicate_define")
            definitions[name] = value
        elif lower.startswith("ob"):
            families["inline"].append(lower)
        elif lower.startswith("o"):
            families["opt"].append(lower)
        elif re.match(r"m[dt]", lower):
            families["runtime"].append(lower)
        elif lower.startswith("fp"):
            families["fp"].append(lower)
        elif lower.startswith("gs"):
            families["gs"].append(lower)
        elif lower.startswith("brepro"):
            families["brepro"].append(lower)
        elif lower.startswith("arch"):
            families["arch"].append(lower)
    for family, expected in (("opt", "o2"), ("inline", "ob2"), ("runtime", "md"),
                             ("fp", "fp:precise"), ("gs", "gs"), ("brepro", "brepro")):
        require(families[family] == [expected], "compiler_option_family_mismatch")
    require(families["arch"] in ([], ["arch:avx2"]), "unexpected_arch_option")
    require("_DEBUG" not in definitions, "debug_define_present")
    require(all(definitions.get(name) == name for name in REQUIRED_DEFINES),
            "missing_or_nonbare_required_define")
    require(definitions.get("CMAKE_INTDIR") in ('CMAKE_INTDIR="Release"', 'CMAKE_INTDIR=Release'),
            "nonrelease_compile_record")
    return bool(families["arch"])


def validate_records(text, tokenizer, opus_home=None):
    """Strict source/command pairs; tokenizer injection is the synthetic-data seam."""
    require(isinstance(text, str) and "\x00" not in text, "invalid_tlog_text")
    lines = text.rstrip("\r\n").splitlines()
    require(len(lines) == 304, "expected_152_source_command_pairs")
    sources = set()
    avx_sources = set()
    for index in range(0, len(lines), 2):
        header, command = lines[index:index + 2]
        require(header.startswith("^") and len(header) > 1 and not command.startswith("^")
                and bool(command.strip()), "malformed_source_command_pair")
        source = windows_path(header[1:])
        require(source.endswith(".c") and source not in sources, "duplicate_or_non_c_source")
        sources.add(source)
        tokens = tokenizer(command)
        require(isinstance(tokens, (list, tuple)) and bool(tokens), "empty_command_tokens")
        require(windows_path(tokens[-1]) == source, "source_command_binding_mismatch")
        # There must be exactly one input C source, in the final argument position.
        c_inputs = [windows_path(t) for t in tokens if re.match(r"^[A-Za-z]:[\\/]", t)
                    and t.lower().endswith(".c")]
        require(c_inputs == [source], "multiple_or_unbound_c_inputs")
        avx = validate_options(tokens[:-1])
        relative = source.split("/opus/")
        require(len(relative) == 2, "source_not_in_one_opus_tree")
        if avx:
            require(relative[1] in AVX2_SOURCES, "avx2_on_wrong_source")
            avx_sources.add(relative[1])
    require(len(sources) == 152 and avx_sources == AVX2_SOURCES, "wrong_source_or_avx2_set")
    require(len({source.split("/opus/")[0] for source in sources}) == 1,
            "multiple_opus_source_trees")
    if opus_home is not None:
        require(all(source.startswith(opus_home + "/") for source in sources),
                "compiler_sources_not_in_cache_home")
    return {"compiler_records": 152, "avx2_records": 3}


def validate_test_output(text):
    require(isinstance(text, str) and "\x00" not in text and "\x1b" not in text,
            "invalid_test_output")
    lines = text.splitlines()
    # Ignore ordinary Cargo/compiler noise, but no malformed/duplicate harness lines.
    running = [x for x in lines if x.startswith("running ") or
               re.search(r"\brunning\s+[0-9]+\s+tests?\b", x)]
    require(running == ["running 9 tests"], "wrong_running_test_marker")
    summaries = [x for x in lines if "test result:" in x]
    require(len(summaries) == 1 and re.fullmatch(
        r"test result: ok\. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in [0-9]+\.[0-9]+s",
        summaries[0]) is not None, "wrong_test_summary")
    entries = [x for x in lines if x.lstrip().startswith("test ") and
               not x.lstrip().startswith("test result:")]
    matches = [re.fullmatch(r"test (\S+) \.\.\. ok", x) for x in entries]
    require(len(matches) == 9 and all(matches), "malformed_or_extra_test_record")
    require(Counter(m[1] for m in matches) == Counter(EXPECTED_TESTS), "wrong_test_names")
    return 9


def unique_json_object(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value, "duplicate_json_key")
        value[key] = item
    return value


def opus_output_directory(text, target):
    """Use Cargo's documented OUT_DIR message, not its private folder layout."""
    matches = []
    for line in text.splitlines():
        if not line.startswith("{"):
            continue
        try:
            message = json.loads(line, object_pairs_hook=unique_json_object)
        except json.JSONDecodeError as error:
            raise QualificationError("malformed_cargo_json") from error
        require(isinstance(message, dict) and isinstance(message.get("reason"), str),
                "invalid_cargo_message")
        if (message["reason"] == "build-script-executed" and
                message.get("package_id") == OPUS_PACKAGE_ID):
            matches.append(message)
    message = one(matches, "expected_one_opus_build_script")
    out = message.get("out_dir")
    normalized = windows_path(out)
    base = windows_path(str(target))
    require(normalized.startswith(base.rstrip("/") + "/"), "opus_output_outside_target")
    require(message.get("linked_libs") == ["static=opus"], "wrong_opus_linked_libraries")
    paths = message.get("linked_paths")
    require(isinstance(paths, list) and len(paths) == 1 and isinstance(paths[0], str) and
            paths[0].startswith("native=") and
            windows_path(paths[0][7:]) == normalized + "/lib", "wrong_opus_linked_path")
    return Path(out)


def plain_native_path(path, target):
    """Reject links/junctions in every component before reading native evidence."""
    require(path.is_relative_to(target), "native_path_outside_target")
    current = target
    for part in (None, *path.relative_to(target).parts):
        if part is not None:
            current /= part
        info = current.lstat()
        require(not stat.S_ISLNK(info.st_mode) and
                not getattr(info, "st_file_attributes", 0) & 0x400, "native_path_is_reparse_point")
    return path


def validate_vswhere(raw):
    require(isinstance(raw, bytes), "vswhere_requires_utf8_bytes")
    try:
        instances = json.loads(raw.decode("utf-8", "strict"), object_pairs_hook=unique_json_object)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise QualificationError("invalid_vswhere_utf8_json") from error
    require(isinstance(instances, list) and bool(instances), "vs17_cpp_instance_missing")
    approved = set()
    for instance in instances:
        require(isinstance(instance, dict) and
                isinstance(instance.get("installationVersion"), str) and
                re.fullmatch(r"17\.[0-9]+\.[0-9]+\.[0-9]+", instance["installationVersion"]) and
                instance.get("isComplete") is True and instance.get("isLaunchable") is True,
                "unapproved_vs_instance")
        path = windows_path(instance.get("installationPath"))
        require(path not in approved, "duplicate_vs_instance")
        approved.add(path)
    return approved


def validate_cache(text, approved_instances, selected_cmake, out_dir):
    cache = {}
    for line in text.splitlines():
        if not line or line.startswith(("//", "#")):
            continue
        match = re.fullmatch(r"([^:=]+):([^=]+)=(.*)", line)
        require(match is not None and match[1] not in cache, "malformed_or_duplicate_cache_key")
        cache[match[1]] = (match[2], match[3])
    expected = {
        "CMAKE_GENERATOR": ("INTERNAL", GENERATOR),
        "CMAKE_GENERATOR_PLATFORM": ("INTERNAL", "x64"),
        "CMAKE_GENERATOR_TOOLSET": ("INTERNAL", "host=x64"),
        "CMAKE_BUILD_TYPE": ("UNINITIALIZED", "Release"),
        "CMAKE_C_FLAGS_RELEASE": ("STRING", "/O2 /Ob2 /DNDEBUG"),
        "OPUS_HARDENING": ("BOOL", "ON"),
        "OPUS_DISABLE_INTRINSICS": ("BOOL", "OFF"),
        "OPUS_BUILD_SHARED_LIBRARY": ("BOOL", "OFF"),
    }
    require(all(cache.get(key) == value for key, value in expected.items()),
            "unexpected_vs17_release_cache")
    for key in ("CMAKE_GENERATOR_INSTANCE", "CMAKE_COMMAND"):
        require(key in cache and cache[key][0] == "INTERNAL", "missing_cache_tool_identity")
    require(windows_path(cache["CMAKE_GENERATOR_INSTANCE"][1]) in approved_instances,
            "cache_instance_not_approved")
    require(windows_path(cache["CMAKE_COMMAND"][1]) == windows_path(str(selected_cmake)),
            "cache_cmake_not_selected")
    for key, kind, value in (("CMAKE_INSTALL_PREFIX", "PATH", str(out_dir)),
                             ("CMAKE_CACHEFILE_DIR", "INTERNAL", str(out_dir / "build"))):
        require(key in cache and cache[key][0] == kind and
                windows_path(cache[key][1]) == windows_path(value), "cache_output_binding_mismatch")
    require("CMAKE_HOME_DIRECTORY" in cache and cache["CMAKE_HOME_DIRECTORY"][0] == "INTERNAL",
            "missing_cache_source_home")
    home = windows_path(cache["CMAKE_HOME_DIRECTORY"][1])
    require(home.endswith("/opusic-sys-0.7.7/opus"), "unexpected_opus_source_home")
    return home


def validate_rustc(text):
    lines = text.splitlines()
    release = [x[9:] for x in lines if x.startswith("release: ")]
    require([x for x in lines if x.startswith("host: ")] == ["host: x86_64-pc-windows-msvc"] and
            len(release) == 1 and re.fullmatch(r"1\.[0-9]+\.[0-9]+", release[0]) is not None,
            "expected_stable_x64_msvc_rustc")
    return release[0]


def child_environment(parent, cargo_bin, system_root, cmake, target):
    env = {key.upper(): value for key, value in parent.items() if key.upper() in ENV_ALLOWLIST}
    env.update({
        "PATH": os.pathsep.join(str(x) for x in (cargo_bin, system_root / "System32", system_root)),
        "CMAKE": str(cmake), "CARGO_TARGET_DIR": str(target), "MSBUILDDISABLENODEREUSE": "1",
        "RUSTUP_AUTO_INSTALL": "0", "RUSTUP_TOOLCHAIN": "stable", "CARGO_NET_OFFLINE": "true",
        "CARGO_TERM_COLOR": "never",
    })
    return env


def print_build_tail(raw):
    # Bound bytes as well as lines, even if one giant line hit the capture cap.
    for line in raw[-16384:].decode("utf-8", "replace").splitlines()[-80:]:
        print(line[:1000], file=sys.stderr)


def main():
    build_raw = b""
    logs_created = False
    try:
        require(sys.argv[1:] == [] and sys.flags.isolated, "run_explicitly_with_python_isolated")
        require(sys.platform == "win32" and os.name == "nt" and
                os.environ.get("GITHUB_ACTIONS") == "true" and
                os.environ.get("RUNNER_OS") == "Windows" and
                os.environ.get("RUNNER_ARCH") == "X64" and
                os.environ.get("PROCESSOR_ARCHITECTURE") == "AMD64", "ci_windows_amd64_only")
        repo = Path(__file__).resolve().parent.parent
        require((repo / "Cargo.lock").is_file() and (repo / "Cargo.toml").is_file(),
                "expected_promoted_repository_tools_location")
        target = repo / "target" / "vs17-functional"
        # This MUST precede helper loading, directory creation and any child.
        require(not target.exists() and not target.is_symlink(), "functional_target_not_fresh")
        parent = {key.upper(): value for key, value in os.environ.items()}
        system_root = Path(parent["SYSTEMROOT"])
        cargo_home = Path(parent.get("CARGO_HOME", str(Path(parent["USERPROFILE"]) / ".cargo")))
        cargo_bin = cargo_home / "bin"
        cargo, rustc = cargo_bin / "cargo.exe", cargo_bin / "rustc.exe"
        found_cmake = shutil.which("cmake.exe")
        require(found_cmake is not None, "cmake_missing")
        cmake = Path(found_cmake).resolve()
        vswhere = Path(parent["PROGRAMFILES(X86)"]) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
        require(all(path.is_absolute() and path.is_file() for path in (cargo, rustc, cmake, vswhere)),
                "required_absolute_tool_missing")
        env = child_environment(parent, cargo_bin, system_root, cmake, target)
        require(shutil.which("ninja", path=env["PATH"]) is None, "ninja_in_child_discovery")
        # Windows CreateProcess/Rust lookup may search these before PATH as well.
        for folder in (repo, Path(sys.executable).parent, cargo_bin, system_root / "System32", system_root):
            require(not any((folder / ("ninja" + suffix)).exists()
                            for suffix in (".exe", ".com", ".bat", ".cmd", "")),
                    "ninja_in_child_implicit_discovery")
        helper = load_owned_helper()
        temp_root = Path(tempfile.gettempdir()).resolve()
        require(not temp_root.is_relative_to(target.resolve()), "temporary_root_inside_target")
        log_dir = Path(tempfile.mkdtemp(prefix="rambler-vs17-functional-", dir=temp_root))
        logs_created = True
        require(not log_dir.resolve().is_relative_to(target.resolve()), "log_inside_target")
        # Shared 720-second budget includes metadata; last child has <=720 seconds
        # plus the unchanged helper's bounded cleanup, below the 13-minute step.
        deadline = time.monotonic() + 720
        sequence = 0

        def capture(argv, timeout, build=False):
            nonlocal sequence, build_raw
            sequence += 1
            remaining = deadline - time.monotonic()
            require(remaining > 0, "qualification_deadline_exhausted")
            result, raw = helper.capture_windows(
                argv, log_dir / (str(sequence) + ".log"), timeout=min(timeout, remaining),
                limit=(16 if build else 8) * 1024 * 1024, cwd=repo, env=env)
            if build:
                build_raw = raw
            require(helper.capture_ok(result), "owned_child_failed_or_not_drained")
            return raw

        approved = validate_vswhere(capture([
            str(vswhere), "-products", "*", "-utf8", "-version", "[17.0,18.0)",
            "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-format", "json",
        ], 30))
        rust_release = validate_rustc(capture([str(rustc), "+stable", "-vV"], 30).decode("utf-8", "strict"))
        raw = capture([
            str(cargo), "+stable", "test", "-p", "rambler-audio", "--example", "profile_pipeline",
            "--release", "--locked", "--offline", "--message-format=json-render-diagnostics",
            "--", "--color", "never",
        ], 720, build=True)
        text = raw.decode("utf-8", "strict")
        validate_test_output(text)
        out = opus_output_directory(text, target)
        cache = plain_native_path(out / "build/CMakeCache.txt", target)
        root = cache.parent
        opus_home = validate_cache(cache.read_text(encoding="utf-8"), approved, cmake, out)
        tlog = plain_native_path(root / "opus.dir/Release/opus.tlog/CL.command.1.tlog", target)
        record_summary = validate_records(tlog.read_text(encoding="utf-16"), windows_arguments, opus_home)
        built = plain_native_path(root / "Release/opus.lib", target)
        installed = plain_native_path(out / "lib/opus.lib", target)
        require(built.is_file() and installed.is_file() and built.stat().st_size > 0, "static_archive_missing")
        archive_hash = hashlib.sha256(built.read_bytes()).hexdigest()
        require(archive_hash == hashlib.sha256(installed.read_bytes()).hexdigest(), "archive_identity_mismatch")
        print(json.dumps({
            "qualification": "functional-only", "vs_major": 17, "generator": GENERATOR,
            "platform": "x64", "host_toolset": "host=x64", "configuration": "Release",
            "rust_stable_release": rust_release, "test_passed": 9, **record_summary,
            "required_bare_defines": sorted(REQUIRED_DEFINES), "brepro_per_record": 1,
            "archive_sha256": archive_hash, "installed_archive_matches": True,
        }, sort_keys=True))
        return 0
    except Exception as error:
        # Metadata captures (vswhere/rustc) are never dumped: they contain paths.
        # No result.command, environment values, traceback or exception repr.
        print_build_tail(build_raw)
        code = str(error) if isinstance(error, QualificationError) else "qualification_internal_failure"
        print("VS17 functional qualification failed: " + code, file=sys.stderr)
        if logs_created:
            print("Owned temporary logs retained locally; no upload or cleanup claimed.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
