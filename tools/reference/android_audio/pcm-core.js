'use strict';
// Original, dependency-free PCM delivery logic. Never opens a file, device, or socket.
(function (root) {
  const MAX_BYTES = 20 * 16000 * 2;
  const MAX_ARMS = 3;
  const BYTES_PER_MS = 32;
  const CHUNK_BYTES = 640; // At most 20 ms per positive read.

  function check(ok, message) { if (!ok) throw new Error(message); }
  function integer(x) { return Number.isSafeInteger(x); }
  function digest(x) { return typeof x === 'string' && /^[0-9a-f]{64}$/.test(x); }
  function descriptor(x) {
    check(x !== null && typeof x === 'object', 'descriptor required');
    check(integer(x.byteCount) && x.byteCount > 0 && x.byteCount <= MAX_BYTES && x.byteCount % 2 === 0,
      'PCM must contain 1..640000 even bytes');
    check(digest(x.sha256), 'lowercase SHA256 required');
    return { byteCount: x.byteCount, sha256: x.sha256 };
  }

  // SHA-256 over bytes; fixed original implementation, tested against node:crypto.
  function sha256(bytes) {
    check(bytes instanceof Uint8Array, 'Uint8Array required');
    const k = [
      0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
      0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
      0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
      0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
      0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
      0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
      0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
      0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
    const h = [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    const padded = new Uint8Array(Math.ceil((bytes.length + 9) / 64) * 64);
    padded.set(bytes); padded[bytes.length] = 0x80;
    const view = new DataView(padded.buffer);
    const bits = bytes.length * 8;
    view.setUint32(padded.length - 8, Math.floor(bits / 0x100000000));
    view.setUint32(padded.length - 4, bits >>> 0);
    const w = new Uint32Array(64);
    const r = (x, n) => (x >>> n) | (x << (32 - n));
    for (let off = 0; off < padded.length; off += 64) {
      for (let j = 0; j < 16; j++) w[j] = view.getUint32(off + j * 4);
      for (let j = 16; j < 64; j++) {
        const x = w[j - 15], y = w[j - 2];
        w[j] = (w[j - 16] + (r(x, 7) ^ r(x, 18) ^ (x >>> 3)) + w[j - 7] +
          (r(y, 17) ^ r(y, 19) ^ (y >>> 10))) >>> 0;
      }
      let [a,b,c,d,e,f,g,hh] = h;
      for (let j = 0; j < 64; j++) {
        const t1 = (hh + (r(e, 6) ^ r(e, 11) ^ r(e, 25)) + ((e & f) ^ (~e & g)) + k[j] + w[j]) >>> 0;
        const t2 = ((r(a, 2) ^ r(a, 13) ^ r(a, 22)) + ((a & b) ^ (a & c) ^ (b & c))) >>> 0;
        hh=g; g=f; f=e; e=(d+t1)>>>0; d=c; c=b; b=a; a=(t1+t2)>>>0;
      }
      [a,b,c,d,e,f,g,hh].forEach((v, j) => { h[j] = (h[j] + v) >>> 0; });
    }
    return h.map(x => x.toString(16).padStart(8, '0')).join('');
  }

  function decodeBase64(s, expectedSize) {
    check(integer(expectedSize) && expectedSize > 0 && expectedSize <= MAX_BYTES, 'invalid size');
    check(typeof s === 'string' && s.length === 4 * Math.ceil(expectedSize / 3), 'base64 size mismatch');
    check(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(s), 'canonical base64 required');
    const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
    const out = new Uint8Array(expectedSize);
    let p = 0;
    for (let i = 0; i < s.length; i += 4) {
      const a = alphabet.indexOf(s[i]), b = alphabet.indexOf(s[i+1]);
      const c = s[i+2] === '=' ? 0 : alphabet.indexOf(s[i+2]);
      const d = s[i+3] === '=' ? 0 : alphabet.indexOf(s[i+3]);
      if (s[i+2] === '=') check((b & 15) === 0, 'noncanonical base64 padding');
      else if (s[i+3] === '=') check((c & 3) === 0, 'noncanonical base64 padding');
      const chunk = (a << 18) | (b << 12) | (c << 6) | d;
      const count = s[i+2] === '=' ? 1 : s[i+3] === '=' ? 2 : 3;
      for (let j = 0; j < count; j++) {
        check(p < expectedSize, 'decoded size mismatch');
        out[p++] = (chunk >>> (16 - j * 8)) & 255;
      }
    }
    check(p === expectedSize, 'decoded size mismatch');
    return out;
  }

  class PcmEngine {
    constructor(allowlist) {
      check(Array.isArray(allowlist) && allowlist.length > 0 && allowlist.length <= MAX_ARMS,
        'one to three approved descriptors required');
      this.allowlist = allowlist.map(descriptor);
      this.arms = 0; this.pending = null; this.sessions = []; this.stopped = false;
    }
    arm(input) {
      check(!this.stopped, 'stopped permanently');
      check(this.arms < MAX_ARMS, 'three-arm lifetime cap reached');
      check(this.pending === null, 'one pending arm only');
      check(!this.sessions.some(s => !s.eofObserved && !s.cancelled), 'previous recording must reach EOF');
      const approved = descriptor(input);
      check(this.allowlist.some(x => x.sha256 === approved.sha256 && x.byteCount === approved.byteCount),
        'PCM not pre-approved');
      const bytes = decodeBase64(input.pcmBase64, approved.byteCount);
      check(sha256(bytes) === approved.sha256, 'PCM SHA256 mismatch');
      // A headerless stream is required, not a WAV/Ogg/FLAC container.
      const starts = text => [...text].every((x, i) => bytes[i] === x.charCodeAt(0));
      check(!starts('RIFF') && !starts('OggS') && !starts('fLaC') && !starts('ID3'), 'container header rejected');
      this.pending = { ...approved, bytes };
      this.arms++;
      return { armedCount: this.arms, byteCount: approved.byteCount, sha256: approved.sha256 };
    }
    claim() {
      check(!this.stopped && this.pending !== null, 'unarmed recording denied');
      const s = { ...this.pending, id: this.sessions.length + 1, position: 0,
        startMs: null, eofObserved: false, cancelled: false, busy: false, lease: null };
      this.pending = null; this.sessions.push(s);
      return s.id;
    }
    get(id) {
      const s = this.sessions.find(x => x.id === id);
      check(s !== undefined, 'unknown recording denied');
      return s;
    }
    reserveRead(id, destinationLength, offset, length, nowMs) {
      const s = this.get(id);
      check(integer(destinationLength) && destinationLength >= 0 && integer(offset) && integer(length) &&
        offset >= 0 && length >= 0 && offset <= destinationLength && length <= destinationLength - offset,
      'read bounds');
      if (length === 0) return { n: 0 };
      check(!s.busy, 'concurrent or reentrant read denied');
      if (this.stopped || s.cancelled) return { n: -1 };
      if (s.position === s.byteCount) { s.eofObserved = true; return { n: -1 }; }
      check(Number.isFinite(nowMs) && nowMs >= 0, 'monotonic clock required');
      if (s.startMs === null) s.startMs = nowMs;
      const n = Math.min(length, s.byteCount - s.position, CHUNK_BYTES);
      const lease = { id, n, offset, sourceOffset: s.position,
        deadlineMs: s.startMs + (s.position + n) / BYTES_PER_MS };
      s.busy = true; s.lease = lease;
      return lease;
    }
    live(lease) {
      const s = this.get(lease.id);
      return !this.stopped && !s.cancelled && s.lease === lease;
    }
    finishRead(lease) {
      const s = this.get(lease.id);
      check(s.lease === lease && s.busy, 'invalid read lease');
      s.busy = false; s.lease = null;
      if (this.stopped || s.cancelled) return -1;
      s.position += lease.n;
      return lease.n;
    }
    abortRead(lease) {
      const s = this.get(lease.id);
      check(s.lease === lease, 'invalid read lease');
      s.busy = false; s.lease = null; s.cancelled = true;
    }
    cancel(id) { this.get(id).cancelled = true; }
    stop() {
      this.stopped = true;
      if (this.pending) this.pending.bytes.fill(0);
      this.pending = null;
      for (const s of this.sessions) s.cancelled = true;
      return this.status();
    }
    status() {
      return { cap: MAX_ARMS, armedCount: this.arms, stopped: this.stopped,
        pending: this.pending ? { sha256: this.pending.sha256, byteCount: this.pending.byteCount } : null,
        recordings: this.sessions.map(s => ({ id: s.id, sha256: s.sha256, byteCount: s.byteCount,
          deliveredBytes: s.position, eofObserved: s.eofObserved, cancelled: s.cancelled, inFlight: s.busy })) };
    }
  }
  const api = { MAX_BYTES, MAX_ARMS, BYTES_PER_MS, CHUNK_BYTES, sha256, decodeBase64, PcmEngine };
  root.ReferencePcm = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(globalThis);
