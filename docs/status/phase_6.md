# Phase 6 status: Art and performance pass

Date: 2026-10-10. Branch: `main-ygr4w2`.

## What works

- **Art in code** (`crates/client/src/art.rs`). Low-poly toy figures (1.5 heads tall, big hands, boots) with an optional hat, all one mesh per figure. Bottles, glasses, chips, stools and the mop, each under 300 triangles. A wooden counter with a lighter top and a brass rail, felt tables, slot machines with lit screens, warm plaster walls inside and cold brick outside. Each room has a floor color and a neon accent strip. Static walls and furniture merge into two meshes.
- **Baked lighting** (`tools bake_lighting`). One lightmap per room floor (eight rooms), with lamp light shadowed by walls, the counter and tables, the moon outdoors, ambient light and wall occlusion. Written as uncompressed RGBA8 sRGB KTX2 under `web/assets/lightmaps` (four texels per meter; the largest is 112x80). A test fails if the committed files are stale.
- **KTX2 pipeline**: the client loads the lightmaps through Bevy's KTX2 loader; the build's size report (`scripts/size-report.mjs`, run by CI) lists every file in the bundle and enforces the size budgets.
- **Post stack**: HDR, bloom on the neon, a vignette that closes in when Buzzed or spinning, chromatic aberration that grows with drink, CSS screen blur and film grain. The Low preset drops bloom and shadows.
- **Wasm threads**: not kept. Bevy 0.19 runs its task pool single-threaded on wasm32 (see `docs/DECISIONS.md`), so a threaded build cannot shorten the frame.

## Budgets measured

Machine: the cloud session, 4 vCPUs (Intel Xeon @ 2.80 GHz), 15 GB RAM. No GPU: Chromium renders WebGL2 with SwiftShader. Measured by `tests/e2e/phase_6.spec.ts` in each room (Low preset, see below):

| Room | Meshes drawn (draw calls, before batching) | Triangles | Main-thread CPU per frame, median (p95) |
|---|---|---|---|
| Bar | 186 | 12,202 | 7.3 ms (9.2) |
| Office | 10 | 1,810 | 6.3 ms (6.9) |
| Roof | 9 | 1,798 | 6.5 ms (7.9) |
| Parking lot | 204 | 12,378 | 7.0 ms (8.3) |
| Pier | 222 | 12,584 | 6.8 ms (7.5) |

| Item | Budget | Measured |
|---|---|---|
| Draw calls per frame | 400 | 222 at most |
| Triangles on screen | 250k | 12.6k at most |
| Main-thread CPU per frame | 8 ms | 7.3 ms (median, worst room) |
| First load (WebGL2 variant), Brotli | 40 MB | 7.98 MB |

- CPU per frame is the thread CPU time of each animation-frame callback from a Chrome trace. Wall time inside the callback (median 7.6 to 11 ms) also counts waits on SwiftShader, the software rasterizer.
- GPU frame time cannot be measured here. With the High preset (bloom and moon shadows), SwiftShader needs about 250 ms per frame, so the walk runs in Low; meshes, triangles and CPU time are the same in both presets. On a real GPU the plan's 1080p target on Iris Xe stays a design target.
- The test runs in Chromium, Firefox and WebKit in CI; the CPU check runs in Chromium only (the trace protocol is Chromium's).

## What was swapped or deferred

See `docs/DECISIONS.md`, Phase 6 section: art made in code (no blend-shape faces, no 11-body character ragdoll, no rain particles, no diegetic house sign or clock), lightmaps on floors only, and no wasm threads.
