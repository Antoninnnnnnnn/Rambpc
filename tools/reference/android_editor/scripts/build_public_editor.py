"""Opt-in, local-only builder for the isolated public voice editor fixture.

Importing this module is pure: no SDK discovery, subprocesses, output or key access.
The original editor builder, manifest, sources and build/ are never modified.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "build-public-editor"
MANIFEST = ROOT / "public-editor-source/AndroidManifest.xml"
PACKAGE = "org.rambpc.publicvoiceprobe"
ACTIVITY = "org.rambpc.referenceprobe.PublicVoiceReferenceActivity"
JAVA_SOURCES = (
    ROOT / "public-editor-source/java/org/rambpc/referenceprobe/PublicVoiceReferenceActivity.java",
    ROOT / "source/java/org/rambpc/referenceprobe/CaseCatalog.java",
)
RESOURCE = ROOT / "source/res/layout/main.xml"
SOURCE_INPUTS = (MANIFEST, *JAVA_SOURCES, RESOURCE)
NS = "{http://schemas.android.com/apk/res/android}"

# Exact allowlist, including attributes: unknown additions fail closed, even under -O.
MANIFEST_ATTRIBUTES = {
    "package": PACKAGE, NS + "versionCode": "1", NS + "versionName": "1.0",
}
SDK_ATTRIBUTES = {NS + "minSdkVersion": "26", NS + "targetSdkVersion": "34"}
APP_ATTRIBUTES = {
    NS + "label": "Public Voice Reference", NS + "allowBackup": "false",
    NS + "debuggable": "true", NS + "usesCleartextTraffic": "false",
}
ACTIVITY_ATTRIBUTES = {
    NS + "name": ACTIVITY, NS + "exported": "true",
    NS + "excludeFromRecents": "true", NS + "launchMode": "singleTop",
}
# aapt2 may add these informational attributes, never components/permissions.
COMPILED_ROOT_ATTRIBUTES = {
    "platformBuildVersionCode", "platformBuildVersionName",
    NS + "compileSdkVersion", NS + "compileSdkVersionCodename",
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check_manifest(manifest):
    """Validate the complete source manifest, not just selected permission names."""
    require(manifest.tag == "manifest", "Expected manifest root")
    require(manifest.attrib == MANIFEST_ATTRIBUTES, "Unexpected manifest attributes/package")
    require([child.tag for child in manifest] == ["uses-sdk", "application"],
            "Only uses-sdk and application are allowed; no permissions")
    sdk, app = manifest
    require(sdk.attrib == SDK_ATTRIBUTES and not list(sdk), "Unexpected SDK declaration")
    require(app.attrib == APP_ATTRIBUTES, "Unexpected application attributes")
    require([child.tag for child in app] == ["activity"], "Exactly one activity is allowed")
    activity = app[0]
    require(activity.attrib == ACTIVITY_ATTRIBUTES and not list(activity),
            "Unexpected activity, attributes or children (including intent filters)")
    require(all(not (node.text or "").strip() and not (node.tail or "").strip()
                for node in manifest.iter()), "Unexpected manifest text")


def check_permissions_dump(text):
    # An empty/malformed dump is not evidence of zero permissions. Unknown output
    # (including uses-permission-sdk-23 and future variants) must be reviewed.
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    require(lines == [f"package: {PACKAGE}"], "APK permission dump is not the zero-permission allowlist")


def decode_attribute(value, expected):
    """Decode only aapt2 xmltree representations needed by this fixed manifest."""
    quoted = re.fullmatch(r'"([^"\\]*)"(?: \(Raw: "\1"\))?', value)
    if quoted:
        return quoted.group(1)
    if expected in ("true", "false") and value in ("true", "false"):
        return value
    if expected == "singleTop" and value == "1":
        return expected
    if expected and expected.isdecimal() and re.fullmatch(r"[0-9]+", value):
        return str(int(value))
    if expected == "singleTop" and re.fullmatch(r"0x[0-9a-fA-F]+", value):
        require(int(value, 16) == 1, "Unexpected launch mode")
        return expected
    typed = re.fullmatch(r"\(type 0x([0-9a-fA-F]+)\)0x([0-9a-fA-F]+)", value)
    if typed:
        kind, number = (int(part, 16) for part in typed.groups())
        if expected in ("true", "false") and kind == 0x12:
            require(number in (0, 0xffffffff), "Unexpected boolean encoding")
            return "true" if number else "false"
        if expected == "singleTop" and kind in (0x10, 0x11):
            require(number == 1, "Unexpected launch mode")
            return expected
        if expected and expected.isdecimal() and kind in (0x10, 0x11):
            return str(number)
    raise ValueError(f"Unsupported compiled attribute encoding: {value}")


def check_apk_manifest(text):
    """Fail closed on xmltree elements, nesting, attributes and fixed values."""
    root = None
    stack = []
    for line in text.splitlines():
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip())
        item = line.strip()
        if re.fullmatch(r"N: android=http://schemas\.android\.com/apk/res/android(?: \(line=\d+\))?", item):
            require(root is None, "Unexpected namespace placement")
            continue
        element = re.fullmatch(r"E: ([\w-]+) \(line=\d+\)", item)
        if element:
            while stack and stack[-1][0] >= indent:
                stack.pop()
            node = ET.Element(element.group(1))
            if stack:
                stack[-1][1].append(node)
            else:
                require(root is None, "Multiple manifest roots")
                root = node
            stack.append((indent, node))
            continue
        attribute = re.fullmatch(
            r"A: ((?:http://schemas\.android\.com/apk/res/android:)?[\w:.-]+)"
            r"(?:\(0x[0-9a-fA-F]+\))?=(.*)", item)
        require(attribute is not None and bool(stack) and indent > stack[-1][0],
                f"Unsupported APK manifest dump line: {item}")
        name, value = attribute.groups()
        uri_prefix = "http://schemas.android.com/apk/res/android:"
        if name.startswith(uri_prefix):
            name = NS + name[len(uri_prefix):]
        elif name.startswith("android:"):
            name = NS + name[8:]
        node = stack[-1][1]
        require(name not in node.attrib, "Duplicate APK attribute")
        if node is root and name in COMPILED_ROOT_ATTRIBUTES:
            # Compiler metadata is not copied into the security policy tree.
            continue
        expected = {
            "manifest": MANIFEST_ATTRIBUTES, "uses-sdk": SDK_ATTRIBUTES,
            "application": APP_ATTRIBUTES, "activity": ACTIVITY_ATTRIBUTES,
        }.get(node.tag, {}).get(name)
        require(expected is not None, f"Unexpected APK attribute: {name}")
        node.set(name, decode_attribute(value, expected))
    require(root is not None, "Empty APK manifest dump")
    check_manifest(root)


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_hashes():
    # Never scan source/, res/, scripts/, or the original build/ for inputs.
    return {path.relative_to(ROOT).as_posix(): sha256(path) for path in SOURCE_INPUTS}


def run(args, capture=False):
    result = subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True,
                            text=True, capture_output=capture)
    return result.stdout if capture else None


def verify_apk(aapt, apk):
    permissions = run([aapt, "dump", "permissions", apk], capture=True)
    check_permissions_dump(permissions)
    tree = run([aapt, "dump", "xmltree", apk, "--file", "AndroidManifest.xml"], capture=True)
    check_apk_manifest(tree)
    return tree + "\n" + permissions


def main():
    sdk_path = (os.environ.get("REFERENCE_ANDROID_SDK") or os.environ.get("ANDROID_SDK_ROOT")
                or os.environ.get("ANDROID_HOME"))
    require(bool(sdk_path), "Set REFERENCE_ANDROID_SDK or ANDROID_SDK_ROOT to an existing SDK")
    sdk = Path(sdk_path)
    tools = sdk / "build-tools" / os.environ.get("REFERENCE_BUILD_TOOLS", "36.0.0")
    android_jar = sdk / "platforms" / os.environ.get("REFERENCE_PLATFORM", "android-37.0") / "android.jar"
    suffix = ".exe" if os.name == "nt" else ""
    aapt, zipalign = tools / ("aapt2" + suffix), tools / ("zipalign" + suffix)
    signer_jar, d8_jar = tools / "lib/apksigner.jar", tools / "lib/d8.jar"
    java, javac = shutil.which("java"), shutil.which("javac")
    require(bool(java and javac), "Existing JDK 17+ required")
    for path in (android_jar, aapt, zipalign, signer_jar, d8_jar, *SOURCE_INPUTS):
        require(path.is_file(), f"Missing prerequisite: {path}")
    key = ROOT / "build/reference-test-only.p12"
    require(key.is_file(), "Existing build/reference-test-only.p12 required; will not create a key")
    check_manifest(ET.parse(MANIFEST).getroot())
    hashes = source_hashes()
    output_names = ("generated", "classes", "dex", "empty-sourcepath", "classes.jar",
                    "resources.zip", "resources.apk", "unsigned.apk", "aligned.apk",
                    "public-voice-reference.apk", "signing-verification.txt",
                    "alignment-verification.txt", "manifest-verification.txt", "build-metadata.json")
    for path in (BUILD, *(BUILD / name for name in output_names)):
        require(path.resolve() == path, "Public editor outputs must not redirect")
    BUILD.mkdir(exist_ok=True)
    for name in ("generated", "classes", "dex", "empty-sourcepath"):
        directory = BUILD / name
        if directory.exists():
            shutil.rmtree(directory)
        directory.mkdir()
    run([aapt, "compile", "-o", BUILD / "resources.zip", RESOURCE])
    run([aapt, "link", "-o", BUILD / "resources.apk", "--manifest", MANIFEST,
         "-I", android_jar, "--java", BUILD / "generated", BUILD / "resources.zip"])
    generated = BUILD / "generated/org/rambpc/publicvoiceprobe/R.java"
    require(generated.is_file(), "Expected generated package R.java")
    run([javac, "-source", "8", "-target", "8", "-encoding", "UTF-8",
         "-classpath", android_jar, "-sourcepath", BUILD / "empty-sourcepath", "-implicit:none",
         "-d", BUILD / "classes", *JAVA_SOURCES, generated])
    classes = sorted((BUILD / "classes").rglob("*.class"))
    require(bool(classes), "No compiled classes")
    with zipfile.ZipFile(BUILD / "classes.jar", "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in classes:
            archive.write(path, path.relative_to(BUILD / "classes").as_posix())
    run([java, "-cp", d8_jar, "com.android.tools.r8.D8", "--min-api", "26",
         "--lib", android_jar, "--output", BUILD / "dex", BUILD / "classes.jar"])
    unsigned = BUILD / "unsigned.apk"
    shutil.copyfile(BUILD / "resources.apk", unsigned)
    with zipfile.ZipFile(unsigned, "a", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.write(BUILD / "dex/classes.dex", "classes.dex")
    verify_apk(aapt, unsigned)
    require(hashes == source_hashes(), "Source inputs changed during build")
    run([zipalign, "-f", "4", unsigned, BUILD / "aligned.apk"])
    apk = BUILD / "public-voice-reference.apk"
    signer = [java, "-jar", signer_jar]
    run([*signer, "sign", "--ks", key, "--ks-key-alias", "reference-probe",
         "--ks-pass", "pass:reference-test-only", "--key-pass", "pass:reference-test-only",
         "--v4-signing-enabled", "false", "--out", apk, BUILD / "aligned.apk"])
    verification = run([*signer, "verify", "--verbose", apk], capture=True)
    alignment = run([zipalign, "-c", "-v", "4", apk], capture=True)
    manifest_dump = verify_apk(aapt, apk)
    require(hashes == source_hashes(), "Source inputs changed during build")
    metadata = {"package": PACKAGE, "activity": ACTIVITY, "apk_sha256": sha256(apk),
                "apk_bytes": apk.stat().st_size, "source_files_sha256": hashes,
                "manifest_permission_count": 0, "manifest_activity_count": 1,
                "device_execution": False, "downloads": False, "host_tests_run_by_builder": False}
    for name, text in (("signing-verification.txt", verification),
                       ("alignment-verification.txt", alignment),
                       ("manifest-verification.txt", manifest_dump),
                       ("build-metadata.json", json.dumps(metadata, indent=2) + "\n")):
        (BUILD / name).write_text(text, encoding="utf-8")
    print(json.dumps(metadata, indent=2))


if __name__ == "__main__":
    main()
