// The web player in a real browser, run without a person (ADR 0024).
// `cargo xtask gpu` runs it; it needs Node and a Chromium browser, nothing else.
//
//   node check.mjs --rwgpu <rwgpu> --player <web player folder> --out <folder>
//                  [--browser <program>] [--paths webgpu,cpu,software] [--measure]
//                  <example> ...
//
// For each example it starts `rwgpu serve <example> --port 0 --web-player <folder>`
// and opens the page once per path: `webgpu` (the page as a person gets it: WebGPU
// where the browser gives an adapter), `cpu` (?renderer=cpu, the CPU reference
// renderer) and `software` (?renderer=software, the browser's fallback adapter).
// Each playtest is played to its end and the page reports the frame, the state
// hash and the reference's image hash. On a WebGPU path the frame the GPU drew is
// written to <out> with a line in <out>/frames.txt, for `gpu_check --web-frames`;
// on the CPU path the canvas is read back into a file too, for xtask to hash. `--measure` adds, on
// Tiny Town: a playtest replayed in real time, the device lost halfway through a
// playtest, the scene left alone for 3 s, and the camera turning for 3 s.
//
// The browser runs headless with a profile folder of its own under <out>,
// removed after; it is driven over the DevTools protocol on a pipe. The result
// is one JSON object on stdout; the judging is xtask's. Exit code 1 only when
// the check itself could not run.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { gzipSync } from 'node:zlib';

const argv = process.argv.slice(2);
const opt = (name, dflt) => { const i = argv.indexOf(name); return i >= 0 ? argv.splice(i, 2)[1] : dflt; };
const has = (name) => { const i = argv.indexOf(name); if (i >= 0) argv.splice(i, 1); return i >= 0; };
const rwgpu = resolve(opt('--rwgpu', 'rwgpu'));
const player = resolve(opt('--player', 'target/web-player'));
const out = resolve(opt('--out', 'target/web-check'));
const paths = opt('--paths', 'webgpu,cpu').split(',');
const measure = has('--measure');
const examples = argv.map((e) => resolve(e));
const T0 = Date.now();
const log = (...a) => console.error(`[${((Date.now() - T0) / 1000).toFixed(1)} s]`, ...a);

// A Chromium browser: --browser, RWGPU_BROWSER, or where the usual ones live.
function browser() {
  const given = opt('--browser', process.env.RWGPU_BROWSER);
  if (given) return given;
  const fixed = {
    darwin: ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', '/Applications/Chromium.app/Contents/MacOS/Chromium',
      '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge'],
    win32: ['C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe', 'C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe',
      'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe'],
  }[process.platform] || [];
  const found = fixed.find((p) => existsSync(p));
  if (found) return found;
  for (const name of ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'microsoft-edge']) {
    const r = spawnSync(process.platform === 'win32' ? 'where' : 'which', [name], { encoding: 'utf8' });
    if (r.status === 0 && r.stdout.trim()) return r.stdout.split(/\r?\n/)[0].trim();
  }
  return null;
}

