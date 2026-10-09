# Phase 2 status: Shift loop and economy

Date: 2026-10-09. Branch: `main-ygr4w2`.

## What works

- **Shift clock.** The host runs Setup (2 min), Open (9), Last call (2) and Payment (1), then the next shift; three shifts make a week. The clock runs only while someone is in the room. Clients show it in a HUD line.
- **Money.** A shared house pool and a pocket per player. E at the office safe moves $100 from the pocket to the house. At the end of each week the loan shark collects the schedule (8,000 to 40,000; from week 6 the whole balance). Two missed payments in a row lose the run; paying off 120,000 wins it. Win and loss screens show, then a new run starts at a higher new game plus level.
- **Office.** A walled room in the back-right corner with a doorway and the safe. Player collision, host physics and client meshes all read one list of blocks.
- **Customers.** Waves arrive during Open (4 + 2 per week, every 90 s) with seeded cash. They walk a `vleue_navigator` navmesh to free stools at the counter, sit and order. Served, they pay and drink, and order again 2 minutes later; unserved for 20 s, they leave. Everyone leaves at Last call. A customer whose stool is moved leaves.
- **Beer tap.** Hold E at the tap; the fill rises and foam depends on the look pitch. Release in the green zone for a perfect pour, or overflow into a puddle. Glasses are physics props: sprinting spills, throwing empties. A glass at rest in front of a waiting customer is served: $8 to the house, $2 tip to the pourer for a perfect pour. The pour gauge is predicted on the client, so a release lands where the gauge showed it.
- **Drunk meter.** R drinks the beer in hand (+20, $5). The meter decays 1 point per 2 s. Courage sways the camera; Sloppy walks 10% faster, throws wide, spills more, blurs the screen and sounds lower to other players; Wasted stumbles every 8 s. At 100 the player passes out for 45 s: their body lies on the floor as a physics capsule, props stack on it, and others drag it with E. Puddles trip sprinters and Sloppy walkers.
- **Determinism.** A scripted 14-minute shift hashes to the same host state on every native run and in the browser's wasm host, in Chromium and Firefox.

## Tests

- **Native unit tests** in `shared` cover shift timings, the economy (with property tests for money conservation), customer rules, pour rules, drunk tiers, collision with the office, and the `libm` trigonometry.
- **Native integration tests** (`crates/bots/tests`, `crates/host`): shift clock and week rollover; deposits, winning, losing and the next run; customers seating, ordering, leaving, determinism by seed and bandwidth; pouring, overflow and serving; drinking, decay, stumbles, puddle slips, passing out, dragging and waking; the navmesh routes; 8 bots at the bar for 1,000 ticks with every client's replicated state matching the host's every 100 ticks; the 14-minute replay twice against the golden hash.
- **Browser** (`tests/e2e/phase_2.spec.ts`, 10 tests): shift clock; safe deposit; win and next run; loss; customers; perfect pour served and tipped; drinking; pass-out, drag and wake; drunk voice pitch; wasm replay hash. All 10 pass locally in Chromium and Firefox. WebKit runs in CI only (its build opens no WebRTC connection in this container) and skips the voice test (no fake microphone).
- **CI.** All jobs green on run 37887763075: Rust (with the native replay), wasm build, signaling, Playwright in Chromium, Firefox and WebKit (the wasm replay matches native in all three), deploy and live smoke tests.

## What was swapped or deferred

See `docs/DECISIONS.md`, Phase 2 section. Main items:
- HUD text instead of the diegetic bar clock and LED sign (art pass, Phase 6).
- Screen blur is a CSS filter on the canvas: Bevy turns off depth of field on WebGL2.
- The pass-out ragdoll is one capsule; the 11-body ragdoll belongs to the art pass.
- Customers have no physics body yet (they pass through players and props).
- Max bet x1.5, "cannot deal" and shuffled bet buttons wait for the tables in Phase 3; the rule functions exist.
- The wasm host is built with WebAssembly SIMD, so it rounds like the native host.

## Budgets measured

| Item | Budget | Measured |
|---|---|---|
| Host tick, wasm release in a Chromium Worker, 1 player and 10 customers | 6 ms | avg 1.4 to 1.7 ms; worst single tick 9.7 ms; 63.8 to 64.2 ticks/s |
| Net down per client, 10 customers walking | 40 KB/s | 4.0 KB/s |
| Client wasm, release | 25 MiB raw, 12 MB Brotli | 22.8 MB raw, 5.2 MB Brotli |
| Host wasm, release (with SIMD) | | 11.0 MB raw, 2.2 MB Brotli |
| First load, release | 40 MB | 7.5 MB Brotli |
| 14-minute replay, native debug build | | about 25 s per run |
| 14-minute replay, wasm release in Chromium | | about 45 s |

The client wasm grew from 18.8 to 22.8 MB raw, mostly from `bevy_ui` text and fonts. It stays under the 25 MiB Cloudflare Pages file limit, but with little room: the art pass must watch it.

The worst single tick (9.7 ms) is under the plan's 10 ms soak target but close to it. Profiling belongs to Phase 6 and 7.

## Known gaps

- Customers do not use the tables yet (Phase 3).
- Stools are physics props: a player who walks along the counter front pushes them, and a customer whose stool moves leaves. This is intended, but it can feel harsh.
- Voice has no wall occlusion yet (it needs the room layouts).

## Deployed URLs

- Client (branch preview): https://main-ygr4w2.last-call-21u.pages.dev
- Signaling Worker: https://last-call-signal.drewduncanjr.workers.dev/health
- CI deploys every push and runs the Chromium suite against the new deployment.
