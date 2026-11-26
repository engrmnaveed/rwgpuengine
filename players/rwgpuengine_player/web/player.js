// rwgpu web player: the page around the player module (ADR 0024).
//
// The module (rwgpu_web_bg.wasm, with the glue rwgpu_web.js that the
// `wasm-bindgen` tool writes) is rwgpuengine_player's `web` module: the
// engine's session, the native player's bindings, clock and fit, and two ways
// to show the frame: WebGPU, or the CPU reference renderer where the browser
// has no WebGPU, gives no adapter, or loses the device. This script only
// hands the module the canvas, the events and the animation frames, and puts
// on the screen what it gives back. No framework, no build step.
import init, * as rw from './rwgpu_web.js';

const PIXELS = 1, FINISHED = 2, LOST = 4, DREW = 8;
const PAD = { left: 'KeyA', up: 'KeyW', down: 'KeyS', right: 'KeyD' };

const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);

let wasm = null;          // the module's exports (its memory)
let canvas = null;
let ctx = null;           // the 2D context, on the CPU path
let resizer = null;
let started = false;
let paused = false;
let pausedByError = false; // resume automatically once the files are fixed
let lastEventFrame = 0;
let bundleHash = '';
let finished = null;      // resolves a real-time replay of the checks

// What the page did, for people and for `cargo xtask gpu`'s browser check.
const page = { path: null, adapter: '', fallback: '', firstFrameMs: null, error: null };
window.rwgpu = page;

function showProblems(text) {
  const box = $('errors');
  const lines = [];
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

// One unobtrusive line says how the frame is drawn.
function showPath() {
  $('renderer').textContent = page.path === 'webgpu'
    ? `WebGPU · ${page.adapter}`
    : `CPU reference renderer · ${page.fallback}`;
}

// A problem from the module comes as rwgpu's JSON; the line wants its sentence.
function reason(e) {
  const text = String(e && e.message || e);
  try {
    const d = JSON.parse(text).diagnostics[0];
    return `${d.message} [${d.code}]`;
  } catch (_) { return text; }
}

// The canvas covers this many of its own pixels: device pixels on WebGPU,
// so the frame is scaled once, nearest pixel, as in the native window.
function watchSize() {
  resizer = new ResizeObserver(([entry]) => {
    const box = entry.devicePixelContentBoxSize && entry.devicePixelContentBoxSize[0];
    const w = box ? box.inlineSize : Math.round(entry.contentRect.width * devicePixelRatio);
    const h = box ? box.blockSize : Math.round(entry.contentRect.height * devicePixelRatio);
    if (w > 0 && h > 0 && page.path === 'webgpu') {
      canvas.width = w;
      canvas.height = h;
      rw.resize(w, h);
    }
  });
  try { resizer.observe(canvas, { box: 'device-pixel-content-box' }); } catch (_) { resizer.observe(canvas); }
}

// From here on the CPU reference renderer draws, on a 2D canvas of the
// frame's size that CSS scales with nearest pixels. A canvas that had a
// WebGPU context cannot give a 2D one, so it is replaced.
function useCpu(why) {
  if (resizer) { resizer.disconnect(); resizer = null; }
  if (page.path === 'webgpu' || canvas.dataset.gpu) {
    const fresh = canvas.cloneNode(false);
    canvas.replaceWith(fresh);
    canvas = fresh;
    wire(canvas);
  }
  ctx = canvas.getContext('2d');
  page.path = 'cpu';
  page.fallback = why;
  fitCanvas();
  showPath();
}

function fitCanvas() {
  const w = rw.width(), h = rw.height();
  if (!w) return;
  canvas.style.aspectRatio = `${w} / ${h}`;
  if (page.path === 'cpu' && (canvas.width !== w || canvas.height !== h)) { canvas.width = w; canvas.height = h; }
}

function putPixels() {
  const w = rw.width(), h = rw.height();
  const px = new Uint8ClampedArray(wasm.memory.buffer, rw.pixels(), w * h * 4);
  ctx.putImageData(new ImageData(px, w, h), 0, 0);
}

// What a call that steps or draws asks the page to do.
function handle(flags) {
  if (flags & LOST) useCpu(rw.fallback());
  if (flags & PIXELS) putPixels();
  if ((flags & DREW) && page.firstFrameMs === null) page.firstFrameMs = performance.now();
  if ((flags & FINISHED) && finished) { const done = finished; finished = null; setPaused(true); done(); }
}

function status() {
  $('frame').textContent = rw.frame();
  $('hash').textContent = rw.state_hash();
  const events = JSON.parse(rw.events(lastEventFrame) || '[]');
  if (!events.length) return;
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

async function loadBundle() {
  const text = await (await fetch('bundle', { cache: 'no-store' })).text();
  if (!rw.set_bundle(text)) { showProblems(rw.out()); return false; }
  return true;
}

function start() {
  const ok = rw.start($('scene').value, $('input').value);
  showProblems(rw.out());
  $('events').innerHTML = '';
  lastEventFrame = 0;
  started = ok;
  if (!ok) { pausedByError = !paused; setPaused(true); return false; }
  if (pausedByError) { pausedByError = false; setPaused(false); }
  fitCanvas();
  rw.resume();
  return true;
}

function setPaused(p) {
  if (paused && !p) rw.resume();
  paused = p;
  $('play').textContent = paused ? 'Play' : 'Pause';
}

function restart(fromUrl) {
  if (!start()) return;
  const frames = fromUrl ? parseInt(params.get('frames') || '', 10) : NaN;
  if (Number.isFinite(frames)) {
    handle(rw.replay(frames));
    setPaused(true);
    $('note').textContent = `ran ${frames} frames`;
  } else {
    $('note').textContent = '';
  }
  status();
}

function fill(select, values, chosen) {
  for (const v of values) {
    const o = document.createElement('option');
    o.value = o.textContent = v;
    select.appendChild(o);
  }
  if (chosen != null) select.value = chosen;
}

// Canvas pixels of a pointer event: the module takes them to frame pixels.
function at(e) {
  const r = canvas.getBoundingClientRect();
  return [(e.clientX - r.left) * canvas.width / r.width, (e.clientY - r.top) * canvas.height / r.height];
}

function stepped(flags) {
  handle(flags);
  status();
}

function wire(c) {
  const touch = (e) => e.pointerType === 'touch';
  c.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    c.setPointerCapture(e.pointerId);
    rw.shift(e.shiftKey);
    rw.pointer_down(e.pointerId, touch(e), e.button, ...at(e));
    if (paused) stepped(rw.step_once());
  });
  c.addEventListener('pointermove', (e) => rw.pointer_move(e.pointerId, touch(e), ...at(e)));
  c.addEventListener('pointerup', (e) => {
    rw.pointer_up(e.pointerId, touch(e), e.button, ...at(e), false);
    if (paused) stepped(rw.step_once());
  });
  c.addEventListener('pointercancel', (e) => rw.pointer_up(e.pointerId, touch(e), e.button, ...at(e), true));
  c.addEventListener('contextmenu', (e) => e.preventDefault());
  c.addEventListener('wheel', (e) => { e.preventDefault(); rw.wheel(e.deltaMode, e.deltaY, e.ctrlKey); }, { passive: false });
}

