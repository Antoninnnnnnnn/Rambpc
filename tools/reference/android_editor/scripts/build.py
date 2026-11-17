import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "source"
BUILD = ROOT / "build"
SDK_PATH = os.environ.get("REFERENCE_ANDROID_SDK") or os.environ.get("ANDROID_SDK_ROOT") or os.environ.get("ANDROID_HOME")
if not SDK_PATH:
    raise SystemExit("Set REFERENCE_ANDROID_SDK or ANDROID_SDK_ROOT to an existing SDK")
SDK = Path(SDK_PATH)
SUFFIX = ".exe" if os.name == "nt" else ""
TOOLS = SDK / "build-tools" / os.environ.get("REFERENCE_BUILD_TOOLS", "36.0.0")
ANDROID_JAR = SDK / "platforms" / os.environ.get("REFERENCE_PLATFORM", "android-37.0") / "android.jar"
JAVA = shutil.which("java")
JAVAC = shutil.which("javac")
KEYTOOL = shutil.which("keytool")
APK = BUILD / "reference-probe.apk"
NS = "{http://schemas.android.com/apk/res/android}"
JAVA_SOURCE_NAMES = (
    "AudioInputInventory.java", "AudioInputInventoryActivity.java",
    "CaseCatalog.java", "ReferenceActivity.java",
)
JAVA_SOURCES = [SOURCE / "java/org/rambpc/referenceprobe" / name for name in JAVA_SOURCE_NAMES]


def run(args, capture=False):
    # Fixed own-source working directory, never an SDK/download/extracted-code directory.
    result = subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True,
                            text=True, capture_output=capture)
    return result.stdout if capture else None


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def check_manifest():
    manifest = ET.parse(SOURCE / "AndroidManifest.xml").getroot()
    assert manifest.get("package") == "org.rambpc.referenceprobe"
    assert not any(element.tag.startswith("uses-permission") for element in manifest.iter())
    assert [element.tag for element in manifest] == ["uses-sdk", "application"]
    app = manifest.find("application")
    assert app.get(NS + "allowBackup") == "false"
    assert app.get(NS + "debuggable") == "true"
    assert app.get(NS + "usesCleartextTraffic") == "false"
    assert len(list(app)) == 2 and all(element.tag == "activity" for element in app)
    activities = {element.get(NS + "name"): element for element in app}
    assert set(activities) == {".ReferenceActivity", ".AudioInputInventoryActivity"}
    assert not app.findall(".//intent-filter")
    assert all(element.get(NS + "exported") == "true" for element in app)
    assert all(not list(element) for element in app)
    inventory = activities[".AudioInputInventoryActivity"]
    assert inventory.get(NS + "theme") == "@android:style/Theme.NoDisplay"
    assert inventory.get(NS + "windowSoftInputMode") == "stateAlwaysHidden"
    assert inventory.get(NS + "noHistory") == "true"
    assert inventory.get(NS + "excludeFromRecents") == "true"
    assert set((SOURCE / "java").rglob("*.java")) == set(JAVA_SOURCES), "Unexpected Java source set"


