"""Pure fixture tests; no SDK, APK, device, generated output, or key access."""
import copy
import importlib.util
from pathlib import Path
import unittest
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location("public_builder", Path(__file__).with_name("build_public_editor.py"))
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)

MANIFEST = '''<manifest xmlns:android="http://schemas.android.com/apk/res/android"
 package="org.rambpc.publicvoiceprobe" android:versionCode="1" android:versionName="1.0">
 <uses-sdk android:minSdkVersion="26" android:targetSdkVersion="34"/>
 <application android:label="Public Voice Reference" android:allowBackup="false"
 android:debuggable="true" android:usesCleartextTraffic="false">
 <activity android:name="org.rambpc.referenceprobe.PublicVoiceReferenceActivity"
 android:exported="true" android:excludeFromRecents="true" android:launchMode="singleTop"/>
 </application></manifest>'''
DUMP = '''N: android=http://schemas.android.com/apk/res/android (line=2)
  E: manifest (line=2)
    A: http://schemas.android.com/apk/res/android:versionCode(0x0101021b)=1
    A: http://schemas.android.com/apk/res/android:versionName(0x0101021c)="1.0" (Raw: "1.0")
    A: http://schemas.android.com/apk/res/android:compileSdkVersion(0x01010572)=37
    A: package="org.rambpc.publicvoiceprobe" (Raw: "org.rambpc.publicvoiceprobe")
    A: platformBuildVersionCode=37
      E: uses-sdk (line=4)
        A: http://schemas.android.com/apk/res/android:minSdkVersion(0x0101020c)=26
        A: http://schemas.android.com/apk/res/android:targetSdkVersion(0x01010270)=34
      E: application (line=6)
        A: http://schemas.android.com/apk/res/android:label(0x01010001)="Public Voice Reference" (Raw: "Public Voice Reference")
        A: http://schemas.android.com/apk/res/android:debuggable(0x0101000f)=true
        A: http://schemas.android.com/apk/res/android:allowBackup(0x01010280)=false
        A: http://schemas.android.com/apk/res/android:usesCleartextTraffic(0x010104ec)=false
          E: activity (line=8)
            A: http://schemas.android.com/apk/res/android:name(0x01010003)="org.rambpc.referenceprobe.PublicVoiceReferenceActivity" (Raw: "org.rambpc.referenceprobe.PublicVoiceReferenceActivity")
            A: http://schemas.android.com/apk/res/android:exported(0x01010010)=true
            A: http://schemas.android.com/apk/res/android:excludeFromRecents(0x01010017)=true
            A: http://schemas.android.com/apk/res/android:launchMode(0x0101001d)=0x00000001
'''

class Tests(unittest.TestCase):
    def test_good(self):
        b.check_manifest(ET.fromstring(MANIFEST))
        for value in ('0x00000001', '1', '(type 0x10)0x1', '(type 0x11)0x1'):
            b.check_apk_manifest(DUMP.replace('0x00000001', value))
        b.check_apk_manifest(DUMP.replace('=true', '=(type 0x12)0xffffffff').replace('=false', '=(type 0x12)0x0'))
        b.check_permissions_dump('package: org.rambpc.publicvoiceprobe\n')

    def test_source_mutations(self):
        for change in ('permission', 'activity', 'filter', 'theme', 'window', 'launch', 'package'):
            root = ET.fromstring(MANIFEST)
            app, activity = root[1], root[1][0]
            if change == 'permission': ET.SubElement(root, 'uses-permission')
            if change == 'activity': app.append(copy.deepcopy(activity))
            if change == 'filter': ET.SubElement(activity, 'intent-filter')
            if change == 'theme': app.set(b.NS + 'theme', '@android:style/Theme')
            if change == 'window': activity.set(b.NS + 'windowSoftInputMode', 'stateAlwaysHidden')
            if change == 'launch': activity.set(b.NS + 'launchMode', 'singleTask')
            if change == 'package': root.set('package', 'org.rambpc.referenceprobe')
            with self.subTest(change=change), self.assertRaises(ValueError): b.check_manifest(root)

    def test_dump_mutations(self):
        for before, after in [('=0x00000001', '=0x00000002'), ('=0x00000001', '=(type 0x10)0x2'),
                              ('=true', '=false'), ('=false', '=true'),
                              ('http://schemas.android.com/apk/res/android:exported', 'evil:exported'),
                              ('          E: activity', '          E: service'),
                              ('      E: application', '      E: uses-permission (line=5)\n      E: application')]:
            with self.subTest(before=before, after=after), self.assertRaises(ValueError):
                b.check_apk_manifest(DUMP.replace(before, after, 1))
        for dump in ('', 'package: org.rambpc.publicvoiceprobe\nuses-permission: name=android.permission.RECORD_AUDIO',
                     'package: org.rambpc.referenceprobe'):
            with self.assertRaises(ValueError): b.check_permissions_dump(dump)

    def test_fixed_source_list(self):
        self.assertEqual([p.relative_to(b.ROOT).as_posix() for p in b.JAVA_SOURCES], [
            'public-editor-source/java/org/rambpc/referenceprobe/PublicVoiceReferenceActivity.java',
            'source/java/org/rambpc/referenceprobe/CaseCatalog.java'])

if __name__ == '__main__': unittest.main()