// The browser, headless, on a DevTools pipe.
async function launch(program, flags) {
  mkdirSync(out, { recursive: true });
  const profile = mkdtempSync(join(out, 'profile-'));
  const args = ['--headless', '--remote-debugging-pipe', `--user-data-dir=${profile}`, '--no-first-run',
    '--no-default-browser-check', '--use-mock-keychain', '--password-store=basic', '--mute-audio', ...flags, 'about:blank'];
  const child = spawn(program, args, { stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe'] });
  let stderr = '';
  child.stderr.on('data', (d) => { stderr = (stderr + d).slice(-4000); });
  const [w, r] = [child.stdio[3], child.stdio[4]];
  // A browser that exits resets its end of the pipes: that is not an error here.
  w.on('error', () => {});
  r.on('error', () => {});
  const waiting = new Map();
  const events = [];
  const notes = [];
  let id = 0, buf = '';
  r.on('data', (d) => {
    buf += d;
    for (let i; (i = buf.indexOf('\0')) >= 0; buf = buf.slice(i + 1)) {
      const m = JSON.parse(buf.slice(0, i));
      if (m.method === 'Runtime.exceptionThrown') events.push(m.params.exceptionDetails.exception?.description || 'an exception');
      if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') {
        events.push(m.params.args.map((a) => a.value ?? a.description).join(' '));
      }
      // What the browser itself says (WebGPU's validation messages, for one): reported, not judged.
      if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'warning') notes.push(m.params.args.map((a) => a.value ?? a.description).join(' '));
      if (m.method === 'Log.entryAdded' && ['error', 'warning'].includes(m.params.entry.level)) notes.push(`${m.params.entry.source}: ${m.params.entry.text}`);
      const p = waiting.get(m.id);
      if (p) { waiting.delete(m.id); m.error ? p[1](new Error(`${m.error.message}`)) : p[0](m.result); }
    }
  });
  const send = (method, params = {}, sessionId) => new Promise((ok, no) => {
    waiting.set(++id, [ok, no]);
    w.write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + '\0');
  });
  const exited = new Promise((ok) => child.on('exit', ok));
  const { product } = await send('Browser.getVersion');
  return {
    product,
    events,
    notes,
    stderr: () => stderr,
    async open(url) {
      const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
      const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
      await send('Runtime.enable', {}, sessionId);
      await send('Log.enable', {}, sessionId);
      await send('Page.navigate', { url }, sessionId);
      const evaluate = async (expression, seconds = 120) => {
        let timer;
        const timeout = new Promise((_, no) => {
          timer = setTimeout(() => no(new Error(`no answer in ${seconds} s to ${expression.slice(0, 80)}`)), seconds * 1000);
        });
        try {
          const res = await Promise.race([timeout, send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, sessionId)]);
          const ex = res.exceptionDetails;
          if (ex) throw new Error(`${ex.exception?.description || ex.exception?.value || ex.text}\n  the page also said: ${[...events, ...notes].join(' | ')}`);
          return res.result.value;
        } finally {
          clearTimeout(timer);
        }
      };
      let ready;
      try {
        ready = await evaluate(`new Promise((ok) => { const t = setInterval(() => { const p = window.rwgpu;
          if (p && ((p.ready && p.firstFrameMs !== null) || p.error)) { clearInterval(t); ok(JSON.stringify({ error: p.error, path: p.path, adapter: p.adapter,
          fallback: p.fallback, firstFrameMs: p.firstFrameMs })); } }, 10); })`, 60);
      } catch (e) {
        // Say what the page was doing.
        const state = await evaluate(`JSON.stringify({ url: location.href, readyState: document.readyState, hidden: document.hidden,
          visibility: document.visibilityState, now: performance.now(), page: window.rwgpu || null })`, 5).catch((f) => String(f));
        throw new Error(`${e.message.split('\n')[0]}; the page: ${state}`);
      }
      return { ready: JSON.parse(ready), evaluate, close: () => send('Target.closeTarget', { targetId }) };
    },
    async close() {
      // The browser may exit before it answers this: do not wait for the answer.
      send('Browser.close').catch(() => {});
      const quit = (ms) => {
        let timer;
        return Promise.race([exited, new Promise((ok) => { timer = setTimeout(ok, ms); })]).finally(() => clearTimeout(timer));
      };
      await quit(5000);
      if (child.exitCode === null) { child.kill(); await quit(5000); }
      // On Windows Chrome's crash handler can hold the folder a moment longer; it is under <out>, which is ignored.
      try { rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }); } catch (_) { /* left */ }
    },
  };
}

// `rwgpu serve` on a free port; resolves to its address.
function serve(example) {
  const child = spawn(rwgpu, ['serve', example, '--port', '0', '--web-player', player], { stdio: ['ignore', 'pipe', 'pipe'] });
  let text = '';
  return new Promise((ok, no) => {
    const failed = () => no(new Error(`rwgpu serve ${example} did not start: ${text}`));
    child.on('exit', failed);
    const take = (d) => {
      text += d;
      const m = text.match(/at (http:\/\/127\.0\.0\.1:\d+\/)/);
      if (m) { child.off('exit', failed); ok({ url: m[1], line: text.split('\n')[0], stop: () => child.kill() }); }
    };
    child.stdout.on('data', take);
    child.stderr.on('data', (d) => { text += d; });
  });
}

const QUERY = { webgpu: '', cpu: '?renderer=cpu', software: '?renderer=software' };

