// slate web player: glue between the page and slate_web.wasm.
//
// The wasm module exports a small C ABI (see crates/slate_web/src/lib.rs):
// strings go in as (pointer, length) into linear memory, text comes back
// through slate_out_ptr/slate_out_len. No wasm-bindgen, no build step.
'use strict';

const ACTION = { up: 1, down: 2, left: 4, right: 8, confirm: 16, cancel: 32 };
const KEYS = {
  ArrowUp: 'up', KeyW: 'up', ArrowDown: 'down', KeyS: 'down',
  ArrowLeft: 'left', KeyA: 'left', ArrowRight: 'right', KeyD: 'right',
  Enter: 'confirm', Space: 'confirm', Escape: 'cancel',
};

const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
const enc = new TextEncoder();
const dec = new TextDecoder();

let w = null;              // wasm exports
let ctx = null;
let paused = false;
let pausedByError = false;  // resume automatically once the files are fixed
let pressed = 0;           // actions pressed since the last step
const held = new Set();    // actions currently held
const taps = [];           // queued taps, in frame pixels
let lastEventFrame = 0;
let bundleHash = '';
let stepMs = 1000 / 60;

function withStr(s, fn) {
  const bytes = enc.encode(s);
  const ptr = w.slate_alloc(bytes.length);
  new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);
  try { return fn(ptr, bytes.length); } finally { w.slate_free(ptr, bytes.length); }
}

function out() {
  return dec.decode(new Uint8Array(w.memory.buffer, w.slate_out_ptr(), w.slate_out_len()));
}

function showProblems(text) {
  const box = $('errors');
  let lines = [];
  try {
    const j = JSON.parse(text);
    for (const d of j.diagnostics || []) {
      const loc = d.file ? `${d.file}${d.line ? ':' + d.line + ':' + d.column : ''}` : '';
      lines.push(`${d.severity}[${d.code}]: ${d.message}${loc ? '\n  --> ' + loc : ''}${d.hint ? '\n  = help: ' + d.hint : ''}`);
    }
    if (j.error) lines.push(j.error);
  } catch (_) { if (text) lines.push(text); }
  box.textContent = lines.join('\n\n');
  box.style.display = lines.length ? 'block' : 'none';
  $('noerrors').style.display = lines.length ? 'none' : 'block';
}

async function loadBundle() {
  const res = await fetch('/bundle', { cache: 'no-store' });
  const text = await res.text();
  const status = withStr(text, (p, n) => w.slate_set_bundle(p, n));
  if (status !== 0) { showProblems(out()); return false; }
  return true;
}

function start() {
  const scene = $('scene').value;
  const input = $('input').value;
  const status = withStr(scene, (sp, sn) => withStr(input, (ip, il) => w.slate_start(sp, sn, ip, il)));
  showProblems(out());
  $('events').innerHTML = '';
  lastEventFrame = 0;
  if (status !== 0) { pausedByError = !paused; setPaused(true); return false; }
  if (pausedByError) { pausedByError = false; setPaused(false); }
  const cw = w.slate_width(), ch = w.slate_height();
  const canvas = $('screen');
  if (canvas.width !== cw || canvas.height !== ch) { canvas.width = cw; canvas.height = ch; }
  stepMs = 1000 / w.slate_tick_rate();
  return true;
}

function step() {
  let down = pressed;
  for (const a of held) down |= ACTION[a];
  const tap = taps.shift();
  w.slate_step(pressed, down, tap ? 1 : 0, tap ? tap[0] : 0, tap ? tap[1] : 0);
  pressed = 0;
}

function draw() {
  const width = w.slate_width(), height = w.slate_height();
  if (!width) return;
  // Most frames change nothing: slate_present repaints only when the picture
  // did, and says so (it always does on the first frame after a start).
  if (w.slate_present()) {
    const pixels = new Uint8ClampedArray(w.memory.buffer, w.slate_pixels(), width * height * 4);
    ctx.putImageData(new ImageData(pixels, width, height), 0, 0);
  }
  $('frame').textContent = w.slate_frame();
  w.slate_hash();
  $('hash').textContent = out();
  w.slate_events(lastEventFrame);
  const events = JSON.parse(out() || '[]');
  if (events.length) {
    const list = $('events');
    for (const e of events) {
      const li = document.createElement('li');
      li.innerHTML = `<span class="f">[${e.frame}] ${e.kind}</span> `;
      li.appendChild(document.createTextNode(e.message));
      list.prepend(li);
    }
    while (list.children.length > 200) list.lastChild.remove();
    lastEventFrame = events[events.length - 1].frame + 1;
  }
}

