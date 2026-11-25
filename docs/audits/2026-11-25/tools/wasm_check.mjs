// Headless check: run slate_web.wasm under Node (V8) and print state and
// image hashes for every playtest of an example, plus render timing.
// usage: node wasm_check.mjs <wasm> <example dir> [--bench]
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const [wasmPath, dir, flag] = process.argv.slice(2);
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const w = instance.exports;
const enc = new TextEncoder();
const dec = new TextDecoder();

function withStr(s, fn) {
  const bytes = enc.encode(s);
  const ptr = w.slate_alloc(bytes.length);
  new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);
  try { return fn(ptr, bytes.length); } finally { w.slate_free(ptr, bytes.length); }
}
const out = () => dec.decode(new Uint8Array(w.memory.buffer, w.slate_out_ptr(), w.slate_out_len()));

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
let bundle = 'slate-bundle 1\n';
for (const [p, t] of files) bundle += p + '\0' + t + '\0';
if (withStr(bundle, (p, n) => w.slate_set_bundle(p, n)) !== 0) throw new Error(out());

const tests = files.map((f) => f[0]).filter((p) => p.startsWith('tests/'));
for (const t of tests) {
  const status = withStr('', (sp, sn) => withStr(t, (ip, il) => w.slate_start(sp, sn, ip, il)));
  if (status !== 0) { console.log(`${t}: START FAILED ${out()}`); continue; }
  const frames = w.slate_script_frames();
  w.slate_run_to(frames);
  w.slate_hash();
  const state = out();
  w.slate_image_hash();
  const image = out();
  let line = `${t.replace('tests/', '').replace('.toml', '')} frames=${frames} state=${state} image=${image}`;
  if (flag === '--bench') {
    for (let i = 0; i < 30; i++) w.slate_render();
    const batches = [];
    for (let b = 0; b < 5; b++) {
      const t0 = performance.now();
      for (let i = 0; i < 100; i++) w.slate_render();
      batches.push((performance.now() - t0) / 100);
    }
    batches.sort((a, b) => a - b);
    line += ` render_ms=${batches[2].toFixed(3)} (${w.slate_width()}x${w.slate_height()})`;
  }
  console.log(line);
}
