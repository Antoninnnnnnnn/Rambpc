// node paste_matrix.mjs <edge|chrome|brave|code> <exe> <paste_probe.exe> <scratch> [runs]
// Runs only in unique disposable profiles. The native probe checks the fixture PID again.
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { setTimeout as delay } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';

const [app, exe, probe, scratch, count = '20'] = process.argv.slice(2);
if (!['edge', 'chrome', 'brave', 'code'].includes(app) || !exe || !probe || !scratch || !/^[1-9]\d*$/.test(count)) {
  throw Error('Invalid fixture arguments');
}
const runs = Number(count);
const root = await mkdtemp(join(scratch, `sambler-paste-${app}-`));
const profile = join(root, 'profile');
const html = pathToFileURL(join(root, 'editor.html')).href;
const code = app === 'code';
const one = 'sambler fixture é🦀';
const two = `${one}\nsecond line`;
const flags = ['--force-renderer-accessibility', '--disable-background-networking', '--no-proxy-server', '--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost'];
let processHandle, probeHandle, socket, lines;
const statuses = Object.create(null);
function summarize(name, samples) {
  const percentiles = key => {
    const values = samples.map(s => s[key]).filter(Number.isFinite).sort((a, b) => a - b);
    const q = p => values[Math.ceil(p * values.length) - 1]?.toFixed(1) ?? '-';
    return `${q(0.5)}/${q(0.95)}`;
  };
  const counts = ['exact', 'caret', 'undo', 'safe'].map(k => `${k}=${samples.filter(s => s[k]).length}/${runs}`).join(' ');
  const times = ['capture_ms', 'recheck_ms', 'visible_ms', 'insertion_ms'].map(k => `${k}=${percentiles(k)}`).join(' ');
  console.log(`${app} ${name} ${counts} ${times}`);
}
async function waitFor(callback, tries = 90, pause = 15) {
  for (let i = 0; i < tries; i++) {
    const value = await callback();
    if (value) return value;
    await delay(pause);
  }
  return undefined;
}
function cdp(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++cdp.id;
    const onResponse = ({data}) => {
      const payload = JSON.parse(data);
      if (payload.id !== id) return;
      socket.removeEventListener('message', onResponse);
      clearTimeout(timer);
      if (payload.error) reject(Error(`${method} failed`)); else resolve(payload.result);
    };
    const timer = setTimeout(() => {
      socket.removeEventListener('message', onResponse);
      reject(Error(`${method} timed out`));
    }, 2500);
    socket.addEventListener('message', onResponse);
    socket.send(JSON.stringify({id, method, params}));
  });
}
cdp.id = 0;
const evalJs = async expression => {
  const reply = await cdp('Runtime.evaluate', { expression, returnByValue: true });
  if (reply.exceptionDetails) throw Error('Fixture script failed');
  return reply.result.value;
};
async function native(command) {
  const reply = lines.next();
  probeHandle.stdin.write(`${command}\n`);
  const result = await reply;
  if (result.done) throw Error('Native fixture exited');
  const line = result.value;
  const fields = Object.fromEntries([...line.matchAll(/(\w+)=([\w.()-]+)/g)].map(([,k,v]) => [k, v]));
  if (fields.foreground_lost || fields.wrong_fixture) throw Error(`Disposable fixture lost foreground during ${command}: ${line}`);
  for (const key of ['capture_ms', 'recheck_ms', 'epoch', 'visible_ms', 'insertion_ms']) fields[key] = Number(fields[key]);
  for (const key of ['exact', 'caret', 'restored', 'keys']) fields[key] = fields[key] === 'true';
  if (line.includes('reason: Unsupported')) fields.unsupported = true;
  if (line.includes('Uncertain') || line.includes('Err(')) statuses[line.replace(/\d+(\.\d+)?/g, '#')] = true;
  return fields;
}
async function launchBrowser() {
  await writeFile(join(root, 'editor.html'), `<!doctype html><meta charset=utf-8><title>Sambler offline fixture</title>
  <form id="fixture-form" style="display:grid;gap:12px;padding:30px">
    <input id="single" aria-label="Fixture input" type="text">
    <textarea id="multi" aria-label="Fixture textarea"></textarea>
    <div id="rich" role="textbox" aria-label="Fixture rich editor" aria-multiline="true" contenteditable="true" style="border:1px solid;padding:12px;white-space:pre-wrap"></div>
    <button>Fixture submit</button>
  </form>
  <script>
    window.submits = 0;
    document.querySelector('form').onsubmit = e => { e.preventDefault(); window.submits++; };
    window.fixtureContents = el => el.isContentEditable ? el.innerText : el.value;
    window.setupFixture = id => {
      const el = document.getElementById(id);
      el.focus();
      if (el.isContentEditable) {
        el.textContent = 'left  right';
        getSelection().collapse(el.firstChild, 5);
      } else {
        el.value = 'left  right';
        el.setSelectionRange(5, 5);
      }
      window.visible = null;
      el.addEventListener('input', function painted(e) {
        if (e.inputType !== 'insertFromPaste') return;
        el.removeEventListener('input', painted);
        requestAnimationFrame(() => setTimeout(() => {
          let caret;
          if (el.isContentEditable) {
            const selection = getSelection();
            if (!selection.rangeCount || !selection.isCollapsed || !el.contains(selection.anchorNode)) caret = false;
            else {
              const tail = selection.getRangeAt(0).cloneRange();
              tail.setEnd(el, el.childNodes.length);
              caret = tail.toString() === ' right';
            }
          } else {
            caret = el.selectionStart === el.value.length - 6 && el.selectionStart === el.selectionEnd;
          }
          window.visible = { at: performance.timeOrigin + performance.now(), exactValue: fixtureContents(el), caret };
        }, 0));
      }, { signal: (window.fixtureSignal ??= new AbortController()).signal });
      return document.activeElement === el;
    };
  </script>`);
  processHandle = spawn(exe, [...flags, `--user-data-dir=${profile}`, '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1', '--no-first-run', '--no-default-browser-check', '--disable-sync', '--new-window', html], {stdio:'ignore'});
  const address = await waitFor(async () => {
    try {
      const port = (await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0];
      return (await fetch(`http://127.0.0.1:${port}/json/list`, {signal: AbortSignal.timeout(300)}).then(r => r.json())).find(t => t.url === html)?.webSocketDebuggerUrl;
    } catch { return undefined; }
  }, 90, 150);
  if (!address) throw Error('Disposable browser did not start');
  socket = new WebSocket(address);
  await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, {once:true}); socket.addEventListener('error', reject, {once:true}); });
  await cdp('Page.bringToFront');
  await delay(400);
}
async function launchCode() {
  const settings = join(profile, 'User');
  const driver = join(root, 'driver');
  const workspace = join(root, 'workspace');
  const extensions = join(root, 'extensions');
  for (const dir of [settings, driver, workspace, extensions]) await mkdir(dir, {recursive:true});
  await writeFile(join(settings, 'settings.json'), JSON.stringify({
    'telemetry.telemetryLevel': 'off', 'update.mode': 'none', 'workbench.startupEditor': 'none',
    'editor.accessibilitySupport': 'on', 'security.workspace.trust.enabled': false,
  }));
  await writeFile(join(driver, 'package.json'), JSON.stringify({
    name:'offline-sambler-paste-fixture', publisher:'local-fixture', version:'0.0.1',
    engines:{vscode:'^1.90.0'}, main:'./driver.cjs', activationEvents:['onStartupFinished'],
    contributes:{chatParticipants:[{id:'fixture.offline',name:'samblerfixture',fullName:'Offline fixture',description:'Local test participant'}]},
  }));
  await writeFile(join(driver, 'driver.cjs'), `const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');
exports.activate = async context => {
  // Never call a submit command; an isolated dummy participant records any invocation.
  let count = 0;
  const done = path.join(__dirname, 'ready');
  const participant = vscode.chat.createChatParticipant('fixture.offline', () => {
    fs.writeFileSync(done, String(++count)); return {};
  });
  context.subscriptions.push(participant);
  await vscode.commands.executeCommand('workbench.action.chat.open');
  fs.writeFileSync(done, '0');
};`);
  processHandle = spawn(exe, [...flags, '--new-window', '--disable-extensions', '--skip-release-notes', `--extensions-dir=${extensions}`, `--extensionDevelopmentPath=${driver}`, `--user-data-dir=${profile}`, workspace], {stdio:'ignore'});
  const ready = join(driver, 'ready');
  if (!await waitFor(async () => readFile(ready, 'utf8').catch(() => undefined), 100, 150)) throw Error('Disposable chat did not start');
  await delay(500);
  return async () => (await readFile(ready, 'utf8')) === '0';
}
try {
  await mkdir(root, {recursive:true});
  const codeSafe = code ? await launchCode() : await launchBrowser();
  if (!processHandle.pid) throw Error('Fixture has no PID');
  probeHandle = spawn(probe, [app, String(processHandle.pid)], {stdio:['pipe','pipe','inherit'], windowsHide:true});
  lines = createInterface({input:probeHandle.stdout})[Symbol.asyncIterator]();
  if (!(await native('foreground')).keys) throw Error('Could not activate the disposable fixture');
  if (code) {
    for (const mode of ['one','two']) {
      const samples = [];
      for (let i = 0; i < runs; i++) {
        const sample = await native(`chat-${mode}`);
        if (sample.outcome === 'Delivered') await native('undo');
        sample.undo = Boolean(await waitFor(async () => (await native('chat-empty')).restored, 15));
        sample.safe = await codeSafe();
        samples.push(sample);
      }
      summarize(`chat/${mode}`, samples);
    }
  } else {
    // A new Chromium profile may put keyboard focus on browser chrome.
    // Give the disposable page one mouse click before measuring UIA capture.
    const focusPoint = await evalJs("(()=>{const r=document.getElementById('single').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()");
    await cdp('Input.dispatchMouseEvent', {...focusPoint, type:'mousePressed', button:'left', clickCount:1});
    await cdp('Input.dispatchMouseEvent', {...focusPoint, type:'mouseReleased', button:'left', clickCount:1});
    for (const [id, mode] of [['single','one'],['single','two'],['multi','one'],['multi','two'],['rich','one'],['rich','two']]) {
      const samples = [];
      for (let i = 0; i < runs; i++) {
        await evalJs(`window.fixtureSignal?.abort(); window.fixtureSignal = null; window.setupFixture('${id}')`);
        await delay(50);
        if (!(await native('foreground')).keys) throw Error('Could not activate the disposable field fixture');
        const sample = await native(mode);
        if (id === 'single' && mode === 'two') {
          sample.exact = sample.unsupported && await evalJs("document.getElementById('single').value === 'left  right' && !window.visible");
        } else if (sample.outcome === 'Delivered') {
          const state = await waitFor(() => evalJs('window.visible'), 15);
          if (state) {
            sample.exact = state.exactValue === `left ${mode === 'one' ? one : two} right`;
            sample.caret = state.caret;
            sample.visible_ms = state.at - sample.epoch;
          }
          if ((await native('foreground')).keys
              && await evalJs(`document.activeElement === document.getElementById('${id}')`)
              && (await native('undo')).keys) {
            sample.undo = Boolean(await waitFor(() => evalJs(`fixtureContents(document.getElementById('${id}')) === 'left  right'`), 15));
          }
        }
        sample.safe = (await evalJs('window.submits')) === 0;
        samples.push(sample);
      }
      summarize(`${id}/${mode}`, samples);
    }
    for (const mode of ['one', 'two']) {
      const samples = [];
      for (let i = 0; i < runs; i++) {
        if (!(await native('foreground')).keys) throw Error('Could not activate the disposable omnibox fixture');
        await native('address');
        await delay(90);
        const sample = await native(`address-${mode}`);
        if (mode === 'one' && sample.outcome === 'Delivered') {
          await native('undo');
          sample.undo = Boolean(await waitFor(async () => (await native('address-restored')).restored, 15));
        } else if (mode === 'two') {
          sample.exact = sample.unsupported && (await native('address-restored')).restored;
        }
        sample.safe = (await evalJs('location.href')) === html && (await evalJs('window.submits')) === 0;
        await native('escape');
        await delay(60);
        await native('escape');
        await delay(60);
        samples.push(sample);
      }
      summarize(`omnibox/${mode}`, samples);
    }
  }
  for (const status of Object.keys(statuses)) console.log(`Diagnostic ${status}`);
} finally {
  lines?.return?.();
  probeHandle?.stdin.end();
  if (socket?.readyState === WebSocket.OPEN) { await Promise.race([cdp('Browser.close').catch(() => {}), delay(500)]); socket.close(); }
  if (processHandle && processHandle.exitCode === null) {
    const exit = once(processHandle, 'exit');
    if (!socket) processHandle.kill();
    await Promise.race([exit, delay(1400)]);
    if (processHandle.exitCode === null) processHandle.kill();
  }
  await rm(root, {recursive:true, force:true, maxRetries:12, retryDelay:180});
}
