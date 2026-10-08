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
  { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
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
    baseURL: 'http://localhost:8080',
    viewport: { width: 1280, height: 720 },
    trace: 'retain-on-failure',
  },
  projects,
  webServer: {
    command: 'node scripts/serve.mjs dist 8080',
    url: 'http://localhost:8080/index.html',
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