function setPaused(p) {
  paused = p;
  $('play').textContent = paused ? 'Play' : 'Pause';
}

async function restart(fromUrl) {
  if (!start()) { draw(); return; }
  const frames = fromUrl ? parseInt(params.get('frames') || '', 10) : NaN;
  if (Number.isFinite(frames)) {
    w.slate_run_to(frames);
    setPaused(true);
    $('note').textContent = `ran ${frames} frames`;
  } else {
    $('note').textContent = '';
  }
  draw();
}

function fill(select, values, chosen) {
  for (const v of values) {
    const o = document.createElement('option');
    o.value = o.textContent = v;
    select.appendChild(o);
  }
  if (chosen != null) select.value = chosen;
}

async function main() {
  ctx = $('screen').getContext('2d');
  const { instance } = await WebAssembly.instantiateStreaming(fetch('/player.wasm'), {});
  w = instance.exports;
  const project = await (await fetch('/project', { cache: 'no-store' })).json();
  $('project').textContent = `${project.name || '?'} · slate ${project.version}`;
  fill($('scene'), project.scenes, params.get('scene') || project.main_scene);
  fill($('input'), project.tests, params.get('input') || '');
  bundleHash = await (await fetch('/bundle-hash', { cache: 'no-store' })).text();
  if (await loadBundle()) await restart(true);

  $('scene').onchange = () => restart(false);
  $('input').onchange = () => restart(false);
  $('play').onclick = () => setPaused(!paused);
  $('step').onclick = () => { setPaused(true); step(); draw(); };
  $('restart').onclick = () => restart(false);

  addEventListener('keydown', (e) => {
    const a = KEYS[e.code];
    if (!a || e.target.tagName === 'SELECT') return;
    e.preventDefault();
    if (!e.repeat) pressed |= ACTION[a];
    held.add(a);
  });
  addEventListener('keyup', (e) => { const a = KEYS[e.code]; if (a) held.delete(a); });

  for (const b of document.querySelectorAll('.pad button')) {
    const a = b.dataset.action;
    b.addEventListener('pointerdown', (e) => { e.preventDefault(); pressed |= ACTION[a]; held.add(a); });
    for (const ev of ['pointerup', 'pointerleave', 'pointercancel']) b.addEventListener(ev, () => held.delete(a));
  }

  // Touch first: a short touch is a tap, a drag is a swipe. Swipes map to
  // the iso axes: up-right = up (-y), down-left = down (+y),
  // up-left = left (-x), down-right = right (+x).
  const canvas = $('screen');
  let start0 = null;
  const toFrame = (e) => {
    const r = canvas.getBoundingClientRect();
    return [(e.clientX - r.left) * canvas.width / r.width, (e.clientY - r.top) * canvas.height / r.height];
  };
  canvas.addEventListener('pointerdown', (e) => { start0 = toFrame(e); });
  canvas.addEventListener('pointerup', (e) => {
    if (!start0) return;
    const [x0, y0] = start0, [x1, y1] = toFrame(e);
    start0 = null;
    const dx = x1 - x0, dy = y1 - y0;
    if (Math.hypot(dx, dy) < 12) { taps.push([Math.floor(x0), Math.floor(y0)]); if (paused) { step(); draw(); } return; }
    const a = dy < 0 ? (dx > 0 ? 'up' : 'left') : (dx > 0 ? 'right' : 'down');
    pressed |= ACTION[a];
    if (paused) { step(); draw(); }
  });

  // Data edits reload instantly: poll the bundle hash.
  setInterval(async () => {
    try {
      const h = await (await fetch('/bundle-hash', { cache: 'no-store' })).text();
      if (h !== bundleHash) {
        bundleHash = h;
        if (await loadBundle()) { await restart(false); $('note').textContent = 'reloaded'; }
      }
    } catch (_) { /* server stopped */ }
  }, 1000);

  let last = performance.now(), acc = 0;
  const loop = (now) => {
    acc += Math.min(now - last, 250);
    last = now;
    if (!paused && w.slate_width()) {
      let n = 0;
      while (acc >= stepMs && n < 5) { step(); acc -= stepMs; n++; }
      draw();
    } else {
      acc = 0;
    }
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
}

main().catch((e) => showProblems(String(e && e.stack || e)));