async function main() {
  const program = browser();
  if (!program) {
    console.log(JSON.stringify({ browser: null }));
    return;
  }
  // Headless, Chrome gives a WebGPU adapter on GitHub's Windows runner (WARP) only with both of these, and
  // on the Linux runner (SwiftShader) with the first; the fallback adapter (?renderer=software) needs the
  // first too (ADR 0024). RWGPU_BROWSER_FLAGS adds more.
  const flags = ['--enable-unsafe-webgpu', '--ignore-gpu-blocklist',
    ...(process.env.RWGPU_BROWSER_FLAGS || '').split(' ').filter(Boolean)];
  // A profile folder an earlier run could not remove may still be held: what is written here is new anyway.
  try { rmSync(out, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }); } catch (_) { /* left */ }
  mkdirSync(out, { recursive: true });
  const download = WEB_FILES.map((name) => {
    const bytes = readFileSync(join(player, name));
    return { name, bytes: bytes.length, gzip: gzipSync(bytes, { level: 9 }).length };
  });
  const result = {
    browser: null, program: basename(program), flags, download,
    bytes: download.reduce((n, f) => n + f.bytes, 0), gzip: download.reduce((n, f) => n + f.gzip, 0),
    paths: {}, measure: {},
  };
  const b = await launch(program, flags);
  result.browser = b.product;
  log(`browser: ${b.product} (${program})`);
  const frames = [];
  try {
    for (const example of examples) {
      const name = basename(example);
      const server = await serve(example);
      log(server.line);
      for (const path of paths) {
        const tab = await b.open(server.url + QUERY[path]);
        if (tab.ready.error) throw new Error(`${name}, ${path}: the page failed: ${tab.ready.error}`);
        const run = result.paths[path] ||= { adapter: tab.ready.adapter, path: tab.ready.path, fallback: tab.ready.fallback,
          first_frame_ms: [], playtests: [] };
        run.first_frame_ms.push(tab.ready.firstFrameMs);
        log(`${name}, ${path}: ${tab.ready.path === 'webgpu' ? 'WebGPU, ' + tab.ready.adapter : 'CPU reference renderer, ' + tab.ready.fallback}`);
        const tests = JSON.parse(await tab.evaluate(`fetch('project').then((r) => r.json()).then((p) => JSON.stringify(p.tests))`));
        for (const test of tests) {
          const r = JSON.parse(await tab.evaluate(`window.rwgpu.play(${JSON.stringify(test)}).then(JSON.stringify)`));
          const row = { example: name, test, path: r.path, fallback: r.fallback, frame: r.frame, state: r.state, image: r.image,
            draws: r.stats.draws };
          // What the GPU drew, or what the 2D canvas shows: RGBA bytes in a file. A picture that does not
          // come back is that playtest's failure, said on its row; the others still run.
          try {
            const bytes = Buffer.from(await tab.evaluate('window.rwgpu.picture()', 40), 'base64');
            row.picture = `${path}__${name}__${basename(test, '.toml')}.rgba`;
            writeFileSync(join(out, row.picture), bytes);
            if (r.path === 'webgpu') frames.push(`${name} ${test} ${r.frame} ${r.width} ${r.height} ${row.picture}`);
          } catch (e) {
            row.error = String(e.message || e).split('\n')[0];
            // A copy can fail before the page hears that the browser lost the device: give it two seconds to.
            const after = JSON.parse(await tab.evaluate(`new Promise((ok) => { const t0 = performance.now();
              const t = setInterval(() => { const p = window.rwgpu; if (p.path !== 'webgpu' || performance.now() - t0 > 2000) {
              clearInterval(t); ok(JSON.stringify({ path: p.path, fallback: p.fallback })); } }, 20); })`, 10));
            row.path_after = after.path;
            row.fallback = after.fallback || row.fallback;
          }
          run.playtests.push(row);
          log(`  ${test}: frame ${r.frame}, state ${r.state}, ${r.path}${row.error ? ': ' + row.error : ''}`);
        }
        if (measure && name === 'tiny_town' && path === 'webgpu') {
          const m = result.measure;
          m.realtime = JSON.parse(await tab.evaluate(`window.rwgpu.play('tests/turn_and_build.toml', { realtime: true }).then(JSON.stringify)`));
          m.lost = JSON.parse(await tab.evaluate(`window.rwgpu.playLosing('tests/turn_and_build.toml', 45).then(JSON.stringify)`));
          await tab.close();
          const fresh = await b.open(server.url);
          m.idle = JSON.parse(await fresh.evaluate(`window.rwgpu.live(3000).then(JSON.stringify)`));
          m.turning = JSON.parse(await fresh.evaluate(`window.rwgpu.live(3000, { turn: true }).then(JSON.stringify)`));
          await fresh.close();
        } else {
          await tab.close();
        }
      }
      server.stop();
    }
  } finally {
    result.errors = b.events;
    result.notes = b.notes.slice(0, 50);
    await b.close();
  }
  writeFileSync(join(out, 'frames.txt'), frames.join('\n') + '\n');
  console.log(JSON.stringify(result, null, 1));
}

const WEB_FILES = ['index.html', 'player.js', 'rwgpu_web.js', 'rwgpu_web_bg.wasm'];

main().catch((e) => { console.error(`web check: ${e.stack || e}`); process.exit(1); });
