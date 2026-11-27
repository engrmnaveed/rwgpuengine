// The exported folder plays the game: serve a folder made by `rwgpu export web`
// with a plain static server, open its page in a small stand-in for a browser
// (the folder's own index.html and player.js, nothing of ours in between),
// and replay the project's playtests through page addresses
// (`?input=tests/x.toml&frames=N`). The frame, the state hash and the picture
// the page paints must be `rwgpu test`'s and `rwgpu shot`'s.
//
// usage: node web/export-check.mjs <folder> <project dir> --native <rwgpu>
//          [--export-json FILE] [--base /games/town/] [--wasm-type TYPE] [--open-only]
//
// --base       the path the folder is served under (default `/`)
// --wasm-type  the Content-Type sent for .wasm (default application/wasm), to
//              prove the page still loads from a host that sends another
// --open-only  only open the page as linked (the main scene at frame 0)
// --export-json  the command's --json output: sizes by part (player, game)
//
// `cargo xtask ci` runs it for every example (ADR 0025). Exits 1 if anything
// differs, if the page asks for anything outside the folder, or if a request
// fails.
import { readFileSync, readdirSync, mkdtempSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { gzipSync, constants } from 'node:zlib';
import vm from 'node:vm';
import { host } from './static-host.mjs';

const argv = process.argv.slice(2);
const opt = (name, dflt) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : dflt);
const [folderArg, projectArg] = argv;
const folder = resolve(folderArg);
const project = resolve(projectArg);
const native = opt('--native') && resolve(opt('--native'));
const base = opt('--base', '/');
const wasmType = opt('--wasm-type', 'application/wasm');
const openOnly = argv.includes('--open-only');
const exportJson = opt('--export-json');
if (!native || !base.startsWith('/') || !base.endsWith('/')) {
  console.error('usage: node web/export-check.mjs <folder> <project dir> --native <rwgpu> [--base /path/] ...');
  process.exit(2);
}
let failed = false;
const fail = (msg) => { failed = true; console.log(`FAIL: ${msg}`); };

// ---- a plain static host: files of the folder under `base`, nothing else
const site = await host(folder, base, { wasmType });
const { origin, answered } = site;

// ---- FNV-1a 64 in 16-bit pieces (fast enough for whole frames)
function fnv64(bytes) {
  let h0 = 0x2325, h1 = 0x8422, h2 = 0x9ce4, h3 = 0xcbf2;
  for (let i = 0; i < bytes.length; i++) {
    h0 ^= bytes[i];
    // h * 0x100000001b3 = h * 0x1b3 + (h << 40)
    const t0 = h0 * 0x1b3;
    const t1 = h1 * 0x1b3 + (t0 >>> 16);
    const t2 = h2 * 0x1b3 + (t1 >>> 16) + (h0 << 8);
    const t3 = h3 * 0x1b3 + (t2 >>> 16) + (h1 << 8);
    h0 = t0 & 0xffff; h1 = t1 & 0xffff; h2 = t2 & 0xffff; h3 = t3 & 0xffff;
  }
  return [h3, h2, h1, h0].map((x) => x.toString(16).padStart(4, '0')).join('');
}
if (fnv64(new TextEncoder().encode('a')) !== 'af63dc4c8601ec8c') throw new Error('fnv64 is wrong');

// ---- the page, run as a browser would run it, with a stand-in DOM
async function openPage(query) {
  const page = new URL(base + query, origin);
  const asked = [];
  const pageFetch = (u) => {
    const url = new URL(u, page);
    if (url.origin !== origin || !url.pathname.startsWith(base)) {
      fail(`the page asked for ${url.href}, outside the folder`);
      return Promise.reject(new TypeError(`outside the folder: ${url.href}`));
    }
    asked.push(url.pathname.slice(base.length) || 'index.html');
    return fetch(url);
  };
  const html = await (await pageFetch(page.href)).text();
  for (const m of html.matchAll(/\b(?:src|href)="([^"]*)"/g)) {
    if (/^(\/|[a-z][a-z0-9+.-]*:)/i.test(m[1]) && !m[1].startsWith('data:')) fail(`index.html names ${m[1]}, not an address relative to the page`);
  }
  let painted = null;
  const ctx2d = { putImageData(img) { painted = { data: Uint8Array.from(img.data), width: img.width, height: img.height }; } };
  const element = (tag) => ({
    tagName: tag.toUpperCase(), text: '', style: {}, children: [], value: '', dataset: {}, width: 640, height: 480,
    set textContent(v) { this.text = String(v); }, get textContent() { return this.text; },
    set innerHTML(_) { this.children = []; }, get innerHTML() { return ''; },
    appendChild(c) { this.children.push(c); return c; }, prepend(c) { this.children.unshift(c); },
    get lastChild() { const kids = this.children; return { remove() { kids.pop(); } }; },
    addEventListener() {}, getContext: () => ctx2d,
    getBoundingClientRect() { return { left: 0, top: 0, width: this.width, height: this.height }; },
  });
  const byId = new Map();
  for (const m of html.matchAll(/<(\w+)\b[^>]*\bid="([^"]+)"/g)) byId.set(m[2], element(m[1]));
  let ready;
  const loaded = new Promise((ok) => { ready = ok; });
  const intervals = [];
  const document = { title: '', getElementById: (id) => byId.get(id) ?? null, createElement: element,
    createTextNode: (t) => ({ textContent: t }), querySelectorAll: () => [] };
  const context = vm.createContext({
    document, location: { search: page.search, href: page.href }, fetch: pageFetch,
    WebAssembly, TextEncoder, TextDecoder, URLSearchParams, URL, performance, console,
    ImageData: class { constructor(data, width, height) { Object.assign(this, { data, width, height }); } },
    addEventListener() {}, setInterval: (_, ms) => { intervals.push(ms); return 0; },
    // The page asks for its first animation frame once it has loaded and drawn.
    requestAnimationFrame: () => ready(),
  });
  for (const m of html.matchAll(/<script\b[^>]*\bsrc="([^"]+)"/g)) {
    vm.runInContext(await (await pageFetch(m[1])).text(), context, { filename: m[1] });
  }
  const problems = () => byId.get('errors')?.textContent;
  const deadline = Date.now() + 60_000;
  let done = false;
  loaded.then(() => { done = true; });
  while (!done && !problems() && Date.now() < deadline) await new Promise((ok) => setTimeout(ok, 5));
  const text = (id) => byId.get(id)?.textContent;
  return { frame: text('frame'), hash: text('hash'), painted, problems: problems(), asked, intervals, title: document.title };
}

