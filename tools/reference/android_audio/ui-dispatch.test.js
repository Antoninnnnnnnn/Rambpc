'use strict';
const assert = require('node:assert/strict');
const test = require('node:test');
const UI = require('./ui-dispatch.js');
const sha = '1'.repeat(64);
const audio = arm => ({phase:'ready',pin:{...UI.PIN},stream:{stopped:false,armedCount:arm,pending:{sha256:sha,byteCount:3200}}});
const scope = () => ({className:UI.IME,packageName:UI.PIN.packageName,uniqueConnected:1,
  editorPackage:UI.EDITOR,editorAction:4,connected:true});

function family(types, fn, result='void') {
  const method={argumentTypes:types.map(className=>({className})),returnType:{className:result},
    call(receiver,...values){return fn.apply(receiver,values);}};
  Object.defineProperty(method,'implementation',{set(){throw Error('APP METHOD REPLACEMENT FORBIDDEN');}});
  return {overload(...arguments_){assert.deepEqual(arguments_,types);return method;}};
}
function model(options={}) {
  const dispatch=[], events=[], chooses=[], uses=[], timers=new Map();
  let timerId=0, onMain=false, currentAudio=audio(1), pref=options.pref!==false, consumer=null;
  const constant={$h:Symbol('STOP-pva'),$dispose(){}};
  const event=id=>({id,$h:Symbol(id),oak:true,$dispose(){}});
  const startEvent=event('LAUNCH'), stopEvent=event('STOP');
  function ime(configuration={}) {
    const editor={$h:Symbol('editor'),packageName:{value:configuration.editor||UI.EDITOR},
      imeOptions:{value:configuration.action===undefined?4:configuration.action},$dispose(){}};
    const connection={$h:Symbol('connection'),$dispose(){}};
    const object={$h:Symbol('ime'),$dispose(){},editor,connection,
      getClass:()=>({getName:()=>UI.IME}),getPackageName:()=>configuration.package||UI.PIN.packageName,
      getCurrentInputConnection:()=>configuration.disconnected?null:object.connection,
      getCurrentInputEditorInfo:()=>object.editor};
    object.dG=family(['oak'],value=>{assert.equal(onMain,true);dispatch.push(value.id);if(options.consumeArm&&value.id==='LAUNCH')currentAudio.stream.pending=null;});
    object.requestShowSelf=family(['int'],flags=>{assert.equal(onMain,true);assert.equal(flags,0);dispatch.push('SHOW');});
    return object;
  }
  const targets=options.targets?options.targets.map(ime):[ime(options.target||{})];
  function parameter(name){return {getName:()=>name};}
  function method(name,parameters,modifiers,returns='void'){
    return {getName:()=>name,getParameterTypes:()=>parameters.map(parameter),getModifiers:()=>modifiers,
      getReturnType:()=>parameter(returns)};
  }
  const Consumer={class:{isInterface:()=>true,getMethods:()=>[
    method(options.consumerMethod||UI.CONSUMER_METHOD,[UI.CONSUMER_ARGUMENT],0x400),
    method('andThen',[UI.CONSUMER],0,UI.CONSUMER)
  ]}};
  const Kla={
    g:family(['java.lang.String',UI.CONSUMER],(trigger,c)=>{
      assert.equal(onMain,true);assert.equal(trigger,'jetson');dispatch.push('HELPER_G');consumer=c;
      if(options.gThrows)throw Error('mock g failed');
      if(!options.noCallback)c[UI.CONSUMER_METHOD](startEvent);
      if(options.afterHelper)options.afterHelper();
    }),
    h:family([UI.CONSUMER],()=>{throw Error('NO EXISTING KLA INSTANCE MAY BE CALLED');}),
    _c:{value:constant},class:{getDeclaredMethods:()=>[
      method('g',['java.lang.String',UI.CONSUMER],8),method('h',[UI.CONSUMER],0)
    ],getDeclaredField:name=>{assert.equal(name,'c');return {getType:()=>parameter('pva'),getModifiers:()=>24};}}
  };
  const Oak={class:{isInstance:value=>value.oak===true},d:family(['pva'],c=>{
    assert.equal(onMain,true);assert.equal(c,constant);dispatch.push('STOP_FACTORY');return stopEvent;
  },'oak')};
  const LatinIME={dG:family(['oak'],()=>{throw Error('must use selected instance');}),
    requestShowSelf:family(['int'],()=>{throw Error('must use selected instance');})};
  const classes={
    [UI.IME]:LatinIME,kla:Kla,oak:Oak,mut:{b:family(['android.content.Context'],()=>pref,'boolean')},
    [UI.CONSUMER]:Consumer,'java.lang.reflect.Modifier':{
      isStatic:n=>(n&8)!==0,isAbstract:n=>(n&0x400)!==0,isFinal:n=>(n&16)!==0
    }
  };
  const Java={available:true,
    perform:fn=>queueMicrotask(fn),
    scheduleOnMainThread:fn=>queueMicrotask(()=>{const before=onMain;onMain=true;try{fn();}finally{onMain=before;}}),
    use:name=>{uses.push(name);assert(classes[name],'unapproved class '+name);return classes[name];},
    choose:(name,callbacks)=>{chooses.push(name);assert.equal(name,UI.IME);for(const target of targets){if(callbacks.onMatch(target)==='stop')break;}callbacks.onComplete();},
    retain:object=>object,cast:object=>object,vm:{getEnv:()=>({isSameObject:(a,b)=>a===b})},
    registerClass:definition=>{
      assert.equal(definition.name,'org.rambpc.referenceprobe.PixelBoardBoundedLaunchConsumer');
      assert.deepEqual(definition.implements,[Consumer]);
      return {$new(){const result={referenceToken:{value:0},$dispose(){}};
        result[UI.CONSUMER_METHOD]=value=>definition.methods[UI.CONSUMER_METHOD][0].implementation.call(result,value);
        return result;}};
    }
  };
  const originalStatus=()=>currentAudio, originalStop=()=>({stopped:true}), rpc={exports:{status:originalStatus,stop:originalStop}};
  let bridgeLookups=0;
  const bridge=options.lazyBridge?()=>{bridgeLookups++;return Java;}:Java;
  const internals=UI.install(bridge,rpc,e=>events.push(e),{
    setTimeout:fn=>{const id=++timerId;timers.set(id,fn);return id;},clearTimeout:id=>timers.delete(id)
  });
  return {rpc:rpc.exports,dispatch,events,chooses,uses,targets,gate:internals.gate,
    audio:()=>currentAudio,setAudio:x=>{currentAudio=x;},setPref:x=>{pref=x;},
    addTarget:configuration=>targets.push(ime(configuration)),
    expire(){for(const fn of [...timers.values()])fn();},
    callback:()=>consumer&&consumer[UI.CONSUMER_METHOD](startEvent),
    originalStatus,originalStop,bridgeLookups:()=>bridgeLookups};
}