// For the checks (`cargo xtask gpu`): play a playtest and say where it ended.
// `realtime` plays it at the project's rate through the animation frames, as
// a person sees it; otherwise as fast as the steps go, drawing every frame
// whose picture changed.
page.play = async (input, { realtime = false } = {}) => {
  $('input').value = input;
  if (!start()) throw new Error(rw.out());
  const frames = rw.script_frames();
  const t0 = performance.now();
  if (realtime) {
    await new Promise((done) => { finished = done; rw.set_end(frames); setPaused(false); });
  } else {
    handle(rw.replay(frames));
  }
  // A browser reports a lost device later, in a promise: let a few animation frames pass (the loop goes
  // on ticking a finished session, and falls back if it must) before saying which path drew.
  for (let i = 0; i < 3; i++) await new Promise((ok) => requestAnimationFrame(ok));
  status();
  return {
    frame: rw.frame(), width: rw.width(), height: rw.height(), state: rw.state_hash(), image: rw.image_hash(),
    path: page.path, fallback: page.fallback, ms: performance.now() - t0, stats: JSON.parse(rw.stats()),
  };
};

// The frame the GPU drew, as RGBA bytes in base64; or, on the CPU path, the
// pixels the canvas shows, read back from it.
page.picture = async () => {
  let bytes;
  if (page.path === 'webgpu') {
    if (!rw.copy_frame()) throw new Error('nothing was drawn');
    try {
      while (!(bytes = rw.copied())) await new Promise((r) => setTimeout(r, 0));
    } catch (e) {
      throw new Error(`the frame did not come back from the GPU: ${e && e.message || e}; ${rw.stats()}`);
    }
  } else {
    bytes = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
  }
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return btoa(s);
};

page.stats = () => JSON.parse(rw.stats());

// Play a playtest to `at`, lose the GPU device as a browser can, and play on
// to the end: the CPU reference renderer takes over and the game goes on.
page.playLosing = async (input, at) => {
  $('input').value = input;
  if (!start()) throw new Error(rw.out());
  const frames = rw.script_frames();
  handle(rw.replay(at));
  const before = page.path;
  rw.lose_device();
  handle(rw.replay(frames));
  status();
  return { before, frame: rw.frame(), state: rw.state_hash(), image: rw.image_hash(), path: page.path, fallback: page.fallback };
};