// ---- what the native command says
function rwgpu(args, cwd) {
  const run = spawnSync(native, [...args, '--json'], { cwd, maxBuffer: 1 << 26 });
  try { return JSON.parse(run.stdout.toString()); } catch (_) { throw new Error(`rwgpu ${args.join(' ')}: ${run.stderr}`); }
}
const png = join(mkdtempSync(join(tmpdir(), 'rwgpu-export-check-')), 'shot.png');
const shot = (args) => rwgpu(['shot', ...args, '--out', png], project);
const linked = shot(['--frame', '0']);
const checks = [{ name: 'linked', query: '', frame: 0, state: linked.state_hash, image: linked.image_hash }];
if (!openOnly) {
  const tested = rwgpu(['test', project]);
  if (!tested.ok) fail('rwgpu test failed on the project');
  for (const t of tested.tests) {
    const q = (frame) => `?input=${encodeURIComponent(t.path)}&frames=${frame}`;
    const last = shot(['--input', t.path, '--frame', String(t.frames)]);
    checks.push({ name: `${t.name}@${t.frames}`, query: q(t.frames), frame: t.frames, state: t.state_hash, image: last.image_hash });
    for (const s of t.shots) {
      if (s.frame === t.frames && s.image_hash !== last.image_hash) fail(`${t.name}: rwgpu test and rwgpu shot differ`);
      if (s.frame !== t.frames) checks.push({ name: `${t.name}@${s.frame}`, query: q(s.frame), frame: s.frame, state: null, image: s.image_hash });
    }
  }
}

const expectedName = JSON.parse(readFileSync(join(folder, 'project.json'), 'utf8')).name;
const askedAll = new Set();
for (const c of checks) {
  const got = await openPage(c.query);
  got.asked.forEach((a) => askedAll.add(a));
  const image = got.painted ? fnv64(got.painted.data) : '-';
  const same = got.frame === String(c.frame) && (c.state === null || got.hash === c.state) && image === c.image
    && !got.problems && got.intervals.length === 0 && got.title === expectedName;
  if (!same) failed = true;
  console.log(`${c.name} frame=${got.frame} state=${got.hash} image=${image} ${same ? 'SAME' : 'DIFFERENT'}`
    + (same ? '' : ` (expected frame ${c.frame} state ${c.state ?? 'any'} image ${c.image}; title "${got.title}";`
      + ` polls ${got.intervals.length}; problems: ${got.problems || 'none'})`));
}
await site.close();

// ---- every request inside the folder, answered, and every file of the folder used
for (const a of answered) if (a.status !== 200) fail(`${a.path} was answered ${a.status}`);
const files = readdirSync(folder).sort();
const unused = files.filter((f) => !askedAll.has(f));
if (unused.length) fail(`the page never asked for ${unused.join(', ')}`);
console.log(`requests: ${answered.length}, all under ${base}, all answered 200; files asked for: ${[...askedAll].sort().join(', ')}`
  + (wasmType === 'application/wasm' ? '' : ` (.wasm sent as ${wasmType})`));

// ---- sizes, plain and gzipped (gzip -9), by part
if (exportJson) {
  const parts = {};
  for (const f of JSON.parse(readFileSync(exportJson, 'utf8')).files) {
    const bytes = readFileSync(join(folder, f.path));
    const p = (parts[f.part] ??= { bytes: 0, gzip: 0 });
    p.bytes += bytes.length;
    p.gzip += gzipSync(bytes, { level: constants.Z_BEST_COMPRESSION }).length;
  }
  const total = Object.values(parts).reduce((a, p) => ({ bytes: a.bytes + p.bytes, gzip: a.gzip + p.gzip }), { bytes: 0, gzip: 0 });
  console.log(`sizes: ${Object.entries(parts).map(([k, p]) => `${k} ${p.bytes} (gzip -9 ${p.gzip})`).join(', ')}, total ${total.bytes} (gzip -9 ${total.gzip})`);
}
if (failed) process.exit(1);
