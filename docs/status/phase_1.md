# Phase 1 status: Lobby and netcode

Date: 2026-10-08. Branch: `main-ygr4w2`.

## What works

- **Rooms.** "Open the bar" makes a 5-letter room code and an invite link. Opening `/?room=CODE` joins the host over WebRTC: one unordered data channel per player, star through the host. The signaling Worker relays only setup messages.
- **Host.** The host page runs the authoritative sim in a Web Worker (lightyear server, avian physics). The host's own client talks to the Worker over a `MessageChannel`, with the same protocol as everyone else.
- **Players.** Up to 8 players walk around a gray-box bar as capsules. Own movement is predicted; others are interpolated. Inputs are 8 bytes, sent at 32 Hz.
- **Props.** 20 bottles, 100 chips and 10 stools, host-owned. E picks up, holding F charges a throw, Q drops. A held prop is drawn at the holder's predicted hand. Resting props sleep and cost no bandwidth.
- **Voice.** Full WebRTC audio mesh with distance gain: full volume at 2 m, silent at 14 m.
- **Reconnect.** A refreshed tab gets its old player back (same id and position). The host keeps a dropped player for 30 s.
- **Host leaves.** Every client shows "Host left".
- **Hidden host tab.** It keeps a 64 Hz tick.

## Tests

- **Native.** Unit tests in `shared`, `host`, `tools` and the signaling Worker. `crates/bots/tests`: 8 bots with traffic budgets, a slow (13 fps) client, props (throw and drop), reconnect. `crates/host/tests/settle.rs`: every prop sleeps within 6 s.
- **Signaling relay.** `tests/signal/relay.test.mjs` runs against `wrangler dev`.
- **Browser.** `tests/e2e/phase_1.spec.ts` covers: create and join, move, throw, 8 tabs, voice falloff, refresh rejoin, host left, hidden host tab. They pass in Chromium. Firefox and WebKit pass all except these skips (reasons in `docs/DECISIONS.md`):
  - Firefox skips the 8-tab test: headed Firefox draws only its front window.
  - WebKit skips voice: Playwright WebKit has no fake microphone.
  - Firefox and WebKit skip the hidden-tab test: it drives Chromium directly.

## What was swapped

See `docs/DECISIONS.md`, Phase 1 section. Main items:
- lightyear over our own byte pipe, not a lightyear WebRTC transport.
- WebRTC in JS (`web/net.js`), not `matchbox_socket`.
- Clients connect after 15 frames, with a 2x jitter margin.
- Chips use box colliders.
- The host checks which entities a client may drive.

## Budgets measured

| Item | Budget | Measured |
|---|---|---|
| Time to lobby from link | 15 s | 4.7 to 6.3 s (Firefox, Chromium, WebKit) |
| Net up per client, 8 bots | 8 KB/s | 4.7 KB/s |
| Net down per client, 8 bots | 40 KB/s | 4.6 KB/s |
| Host net up, 8 players | 300 KB/s | 37 KB/s |
| Host tick, 8 players, wasm in Chromium Worker (debug build) | 6 ms | avg 1.4 to 1.6 ms; max 16 to 20 ms |
| Host tick, native, 3 players with physics | | worst 2.1 to 4.2 ms |
| Hidden host tab tick rate, 60 s | 64 Hz | above 62 every second |
| "Host left" shown after the host tab closes | 5 s | 0.2 to 1.2 s |
| Awake physics bodies | 300 | 0 at rest; 2 to 75 while props settle |
| Client wasm, release | 25 MiB raw, 12 MB Brotli | 18.8 MB raw, 4.7 MB Brotli |
| Host wasm, release | | 9.2 MB raw, 1.9 MB Brotli |
| First load, release | 40 MB | 6.6 MB Brotli |
| RTT, joined player, local | | 61 to 70 ms |

The single-tick maximum (16 to 20 ms in the debug build) is above the 10 ms soak target for Phase 7. The average is well inside budget. To profile in a release build.

Frame time cannot be measured here (no GPU). On SwiftShader, one 160 x 90 tab draws about 24 fps with its main thread 92% idle; the cost is software rendering.

## Known gaps

- Voice has no wall occlusion or drunk pitch shift yet. They come with the rooms and drinks.
- Bevy once hit a debug-only schedule assertion in WebKit. It did not happen again in later runs.

## Deployed URLs

- Client (branch preview): https://main-ygr4w2.last-call-21u.pages.dev
- Signaling Worker: https://last-call-signal.drewduncanjr.workers.dev/health
- CI deploys every push and runs the Chromium suite against the new deployment.
