// Headless check: run rwgpuengine_web.wasm under Node (V8) and print state and
// image hashes for every playtest of an example, plus render timing.
// usage: node wasm_check.mjs <wasm> <example dir> [--bench] [--native <rwgpu binary>]
//
// `image` is rwgpu_image_hash. On a build with the paint path (ADR 0018)
// `shown` is the hash of the pixels a page gets from rwgpu_present and
// rwgpu_render, computed here. `--native` runs `rwgpu shot` for the same
// playtest and frame and compares its state hash and its image hash (the
// reference rasterizer's) with the wasm's: the real native-equals-web check.
// `--bench` times rwgpu_render: rasterize + report before ADR 0018, paint
// after. Exits 1 if anything differs.
import { readFileSync, readdirSync, statSync, mkdtempSync } from 'node:fs';
import { join, relative, sep, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';

const [wasmPath, dir, ...flags] = process.argv.slice(2);
const flag = flags.includes('--bench') ? '--bench' : '';
const native = flags.includes('--native') ? resolve(flags[flags.indexOf('--native') + 1]) : null;
let failed = false;
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const w = instance.exports;
const enc = new TextEncoder();
const dec = new TextDecoder();

function withStr(s, fn) {
  const bytes = enc.encode(s);
  const ptr = w.rwgpu_alloc(bytes.length);
  new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);
  try { return fn(ptr, bytes.length); } finally { w.rwgpu_free(ptr, bytes.length); }
}
const out = () => dec.decode(new Uint8Array(w.memory.buffer, w.rwgpu_out_ptr(), w.rwgpu_out_len()));

// Same walk as Vfs::from_dir: .toml files, skipping hidden folders, target/ and out/.
const files = [];
(function walk(d) {
  for (const name of readdirSync(d).sort()) {
    const p = join(d, name);
    if (statSync(p).isDirectory()) {
      if (name.startsWith('.') || name === 'target' || name === 'out') continue;
      walk(p);
    } else if (name.endsWith('.toml')) {
      files.push([relative(dir, p).split(sep).join('/'), readFileSync(p, 'utf8')]);
    }
  }
})(dir);
files.sort((a, b) => (a[0] < b[0] ? -1 : 1));
let bundle = 'rwgpu-bundle 1\n';
for (const [p, t] of files) bundle += p + '\0' + t + '\0';
if (withStr(bundle, (p, n) => w.rwgpu_set_bundle(p, n)) !== 0) throw new Error(out());

const tests = files.map((f) => f[0]).filter((p) => p.startsWith('tests/'));
for (const t of tests) {
  const status = withStr('', (sp, sn) => withStr(t, (ip, il) => w.rwgpu_start(sp, sn, ip, il)));
  if (status !== 0) { console.log(`${t}: START FAILED ${out()}`); continue; }
  const frames = w.rwgpu_script_frames();
  w.rwgpu_run_to(frames);
  w.rwgpu_hash();
  const state = out();
  w.rwgpu_image_hash();
  const image = out();
  let line = `${t.replace('tests/', '').replace('.toml', '')} frames=${frames} state=${state} image=${image}`;
  if (w.rwgpu_present) {
    // FNV-1a 64 of what rwgpu_present / rwgpu_render hand to the page.
    const fnv = (ptr) => {
      const px = new Uint8Array(w.memory.buffer, ptr, w.rwgpu_width() * w.rwgpu_height() * 4);
      let h = 0xcbf29ce484222325n;
      for (const b of px) h = ((h ^ BigInt(b)) * 0x100000001b3n) & 0xffffffffffffffffn;
      return h.toString(16).padStart(16, '0');
    };
    const first = w.rwgpu_present();
    const presented = fnv(w.rwgpu_pixels());
    const again = w.rwgpu_present();
    const rendered = fnv(w.rwgpu_render());
    const ok = first === 1 && again === 0 && presented === image && rendered === image;
    failed ||= !ok;
    line += ` shown=${presented} ${ok ? 'SAME' : 'DIFFERENT'}`;
  }
  if (native) {
    const png = join(mkdtempSync(join(tmpdir(), 'rwgpu-check-')), 'shot.png');
    const run = spawnSync(native, ['shot', '--input', t, '--frame', String(frames), '--out', png, '--json'], { cwd: dir });
    let j = {};
    try { j = JSON.parse(run.stdout.toString()); } catch (_) { /* reported below */ }
    const ok = j.state_hash === state && j.image_hash === image;
    failed ||= !ok;
    line += ok ? ' native=EQUAL' : ` native=DIFFERENT (state ${j.state_hash} image ${j.image_hash})`;
  }
  if (flag === '--bench') {
    for (let i = 0; i < 30; i++) w.rwgpu_render();
    const batches = [];
    for (let b = 0; b < 5; b++) {
      const t0 = performance.now();
      for (let i = 0; i < 100; i++) w.rwgpu_render();
      batches.push((performance.now() - t0) / 100);
    }
    batches.sort((a, b) => a - b);
    line += ` render_ms=${batches[2].toFixed(3)} (${w.rwgpu_width()}x${w.rwgpu_height()})`;
  }
  console.log(line);
}
if (failed) process.exit(1);
