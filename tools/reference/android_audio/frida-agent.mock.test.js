'use strict';
const assert = require('node:assert/strict');
const test = require('node:test');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const Core = require('./pcm-core.js');
const APK_SHA = 'a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8';
const VERSION = '18.6.3.1002251336-release-arm64-v8a';
const hash = bytes => crypto.createHash('sha256').update(Buffer.from(bytes)).digest('hex');
const fixture = size => Uint8Array.from({length:size},(_,i)=>(i*73+11)&255);
const descriptor = bytes => ({byteCount:bytes.length,sha256:hash(bytes)});
const payload = bytes => ({...descriptor(bytes),pcmBase64:Buffer.from(bytes).toString('base64')});

// Handwritten Java model only. No APK/Dex/extracted source is imported or executed.
// APK hash return is mocked, deliberately NOT a runtime identity validation claim.
function model(options = {}) {
  const events=[], uses=[];
  let clock=1000, digests=0, originalCalls=0, onSleep=null;
  function overload(args, fn, result='void') {
    return {argumentTypes:args.map(className=>({className})),returnType:{className:result},implementation:null,
      call(receiver,...values) { return fn.apply(receiver,values); }};
  }
  function family(methods) {
    return {overloads:methods,overload(...args) {
      const match=methods.find(m=>m.argumentTypes.map(t=>t.className).join(',')===args.join(','));
      if(!match) throw Error('bad mock signature');
      return match;
    }};
  }
  const noarg=overload([],()=>{originalCalls++;throw Error('ORIGINAL CONSTRUCTOR FORBIDDEN');});
  const bytearg=overload(['[B'],()=>{originalCalls++;throw Error('ORIGINAL CONSTRUCTOR FORBIDDEN');});
  const read=overload(['[B','int','int'],()=>{originalCalls++;throw Error('ORIGINAL READ FORBIDDEN');},'int');
  const superInit=overload([],function(){this.superCalls++;if(options.superFails)throw Error('mock ART failure');});
  const Stream={_a:{},_b:{},$init:family([noarg,bytearg]),read:family([read]),class:{
    getName:()=> 'aaqe', getSuperclass:()=>({getName:()=> 'java.io.InputStream'}),
    getDeclaredField:name=>({getType:()=>({getName:()=>name==='a'?'boolean':'android.media.AudioRecord'})})}};
  function digest() {
    const apk=digests++===0, chunks=[];
    return {
      update:family([overload(['[B','int','int'],function(b,off,len){
        if (!apk) chunks.push(Buffer.from(Array.from(b).slice(off,off+len).map(x=>x&255)));
      })]),
      digest() {
        const result=apk?Buffer.from(options.badApkHash?'0'.repeat(64):APK_SHA,'hex'):
          options.badDeliveredHash?Buffer.alloc(32):crypto.createHash('sha256').update(Buffer.concat(chunks)).digest();
        return Int8Array.from(result);
      }
    };
  }
  const app={getPackageName:()=>options.badPackage?'unapproved.app':'com.akshaykadam.pixelboard',
    getPackageManager:()=>({getPackageInfo:()=>({versionName:{value:options.badVersion?'1.3.4':VERSION}})}),
    getApplicationInfo:()=>({sourceDir:{value:'/mock/approved.apk'},splitSourceDirs:{value:options.split?['mock']:null}})};
  const classes={
    'android.app.ActivityThread':{currentApplication:()=>app},
    'java.io.File':{$new:()=>({length:()=>127650866})},
    'java.io.FileInputStream':{$new:()=>{
      let remaining=127650866;
      return {read:family([overload(['[B'],()=>{
        if(remaining===0)return -1;
        const n=Math.min(65536,remaining);remaining-=n;return n;
      },'int')]),close(){}};
    }},
    'java.security.MessageDigest':{getInstance:digest},
    'java.io.InputStream':{$init:family([superInit])},
    'aaqe':Stream,
    'java.io.IOException':{$new:message=>Error(message)},
    'java.lang.NullPointerException':{$new:message=>new TypeError(message)},
    'java.lang.IndexOutOfBoundsException':{$new:message=>new RangeError(message)},
    'android.os.SystemClock':{elapsedRealtime:()=>clock},
    'java.lang.Thread':{sleep:family([overload(['long'],ms=>{clock+=ms;if(onSleep){const f=onSleep;onSleep=null;f();}})])},
    'java.lang.System':{arraycopy(src,off,dst,to,n){
      if(options.copyFails)throw Error('mock JNI copy failure');
      dst.set(src.subarray(off,off+n),to);
    }}
  };
  const Java={available:true,perform:fn=>fn(),use(name){uses.push(name);if(!classes[name])throw Error('unapproved class '+name);return classes[name];},
    array:(type,values)=>Int8Array.from(values),retain:object=>object,
    vm:{getEnv:()=>({isSameObject:(a,b)=>a===b})}};
  const context={ReferencePcm:Core,Java,rpc:{exports:{}},send:e=>events.push(e)};
  vm.createContext(context);
  new vm.Script(fs.readFileSync(path.join(__dirname,'frida-agent.js'),'utf8')).runInContext(context);
  const rpc=context.rpc.exports;
  return {rpc,events,uses,originalCalls:()=>originalCalls,
    prepare:bytes=>rpc.prepare({ambientMicBlocked:true,constructorBypassApproved:true,approvedPcm:[descriptor(bytes)]}),
    object:()=>({$h:Symbol(),_a:{value:true},_b:{value:null},superCalls:0,$dispose(){}}),
    construct:(object,arrayOverload=false)=>(arrayOverload?bytearg:noarg).implementation.call(object),
    read:(object,dst,off,len)=>read.implementation.call(object,dst,off,len),
    onSleep:fn=>{onSleep=fn;}, clock:()=>clock,
    installed:()=>[read,noarg,bytearg].filter(x=>typeof x.implementation==='function').length};
}

