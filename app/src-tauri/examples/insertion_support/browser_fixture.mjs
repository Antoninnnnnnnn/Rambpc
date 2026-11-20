// Offline Chromium UIA/SendInput qualification. Uses a disposable browser profile only.
import { spawn, execFileSync } from 'node:child_process';
import { mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { setTimeout as sleep } from 'node:timers/promises';

const [app, exe, probe, scratch] = process.argv.slice(2);
assert(['edge', 'chrome'].includes(app) && exe && probe && scratch);
const dir = join(scratch, `sambler-${app}-fixture-${process.pid}`);
const user = join(dir, 'profile');
await mkdir(dir, { recursive: true });
const html = join(dir, 'editor.html');
await writeFile(html, `<!doctype html><meta charset=utf-8><title>Offline typing fixture</title>
<form style="display:grid;gap:12px;padding:30px" id="form"><label>Input <input id="single" aria-label="Fixture input" type="text"></label>
<label>Textarea <textarea id="multi" aria-label="Fixture textarea"></textarea></label>
<div id="rich" role="textbox" aria-label="Fixture rich editor" aria-multiline="true" contenteditable="true" style="border:1px solid;padding:12px"></div>
<button>Fixture submit</button></form>
<script>window.submits=0; form.onsubmit=e=>{e.preventDefault();window.submits++};</script>`);
const child = spawn(exe, [`--user-data-dir=${user}`, '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1', '--force-renderer-accessibility', '--disable-background-networking', '--no-first-run', '--no-default-browser-check', '--disable-sync', '--no-proxy-server', '--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost', '--new-window', pathToFileURL(html).href], { stdio: 'ignore', windowsHide: false });
let sock;
const exit = once(child, 'exit');
const cdp = async () => {
  for (let i = 0; i < 80; i++) {
    try {
      const port = (await readFile(join(user, 'DevToolsActivePort'), 'utf8')).split('\n')[0];
      const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then(r => r.json());
      const target = targets.find(t => t.url === pathToFileURL(html).href);
      if (target) return target.webSocketDebuggerUrl;
    } catch { /* browser startup */ }
    await sleep(150);
  }
  throw Error('Offline browser fixture did not start');
};
function call(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++call.id;
    const receive = event => {
      const response = JSON.parse(event.data);
      if (response.id !== id) return;
      sock.removeEventListener('message', receive);
      if (response.error) reject(Error(method)); else resolve(response.result);
    };
    sock.addEventListener('message', receive);
    sock.send(JSON.stringify({ id, method, params }));
  });
}
call.id = 0;
const js = async expression => (await call('Runtime.evaluate', { expression, returnByValue: true })).result.value;
const run = mode => execFileSync(probe, [app, String(child.pid), mode], { encoding: 'utf8', timeout: 45000 });
async function setup(id) {
  await js(`(()=>{ let el=document.getElementById(${JSON.stringify(id)}); el.value=''; el.textContent=''; el.focus(); if(el.setSelectionRange) el.setSelectionRange(0,0); return document.activeElement===el; })()`);
  await sleep(70);
}
function read(id) { return js(`document.getElementById('${id}').${id === 'rich' ? 'innerText' : 'value'}`); }
const expected = 'Éléonore dit : « À jeudi, c’est 14 h. » Café à 5 € 🦀. Let’s meet on Thursday; déjà vu, naïve façade. '.padEnd(200, 'àb');
try {
  sock = new WebSocket(await cdp());
  await new Promise((resolve, reject) => { sock.addEventListener('open', resolve, { once: true }); sock.addEventListener('error', reject, { once: true }); });
  await call('Page.bringToFront');
  await sleep(350);
  const pairOnly = process.env.SAMBLER_FIXTURE_PAIR === '1';
  for (const id of pairOnly ? ['single'] : ['single', 'multi', 'rich']) {
    await setup(id);
    console.log(`${app} ${id}: ${run('probe').trim()}`);
    const timings = [];
    const repeats = process.env.SAMBLER_FIXTURE_QUICK ? 0 : 20;
    for (let i = 0; i < repeats; i++) {
      await setup(id);
      const out = run(pairOnly ? 'u200-pair' : 'u200');
      if (!out.includes('outcome=Delivered')) { console.log(`${app} ${id}: not qualified (${out.trim().replace(/\r?\n/g, '; ')})`); break; }
      const actual = await read(id);
      if (actual !== expected) {
        const differing = Array.from({length: Math.max(actual?.length ?? 0, expected.length)}, (_, j) => j).filter(j => actual?.[j] !== expected[j]).length;
        console.log(`${app} ${id}: mismatch at run ${i + 1}; ${differing} differing units`);
        break;
      }
      timings.push(Number(/elapsed_ms=([\d.]+)/.exec(out)[1]) / 2);
    }
    if (timings.length) {
      timings.sort((a,b) => a-b);
      console.log(`${app} ${id}: ${timings.length}/20 exact; k=${pairOnly ? 2 : 1}; ${timings[Math.floor(timings.length / 2)].toFixed(1)} ms/100 units`);
    }
    await setup(id);
    const out = run('two');
    const matches = (await read(id)) === 'office-fixture\nsecond-fixture';
    console.log(`${app} ${id}: multiline exact=${matches} submits=${await js('window.submits')} ${out.trim().split(/\r?\n/).at(-1)}`);
    if (id !== 'single') {
      await setup(id);
      const packet = run('packet-lf');
      console.log(`${app} ${id}: U+000A exact=${(await read(id)) === 'office-fixture\nsecond-fixture'} submits=${await js('window.submits')} ${packet.trim().split(/\r?\n/).at(-1)}`);
    }
  }
  await setup('single');
  const pairs = run('u200-pair');
  console.log(`${app} k=2 exact=${(await read('single')) === expected} ${pairs.trim().split(/\r?\n/).at(-1)}`);
  await setup('multi');
  await setup('single');
  await js(`document.getElementById('single').addEventListener('input',()=>document.getElementById('multi').focus(),{once:true}); true`);
  const change = run('u200');
  console.log(`${app} in-window focus switch: ${change.trim().split(/\r?\n/).at(-1)}; other field empty=${(await read('multi')) === ''}`);
  assert.equal(await js('window.submits'), 0);
} finally {
  if (sock?.readyState === WebSocket.OPEN) { try { await Promise.race([call('Browser.close'), sleep(500)]); } catch {} }
  sock?.close();
  await Promise.race([exit.catch(() => {}), sleep(2000)]);
  if (child.exitCode === null) child.kill();
  await rm(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
