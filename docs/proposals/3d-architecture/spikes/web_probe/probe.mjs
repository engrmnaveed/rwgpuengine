// Which browsers on this machine give WebGPU when run headless, and with which
// flags. A measurement for ADR 0024, not a check: it prints a table (and adds
// it to the GitHub job summary) and always exits 0.
//
//   node probe.mjs                 every browser it finds, every set of flags
//   PROBE_BROWSERS=/path/to/chrome node probe.mjs
//
// The page (index.html) asks for the default adapter and for the fallback
// adapter, opens a device on each, clears a texture and reads it back. Each
// browser runs with a profile folder of its own, made here and removed after.
// Node only: the browser is driven over the DevTools protocol on a pipe.
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { appendFileSync, existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const page = readFileSync(join(here, 'index.html'));
const server = createServer((_, res) => { res.writeHead(200, { 'content-type': 'text/html' }); res.end(page); });
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const url = `http://127.0.0.1:${server.address().port}/`;

function found() {
  if (process.env.PROBE_BROWSERS) return process.env.PROBE_BROWSERS.split(',');
  const onPath = (name) => {
    const r = spawnSync(process.platform === 'win32' ? 'where' : 'which', [name], { encoding: 'utf8' });
    return r.status === 0 ? r.stdout.split(/\r?\n/)[0].trim() : null;
  };
  const fixed = {
    darwin: ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge'],
    win32: ['C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe', 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe'],
    linux: [],
  }[process.platform] || [];
  const named = ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'microsoft-edge'].map(onPath);
  return [...new Set([...fixed.filter((p) => existsSync(p)), ...named.filter(Boolean)])];
}

const FLAGS = {
  'none': [],
  'unsafe': ['--enable-unsafe-webgpu'],
  'unsafe, Vulkan': ['--enable-unsafe-webgpu', '--enable-features=Vulkan'],
  'unsafe, Vulkan, no blocklist': ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--ignore-gpu-blocklist'],
  'unsafe, adapter swiftshader': ['--enable-unsafe-webgpu', '--use-webgpu-adapter=swiftshader'],
  'unsafe, no blocklist': ['--enable-unsafe-webgpu', '--ignore-gpu-blocklist'],
  // SwiftShader may draw Chrome's own GL too: a WebGPU canvas on SwiftShader keeps its device then.
  'unsafe, no blocklist, unsafe SwiftShader': ['--enable-unsafe-webgpu', '--ignore-gpu-blocklist', '--enable-unsafe-swiftshader'],
  'unsafe, ANGLE SwiftShader': ['--enable-unsafe-webgpu', '--use-angle=swiftshader'],
  // More tries at keeping SwiftShader's canvas on Linux: software compositing, and every GL fallback allowed
  // at once. (`--in-process-gpu` made Chrome hang on the Linux runner, run 45884520276: left out.)
  'unsafe, software compositing': ['--enable-unsafe-webgpu', '--disable-gpu-compositing'],
  'unsafe, all SwiftShader, no blocklist': ['--enable-unsafe-webgpu', '--ignore-gpu-blocklist', '--enable-unsafe-swiftshader',
    '--use-gl=angle', '--use-angle=swiftshader'],
};

async function run(browser, flags) {
  const profile = mkdtempSync(join(tmpdir(), 'web-probe-'));
  const args = ['--headless', '--remote-debugging-pipe', `--user-data-dir=${profile}`, '--no-first-run',
    '--no-default-browser-check', '--use-mock-keychain', '--password-store=basic', ...flags, 'about:blank'];
  const child = spawn(browser, args, { stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe'] });
  let stderr = '';
  child.stderr.on('data', (d) => { stderr += d; });
  const [w, r] = [child.stdio[3], child.stdio[4]];
  // A browser that exits resets its end of the pipes (seen on Linux): not an error here.
  w.on('error', () => {});
  r.on('error', () => {});
  const waiting = new Map();
  let id = 0, buf = '';
  r.on('data', (d) => {
    buf += d;
    for (let i; (i = buf.indexOf('\0')) >= 0; buf = buf.slice(i + 1)) {
      const m = JSON.parse(buf.slice(0, i));
      const p = waiting.get(m.id);
      if (p) { waiting.delete(m.id); m.error ? p[1](new Error(JSON.stringify(m.error))) : p[0](m.result); }
    }
  });
  const send = (method, params = {}, sessionId) => new Promise((ok, no) => {
    waiting.set(++id, [ok, no]);
    w.write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + '\0');
  });
  const t0 = Date.now();
  let out;
  try {
    const timeout = new Promise((_, no) => setTimeout(() => no(new Error('no answer in 60 s')), 60000));
    out = await Promise.race([timeout, (async () => {
      const { product } = await send('Browser.getVersion');
      const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
      const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
      await send('Page.navigate', { url }, sessionId);
      const expr = `new Promise((ok) => { const t = setInterval(() => { if (window.__result) { clearInterval(t); ok(JSON.stringify(window.__result)); } }, 20); })`;
      const res = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }, sessionId);
      return { product, ...JSON.parse(res.result.value) };
    })()]);
  } catch (e) {
    out = { error: e.message, stderr: stderr.slice(-300) };
  }
  // A browser that does not stop is stopped harder: a hung one must not hold the job.
  const exited = (ms) => new Promise((ok) => {
    if (child.exitCode !== null) return ok();
    const t = setTimeout(ok, ms);
    child.on('exit', () => { clearTimeout(t); ok(); });
  });
  child.kill();
  await exited(5000);
  if (child.exitCode === null) { child.kill('SIGKILL'); await exited(5000); }
  // On Windows Chrome's crash handler can hold the folder a moment longer: a temporary folder left behind is fine.
  try { rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }); } catch (_) { /* left */ }
  return { ...out, ms: Date.now() - t0 };
}

const adapter = (a) => !a ? 'none' : a.error ? `error: ${a.error}`
  : `${a.vendor || '?'} / ${a.architecture || '?'}${a.fallback ? ' (fallback)' : ''}, clear ${a.px && a.px.join(',') === '255,128,64,255' ? 'read back' : 'WRONG ' + a.px}`;
const shown = (c) => !c ? 'no adapter' : c.error ? `error: ${c.error}` : [c.px ? `read back ${c.px.join(',')}` : `NOT read back: ${c.mapError}`,
  c.errors && c.errors.length ? `errors: ${c.errors.join('; ')}` : '', c.lost ? `lost: ${c.lost}` : ''].filter(Boolean).join('; ');
const rows = ['| browser | flags | navigator.gpu | default adapter | fallback adapter | 60 frames on a canvas, then a read-back | time |', '|---|---|---|---|---|---|---|'];
for (const browser of found()) {
  for (const [name, flags] of Object.entries(FLAGS)) {
    const r = await run(browser, flags);
    const row = r.error
      ? `| ${browser} | ${name} | ${r.error} ${r.stderr ? JSON.stringify(r.stderr) : ''} | | | | ${r.ms} ms |`
      : `| ${r.product} | ${name} | ${r.gpu ? 'yes' : 'no'} | ${adapter(r.default)} | ${adapter(r.forced)} | ${shown(r.canvas)} | ${r.ms} ms |`;
    console.log(row);
    rows.push(row);
  }
}
server.close();
const label = process.env.LABEL || process.platform;
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n### ${label}\n\n${rows.join('\n')}\n`);
