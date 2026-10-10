// Phase 6: the art pass renders within the budgets of plan section 9.
// No GPU here, so the test enforces the proxies: meshes drawn per frame (an
// upper bound on draw calls), triangles on screen, and main-thread CPU time
// per frame. Screenshots of each room are attached to the report.
import { expect, test, type Page } from '@playwright/test';
import { status, waitFor, walkToOnHost } from './helpers';

const BUDGET = { drawCalls: 400, triangles: 250_000, frameCpuMs: 8 };

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
  // A drawing tab (no ?nodraw), at 720p.
  const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
  const host = await context.newPage();
  host.on('console', (m) => m.type() === 'error' && console.log(`[host] ${m.text().slice(0, 300)}`));
  await host.goto('/?create&gpu=webgl2&novoice&name=Host&chaos=off');
  await waitFor(host, 'joined', (s) => !!s.playerId && (s.propsSeen ?? 0) > 100, 90_000);
  const stops: [string, [number, number][]][] = [
    ['bar', [[0, 3.4]]],
    ['office', [[8.2, 3.0], [8.2, -1.0], [8.2, -3.0], [8.5, -5.0]]],
    ['roof', [[8.2, 3.4], [11.5, 3.4], [16, 2]]],
    ['lot', [[11.5, 3.4], [8.2, 3.4], [0, 3.4], [0, 8.5], [0, 15]]],
    ['pier', [[0, 26], [0, 40]]],
  ];
  const worst = { meshes: 0, triangles: 0, p95: 0 };
  for (const [room, route] of stops) {
    await walkToOnHost(host, route);
    const s = await frameStats(host);
    test.info().annotations.push({ type: room, description: JSON.stringify(s) });
    await test.info().attach(`${room}.png`, { body: await host.screenshot(), contentType: 'image/png' });
    worst.meshes = Math.max(worst.meshes, s.meshes);
    worst.triangles = Math.max(worst.triangles, s.triangles);
    worst.p95 = Math.max(worst.p95, s.p95);
  }
  test.info().annotations.push({ type: 'worst', description: JSON.stringify(worst) });
  expect(worst.meshes).toBeLessThanOrEqual(BUDGET.drawCalls);
  expect(worst.triangles).toBeLessThanOrEqual(BUDGET.triangles);
  // Frame CPU time is checked in Chromium, the performance reference.
  if (browserName === 'chromium') expect(worst.p95).toBeLessThanOrEqual(BUDGET.frameCpuMs);
  expect((await status(host)).error).toBeUndefined();
  await context.close();
});
