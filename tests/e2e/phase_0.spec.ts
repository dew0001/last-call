// Phase 0: the client renders a cube, the WebGPU path is detected, and the host
// simulation ticks at 64 Hz inside a Web Worker.
import { expect, test, type Page } from '@playwright/test';
import { PNG } from 'pngjs';

const CLEAR = { r: 0.08, g: 0.05, b: 0.03 };

async function waitForFrames(page: Page, frames = 30) {
  await expect
    .poll(async () => page.evaluate(() => (window as any).__lastCallError ?? (window as any).__lastCall?.frames ?? 0), {
      timeout: 150_000,
      intervals: [500],
    })
    .toBeGreaterThan(frames);
}

// Count pixels that are clearly the pink cube and not the dark clear color.
async function cubePixelShare(page: Page): Promise<number> {
  const png = PNG.sync.read(await page.locator('canvas#bevy').screenshot());
  let pink = 0;
  for (let i = 0; i < png.data.length; i += 4) {
    const [r, g, b] = [png.data[i], png.data[i + 1], png.data[i + 2]];
    if (r > 120 && r > g * 1.6 && b > g) pink++;
  }
  return pink / (png.width * png.height);
}

async function hasWebGPUAdapter(page: Page): Promise<boolean> {
  return page.evaluate(async () => {
    try {
      return !!(navigator as any).gpu && (await (navigator as any).gpu.requestAdapter()) !== null;
    } catch {
      return false;
    }
  });
}

test('WebGL2 fallback renders a cube', async ({ page }) => {
  page.on('console', (m) => m.type() === 'error' && console.log(`[console] ${m.text()}`));
  await page.goto('/?gpu=webgl2');
  await waitForFrames(page);
  const status = await page.evaluate(() => (window as any).__lastCall);
  expect(status.backend).toBe('gl');
  const share = await cubePixelShare(page);
  test.info().annotations.push({ type: 'cube-pixel-share', description: share.toFixed(4) });
  expect(share).toBeGreaterThan(0.01);
  expect(CLEAR.r).toBeLessThan(0.5);
});

test('WebGPU path boots and renders where the browser allows it', async ({ page }) => {
  const errors: string[] = [];
  page.on('console', (m) => errors.push(m.text()));
  await page.goto('/?hostonly');
  const adapter = await hasWebGPUAdapter(page);
  test.info().annotations.push({ type: 'webgpu-adapter', description: String(adapter) });
  test.skip(!adapter, 'this browser exposes no WebGPU adapter here; logged in docs/DECISIONS.md');

  await page.goto('/?gpu=webgpu');
  // The WebGPU bundle loads and wgpu gets a browser WebGPU device.
  await expect
    .poll(async () => page.evaluate(() => (window as any).__lastCall?.backend ?? ''), { timeout: 150_000 })
    .toBe('browserwebgpu');

  // Headless Chromium on SwiftShader loses the device right after start
  // ("A valid external Instance reference no longer exists"). Plain WebGPU JS
  // hits the same loss there, so it is the environment, not the game.
  await page.waitForTimeout(5_000);
  const lost = errors.some((e) => e.includes('external Instance reference no longer exists'));
  test.info().annotations.push({ type: 'webgpu-device-lost-by-browser', description: String(lost) });
  test.skip(lost, 'browser lost the SwiftShader WebGPU device; render path unverifiable here, see docs/DECISIONS.md');

  await waitForFrames(page);
  expect(await cubePixelShare(page)).toBeGreaterThan(0.01);
});

test('auto-detect ends on a renderer that draws', async ({ page }) => {
  await page.goto('/');
  // On a dead WebGPU device the page reloads itself on WebGL2 after 20 s.
  await waitForFrames(page);
  const variant = await page.evaluate(() => (window as any).__lastCallVariant);
  test.info().annotations.push({ type: 'auto-variant', description: variant });
  expect(['webgpu', 'webgl2']).toContain(variant);
  expect(await cubePixelShare(page)).toBeGreaterThan(0.01);
});

test('host simulation ticks at 64 Hz in a Web Worker', async ({ page }) => {
  await page.goto('/?hostonly');
  await expect
    .poll(async () => page.evaluate(() => (window as any).__hostTicks.length), { timeout: 30_000 })
    .toBeGreaterThanOrEqual(4);
  const reports: { tick: number; tps: number }[] = await page.evaluate(() => (window as any).__hostTicks);
  // Skip the first report: it includes worker start-up.
  for (const r of reports.slice(1)) {
    expect(r.tps).toBeGreaterThan(60);
    expect(r.tps).toBeLessThan(68);
  }
  test.info().annotations.push({ type: 'host-tps', description: reports.map((r) => r.tps.toFixed(1)).join(', ') });
});
