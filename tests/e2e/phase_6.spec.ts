// Phase 6: the art pass renders within the budgets of plan section 9.
// No GPU here, so the test enforces the proxies: meshes drawn per frame (an
// upper bound on draw calls), triangles on screen, and main-thread CPU time
// per frame. Screenshots of each room are attached to the report.
import { expect, test, type Page } from '@playwright/test';
import { status, waitFor, walkToOnHost } from './helpers';

const BUDGET = { drawCalls: 400, triangles: 250_000, frameCpuMs: 8 };

/**
 * Main-thread CPU per frame (Chromium): from a Chrome trace, the thread CPU
 * time (`tdur`) of each animation-frame callback over three seconds. Wall
 * time would also count waits on SwiftShader, the software GPU, and the CPU
 * profiler would switch wasm to its slow debugging tier.
 */
async function cpuPerFrame(page: Page) {
  const browser = page.context().browser()!;
  await browser.startTracing(page, { categories: ['devtools.timeline'] });
  await page.waitForTimeout(3000);
  const trace = JSON.parse((await browser.stopTracing()).toString());
  const frames = (trace.traceEvents as any[]).filter((e) => e.name === 'FireAnimationFrame' && e.ph === 'X' && e.tdur != null);
  const cpu = frames.map((e) => e.tdur / 1000).sort((a, b) => a - b);
  const wall = frames.map((e) => e.dur / 1000).sort((a, b) => a - b);
  const pick = (v: number[], q: number) => v[Math.min(v.length - 1, Math.floor(v.length * q))] ?? 0;
  return { cpuMs: pick(cpu, 0.5), cpuP95: pick(cpu, 0.95), wallMs: pick(wall, 0.5), traced: frames.length };
}

async function frameStats(page: Page) {
  await page.waitForTimeout(2000);
  return page.evaluate(() => {
    const f = [...((window as any).__frameCpu ?? [])].slice(-120).sort((a: number, b: number) => a - b);
    const s = (window as any).__lastCall ?? {};
    return {
      meshes: s.visibleMeshes as number,
      triangles: s.triangles as number,
      p50: f[Math.floor(f.length * 0.5)] as number,
      p95: f[Math.floor(f.length * 0.95)] as number,
      frames: f.length,
    };
  });
}

test('every room draws within the budgets', async ({ browser, browserName }) => {
  test.setTimeout(300_000);
  // A drawing tab (no ?nodraw). Small: there is no GPU here, and SwiftShader
  // rasterizes on the CPU; at 720p a frame waits seconds on it, which would
  // measure the software rasterizer, not the game. Draw calls and triangles
  // do not depend on the size.
  const [vw, vh] = (process.env.VIEW ?? '320x180').split('x').map(Number);
  const context = await browser.newContext({ viewport: { width: vw, height: vh } });
  const host = await context.newPage();
  // Low preset for the walk: with no GPU, SwiftShader needs about 250 ms to
  // draw a frame with bloom and moon shadows (GPU time, which this machine
  // cannot measure). Draw calls, triangles and the game's own CPU time per
  // frame are the same in both presets. High is switched on at the end for
  // the screenshots.
  await host.addInitScript(() => localStorage.setItem('lastcall-settings', JSON.stringify({ graphics: 'low' })));
  host.on('console', (m) => m.type() === 'error' && console.log(`[host] ${m.text().slice(0, 300)}`));
  await host.goto('/?create&gpu=webgl2&novoice&name=Host&chaos=off');
  await waitFor(host, 'joined', (s) => !!s.playerId && (s.propsSeen ?? 0) > 100, 90_000);
  // Warm up: SwiftShader compiles each new shader pipeline on first use.
  for (let i = 0; i < 3; i++) console.log('warming', JSON.stringify(await frameStats(host)));
  const stops: [string, [number, number][]][] = [
    ['bar', [[0, 3.4]]],
    ['office', [[8.2, 3.0], [8.2, -1.0], [8.2, -3.0], [8.5, -5.0]]],
    ['roof', [[8.2, 3.4], [11.5, 3.4], [16, 2]]],
    ['lot', [[11.5, 3.4], [8.2, 3.4], [0, 3.4], [0, 8.5], [0, 15]]],
    ['pier', [[0, 26], [0, 40]]],
  ];
  const worst = { meshes: 0, triangles: 0, p50: 0, p95: 0, cpuMs: 0 };
  for (const [room, route] of stops) {
    await walkToOnHost(host, route);
    const s: any = await frameStats(host);
    if (browserName === 'chromium') Object.assign(s, await cpuPerFrame(host));
    console.log(room, JSON.stringify(s));
    test.info().annotations.push({ type: room, description: JSON.stringify(s) });
    await test.info().attach(`${room}.png`, { body: await host.screenshot(), contentType: 'image/png' });
    worst.meshes = Math.max(worst.meshes, s.meshes);
    worst.triangles = Math.max(worst.triangles, s.triangles);
    worst.p50 = Math.max(worst.p50, s.p50);
    worst.cpuMs = Math.max(worst.cpuMs, s.cpuMs ?? 0);
    worst.p95 = Math.max(worst.p95, s.p95);
  }
  test.info().annotations.push({ type: 'worst', description: JSON.stringify(worst) });
  console.log('worst', JSON.stringify(worst));
  // The High preset, for the record: the pier with bloom and moon shadows.
  await host.evaluate(() => ((window as any).__lcSettings = { ...(window as any).__lcSettings, graphics: 'high' }));
  await host.waitForTimeout(8000);
  await test.info().attach('pier-high.png', { body: await host.screenshot(), contentType: 'image/png' });
  expect(worst.meshes).toBeLessThanOrEqual(BUDGET.drawCalls);
  expect(worst.triangles).toBeLessThanOrEqual(BUDGET.triangles);
  // Main-thread CPU per frame is checked in Chromium (the performance
  // reference, and the only browser with the CPU profiler protocol).
  if (browserName === 'chromium') expect(worst.cpuMs).toBeLessThanOrEqual(BUDGET.frameCpuMs);
  expect((await status(host)).error).toBeUndefined();
  await context.close();
});
