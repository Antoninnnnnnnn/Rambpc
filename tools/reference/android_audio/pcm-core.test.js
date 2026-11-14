'use strict';
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const test = require('node:test');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const Core = require('./pcm-core.js');
const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const fixture = n => Uint8Array.from({ length: n }, (_, i) => (i * 73 + 11) & 255);
const describe = bytes => ({ sha256: hash(bytes), byteCount: bytes.length });
const payload = bytes => ({ ...describe(bytes), pcmBase64: Buffer.from(bytes).toString('base64') });
function create(bytes = fixture(2048)) { return new Core.PcmEngine([describe(bytes)]); }
function start(engine, bytes) { engine.arm(payload(bytes)); return engine.claim(); }
function copy(engine, id, destination, offset, length, now = 100) {
  const lease = engine.reserveRead(id, destination.length, offset, length, now);
  if (lease.n <= 0) return lease.n;
  destination.set(engine.get(id).bytes.subarray(lease.sourceOffset, lease.sourceOffset + lease.n), offset);
  return engine.finishRead(lease);
}

test('SHA256 standard vectors and deterministic lengths including maximum PCM', () => {
  assert.equal(Core.sha256(new Uint8Array()), 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855');
  assert.equal(Core.sha256(Uint8Array.from(Buffer.from('abc'))), 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
  for (const n of [1,2,3,55,56,63,64,65,127,128,2048,65536,640000]) {
    const bytes = fixture(n); assert.equal(Core.sha256(bytes), hash(bytes));
  }
});

test('canonical base64 and explicit bounded sizes', () => {
  for (const n of [1,2,3,4,63,64,640000]) {
    const bytes = fixture(n);
    assert.deepEqual(Core.decodeBase64(Buffer.from(bytes).toString('base64'), n), bytes);
  }
  for (const [s, n] of [['AA==',2], ['AAA=',1], ['AB==',1], ['AAB=',2], ['!A==',1],
    ['A===',1], ['AA\n=',1], ['AAAA',2], ['',0], ['AAAA',640001]]) {
    assert.throws(() => Core.decodeBase64(s,n));
  }
});

test('descriptor, preapproval, size, hash, and container rejection do not consume arm count', () => {
  const bytes = fixture(64), e = create(bytes), p = payload(bytes);
  for (const input of [{...p,byteCount:63}, {...p,byteCount:640002}, {...p,sha256:'A'.repeat(64)},
    {...p,sha256:'0'.repeat(64)}, {...p,pcmBase64:Buffer.alloc(64).toString('base64')}]) {
    assert.throws(() => e.arm(input)); assert.equal(e.arms,0);
  }
  const wav = Uint8Array.from(Buffer.from('RIFFabcdefgh'));
  const ew = create(wav); assert.throws(() => ew.arm(payload(wav)), /container/);
  assert.throws(() => new Core.PcmEngine([]));
  assert.throws(() => new Core.PcmEngine(new Array(4).fill(describe(bytes))));
});

test('exact signed-independent byte sequence, offsets, chunking, EOF and zero-length contract', () => {
  const bytes = fixture(4096), e = create(bytes), id = start(e, bytes), received = [];
  for (;;) {
    const destination = new Uint8Array(777).fill(0xf1);
    const n = copy(e,id,destination,7,700);
    assert.equal(destination.subarray(0,7).every(x => x === 0xf1),true);
    if (n === -1) break;
    assert(n > 0 && n <= 640);
    assert.equal(destination.subarray(7+n).every(x => x === 0xf1),true);
    received.push(...destination.subarray(7,7+n));
  }
  assert.deepEqual(Uint8Array.from(received),bytes);
  assert.equal(e.get(id).position,bytes.length);
  assert.equal(e.status().recordings[0].eofObserved,true);
  assert.equal(copy(e,id,new Uint8Array(0),0,0),0);
  assert.equal(copy(e,id,new Uint8Array(1),0,1),-1);
  assert.equal(copy(e,id,new Uint8Array(1),0,1),-1);
});

test('exhaustive small bounds and odd byte reads leave invalid calls and sentinels untouched', () => {
  for (let size = 0; size <= 5; size++) {
    for (let off = -1; off <= size+1; off++) for (let len = -1; len <= size+1; len++) {
      const bytes=fixture(16), e=create(bytes), id=start(e,bytes), dst=new Uint8Array(size).fill(0xcd);
      const valid=off>=0 && len>=0 && off<=size && len<=size-off;
      if (!valid) {
        assert.throws(() => copy(e,id,dst,off,len));
        assert.equal(e.get(id).position,0);
        assert.equal(dst.every(x=>x===0xcd),true);
      } else {
        const n=copy(e,id,dst,off,len);
        assert.equal(n,len);
        assert.deepEqual(dst.subarray(off,off+len),bytes.subarray(0,len));
        assert.equal(dst.subarray(0,off).every(x=>x===0xcd),true);
        assert.equal(dst.subarray(off+len).every(x=>x===0xcd),true);
      }
    }
  }
  const bytes=fixture(16), e=create(bytes), id=start(e,bytes);
  for (const args of [[4,0.5,1],[4,0,NaN],[4,0,Infinity],[4,0,Number.MAX_SAFE_INTEGER]]) {
    assert.throws(()=>e.reserveRead(id,...args,0));
  }
  assert.throws(()=>e.reserveRead(999,4,0,1,0),/unknown/);
});

test('hard three-arm lifetime cap, unarmed constructor and independent recording reset', () => {
  const a=fixture(6), b=fixture(8), e=new Core.PcmEngine([describe(a),describe(b)]);
  assert.throws(()=>e.claim(),/unarmed/);
  for (let i=0;i<3;i++) {
    const bytes=i%2?b:a;
    e.arm(payload(bytes));
    assert.throws(()=>e.arm(payload(bytes)),/pending|cap/);
    const id=e.claim();
    assert.equal(id,i+1);
    assert.throws(()=>e.claim(),/unarmed/);
    const dst=new Uint8Array(bytes.length);
    assert.equal(copy(e,id,dst,0,dst.length),bytes.length);
    assert.deepEqual(dst,bytes);
    assert.throws(()=>e.arm(payload(bytes)), /EOF|cap/);
    assert.equal(copy(e,id,dst,0,1),-1);
  }
  assert.equal(e.arms,3);
  assert.throws(()=>e.arm(payload(a)),/cap/);
  assert.throws(()=>e.claim(),/unarmed/);
});

test('realtime cumulative pacing uses bytes/32000 seconds with maximum 20ms chunks', () => {
  const bytes=fixture(2048), e=create(bytes), id=start(e,bytes);
  const first=e.reserveRead(id,2048,0,2048,1000);
  assert.equal(first.n,640); assert.equal(first.deadlineMs,1020);
  e.finishRead(first);
  const second=e.reserveRead(id,2048,0,2048,1001);
  assert.equal(second.sourceOffset,640); assert.equal(second.deadlineMs,1040);
  e.finishRead(second);
  const third=e.reserveRead(id,2048,0,3,1002);
  assert.equal(third.deadlineMs,1000+1283/32);
  e.finishRead(third);
});

test('interleaved reads are rejected, never duplicate bytes, and lease identities are checked', () => {
  const bytes=fixture(64), e=create(bytes), id=start(e,bytes);
  const lease=e.reserveRead(id,8,0,8,0);
  assert.throws(()=>e.reserveRead(id,8,0,8,0),/concurrent/);
  assert.equal(e.get(id).position,0);
  assert.throws(()=>e.finishRead({...lease}),/lease/);
  assert.equal(e.finishRead(lease),8);
  assert.throws(()=>e.finishRead(lease),/lease/);
  const next=e.reserveRead(id,8,0,8,0);
  assert.equal(next.sourceOffset,8);
  e.abortRead(next);
  assert.equal(e.get(id).position,8);
  assert.equal(copy(e,id,new Uint8Array(8),0,8),-1);
});

test('stop is irreversible, invalidates in-flight reads and pending arms, and never resets cap', () => {
  const bytes=fixture(64), e=create(bytes), id=start(e,bytes);
  const lease=e.reserveRead(id,8,0,8,0);
  e.stop();
  assert.equal(e.live(lease),false);
  assert.equal(e.finishRead(lease),-1);
  assert.equal(e.get(id).position,0);
  assert.equal(copy(e,id,new Uint8Array(8),0,8),-1);
  assert.throws(()=>e.arm(payload(bytes)),/stopped/);
  assert.throws(()=>e.claim(),/unarmed/);
  assert.equal(e.status().armedCount,1);
  const pending=create(bytes); pending.arm(payload(bytes)); pending.stop();
  assert.equal(pending.pending,null);
  assert.equal(pending.arms,1);
});

test('maximum 20 second stream is finite and byte-identical', () => {
  const bytes=fixture(Core.MAX_BYTES), e=create(bytes), id=start(e,bytes);
  const output=new Uint8Array(bytes.length);
  let p=0;
  while(p<bytes.length) {
    const n=copy(e,id,output,p,Math.min(2048,output.length-p)); assert(n>0); p+=n;
  }
  assert.equal(hash(output),hash(bytes));
  assert.equal(copy(e,id,new Uint8Array(1),0,1),-1);
});

test('status contains hashes/counts only, not PCM/base64/phrase content', () => {
  const bytes=fixture(8), e=create(bytes); e.arm(payload(bytes));
  const snapshot=JSON.stringify(e.status());
  assert(!snapshot.includes('pcmBase64')); assert(!snapshot.includes('bytes":'));
  assert(snapshot.includes(hash(bytes)));
});

test('agent loads without bridge/device; prepare fails closed without Java and cannot retry', async () => {
  const events=[];
  const context={ ReferencePcm:Core, rpc:{exports:{}}, send:e=>events.push(e), console };
  vm.createContext(context);
  new vm.Script(fs.readFileSync(path.join(__dirname,'frida-agent.js'),'utf8')).runInContext(context);
  const rpc=context.rpc.exports;
  assert.equal(rpc.status().phase,'loaded');
  assert.throws(()=>rpc.arm(payload(fixture(8))),/not ready/);
  await assert.rejects(rpc.prepare({constructorBypassApproved:true,ambientMicBlocked:true,
    approvedPcm:[describe(fixture(8))]}),/bridge unavailable/);
  assert.throws(()=>rpc.prepare({}),/one-shot/);
  assert.equal(rpc.stop().phase,'stopped');
  assert.equal(events.some(e=>e.type==='ready'),false);
});
