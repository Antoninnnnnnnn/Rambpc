'use strict';
// Original, separate UI adapter. No method replacement, attach, shell, or backend seam.
(function (root) {
  const PIN = Object.freeze({
    packageName: 'com.akshaykadam.pixelboard',
    releaseTag: '1.3.4',
    versionName: '18.6.3.1002251336-release-arm64-v8a',
    apkSha256: 'a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8',
    apkBytes: 127650866, className: 'aaqe'
  });
  const IME = 'com.android.inputmethod.latin.LatinIME';
  const EDITOR = 'org.rambpc.referenceprobe';
  // Coordinator's pinned-APK DEX confirmation, not JADX's misleading consumer.e():
  // Lkla;.g(Ljava/lang/String;Ljava/util/function/Consumer;)V
  // Consumer->accept(Ljava/lang/Object;)V
  const CONSUMER = 'java.util.function.Consumer';
  const CONSUMER_METHOD = 'accept';
  const CONSUMER_ARGUMENT = 'java.lang.Object';
  const MAX_STARTS = 3, MAX_SHOWS = 3, MAX_INSTANCES = 8, DISPATCH_MS = 5000;
  function check(ok, reason) { if (!ok) throw new Error(reason); }
  function scopeOk(s) {
    check(s && s.uniqueConnected === 1 && s.className === IME && s.packageName === PIN.packageName &&
      s.editorPackage === EDITOR && s.editorAction === 4 && s.connected === true,
    'UI target must be the unique connected PixelBoard LatinIME on the approved probe/action4');
  }
  class UiGate {
    constructor() {
      this.starts = 0; this.shows = 0; this.active = null; this.failed = false;
      this.busy = false; this.identityValidated = false; this.usedArms = new Set();
    }
    enter() { check(!this.busy, 'UI operation already pending'); this.busy = true; }
    leave() { this.busy = false; }
    identity(audio) {
      check(audio && audio.pin && Object.keys(PIN).every(k => audio.pin[k] === PIN[k]), 'wrong audio application pin');
      if (audio.phase === 'ready') this.identityValidated = true;
      check(this.identityValidated, 'audio identity has not been verified ready');
    }
    available(audio, scope) {
      check(!this.failed, 'UI adapter failed permanently');
      this.identity(audio); scopeOk(scope);
    }
    start(audio, scope, ramblerEnabled) {
      this.available(audio, scope);
      check(audio.phase === 'ready' && audio.stream && audio.stream.stopped === false && audio.stream.pending,
        'start requires ready audio and a pending arm');
      check(ramblerEnabled === true, 'Rambler preference must already be enabled');
      check(this.active === null, 'previous UI dispatch has not ended');
      const arm = audio.stream.armedCount, pending = audio.stream.pending;
      check(Number.isInteger(arm) && arm > 0 && arm <= MAX_STARTS && !this.usedArms.has(arm), 'arm already dispatched or invalid');
      check(pending && /^[0-9a-f]{64}$/.test(pending.sha256) && Number.isInteger(pending.byteCount) &&
        pending.byteCount > 0 && pending.byteCount <= 640000 && pending.byteCount % 2 === 0, 'invalid pending PCM descriptor');
      check(this.starts < MAX_STARTS, 'three-start lifetime cap reached');
      this.starts++; this.usedArms.add(arm);
      this.active = { arm, sha256: pending.sha256, byteCount: pending.byteCount, dispatched: false };
      return { ...this.active, startCount: this.starts };
    }
    pendingStillMatches(audio) {
      check(this.active && audio.phase === 'ready' && audio.stream && !audio.stream.stopped && audio.stream.pending &&
        audio.stream.armedCount === this.active.arm && audio.stream.pending.sha256 === this.active.sha256 &&
        audio.stream.pending.byteCount === this.active.byteCount, 'pending audio arm changed before UI dispatch');
    }
    started() { check(this.active !== null, 'no pending UI start'); this.active.dispatched = true; }
    end(audio, scope) {
      this.available(audio, scope);
      check(this.active && this.active.dispatched, 'no dispatched UI start to end');
      return { ...this.active };
    }
    ended() { this.active = null; }
    show(audio, scope) {
      this.available(audio, scope);
      check(audio.phase === 'ready', 'show requires verified ready audio');
      check(this.shows < MAX_SHOWS, 'three-show lifetime cap reached');
      this.shows++;
    }
    fail() { this.failed = true; }
    status() {
      return { starts: this.starts, startCap: MAX_STARTS, shows: this.shows, showCap: MAX_SHOWS,
        failed: this.failed, busy: this.busy, identityValidated: this.identityValidated,
        active: this.active ? { ...this.active } : null };
    }
  }

  function install(bridge, rpc, sendFn, timers = { setTimeout, clearTimeout }) {
    // A compiled ESM entry may assign globalThis.Java only AFTER imported source
    // modules evaluate. Resolve the bridge lazily at RPC time, not import time.
    const getBridge = typeof bridge === 'function' ? bridge : () => bridge;
    let Java = null;
    check(rpc && rpc.exports && typeof rpc.exports.status === 'function', 'audio RPC must be loaded first');
    const exports = rpc.exports, audioStatus = exports.status;
    for (const name of ['uiStatus', 'uiShow', 'uiStart', 'uiEnd']) check(exports[name] === undefined, 'UI adapter already installed');
    const gate = new UiGate();
    let classes = null, consumerClass = null, activeBinding = null, nextToken = 1;
    const entries = new Map(); // At most one pending callback; start budget is three.
    function emit(type, detail = {}) { sendFn({ scope: 'reference-ui-dispatch', type, ...detail }); }
    function same(a, b) { return a !== null && b !== null && Java.vm.getEnv().isSameObject(a.$h, b.$h); }
    function dispose(x) { if (x) x.$dispose(); }
    function releaseBinding() {
      if (activeBinding) for (const x of Object.values(activeBinding)) dispose(x);
      activeBinding = null;
    }
    function inJava(fn) {
      return new Promise((resolve, reject) => {
        Java = getBridge();
        if (!Java || !Java.available) { reject(new Error('UI Java bridge unavailable')); return; }
        Java.perform(() => { try { resolve(fn()); } catch (_) { reject(new Error('UI Java metadata verification failed')); } });
      });
    }
    function onMain(fn) {
      return new Promise((resolve, reject) => {
        try { Java.scheduleOnMainThread(() => { try { resolve(fn()); } catch (_) { reject(new Error('UI main-thread precondition or dispatch failed')); } }); }
        catch (_) { reject(new Error('UI main-thread scheduling failed')); }
      });
    }
    function inspect() {
      check(CONSUMER !== null && CONSUMER_METHOD !== null && CONSUMER_ARGUMENT !== null,
        'Consumer DEX descriptor/SAM not yet confirmed; UI dispatch is disabled');
      if (classes) return classes;
      const LatinIME = Java.use(IME), Kla = Java.use('kla'), Oak = Java.use('oak'), Mut = Java.use('mut');
      const Modifier = Java.use('java.lang.reflect.Modifier');
      const g = Kla.g.overload('java.lang.String', CONSUMER), h = Kla.h.overload(CONSUMER);
      check(g.returnType.className === 'void' && h.returnType.className === 'void', 'wrong voice helper returns');
      const methodList = Kla.class.getDeclaredMethods();
      let gStatic = false, hInstance = false;
      for (let i = 0; i < methodList.length; i++) {
        const m = methodList[i], p = m.getParameterTypes();
        if (m.getName().toString() === 'g' && p.length === 2 && p[0].getName().toString() === 'java.lang.String' &&
            p[1].getName().toString() === CONSUMER) gStatic = Modifier.isStatic(m.getModifiers());
        if (m.getName().toString() === 'h' && p.length === 1 && p[0].getName().toString() === CONSUMER)
          hInstance = !Modifier.isStatic(m.getModifiers());
      }
      check(gStatic && hInstance, 'wrong voice helper modifiers');
      const Consumer = Java.use(CONSUMER);
      check(Consumer.class.isInterface(), 'Consumer must be an interface');
      const methods = Consumer.class.getMethods(), abstract = [];
      for (let i = 0; i < methods.length; i++) {
        if (Modifier.isAbstract(methods[i].getModifiers()) && !Modifier.isStatic(methods[i].getModifiers())) abstract.push(methods[i]);
      }
      check(abstract.length === 1, 'Consumer must have exactly one abstract method');
      const method = abstract[0], arguments_ = method.getParameterTypes();
      check(method.getName().toString() === CONSUMER_METHOD && method.getReturnType().getName().toString() === 'void' &&
        arguments_.length === 1 && arguments_[0].getName().toString() === CONSUMER_ARGUMENT, 'Consumer SAM does not match confirmed DEX pin');
      check(Oak.d.overload('pva').returnType.className === 'oak' && Kla._c !== undefined,
        'STOP event factory/constant shape changed');
      const stopField = Kla.class.getDeclaredField('c');
      check(stopField.getType().getName().toString() === 'pva' && Modifier.isStatic(stopField.getModifiers()) &&
        Modifier.isFinal(stopField.getModifiers()), 'wrong static final STOP constant');
      check(Mut.b.overload('android.content.Context').returnType.className === 'boolean', 'wrong Rambler preference getter');
      check(LatinIME.dG.overload('oak').returnType.className === 'void', 'wrong normal dispatch method');
      LatinIME.requestShowSelf.overload('int');
      classes = { LatinIME, Kla, Oak, Mut, Consumer, g };
      return classes;
    }
    function choose() {
      return new Promise((resolve, reject) => {
        const found = []; let overflow = false;
        try {
          Java.choose(IME, {
            onMatch(object) {
              if (found.length === MAX_INSTANCES) { overflow = true; return 'stop'; }
              found.push(Java.retain(object));
            },
            onComplete() {
              if (overflow) { found.forEach(dispose); reject(new Error('too many LatinIME instances')); }
              else resolve(found);
            }
          });
        } catch (_) { found.forEach(dispose); reject(new Error('specific LatinIME enumeration failed')); }
      });
    }
    function current(ime, count) {
      check(ime.getClass().getName().toString() === IME, 'wrong IME class');
      const connection = ime.getCurrentInputConnection(), editor = ime.getCurrentInputEditorInfo();
      const scope = { className: IME, packageName: ime.getPackageName().toString(), uniqueConnected: count,
        connected: connection !== null, editorPackage: editor === null || editor.packageName.value === null ? null : editor.packageName.value.toString(),
        editorAction: editor === null ? null : Number(editor.imeOptions.value) & 255 };
      scopeOk(scope);
      return { ime, connection, editor, scope };
    }
    function validateBinding(target, bound) {
      check(same(target.ime, bound.ime) && same(target.connection, bound.connection) && same(target.editor, bound.editor),
        'IME/editor/connection changed; refusing dispatch');
    }
    async function selected(fn) {
      const found = await inJava(() => { inspect(); return choose(); });
      try {
        return await onMain(() => {
          // Count all connected instances before filtering to the probe. Multiple
          // connected IMEs are ambiguous even if just one points at the probe.
          const connected = found.filter(ime => ime.getCurrentInputConnection() !== null);
          check(connected.length === 1, 'ambiguous or absent connected LatinIME');
          return fn(current(connected[0], connected.length));
        });
      } finally { found.forEach(dispose); }
    }
    function consumer() {
      if (consumerClass) return consumerClass;
      const implementation = {};
      implementation[CONSUMER_METHOD] = [{ returnType: 'void', argumentTypes: [CONSUMER_ARGUMENT],
        implementation(value) {
          let held = null, entry = null;
          try {
            entry = entries.get(Number(this.referenceToken.value));
            if (!entry || !entry.live) return; // Timed-out/stale callback is dropped, never dispatched.
            check(!entry.seen && value !== null, 'duplicate or null launch callback');
            entry.seen = true;
            check(classes.Oak.class.isInstance(value), 'launch payload must be the normal oak event');
            held = Java.retain(Java.cast(value, classes.Oak));
            // Re-enumerate before the deferred callback as well: an intervening
            // second connected LatinIME must not be hidden by a hard-coded count.
            selected(now => {
              if (!entry.live) return null;
              validateBinding(now, entry.target);
              gate.pendingStillMatches(audioStatus());
              check(classes.Mut.b.overload('android.content.Context').call(classes.Mut, now.ime) === true,
                'Rambler preference changed before launch');
              if (!entry.live) return null;
              now.ime.dG.overload('oak').call(now.ime, held);
              if (!entry.live) return null; // Timeout cannot undo a dispatch already entered.
              gate.started();
              activeBinding = { ime: Java.retain(now.ime), connection: Java.retain(now.connection), editor: Java.retain(now.editor) };
              return { startCount: gate.starts, dispatchOnly: true, arm: gate.active.arm };
            }).then(result => {
              if (entry.live && result !== null) entry.finish(null, result);
            }, () => entry.finish(new Error('normal launch UI dispatch failed')))
              .finally(() => dispose(held));
          } catch (_) {
            dispose(held);
            if (entry && entry.live) entry.finish(new Error('typed launch Consumer failed'));
          }
        }
      }];
      consumerClass = Java.registerClass({
        name: 'org.rambpc.referenceprobe.PixelBoardBoundedLaunchConsumer',
        implements: [classes.Consumer], fields: { referenceToken: 'int' }, methods: implementation
      });
      return consumerClass;
    }
    function launch(target) {
      return new Promise((resolve, reject) => {
        const token = nextToken++, entry = { target, live: true, seen: false, timer: null, consumer: null, finish: null };
        entry.finish = (error, result) => {
          if (!entry.live) return;
          entry.live = false; entries.delete(token); timers.clearTimeout(entry.timer);
          dispose(entry.consumer);
          if (error) { gate.fail(); releaseBinding(); emit('failed', { operation: 'start', startCount: gate.starts }); reject(error); }
          else { emit('start-dispatched', result); resolve(result); }
        };
        entries.set(token, entry);
        entry.timer = timers.setTimeout(() => entry.finish(new Error('launch callback timed out; no retry')), DISPATCH_MS);
        try {
          entry.consumer = consumer().$new();
          entry.consumer.referenceToken.value = token;
          classes.g.call(classes.Kla, 'jetson', entry.consumer);
        } catch (_) { entry.finish(new Error('normal kla.g launch failed; no retry')); }
      });
    }
    async function operation(name, fn) {
      gate.enter();
      try { gate.identity(audioStatus()); return await selected(fn); }
      catch (_) {
        // A denied precondition never switches targets or issues a fallback event.
        // All dispatch failures themselves have already permanently failed the gate.
        throw new Error('UI ' + name + ' refused or failed; do not retry or bypass gates');
      } finally { gate.leave(); }
    }
    exports.uiStatus = () => operation('status', target => ({
      ...gate.status(), targetVerified: true, editorAction: target.scope.editorAction,
      ramblerEnabled: classes.Mut.b.overload('android.content.Context').call(classes.Mut, target.ime),
      consumer: { interface: CONSUMER, method: CONSUMER_METHOD, argument: CONSUMER_ARGUMENT, returns: 'void' },
      dispatchCoverageOnly: true
    }));
    exports.uiShow = () => operation('show', target => {
      gate.show(audioStatus(), target.scope);
      try { target.ime.requestShowSelf.overload('int').call(target.ime, 0); }
      catch (_) { gate.fail(); emit('failed', { operation: 'show' }); throw new Error('show failed'); }
      const result = { showCount: gate.shows, showRequested: true, visibilityProven: false };
      emit('show-requested', result); return result;
    });
    exports.uiStart = () => operation('start', target => {
      gate.start(audioStatus(), target.scope, classes.Mut.b.overload('android.content.Context').call(classes.Mut, target.ime));
      return launch(target);
    });
    exports.uiEnd = () => operation('end', target => {
      gate.end(audioStatus(), target.scope);
      check(activeBinding !== null, 'start binding missing');
      validateBinding(target, activeBinding);
      try {
        // Exact event emitted by kla.h/f, using the existing static pva constant.
        // Parent-approved equivalent UI dispatch; kla.n's executor is NOT exercised.
        const event = classes.Oak.d.overload('pva').call(classes.Oak, classes.Kla._c.value);
        target.ime.dG.overload('oak').call(target.ime, event);
        const result = { startCount: gate.starts, dispatchOnly: true, endEquivalent: true, originalExecutorExercised: false };
        gate.ended(); releaseBinding(); emit('end-dispatched', result); return result;
      } catch (_) { gate.fail(); releaseBinding(); emit('failed', { operation: 'end' }); throw new Error('end failed'); }
    });
    emit('loaded', { methodReplacements: 0, startCap: MAX_STARTS, dispatchCoverageOnly: true });
    return { gate }; // Host tests only; no generic RPC or Java object exposure.
  }
  const api = { PIN, IME, EDITOR, CONSUMER, CONSUMER_METHOD, CONSUMER_ARGUMENT, UiGate, scopeOk, install };
  root.ReferenceUiDispatch = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  if (typeof rpc !== 'undefined') install(() => typeof Java === 'undefined' ? null : Java, rpc, send);
})(globalThis);
