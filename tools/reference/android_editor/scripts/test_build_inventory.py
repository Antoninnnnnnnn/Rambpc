"""Pure allowlist tests: no SDK, generated outputs, subprocesses or key reads."""
import contextlib
import copy
import importlib.util
import io
from pathlib import Path
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

SCRIPT = Path(__file__).with_name("build_inventory.py")
spec = importlib.util.spec_from_file_location("inventory_builder", SCRIPT)
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


def compiled_dump(manifest):
    """Representative aapt2 xmltree format; not an installed-tool qualification."""
    lines = ["N: android=http://schemas.android.com/apk/res/android"]

    def emit(node, indent):
        lines.append(" " * indent + f"E: {node.tag} (line=1)")
        for name, value in node.attrib.items():
            android = name.startswith(builder.NS)
            label = "android:" + name[len(builder.NS):] if android else name
            label += "(0x01010000)" if android else ""
            if value in ("true", "false"):
                value = "(type 0x12)0x" + ("ffffffff" if value == "true" else "0")
            elif name == builder.NS + "windowSoftInputMode" and value == "stateAlwaysHidden":
                value = "(type 0x11)0x3"
            elif name == builder.NS + "theme":
                value = "@0x01030055" if value == "@android:style/Theme.NoDisplay" else "@0x01030224"
            elif value.isdecimal() and name != builder.NS + "versionName":
                value = f"(type 0x10)0x{int(value):x}"
            else:
                value = f'"{value}" (Raw: "{value}")'
            lines.append(" " * (indent + 2) + f"A: {label}={value}")
        for child in node:
            emit(child, indent + 2)
    emit(manifest, 2)
    return "\n".join(lines) + "\n"


# Original public unsigned APK xmltree output, retained verbatim as a regression fixture.
ACTUAL_DUMP = '''N: android=http://schemas.android.com/apk/res/android (line=2)
  E: manifest (line=2)
    A: http://schemas.android.com/apk/res/android:versionCode(0x0101021b)=1
    A: http://schemas.android.com/apk/res/android:versionName(0x0101021c)="1.0" (Raw: "1.0")
    A: http://schemas.android.com/apk/res/android:compileSdkVersion(0x01010572)=37
    A: http://schemas.android.com/apk/res/android:compileSdkVersionCodename(0x01010573)="17" (Raw: "17")
    A: package="org.rambpc.audioinventoryprobe" (Raw: "org.rambpc.audioinventoryprobe")
    A: platformBuildVersionCode=37
    A: platformBuildVersionName=17
      E: uses-sdk (line=4)
        A: http://schemas.android.com/apk/res/android:minSdkVersion(0x0101020c)=26
        A: http://schemas.android.com/apk/res/android:targetSdkVersion(0x01010270)=34
      E: application (line=6)
        A: http://schemas.android.com/apk/res/android:label(0x01010001)="Audio Inventory Probe" (Raw: "Audio Inventory Probe")
        A: http://schemas.android.com/apk/res/android:debuggable(0x0101000f)=true
        A: http://schemas.android.com/apk/res/android:allowBackup(0x01010280)=false
        A: http://schemas.android.com/apk/res/android:usesCleartextTraffic(0x010104ec)=false
          E: activity (line=8)
            A: http://schemas.android.com/apk/res/android:theme(0x01010000)=@0x01030055
            A: http://schemas.android.com/apk/res/android:name(0x01010003)="org.rambpc.referenceprobe.AudioInputInventoryActivity" (Raw: "org.rambpc.referenceprobe.AudioInputInventoryActivity")
            A: http://schemas.android.com/apk/res/android:exported(0x01010010)=true
            A: http://schemas.android.com/apk/res/android:excludeFromRecents(0x01010017)=true
            A: http://schemas.android.com/apk/res/android:windowSoftInputMode(0x0101022b)=0x00000003
            A: http://schemas.android.com/apk/res/android:noHistory(0x0101022d)=true
'''


