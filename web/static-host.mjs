// A plain static web host, for the checks of `rwgpu export web` (ADR 0025):
// the files of one folder under one path, by name, with the Content-Type a
// host sends for each extension, and nothing else. No index of folders, no
// rewriting, no compression, no cache header. Every request is recorded.
//
//   const site = await host(folder, '/rwgpuengine/', { wasmType });
//   site.url        http://127.0.0.1:<port>/rwgpuengine/
//   site.answered   [{ path, status }] in order
//   site.unused()   the folder's files no request asked for
//   site.close()
//
// `web/export-check.mjs` (the CPU player, under Node, in `cargo xtask ci`) and
// `players/rwgpuengine_player/web/check.mjs` (the WebGPU player, in a browser,
// in `cargo xtask gpu`) both serve the exported folder with it.
import { createServer } from 'node:http';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { extname, join } from 'node:path';

export async function host(folder, base, { wasmType = 'application/wasm' } = {}) {
  if (!base.startsWith('/') || !base.endsWith('/')) throw new Error(`a base path starts and ends with /: ${base}`);
  const types = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.wasm': wasmType,
    '.json': 'application/json', '.txt': 'text/plain; charset=utf-8' };
  const answered = [];
  const server = createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, 'http://host').pathname);
    let status = 404, type = 'text/plain', body = Buffer.from('not found');
    if (path.startsWith(base)) {
      const rel = path.slice(base.length) || 'index.html';
      const file = join(folder, rel);
      if (!rel.split('/').includes('..') && existsSync(file) && statSync(file).isFile()) {
        status = 200; type = types[extname(file)] ?? 'application/octet-stream'; body = readFileSync(file);
      }
    }
    answered.push({ path, status });
    res.writeHead(status, { 'content-type': type, 'content-length': body.length });
    res.end(body);
  });
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  const origin = `http://127.0.0.1:${server.address().port}`;
  return {
    origin,
    url: origin + base,
    answered,
    unused: () => {
      const asked = new Set(answered.filter((a) => a.status === 200).map((a) => a.path.slice(base.length) || 'index.html'));
      return readdirSync(folder).sort().filter((f) => !asked.has(f));
    },
    close: () => { server.closeAllConnections?.(); return new Promise((ok) => server.close(ok)); },
  };
}