test('pure gate: exact pin/scope/preference/pending checks do not spend a start on denial',()=>{
  const candidates=[
    [Object.assign(audio(1),{phase:'loaded'}),scope(),true],
    [audio(1),{...scope(),editorPackage:'unapproved.editor'},true],
    [audio(1),{...scope(),uniqueConnected:2},true],
    [audio(1),{...scope(),editorAction:3},true],
    [audio(1),scope(),false]
  ];
  const noArm=audio(1);noArm.stream.pending=null;candidates.push([noArm,scope(),true]);
  const wrongPin=audio(1);wrongPin.pin.apkSha256='0'.repeat(64);candidates.push([wrongPin,scope(),true]);
  for(const args of candidates){const g=new UI.UiGate();assert.throws(()=>g.start(...args));assert.equal(g.starts,0);assert.equal(g.active,null);}
});

test('pure gate: three lifetime starts, one start per arm, no reset, permanent failure and busy guard',()=>{
  const g=new UI.UiGate();g.enter();assert.throws(()=>g.enter(),/pending/);g.leave();
  for(let i=1;i<=3;i++){
    const a=audio(i);g.start(a,scope(),true);assert.equal(g.starts,i);
    assert.throws(()=>g.start(a,scope(),true),/previous/);
    g.pendingStillMatches(a);g.started();g.end(a,scope());g.ended();
    assert.throws(()=>g.start(a,scope(),true),/already/);
  }
  assert.throws(()=>g.start(audio(4),scope(),true));
  assert.equal(g.starts,3);
  const failed=new UI.UiGate();failed.fail();assert.throws(()=>failed.start(audio(1),scope(),true),/permanently/);
});

test('pure gate: arm changes before dispatch and unstarted end are refused',()=>{
  const g=new UI.UiGate();g.start(audio(1),scope(),true);
  assert.throws(()=>g.end(audio(1),scope()),/no dispatched/);
  const consumed=audio(1);consumed.stream.pending=null;assert.throws(()=>g.pendingStillMatches(consumed),/changed/);
  assert.throws(()=>g.pendingStillMatches(audio(2)),/changed/);
});

test('mock UI: readonly status, show, helper launch, constant STOP event, existing RPC preserved',async()=>{
  const m=model({consumeArm:true});
  assert.equal(m.rpc.status,m.originalStatus);assert.equal(m.rpc.stop,m.originalStop);
  const status=await m.rpc.uiStatus();assert.equal(status.targetVerified,true);assert.equal(status.ramblerEnabled,true);
  assert.deepEqual(m.dispatch,[]);
  const shown=await m.rpc.uiShow();assert.equal(shown.showRequested,true);assert.equal(shown.visibilityProven,false);
  const started=await m.rpc.uiStart();assert.equal(started.dispatchOnly,true);assert.equal(started.startCount,1);
  assert.equal(m.audio().stream.pending,null);
  const ended=await m.rpc.uiEnd();assert.equal(ended.endEquivalent,true);assert.equal(ended.originalExecutorExercised,false);
  assert.deepEqual(m.dispatch,['SHOW','HELPER_G','LAUNCH','STOP_FACTORY','STOP']);
  assert(m.chooses.every(x=>x===UI.IME));assert.equal(m.uses.includes('knt'),false);assert.equal(m.uses.includes('iho'),false);
  assert.equal(m.events.find(e=>e.type==='loaded').methodReplacements,0);
});