class InventoryManifestTests(unittest.TestCase):
    def test_actual_dump_and_mutations(self):
        builder.check_apk_manifest(ACTUAL_DUMP)
        mutations = (
            ("N: android=http://schemas.android.com/apk/res/android", "N: android=https://example.invalid/android"),
            ("http://schemas.android.com/apk/res/android:exported", "http://example.invalid/android:exported"),
            ("http://schemas.android.com/apk/res/android:exported", "evil:exported"),
            ("exported(0x01010010)=true", "exported(0x01010010)=false"),
            ("allowBackup(0x01010280)=false", "allowBackup(0x01010280)=true"),
            ("debuggable(0x0101000f)=true", "debuggable(0x0101000f)=1"),
            ("minSdkVersion(0x0101020c)=26", "minSdkVersion(0x0101020c)=27"),
            ("windowSoftInputMode(0x0101022b)=0x00000003", "windowSoftInputMode(0x0101022b)=0x00000004"),
            ("theme(0x01010000)=@0x01030055", "theme(0x01010000)=@0x01030001"),
            ("      E: application (line=6)", "      E: uses-permission-sdk-23 (line=6)\n      E: application (line=6)"),
            ("          E: activity (line=8)", "          E: service (line=8)\n          E: activity (line=8)"),
        )
        for before, after in mutations:
            with self.subTest(mutation=before), self.assertRaises(ValueError):
                self.assertIn(before, ACTUAL_DUMP)
                builder.check_apk_manifest(ACTUAL_DUMP.replace(before, after, 1))


    @classmethod
    def setUpClass(cls):
        cls.original = ET.parse(builder.MANIFEST).getroot()

    def setUp(self):
        self.manifest = copy.deepcopy(self.original)

    def assert_rejected(self):
        with self.assertRaises(ValueError):
            builder.check_manifest(self.manifest)
        with self.assertRaises(ValueError):
            builder.check_apk_manifest(compiled_dump(self.manifest))

    def test_reviewed_manifest(self):
        builder.check_manifest(self.manifest)
        builder.check_apk_manifest(compiled_dump(self.manifest))
        builder.check_apk_manifest(compiled_dump(self.manifest).replace(
            "N: android=http://schemas.android.com/apk/res/android",
            "N: android=http://schemas.android.com/apk/res/android (line=2)"))
        with self.assertRaises(ValueError):
            builder.check_apk_manifest(compiled_dump(self.manifest).replace(
                "N: android=http://schemas.android.com/apk/res/android",
                "N: android=https://example.invalid/android (line=2)"))

    def test_all_permission_variants_rejected(self):
        for tag in ("uses-permission", "uses-permission-sdk-23", "uses-permission-sdk-m",
                    "permission", "permission-tree", "permission-group", "uses-permission-future"):
            with self.subTest(tag=tag):
                self.manifest = copy.deepcopy(self.original)
                ET.SubElement(self.manifest, tag, {builder.NS + "name": "android.permission.RECORD_AUDIO"})
                self.assert_rejected()

    def test_additional_components_and_intent_filters_rejected(self):
        for tag in ("service", "receiver", "provider", "activity-alias", "activity", "meta-data",
                    "uses-library", "intent-filter"):
            with self.subTest(tag=tag):
                self.manifest = copy.deepcopy(self.original)
                ET.SubElement(self.manifest.find("application"), tag)
                self.assert_rejected()
        self.manifest = copy.deepcopy(self.original)
        ET.SubElement(self.manifest.find("application/activity"), "intent-filter")
        self.assert_rejected()

    def test_queries_instrumentation_and_extra_sdk_rejected(self):
        for tag in ("queries", "instrumentation", "uses-sdk", "uses-feature"):
            with self.subTest(tag=tag):
                self.manifest = copy.deepcopy(self.original)
                ET.SubElement(self.manifest, tag)
                self.assert_rejected()

    def test_fixed_package_and_full_qualified_activity(self):
        self.manifest.set("package", "org.rambpc.referenceprobe")
        self.assert_rejected()
        for name in (".AudioInputInventoryActivity", ".ReferenceActivity",
                     "org.rambpc.audioinventoryprobe.AudioInputInventoryActivity"):
            self.manifest = copy.deepcopy(self.original)
            self.manifest.find("application/activity").set(builder.NS + "name", name)
            self.assert_rejected()

    def test_safety_attributes_are_required_and_fixed(self):
        cases = (
            ("application", "allowBackup", "true"), ("application", "debuggable", "false"),
            ("application", "usesCleartextTraffic", "true"),
            ("application/activity", "exported", "false"),
            ("application/activity", "noHistory", "false"),
            ("application/activity", "excludeFromRecents", "false"),
            ("application/activity", "theme", "@android:style/Theme.Material"),
            ("application/activity", "windowSoftInputMode", "stateVisible"),
        )
        for location, attribute, value in cases:
            with self.subTest(attribute=attribute):
                self.manifest = copy.deepcopy(self.original)
                self.manifest.find(location).set(builder.NS + attribute, value)
                self.assert_rejected()
                self.manifest.find(location).attrib.pop(builder.NS + attribute)
                self.assert_rejected()

    def test_unknown_attributes_and_nested_permissions_rejected(self):
        for node in (self.manifest, self.manifest.find("application"),
                     self.manifest.find("application/activity")):
            node.set(builder.NS + "permission", "android.permission.INTERNET")
            self.assert_rejected()
            node.attrib.pop(builder.NS + "permission")
        ET.SubElement(self.manifest.find("application/activity"), "uses-permission")
        self.assert_rejected()

    def test_empty_and_malformed_compiled_dumps_rejected(self):
        for text in ("", "not xmltree", "E: service (line=1)",
                     compiled_dump(self.manifest) + "E: manifest (line=1)\n"):
            with self.subTest(text=text[:30]), self.assertRaises(ValueError):
                builder.check_apk_manifest(text)

    def test_compiler_metadata_allowed_only_at_root(self):
        text = compiled_dump(self.manifest)
        builder.check_apk_manifest(text.replace(
            "    A: package=", "    A: android:compileSdkVersion(0x01010572)=(type 0x10)0x25\n    A: package="))
        with self.assertRaises(ValueError):
            builder.check_apk_manifest(text.replace(
                "E: application (line=1)",
                "E: application (line=1)\n        A: platformBuildVersionCode=(type 0x10)0x25"))

    def test_compiled_mode_and_theme_resource_rejected(self):
        text = compiled_dump(self.manifest)
        for changed in (text.replace("(type 0x11)0x3", "(type 0x11)0x4"),
                        text.replace("@0x01030055", "@0x01030001"),
                        text.replace("@0x01030055", "@0x7f010001")):
            with self.assertRaises(ValueError):
                builder.check_apk_manifest(changed)

    def test_permission_dump_requires_exact_zero_permission_output(self):
        builder.check_permissions_dump(f"package: {builder.PACKAGE}\n")
        for text in ("", "package: org.rambpc.referenceprobe\n",
                     f"package: {builder.PACKAGE}\nuses-permission: name='android.permission.RECORD_AUDIO'\n",
                     f"package: {builder.PACKAGE}\nuses-permission-sdk-23: name='android.permission.INTERNET'\n",
                     f"package: {builder.PACKAGE}\nunknown: value\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                builder.check_permissions_dump(text)


class BuildIsolationTests(unittest.TestCase):
    def test_exact_source_allowlist(self):
        self.assertEqual([path.name for path in builder.JAVA_SOURCES],
                         ["AudioInputInventory.java", "AudioInputInventoryActivity.java"])
        self.assertEqual(builder.SOURCE_INPUTS, (builder.MANIFEST, *builder.JAVA_SOURCES))
        self.assertEqual(builder.BUILD, builder.ROOT / "build-inventory")
        self.assertEqual(builder.MANIFEST, builder.ROOT / "inventory-source/AndroidManifest.xml")

    def test_link_plan_has_no_resources_or_r_generation(self):
        command = builder.resource_free_link_args("aapt2", "android.jar")
        self.assertEqual(command, ["aapt2", "link", "-o", builder.BUILD / "manifest.apk",
                                   "--manifest", builder.MANIFEST, "-I", "android.jar"])

    def test_unconfirmed_link_exits_before_any_io_or_subprocess(self):
        with patch.object(builder, "run") as run, patch.object(Path, "is_file") as stat, \
                patch.object(Path, "mkdir") as mkdir, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                builder.main([])
            run.assert_not_called()
            stat.assert_not_called()
            mkdir.assert_not_called()

    def test_missing_key_fails_without_reading_or_creating_anything(self):
        key = builder.ROOT / "build/reference-test-only.p12"
        # All filesystem operations here are mocked; never stat/read the real key.
        with patch.dict(builder.os.environ, {"REFERENCE_ANDROID_SDK": "fake-sdk"}, clear=True), \
                patch.object(builder.shutil, "which", return_value="fake-java"), \
                patch.object(Path, "is_file", autospec=True, side_effect=lambda path: path != key), \
                patch.object(Path, "open") as open_file, patch.object(Path, "mkdir") as mkdir, \
                patch.object(builder, "run") as run:
            with self.assertRaisesRegex(ValueError, "will not create a key"):
                builder.main(["--resource-free-link-verified"])
            open_file.assert_not_called()
            mkdir.assert_not_called()
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