// Play the scene live for `ms`: with `turn`, a right-drag turns the camera a
// little at every animation frame. Returns what the player did meanwhile.
let onFrame = null;
page.live = async (ms, { turn = false } = {}) => {
  $('input').value = '';
  if (!start()) throw new Error(rw.out());
  const r = canvas.getBoundingClientRect();
  let x = canvas.width / 2;
  const y = canvas.height / 2;
  if (turn) {
    rw.pointer_down(1, false, 2, x, y);
    onFrame = () => { x += 2 * canvas.width / r.width; rw.pointer_move(1, false, x, y); };
  }
  setPaused(false);
  const t0 = performance.now(), f0 = rw.frame();
  await new Promise((done) => setTimeout(done, ms));
  onFrame = null;
  if (turn) rw.pointer_up(1, false, 2, x, y, false);
  setPaused(true);
  return { ms: performance.now() - t0, frames: rw.frame() - f0, stats: JSON.parse(rw.stats()) };
};

async function main() {
  wasm = await init({ module_or_path: new URL('rwgpu_web_bg.wasm', import.meta.url) });
  canvas = $('screen');
  wire(canvas);
  const want = params.get('renderer');
  if (want === 'cpu') {
    rw.use_cpu('asked for with ?renderer=cpu');
  } else if (!navigator.gpu) {
    rw.use_cpu('this browser has no WebGPU');
  } else {
    try {
      canvas.dataset.gpu = '1';
      page.adapter = await rw.gpu_open(canvas, want === 'software');
      page.path = 'webgpu';
      // The browser names the adapter it gives for the same request.
      const a = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance', forceFallbackAdapter: want === 'software' });
      const info = a && a.info;
      const name = info && [info.vendor, info.architecture, info.description].filter((s) => s).join(' ');
      if (name) page.adapter = `${name}${info.isFallbackAdapter ? ' (software)' : ''}`;
    } catch (e) {
      rw.use_cpu(reason(e));
    }
  }
  if (page.path === 'webgpu') { showPath(); watchSize(); } else { useCpu(rw.fallback()); }

  const project = await (await fetch('project', { cache: 'no-store' })).json();
  $('project').textContent = `${project.name || '?'} · rwgpu ${project.version}`;
  fill($('scene'), project.scenes, params.get('scene') || project.main_scene);
  fill($('input'), project.tests, params.get('input') || '');
  bundleHash = await (await fetch('bundle-hash', { cache: 'no-store' })).text();
  if (await loadBundle()) restart(true);

  $('scene').onchange = () => restart(false);
  $('input').onchange = () => restart(false);
  $('play').onclick = () => setPaused(!paused);
  $('step').onclick = () => { setPaused(true); stepped(rw.step_once()); };
  $('restart').onclick = () => restart(false);

  const typing = (e) => ['SELECT', 'INPUT', 'TEXTAREA'].includes(e.target.tagName);
  addEventListener('keydown', (e) => {
    if (typing(e)) return;
    rw.shift(e.shiftKey);
    if (rw.key(e.code, true, e.repeat)) e.preventDefault();
  });
  addEventListener('keyup', (e) => { rw.shift(e.shiftKey); rw.key(e.code, false, false); });
  addEventListener('blur', () => rw.blur());

  for (const b of document.querySelectorAll('.pad button')) {
    const code = PAD[b.dataset.action];
    b.addEventListener('pointerdown', (e) => { e.preventDefault(); rw.key(code, true, false); });
    for (const ev of ['pointerup', 'pointerleave', 'pointercancel']) b.addEventListener(ev, () => rw.key(code, false, false));
  }

  // Data edits reload at once: poll the bundle hash.
  setInterval(async () => {
    try {
      const h = await (await fetch('bundle-hash', { cache: 'no-store' })).text();
      if (h !== bundleHash) {
        bundleHash = h;
        if (await loadBundle()) { restart(false); $('note').textContent = 'reloaded'; }
      }
    } catch (_) { /* the server stopped */ }
  }, 1000);

  // The module steps at the project's rate whatever the display does, and
  // draws only when the picture changed. A hidden tab gets no animation
  // frames, so it costs nothing; on return at most five steps are caught up.
  const loop = (now) => {
    if (onFrame) onFrame();
    if (started && !paused) stepped(rw.tick(now));
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
  page.ready = true;
}

main().catch((e) => { page.error = String(e && e.stack || e); showProblems(page.error); });