test('mock UI: wrong editor/package/action, absent/ambiguous connections and overflow never dispatch',async()=>{
  for(const options of [
    {target:{editor:'wrong.editor'}},{target:{package:'wrong.package'}},{target:{action:3}},
    {target:{disconnected:true}},{targets:[{},{}]},{targets:new Array(9).fill({disconnected:true})},
    {pref:false},{consumerMethod:'e'}
  ]){
    const m=model(options);await assert.rejects(m.rpc.uiStart());assert.deepEqual(m.dispatch,[]);assert.equal(m.gate.starts,0);
  }
  const m=model();m.audio().stream.pending=null;await assert.rejects(m.rpc.uiStart());assert.deepEqual(m.dispatch,[]);
});

test('mock UI: three normal dispatches maximum; show bound and no lifecycle resets',async()=>{
  const m=model();
  for(let i=1;i<=3;i++){
    m.setAudio(audio(i));await m.rpc.uiShow();await m.rpc.uiStart();await m.rpc.uiEnd();
  }
  m.setAudio(audio(3));await assert.rejects(m.rpc.uiStart());await assert.rejects(m.rpc.uiShow());
  assert.equal(m.dispatch.filter(x=>x==='LAUNCH').length,3);assert.equal(m.dispatch.filter(x=>x==='STOP').length,3);
  assert.equal(m.gate.starts,3);
});

test('mock UI: focus/connection change refuses normal end without alternate event/backend',async()=>{
  const m=model();await m.rpc.uiStart();m.targets[0].connection={$h:Symbol('replacement'),$dispose(){}};
  await assert.rejects(m.rpc.uiEnd());
  assert.equal(m.dispatch.includes('STOP_FACTORY'),false);assert.equal(m.dispatch.includes('STOP'),false);
});

test('mock UI: timeout permanently fails, drops stale callback and never retries',async()=>{
  const m=model({noCallback:true}),pending=m.rpc.uiStart();
  await new Promise(setImmediate);
  await assert.rejects(m.rpc.uiEnd(),/pending/);
  m.expire();await assert.rejects(pending);
  m.callback();await new Promise(setImmediate);
  assert.deepEqual(m.dispatch,['HELPER_G']);assert.equal(m.gate.failed,true);assert.equal(m.gate.starts,1);
  await assert.rejects(m.rpc.uiStart());assert.deepEqual(m.dispatch,['HELPER_G']);
});

test('mock UI: helper failure permanently fails without invoking any backend or fallback',async()=>{
  const m=model({gThrows:true});await assert.rejects(m.rpc.uiStart());
  assert.equal(m.gate.failed,true);assert.equal(m.gate.starts,1);assert.deepEqual(m.dispatch,['HELPER_G']);
  await assert.rejects(m.rpc.uiStart());assert.deepEqual(m.dispatch,['HELPER_G']);
});

test('mock UI: deferred callback revalidates uniqueness, preference, audio arm and connection',async()=>{
  const mutations=[
    m=>m.addTarget({}),
    m=>m.setPref(false),
    m=>{m.audio().stream.pending=null;},
    m=>{m.targets[0].connection={$h:Symbol('replaced'),$dispose(){}};},
    m=>{m.targets[0].editor.imeOptions.value=3;}
  ];
  for(const mutate of mutations){
    let m;m=model({afterHelper:()=>mutate(m)});
    await assert.rejects(m.rpc.uiStart());
    assert.deepEqual(m.dispatch,['HELPER_G']);assert.equal(m.gate.failed,true);assert.equal(m.gate.starts,1);
  }
});

test('mock UI: normal bound End remains available after audio stop, without restarting audio',async()=>{
  const m=model();await m.rpc.uiStart();
  m.audio().phase='stopped';m.audio().stream.stopped=true;m.audio().stream.pending=null;
  await m.rpc.uiEnd();
  assert.deepEqual(m.dispatch,['HELPER_G','LAUNCH','STOP_FACTORY','STOP']);
  await assert.rejects(m.rpc.uiStart());
});

test('mock UI: Java bridge resolves lazily after source-module composition',async()=>{
  const m=model({lazyBridge:true});assert.equal(m.bridgeLookups(),0);
  await m.rpc.uiStatus();assert(m.bridgeLookups()>0);
  await m.rpc.uiStart();await m.rpc.uiEnd();
  assert.deepEqual(m.dispatch,['HELPER_G','LAUNCH','STOP_FACTORY','STOP']);
});
