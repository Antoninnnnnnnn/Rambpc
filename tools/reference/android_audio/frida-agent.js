'use strict';
// Run only as the second half of the locally-built Frida CLI bundle.
// No generic AudioRecord hook, original aaqe constructor, network, service, or shell API.
(function () {
  const PIN = Object.freeze({
    packageName: 'com.akshaykadam.pixelboard',
    releaseTag: '1.3.4',
    versionName: '18.6.3.1002251336-release-arm64-v8a',
    apkSha256: 'a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8',
    apkBytes: 127650866,
    className: 'aaqe'
  });
  const Core = globalThis.ReferencePcm;
  let phase = 'loaded', attempted = false, engine = null, refs = null;
  let deniedConstructors = 0, bypassedConstructors = 0;
  const recordings = []; // Retained object + private Java PCM, maximum three.
  function event(type, details) { send({ scope: 'reference-audio-injection', type, ...details }); }
  function check(ok, message) { if (!ok) throw new Error(message); }
  function metadata() {
    const stream = engine ? engine.status() : null;
    if (stream) for (const s of stream.recordings) {
      const recording = recordings.find(r => r.id === s.id);
      s.deliveredSha256 = recording ? recording.deliveredSha256 : null;
    }
    return { phase, pin: PIN, deniedConstructors, bypassedConstructors, stream };
  }
  function hexDigest(bytes) {
    let result = '';
    for (let i = 0; i < bytes.length; i++) result += (bytes[i] & 255).toString(16).padStart(2, '0');
    return result;
  }
  function insideJava(fn) {
    return new Promise((resolve, reject) => {
      if (typeof Java === 'undefined' || !Java.available) {
        reject(new Error('Java bridge unavailable; use the Frida 17.17 CLI Java bridge, not raw create_script'));
        return;
      }
      Java.perform(() => {
        try { resolve(fn()); } catch (_) { reject(new Error('Java operation failed; keep ambient microphone blocked')); }
      });
    });
  }
  function javaError(kind, message) { return refs[kind].$new(message); }
  function denyConstructor() {
    deniedConstructors++;
    event('constructor-denied', { originalConstructorCalled: false, deniedConstructors });
    throw javaError('IOException', 'Reference capture is unarmed or stopped');
  }
  function findRecording(object) {
    const env = Java.vm.getEnv();
    for (const recording of recordings) {
      if (env.isSameObject(object.$h, recording.object.$h)) return recording;
    }
    return null;
  }
  function verifyShape(Stream, InputStream) {
    check(Stream.class.getName().toString() === PIN.className, 'wrong class');
    check(Stream.class.getSuperclass().getName().toString() === 'java.io.InputStream', 'wrong superclass');
    const constructors = Stream.$init.overloads.map(o => o.argumentTypes.map(t => t.className).join(',')).sort();
    check(JSON.stringify(constructors) === JSON.stringify(['', '[B']), 'constructor shape changed');
    const a = Stream.class.getDeclaredField('a'), b = Stream.class.getDeclaredField('b');
    check(a.getType().getName().toString() === 'boolean', 'wrong stop field');
    check(b.getType().getName().toString() === 'android.media.AudioRecord', 'wrong audio field');
    check(Stream._a !== undefined && Stream._b !== undefined, 'field wrappers unavailable');
    check(InputStream.$init.overload().argumentTypes.length === 0, 'wrong superclass constructor');
    check(Stream.read.overload('[B', 'int', 'int').returnType.className === 'int', 'wrong read signature');
  }
  function verifyApk() {
    const ActivityThread = Java.use('android.app.ActivityThread');
    const app = ActivityThread.currentApplication();
    check(app !== null && app.getPackageName().toString() === PIN.packageName, 'wrong process');
    const info = app.getPackageManager().getPackageInfo(PIN.packageName, 0);
    check(info.versionName.value.toString() === PIN.versionName, 'wrong manifest version');
    const appInfo = app.getApplicationInfo();
    const splits = appInfo.splitSourceDirs.value;
    check(splits === null || splits.length === 0, 'split APKs not approved');
    const File = Java.use('java.io.File');
    const file = File.$new(appInfo.sourceDir.value.toString());
    check(Number(file.length()) === PIN.apkBytes, 'wrong APK size');
    const digest = Java.use('java.security.MessageDigest').getInstance('SHA-256');
    const input = Java.use('java.io.FileInputStream').$new(file);
    const buffer = Java.array('byte', new Array(65536).fill(0));
    let hashedBytes = 0;
    try {
      for (;;) {
        const n = input.read.overload('[B').call(input, buffer);
        if (n === -1) break;
        check(n > 0, 'APK read stalled');
        hashedBytes += n;
        check(hashedBytes <= PIN.apkBytes, 'APK grew while hashing');
        digest.update.overload('[B', 'int', 'int').call(digest, buffer, 0, n);
      }
      check(hashedBytes === PIN.apkBytes, 'APK changed while hashing');
    } finally { input.close(); }
    const raw = digest.digest();
    let sha256 = '';
    for (let i = 0; i < raw.length; i++) sha256 += (raw[i] & 255).toString(16).padStart(2, '0');
    check(sha256 === PIN.apkSha256, 'wrong APK SHA256');
    return { apkSha256: sha256, apkBytes: PIN.apkBytes };
  }
  function construct() {
    // Do not delegate either aaqe overload: both original bodies reach AudioRecord.
    if (phase !== 'ready' || engine === null) return denyConstructor();
    let id;
    try { id = engine.claim(); } catch (_) { return denyConstructor(); }
    let retained = null;
    try {
      // Direct superclass initialization ONLY. On this ART/device this is an explicit
      // unverified constructor-bypass assumption, NOT a call to aaqe's original $init.
      refs.superInit.call(this);
      this._a.value = false;
      this._b.value = null;
      check(this._b.value === null, 'audio field must remain null');
      retained = Java.retain(this);
      const session = engine.get(id);
      const signed = Array.from(session.bytes, b => b > 127 ? b - 256 : b);
      const javaPcm = Java.array('byte', signed);
      recordings.push({ id, object: retained, javaPcm, eofSent: false, deliveredSha256: null,
        deliveryDigest: refs.MessageDigest.getInstance('SHA-256') });
      bypassedConstructors++;
      event('constructor-bypass', { id, originalConstructorCalled: false,
        superclassConstructorCalled: true, audioRecordFieldNull: true,
        sha256: session.sha256, byteCount: session.byteCount });
    } catch (_) {
      engine.cancel(id);
      phase = 'failed'; // Never invoke a real constructor as recovery.
      engine.stop();
      if (retained !== null) retained.$dispose();
      event('fatal', { stage: 'constructor-bypass', originalConstructorCalled: false });
      throw javaError('IOException', 'Reference constructor bypass failed; stop this test');
    }
  }
  function read(destination, offset, length) {
    if (destination === null) throw javaError('NullPointerException', 'destination');
    const destinationLength = destination.length;
    if (!Number.isInteger(offset) || !Number.isInteger(length) || offset < 0 || length < 0 ||
        offset > destinationLength || length > destinationLength - offset) {
      throw javaError('IndexOutOfBoundsException', 'Reference read bounds');
    }
    // Unknown/pre-existing objects never delegate into AudioRecord.
    const recording = findRecording(this);
    if (recording === null) throw javaError('IOException', 'Unknown reference stream denied');
    if (length === 0) return 0;
    if (phase !== 'ready' || engine.stopped) return -1;
    if (this._a.value) engine.cancel(recording.id); // Original aaqe.b() cancellation remains effective.
    let lease = null;
    try {
      lease = engine.reserveRead(recording.id, destinationLength, offset, length,
        Number(refs.Clock.elapsedRealtime()));
      if (lease.n === -1) {
        if (engine.get(recording.id).eofObserved && !recording.eofSent) {
          recording.eofSent = true;
          const s = engine.get(recording.id);
          recording.deliveredSha256 = hexDigest(recording.deliveryDigest.digest());
          if (recording.deliveredSha256 !== s.sha256) {
            phase = 'failed'; engine.stop();
            event('fatal', { stage: 'delivered-hash', id: s.id });
            throw new Error('delivered hash mismatch');
          }
          event('eof', { id: s.id, sha256: s.sha256, deliveredSha256: recording.deliveredSha256,
            byteCount: s.byteCount, deliveredBytes: s.position });
        }
        return -1;
      }
      if (lease.n === 0) return 0;
      while (engine.live(lease)) {
        if (this._a.value) { engine.cancel(recording.id); break; }
        const remaining = Math.ceil(lease.deadlineMs - Number(refs.Clock.elapsedRealtime()));
        if (remaining <= 0) break;
        // Releases the JS runtime lock; a concurrent read sees the busy lease and is
        // denied instead of waiting on a Java mutex while holding the JS lock.
        refs.Thread.sleep.overload('long').call(refs.Thread, Math.min(20, remaining));
      }
      if (!engine.live(lease)) return engine.finishRead(lease);
      if (this._a.value) { engine.cancel(recording.id); return engine.finishRead(lease); }
      refs.System.arraycopy(recording.javaPcm, lease.sourceOffset, destination, offset, lease.n);
      if (!engine.live(lease)) return engine.finishRead(lease);
      recording.deliveryDigest.update.overload('[B', 'int', 'int').call(
        recording.deliveryDigest, destination, offset, lease.n);
      const n = engine.finishRead(lease);
      return n;
    } catch (_) {
      const contention = lease === null && engine.get(recording.id).busy;
      if (lease !== null && lease.id !== undefined && engine.get(lease.id).lease === lease) engine.abortRead(lease);
      // Only a contention denial leaves the already-reserved other read intact.
      // Unexpected JNI/hash/copy/clock errors terminate this agent's test, not replay.
      if (!contention) {
        phase = 'failed'; engine.stop();
        event('fatal', { stage: 'read', id: recording.id });
      }
      throw javaError('IOException', 'Reference read failed or concurrent read denied');
    }
  }
  rpc.exports = {
    status() { return metadata(); },
    prepare(options) {
      check(!attempted && phase === 'loaded', 'prepare is one-shot; no retry or fallback');
      attempted = true; phase = 'preparing';
      if (!(options && options.constructorBypassApproved === true && options.ambientMicBlocked === true)) {
        phase = 'failed';
        throw new Error('explicit constructor caveat and external microphone containment acknowledgments required');
      }
      return insideJava(() => {
        try {
          check(phase === 'preparing', 'stopped while preparing');
          check(Core !== undefined, 'core missing');
          const verified = verifyApk();
          check(phase === 'preparing', 'stopped while verifying APK');
          const Stream = Java.use(PIN.className);
          const InputStream = Java.use('java.io.InputStream');
          verifyShape(Stream, InputStream);
          engine = new Core.PcmEngine(options.approvedPcm);
          refs = {
            IOException: Java.use('java.io.IOException'),
            NullPointerException: Java.use('java.lang.NullPointerException'),
            IndexOutOfBoundsException: Java.use('java.lang.IndexOutOfBoundsException'),
            Clock: Java.use('android.os.SystemClock'), Thread: Java.use('java.lang.Thread'),
            System: Java.use('java.lang.System'), MessageDigest: Java.use('java.security.MessageDigest'),
            superInit: InputStream.$init.overload()
          };
          // Three implementations only: read([B,int,int), $init(), $init([B).
          Stream.read.overload('[B', 'int', 'int').implementation = read;
          Stream.$init.overload().implementation = construct;
          Stream.$init.overload('[B').implementation = construct;
          check(phase === 'preparing', 'stopped while preparing');
          phase = 'ready';
          event('ready', { ...verified, packageName: PIN.packageName, versionName: PIN.versionName,
            releaseTag: PIN.releaseTag, className: PIN.className,
            scope: 'reference-audio-injection', hookCount: 3, cap: 3,
            constructorBypassArtValidated: false });
          return metadata();
        } catch (_) {
          phase = 'failed';
          if (engine) engine.stop();
          event('fatal', { stage: 'prepare', noCapturePermitted: true });
          throw new Error('prepare failed; stop the test; no fallback permitted');
        }
      }).catch(error => {
        phase = 'failed';
        if (engine) engine.stop();
        throw error;
      });
    },
    arm(input) {
      check(phase === 'ready' && engine !== null, 'not ready');
      const result = engine.arm(input);
      event('armed', result);
      return result;
    },
    stop() {
      phase = 'stopped';
      if (engine) engine.stop();
      // Do not unhook or detach: unarmed/extra constructors must still fail closed.
      event('stopped', { hookRemovalRequested: false });
      return metadata();
    }
  };
  event('loaded', { packageName: PIN.packageName, releaseTag: PIN.releaseTag,
    bridgeExpected: 'Frida CLI 17.17 Java global', capturePermitted: false });
})();
