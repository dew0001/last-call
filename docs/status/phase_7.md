# Phase 7 status: Polish and release

Date: 2026-10-10. Branch: `main-ygr4w2`.

## What works

- **Settings and gamepad.** Esc (or the gear) opens the settings: mouse sensitivity, volume, voice mode (open mic, push to talk on Left Alt, off) and graphics (High, or Low with no shadows and no bloom; Low is the default on a machine with no GPU). Kept in localStorage. Gamepads work in the browser: left stick walks, right stick looks, A interact, B drop, X use, right trigger throw, left stick click sprint.
- **Achievements and hats.** Six achievements, kept in localStorage with the player's UUID: 21 three times in a shift, passing out on the roof, catching the boot, landing a shark, scoring in the gauntlet, winning a fight pit round. Each unlocks a hat; the lobby picks one and every room shows it.
- **New game plus.** Each level: +25% debt and payments, one more chaos event a shift (up to four), customers 10% less patient (down to 60%).
- **Tutorial shift.** The lobby's Tutorial button opens a calm first shift (no chaos, customers at the bar) with prompts that wait for each step: walk, pour, serve, bank at the safe, try a table, then how the loan shark collects.
- **Crash reports.** A wasm panic in a client, or an error in the host Worker, posts once to the signaling Worker's `/report`, which keeps the newest 200 in Workers KV. The deploy job creates the KV namespace on its first run.
- **Deploy from CI.** Every push deploys: Cloudflare Pages for the client, `wrangler deploy` for the signaling Worker, then smoke tests against the live site (from Phase 1; it skips with a notice until the Cloudflare secrets are set, see `docs/YOU_DO_THIS.md`).

## Load and soak

- **Load**: 50 rooms of 8 (a host and seven players each) join through the signaling Worker under `wrangler dev` at once, each with an offer and an answer per player: all connected in 2.9 s (budget: 15 s to the lobby).
- **Soak, native** (`tools soak 3600`, release build): SOAK_NATIVE
- **Soak, wasm in headless Chromium** (`SOAK_SECS=3600`, `tests/e2e/phase_7.spec.ts`): SOAK_WASM
- CI runs the wasm soak for two minutes on every push.

## Tests

- `tests/e2e/phase_7.spec.ts`: the load test, a crash report accepted and a bad one refused, settings persisted and the panel opening on Esc, an achievement toast and the chosen hat reaching the next room, the tutorial's first prompts, a joining player's hat, and the wasm soak.
- Signaling Worker unit tests for reports (`crates/signal/src/report.rs`), and NG+ patience (`crates/shared/src/economy.rs`).

## What was swapped or deferred

See `docs/DECISIONS.md`, Phase 7 section: hats are the only visible cosmetic (no lighters, voice lines or chip skins); native gamepads are off (libudev); the load test covers signaling, while browser-to-browser WebRTC is covered by the Phase 1 tests.
