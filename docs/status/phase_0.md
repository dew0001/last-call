# Phase 0 status: Skeleton

Date: 2026-10-08. Branch: `main-ygr4w2`.

## What works

- The workspace (`shared`, `client`, `host`, `bots`, `tools`) compiles for `wasm32-unknown-unknown` and native. 14 unit tests pass, including one property test.
- The client renders a lit, spinning cube on WebGL2 in headless Chromium 141, Firefox 142 and WebKit 26. Playwright checks the pink cube pixels in a screenshot.
- The WebGPU client is compiled and feature-detected. `web/boot.js` picks it when the browser gives an adapter. If it does not draw within 20 s, the tab reloads on WebGL2.
- The host simulation runs at 64 ticks per second natively (`host-native`) and inside a module Web Worker in all three browsers.
- The signaling Worker (Rust, `worker` 0.8.7) runs under `wrangler dev` and answers `/health`.
- CI runs fmt, clippy with `-D warnings` (native, wasm, Worker), tests, native build, release wasm build, Worker build with a `/health` check, and Playwright in three browsers. The deploy job skips cleanly when the Cloudflare secrets are missing.

## What was swapped

See `docs/DECISIONS.md`: `trunk` replaced by `scripts/build-web.sh`, `bincode` kept on 2.0.1, Firefox runs headed under `xvfb-run`, custom host loop instead of Bevy's schedule runner.

## Renderer paths verified in the cloud

| Browser | WebGL2 | WebGPU |
|---|---|---|
| Chromium 141 | verified (SwiftShader) | adapter and device created, then the browser loses the device; drawing not verified |
| Firefox 142 | verified (Mesa llvmpipe, Xvfb) | no adapter; not verified |
| WebKit 26 | verified | no adapter; not verified |

## Budgets measured

| Item | Budget | Measured |
|---|---|---|
| Client wasm, release, raw | under 25 MiB | 15.4 MB (WebGL2), 15.5 MB (WebGPU) |
| Client wasm, release, Brotli | 12 MB | 4.0 MB each |
| Host wasm, release, Brotli | | 0.12 MB |
| First load, Brotli | 40 MB | 4.14 MB |
| Host tick rate | 64 Hz | 64.0 natively, 60 to 68 asserted in the Worker |

Frame time, draw calls and network budgets start in Phase 1.

## Cloud machine

Intel Xeon @ 2.10 GHz, 4 cores, 15 GB RAM, no GPU. Linux 6.18.

## Deployed URLs

- Client (branch preview): https://main-ygr4w2.last-call-21u.pages.dev
- Client (production, after a merge to `main`): https://last-call-21u.pages.dev
- Signaling Worker: https://last-call-signal.drewduncanjr.workers.dev/health

The name `last-call.pages.dev` was taken, so Cloudflare added the suffix `-21u`. The cloud session's proxy blocks `*.pages.dev` and `*.workers.dev`. So the CI deploy job checks the live sites: it calls the Worker's `/health` and runs the Chromium suite against each new deployment.
