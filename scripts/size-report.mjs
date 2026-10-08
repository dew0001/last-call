// Report raw and Brotli sizes of every file in a build directory.
// With --enforce, fail when a budget from section 9 of the plan is missed:
//   any single file >= 25 MiB raw (Cloudflare Pages limit)
//   each client wasm > 12 MB Brotli
//   first load (one client variant + host + everything else) > 40 MB Brotli
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { brotliCompressSync, constants } from 'node:zlib';

const dir = process.argv[2] ?? 'dist';
const enforce = process.argv.includes('--enforce');
const MiB = 1024 * 1024;
const MB = 1000 * 1000;

function walk(d) {
  return readdirSync(d).flatMap((n) => {
    const p = join(d, n);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

const rows = walk(dir).map((p) => {
  const buf = readFileSync(p);
  const br = brotliCompressSync(buf, { params: { [constants.BROTLI_PARAM_QUALITY]: 9 } }).length;
  return { file: relative(dir, p), raw: buf.length, br };
});
rows.sort((a, b) => b.raw - a.raw);
for (const r of rows) {
  console.log(`${r.file.padEnd(40)} ${String(r.raw).padStart(11)} raw ${String(r.br).padStart(11)} brotli`);
}

const errors = [];
for (const r of rows) {
  if (r.raw >= 25 * MiB) errors.push(`${r.file} is ${r.raw} bytes, over the 25 MiB Pages file limit`);
  if (/client_web(gl2|gpu)_bg\.wasm$/.test(r.file) && r.br > 12 * MB) errors.push(`${r.file} is ${r.br} bytes Brotli, over 12 MB`);
}
const other = rows.filter((r) => !/client_webgpu/.test(r.file)).reduce((s, r) => s + r.br, 0);
console.log(`first load (webgl2 variant, Brotli): ${(other / MB).toFixed(2)} MB`);
if (other > 40 * MB) errors.push(`first load ${other} bytes Brotli, over 40 MB`);

if (enforce && errors.length) {
  for (const e of errors) console.error(`error: ${e}`);
  process.exit(1);
}
