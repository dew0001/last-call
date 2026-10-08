import { defineConfig, devices } from '@playwright/test';

// Browsers to run. The cloud session can only download Chromium, so it sets
// PW_BROWSERS=chromium; CI runs all three.
const wanted = (process.env.PW_BROWSERS ?? 'chromium,firefox,webkit').split(',');

// The cloud session and CI runners have no GPU. Chromium renders WebGL2 through
// SwiftShader, and is asked for a WebGPU adapter on SwiftShader's Vulkan too.
const chromiumArgs = [
  '--use-angle=swiftshader',
  '--enable-unsafe-swiftshader',
  '--enable-unsafe-webgpu',
  '--enable-features=Vulkan',
  '--use-webgpu-adapter=swiftshader',
  '--ignore-gpu-blocklist',
];

const projects = [
  {
    name: 'chromium',
    use: { ...devices['Desktop Chrome'], channel: 'chromium', launchOptions: { args: chromiumArgs } },
  },
  {
    name: 'firefox',
    // Headless Firefox finds no GL driver on a machine with no GPU. Run it
    // headed on a virtual display (`xvfb-run`) so it uses Mesa llvmpipe.
    use: {
      ...devices['Desktop Firefox'],
      headless: !process.env.DISPLAY,
      launchOptions: { firefoxUserPrefs: { 'webgl.force-enabled': true } },
    },
  },
  { name: 'webkit', use: { ...devices['Desktop Safari'] } },
].filter((p) => wanted.includes(p.name));

export default defineConfig({
  testDir: './tests/e2e',
  timeout: 180_000,
  expect: { timeout: 120_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    // BASE_URL runs the suite against a deployed site instead of the local server.
    baseURL: process.env.BASE_URL ?? 'http://localhost:8080',
    viewport: { width: 1280, height: 720 },
    // Traces would hold the 15 to 70 MB wasm bodies and overflow; keep screenshots.
    trace: 'off',
    screenshot: 'only-on-failure',
  },
  projects,
  webServer: process.env.BASE_URL
    ? undefined
    : {
        command: 'node scripts/serve.mjs dist 8080',
        url: 'http://localhost:8080/index.html',
        reuseExistingServer: !process.env.CI,
        timeout: 30_000,
      },
});