def main():
    if sys.flags.optimize:
        raise SystemExit("Run without -O: build safety assertions must be enabled")
    prerequisites = [ANDROID_JAR, TOOLS / ("aapt2" + SUFFIX), TOOLS / ("zipalign" + SUFFIX),
                     TOOLS / "lib" / "apksigner.jar", TOOLS / "lib" / "d8.jar"]
    assert JAVA and JAVAC and KEYTOOL, "Java 17 JDK is required"
    for path in prerequisites:
        assert path.is_file(), f"Missing build prerequisite: {path}"
    check_manifest()
    BUILD.mkdir(exist_ok=True)
    for directory in ["generated", "classes", "dex", "test-classes"]:
        generated = BUILD / directory
        if generated.exists():
            shutil.rmtree(generated)
        generated.mkdir()
    run([JAVAC, "-encoding", "UTF-8", "-d", BUILD / "test-classes",
         SOURCE / "java/org/rambpc/referenceprobe/CaseCatalog.java",
         SOURCE / "java/org/rambpc/referenceprobe/AudioInputInventory.java",
         ROOT / "scripts/CaseCatalogTest.java", ROOT / "scripts/AudioInputInventoryTest.java"])
    run([JAVA, "-cp", BUILD / "test-classes", "CaseCatalogTest"])
    run([JAVA, "-cp", BUILD / "test-classes", "AudioInputInventoryTest"])
    aapt = TOOLS / ("aapt2" + SUFFIX)
    run([aapt, "compile", "--dir", SOURCE / "res", "-o", BUILD / "resources.zip"])
    run([aapt, "link", "-o", BUILD / "resources.apk", "--manifest", SOURCE / "AndroidManifest.xml",
         "-I", ANDROID_JAR, "--java", BUILD / "generated", BUILD / "resources.zip"])
    sources = JAVA_SOURCES + [BUILD / "generated/org/rambpc/referenceprobe/R.java"]
    run([JAVAC, "-source", "8", "-target", "8", "-encoding", "UTF-8", "-classpath", ANDROID_JAR,
         "-d", BUILD / "classes", *sources])
    classes_jar = BUILD / "classes.jar"
    with zipfile.ZipFile(classes_jar, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted((BUILD / "classes").rglob("*.class")):
            archive.write(path, path.relative_to(BUILD / "classes").as_posix())
    run([JAVA, "-cp", TOOLS / "lib/d8.jar", "com.android.tools.r8.D8", "--min-api", "26",
         "--lib", ANDROID_JAR, "--output", BUILD / "dex", classes_jar])
    unsigned = BUILD / "unsigned.apk"
    shutil.copyfile(BUILD / "resources.apk", unsigned)
    with zipfile.ZipFile(unsigned, "a", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.write(BUILD / "dex/classes.dex", "classes.dex")
    run([TOOLS / ("zipalign" + SUFFIX), "-f", "4", unsigned, BUILD / "aligned.apk"])
    key = BUILD / "reference-test-only.p12"
    if not key.exists():
        # New disposable local debug identity. Not an existing credential or production key.
        run([KEYTOOL, "-genkeypair", "-keystore", key, "-storetype", "PKCS12",
             "-storepass", "reference-test-only", "-keypass", "reference-test-only",
             "-alias", "reference-probe", "-keyalg", "RSA", "-keysize", "2048",
             "-validity", "3650", "-dname", "CN=Reference Probe Local Test"])
    signer = [JAVA, "-jar", TOOLS / "lib/apksigner.jar"]
    run([*signer, "sign", "--ks", key, "--ks-key-alias", "reference-probe",
         "--ks-pass", "pass:reference-test-only", "--key-pass", "pass:reference-test-only",
         "--v4-signing-enabled", "false", "--out", APK, BUILD / "aligned.apk"])
    verification = run([*signer, "verify", "--verbose", APK], capture=True)
    alignment = run([TOOLS / ("zipalign" + SUFFIX), "-c", "-v", "4", APK], capture=True)
    permissions = run([aapt, "dump", "permissions", APK], capture=True)
    assert "uses-permission:" not in permissions, permissions
    badging = run([aapt, "dump", "badging", APK], capture=True)
    manifest_dump = run([aapt, "dump", "xmltree", APK, "--file", "AndroidManifest.xml"], capture=True)
    (BUILD / "signing-verification.txt").write_text(verification, encoding="utf-8")
    (BUILD / "alignment-verification.txt").write_text(alignment, encoding="utf-8")
    (BUILD / "manifest-verification.txt").write_text(manifest_dump + "\n" + permissions + "\n" + badging, encoding="utf-8")
    paths = sorted(SOURCE.rglob("*")) + sorted((ROOT / "scripts").rglob("*"))
    hashes = {str(path.relative_to(ROOT)).replace("\\", "/"): sha256(path)
              for path in paths if path.is_file()}
    aggregate = hashlib.sha256("".join(f"{name}\0{value}\n" for name, value in sorted(hashes.items())).encode()).hexdigest()
    metadata = {"package": "org.rambpc.referenceprobe", "apk_sha256": sha256(APK),
                "apk_bytes": APK.stat().st_size, "source_files_sha256": hashes,
                "source_tree_sha256": aggregate,
                "source_tree_hash_algorithm": "SHA256(sorted POSIX relative path + NUL + lowercase file SHA256 + LF)",
                "build_tool_files_sha256": {str(path): sha256(path) for path in prerequisites},
                "device_execution": False, "downloads": False,
                "manifest_permission_count": 0, "catalog_host_test": "passed",
                "audio_inventory_host_test": "passed"}
    (BUILD / "build-metadata.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in metadata.items() if key not in ["source_files_sha256", "build_tool_files_sha256"]}, indent=2))
    print(verification)


if __name__ == "__main__":
    main()