test('mock Java: exactly three hooks, unarmed denial, superclass-only construction and delivered hash/EOF',async()=>{
  const bytes=fixture(2048),m=model();
  const ready=await m.prepare(bytes);
  assert.equal(ready.phase,'ready');assert.equal(m.installed(),3);
  assert.throws(()=>m.construct(m.object()),/unarmed/);
  m.rpc.arm(payload(bytes));
  const object=m.object();m.construct(object);
  assert.equal(object.superCalls,1);assert.equal(object._a.value,false);assert.equal(object._b.value,null);
  const output=[];
  for(;;){const dst=new Int8Array(800).fill(1),n=m.read(object,dst,3,700);if(n===-1)break;output.push(...Array.from(dst.subarray(3,3+n),x=>x&255));}
  assert.deepEqual(Uint8Array.from(output),bytes);
  assert(m.clock()>=1000+bytes.length/32);
  assert.equal(m.originalCalls(),0);
  assert.equal(m.uses.includes('android.media.AudioRecord'),false);
  const eof=m.events.find(e=>e.type==='eof');
  assert.equal(eof.deliveredSha256,hash(bytes));assert.equal(eof.deliveredBytes,bytes.length);
  assert.equal(m.rpc.status().stream.recordings[0].deliveredSha256,hash(bytes));
  assert.equal(m.events.filter(e=>e.type==='constructor-bypass').length,1);
  assert.equal(m.read(object,new Int8Array(1),0,1),-1);
  assert.equal(m.events.filter(e=>e.type==='eof').length,1);
});

test('mock Java: byte-array constructor is independent and both constructors obey three-arm cap',async()=>{
  const bytes=fixture(8),m=model();await m.prepare(bytes);
  for(let i=0;i<3;i++){
    m.rpc.arm(payload(bytes));const object=m.object();m.construct(object,i%2===1);
    const dst=new Int8Array(8);assert.equal(m.read(object,dst,0,8),8);
    assert.equal(m.read(object,dst,0,1),-1);
  }
  assert.throws(()=>m.rpc.arm(payload(bytes)),/cap/);
  assert.throws(()=>m.construct(m.object()),/unarmed/);
  assert.equal(m.rpc.status().bypassedConstructors,3);assert.equal(m.originalCalls(),0);
});

test('mock Java: null/bounds, unknown objects, cancellation, in-flight stop and concurrent reads fail closed',async()=>{
  const bytes=fixture(2048),m=model();await m.prepare(bytes);m.rpc.arm(payload(bytes));const object=m.object();m.construct(object);
  assert.throws(()=>m.read(object,null,0,1),/destination/);
  assert.throws(()=>m.read(object,new Int8Array(8),-1,1),/bounds/);
  assert.throws(()=>m.read(m.object(),new Int8Array(8),0,1),/Unknown/);
  m.onSleep(()=>{assert.throws(()=>m.read(object,new Int8Array(8),0,8),/concurrent/);});
  assert.equal(m.read(object,new Int8Array(8),0,8),8);
  m.onSleep(()=>m.rpc.stop());
  const untouched=new Int8Array(640).fill(17);
  assert.equal(m.read(object,untouched,0,640),-1);assert(untouched.every(x=>x===17));
  assert.equal(m.rpc.status().stream.recordings[0].deliveredBytes,8);
  assert.throws(()=>m.construct(m.object()),/unarmed/);
  assert.equal(m.originalCalls(),0);
  const c=model();await c.prepare(bytes);c.rpc.arm(payload(bytes));const cancelled=c.object();c.construct(cancelled);
  cancelled._a.value=true;assert.equal(c.read(cancelled,new Int8Array(8),0,8),-1);
});

test('mock Java: identity mismatch refuses all hooks and superclass failure never calls original/fallback',async()=>{
  const bytes=fixture(8);
  for(const options of [{badPackage:true},{badVersion:true},{badApkHash:true},{split:true}]){
    const m=model(options);await assert.rejects(m.prepare(bytes));assert.equal(m.installed(),0);
    assert.equal(m.rpc.status().phase,'failed');assert.equal(m.originalCalls(),0);
  }
  const m=model({superFails:true});await m.prepare(bytes);m.rpc.arm(payload(bytes));
  assert.throws(()=>m.construct(m.object()),/bypass failed/);
  assert.equal(m.rpc.status().phase,'failed');assert.equal(m.originalCalls(),0);
  assert.throws(()=>m.construct(m.object()),/unarmed/);
});

test('mock Java: copy/JNI and delivered-hash failures permanently stop the test without replay',async()=>{
  const bytes=fixture(8);
  for(const options of [{copyFails:true},{badDeliveredHash:true}]){
    const m=model(options);await m.prepare(bytes);m.rpc.arm(payload(bytes));const object=m.object();m.construct(object);
    if(options.copyFails){
      assert.throws(()=>m.read(object,new Int8Array(8),0,8),/read failed/);
      assert.equal(m.rpc.status().stream.recordings[0].deliveredBytes,0);
    }else{
      assert.equal(m.read(object,new Int8Array(8),0,8),8);
      assert.throws(()=>m.read(object,new Int8Array(1),0,1),/read failed/);
      assert.equal(m.events.some(e=>e.type==='eof'),false);
    }
    assert.equal(m.rpc.status().phase,'failed');
    assert.throws(()=>m.rpc.arm(payload(bytes)),/not ready/);
    assert.throws(()=>m.construct(m.object()),/unarmed/);
    assert.equal(m.originalCalls(),0);
  }
});
