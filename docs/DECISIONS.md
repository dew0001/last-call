# Decisions

Every deviation from `LAST_CALL_PLAN.md`, and every choice the plan left open, with the reason.

## Phase 0

### Crate and tool versions (checked 2026-10-08)

| Item | Version | Note |
|---|---|---|
| Rust toolchain | 1.97.0 | Pinned in `rust-toolchain.toml`. |
| `bevy` | 0.19.1 | Current stable. 0.20 is only a release candidate. |
| `avian3d` | 0.7.0 (planned) | Supports Bevy 0.19. Added in Phase 1. |
| `lightyear` | 0.30.1 (planned) | Supports Bevy 0.19. Added in Phase 1. |
| `bevy_replicon` | 0.44.3 (fallback) | Supports Bevy 0.19. |
| `matchbox_socket` | 0.14.0 (planned) | Added in Phase 1. |
| `worker` | 0.8.7 | Has Durable Object WebSocket support, so the signaling Worker stays in Rust. |
| `serde` | 1.0.229 | |
| `bincode` | 2.0.1 | `bincode` 3.0.0 on crates.io is an empty end-of-life release. 2.0.1 with the `serde` feature is the last working version. If it causes trouble, the plan's alternative `rkyv` applies. |
| `rand_chacha` | 0.10.0 | |
| `wasm-bindgen` / CLI | 0.2.129 | The CLI version must match `Cargo.lock`. `scripts/install-tools.sh` reads it from there. |
| `wasm-opt` (binaryen) | 125 | |
| `worker-build` | 0.8.7 | |
| `trunk` | not used | See below. |
| `wrangler` | 4.140.0 | npm devDependency. |
| `@playwright/test` | 1.56.1 | Matches the Chromium 141 build (r1194) already in the cloud session. |

### `trunk` replaced by a build script

Bevy's `webgpu` feature replaces WebGL2 instead of adding to it. So WebGPU-first with a WebGL2 fallback needs two client wasm bundles, plus a third wasm module for the host Web Worker. `trunk` builds one main bundle per page. `scripts/build-web.sh` runs `cargo build`, `wasm-bindgen` and (in release) `wasm-opt -Oz` for all three, then copies `web/` into `dist/`. `web/boot.js` picks a bundle at load time. `make dev` serves `dist/` with `scripts/serve.mjs`, which sends the same COOP/COEP headers as `web/_headers`.

### Renderer selection and fallback

`web/boot.js` uses WebGPU when `navigator.gpu.requestAdapter()` returns an adapter, else WebGL2. `?gpu=webgl2` or `?gpu=webgpu` forces one. If the auto-picked WebGPU build has not drawn 30 frames after 20 s, the tab remembers that in `sessionStorage` and reloads on WebGL2.

### What the cloud session could and could not verify

The cloud session has no GPU and runs headless Chromium 141 only.

- WebGL2 in Chromium (SwiftShader through ANGLE): verified. The cube renders.
- WebGPU in Chromium: partly verified. The browser gives a SwiftShader WebGPU adapter. The WebGPU bundle loads, wgpu creates a device, and Bevy reports the `BrowserWebGpu` backend. Then the browser loses the device with "A valid external Instance reference no longer exists." Plain WebGPU JavaScript with no Bevy loses the device the same way, with every flag set tried. So actual WebGPU drawing is not verified here. The automatic WebGL2 fallback is verified.
- WebGL2 in Firefox 142: verified, after a fix. Headless Firefox finds no GL driver ("Exhausted GL driver options"), even with `webgl.force-enabled`. Headed Firefox on a virtual display (`xvfb-run`) uses Mesa llvmpipe and gets WebGL2. So Playwright runs under `xvfb-run`, and Firefox runs headed whenever `DISPLAY` is set.
- WebGL2 in WebKit 26 (Playwright build 2215): verified.
- WebGPU in Firefox and WebKit: not verified. Neither exposes a WebGPU adapter in this environment, so the test skips.
- Firefox and WebKit first ran in CI only, because the network policy blocked the browser download. The user opened network access, and all three browsers now run in the cloud session too.

### Host simulation loop

Bevy's built-in schedule runner calls `window.setTimeout`, and a Web Worker has no `window`. The host uses its own loop: a `setTimeout` chain on the Worker global scope in the browser and a sleep loop natively. Both use `host::runner::Pacer`. The simulation never reads a clock; it runs one `SimTick` schedule per tick. Most catch-up per wake-up is 8 ticks.

### Signaling Worker outside the Cargo workspace

`crates/signal` has its own `Cargo.lock`. This keeps the `worker` crate's `wasm-bindgen` pin apart from Bevy's. Its release profile does not use `strip`, because stripping removed the externref table that `wasm-bindgen` needs.

### Rust edition

All crates use edition 2024.

### Deploy on every push

The working branch is not `main`. Every push runs the deploy job when the Cloudflare secrets exist. `main` deploys the Pages production site. Other branches deploy a Pages preview URL. There is one signaling Worker, deployed from whichever branch pushed last. This is fine while there is one line of work.

### Playwright traces off

A trace stores every response body, and the debug wasm is about 70 MB. WebKit traces overflowed ("Invalid string length"). Tests keep screenshots on failure and print all browser console output instead.

### Public addresses

The Pages project is `last-call`. The free address `last-call.pages.dev` was taken, so Cloudflare assigned `last-call-21u.pages.dev`. The signaling Worker is at `last-call-signal.drewduncanjr.workers.dev` (the account's free `workers.dev` subdomain). The deploy job checks both after each deploy, because the cloud session cannot reach them.

### One retry when the renderer fails to start

In one CI run, WebKit failed to create its first WebGL context right after starting ("Unable to find a GPU"). The next page in the same browser worked, and 8 local repeats all passed. Players can hit the same start-up race, so `web/boot.js` reloads the page once if the renderer fails before the first frame. A second failure shows a message. No test forces this path yet.

## Phase 1

### Netcode: lightyear 0.30 with a raw connection and our own byte pipe

lightyear 0.30 has no WebRTC transport. The host runs lightyear's server with `RawServer` (no handshake). The page or a test harness hands it ready-made links. Each link carries a `shared::pipe::PipeIo`: a crossbeam byte pipe with traffic counters. Native bots use pipe pairs. In the browser, JS moves the same bytes between WebRTC data channels, the host Worker, and the host's own client (a `MessageChannel` port). lightyear does reliability, so one unordered, no-retransmit data channel per player is enough. There is no separate reliable data channel.

The host's server entity is marked `Linked` by hand. A raw server only becomes `Started` when linked, and with no listening socket nothing else links it. Without this, messages flowed but replication never started.

### WebRTC in JavaScript (`web/net.js`) instead of `matchbox_socket`

The host tab must relay packets between WebRTC and the host Worker while the tab is hidden. Browsers pause animation frames in hidden tabs, so a relay inside the Bevy frame loop would stall. `net.js` is event driven and keeps relaying. It speaks matchbox's signaling protocol, so the signaling Worker matches matchbox's design. The voice mesh will be JS too (plan section 8).

### Inputs quantized and sent at 32 Hz

The first 8-bot run measured 8.4 KB/s upload per client, over the 8 KB/s budget. `PlayerInput` is now 8 bytes (move as two `i8`, yaw `u16`, pitch `i16`, buttons `u16`). Inputs go out every 2 ticks, and each packet repeats the last 4 sends (about 125 ms of loss cover). Measured with 8 bots: 4.7 KB/s up, 4.6 KB/s down per client, 37 KB/s host up. Quantizing also makes host and client compute identical values.

### Lobby is an HTML overlay

The title lobby (Open the bar, join by code) and the room banner are HTML over the canvas. They are simple, accessible, and need no font assets. The plan's "diegetic where possible" rule covers in-game UI, which stays in Bevy.

### Multi-tab browser tests use 320 x 180 viewports

With no GPU, SwiftShader draws every pixel on the CPU. Three tabs at 640 x 360 drew under 3 frames per second, and the client's input timing broke down. The main thread was 97.5% idle in a CPU profile, so the cost is software rendering, not game code. Small viewports keep the tests about real behavior. Headed Firefox stops drawing a covered window, so tests bring a tab to the front before reading its view.

### Clippy type complexity

`clippy.toml` raises `type-complexity-threshold` to 600, because Bevy queries are long by design.

### Clock sync: connect after warm-up, 2x jitter margin

In the browser, the first pings are measured while the page compiles shaders. lightyear's first reading was 822 ms RTT with 312 ms jitter. Its default margin (4x jitter) then put the client about 2 seconds ahead of the host, and it corrected only slowly. Players saw their actions land seconds late. Now the client connects only after drawing 15 frames (`shared::client::ConnectAfterFrames`; native bots use 0), and the margin is 2x jitter (about 95% of packets per lightyear's notes). Measured after the fix: 61 to 70 ms RTT, client about 12 ticks (190 ms) ahead.

### Physics props

Avian 3D runs only on the host. Clients get a replicated `PropPose` that the host writes only when a prop moves (more than 1 mm or 0.3 degrees), so resting props cost no bandwidth. Players are kinematic capsules that push props. A held prop is kinematic and steered to the hand. A thrown one becomes dynamic again. The holder's client draws its held prop at its own predicted hand, so carrying feels instant.

Chips use a flat box collider (still drawn as a disc). Thin cylinders in stacks of 10 never fell asleep: 73 of 100 stayed awake, and download rose from 4.6 to 27.8 KB/s per client. With boxes, all 130 props sleep within 6 seconds (`crates/host/tests/settle.rs`).

### Join slots

Players joining in the same frame all got slot 0 and spawned on one spot, because spawn commands apply later. The join handler now tracks slots taken in the same pass and gives the lowest free slot.

### Voice

`web/voice.js` is a full mesh of plain WebRTC audio. The signaling Worker has a voice mode (`?role=voice&peer=PLAYER_ID`) that relays between any two voice peers in a room. Each remote voice runs source, then a distance gain (full at 2 m, silent at 14 m), then an analyser, then the speakers. Wall occlusion and the drunk pitch shift come with the rooms and drinks in later phases.

Test notes:
- Chromium uses its fake microphone (a beep).
- Firefox uses its fake stream pref. It also needs a sound output device to run Web Audio at all, so `scripts/e2e.sh` starts a PulseAudio null sink and exports `PULSE_SERVER`. CI installs `pulseaudio`.
- Playwright WebKit has no fake microphone, so the voice test skips there.

### Firefox 8-tab test skipped

Headed Firefox runs the frame loop only for the window in front, and `bringToFront` does not restart a covered window. With 8 windows on one virtual display, most never draw. The 8-tab test runs in Chromium and WebKit. Firefox runs the 2-tab join, move, throw and voice tests. Native bots cover 8 players.

### Hidden host tab test drives Chromium directly

Playwright keeps every page "visible": it emulates focus and turns off background throttling. A covered window, a minimized window, or another tab all left the host page reporting `visible`. The hidden-tab test (`tests/e2e/raw-chromium.ts`) starts a plain headed Chromium over the DevTools protocol, opens the player in a second tab, and activates it. The host tab then reports `hidden`, and its main-thread timers slow to about 1 per second, like a real background tab. The host Worker held above 62 ticks per second for 60 s, and the player kept moving in the host sim. This test runs in the Chromium project only.

### Input authorization and the input marker

A client kept lightyear's input marker on another player after that player's controller changed during a reconnect. Its idle (zero) inputs then overrode the real player's. Two fixes:
- Clients mark only the player whose id came in their Welcome.
- The host runs lightyear's `authorize_controlled_targets`, so a client's inputs for an entity it does not control are dropped. This also stops a modified client from driving someone else.

### Reconnect

The host keeps a player whose link dropped for 30 seconds (`AwaitingReconnect`). A client that joins with the same UUID (from `localStorage`) gets the same entity, position and slot. After 30 seconds the player is removed.

### PROTOCOL.md from a tool, not a build script

Plan section 3.3 asks for a build script. A build script that writes into `docs/` would edit the source tree during every build. Instead, `cargo run -p last_call_tools -- protocol_doc` writes `docs/PROTOCOL.md`, and CI runs it with `--check` to fail when the file is stale.

### CI hardening for slow GPU-less runners

CI run 7 showed more failures, all on GitHub's 4-core runners with software rendering:
- **Tabs never connected.** A tab drawing 12 frames in a minute never reached the 15-frame warm-up, so it never connected. Clients now connect after 15 frames or 3 seconds, whichever comes first. A slow real device needs this too.
- **Tabs starved of frames.** Multi-tab tests check game state, not pixels, so their tabs pass `?nodraw`: the full client runs with no camera. Phase 0 still checks rendering.
- **Start-up tick spike.** All 130 props settled at once when the room started, and one second in Firefox ticked at 56 Hz. Props are placed at rest, so they now start asleep. Picking one up wakes it. `crates/host/tests/settle.rs` wakes them all and checks they sleep again within 6 seconds.
- **Lost start-up error.** In WebKit, a start-up GPU panic was followed by a memory error. The generic handler kept the second message, so the one-time renderer retry never matched. The first error is now kept.

### WebKit test browser runs without wasm fast memory

WebKit (Playwright build 2215, WebKitGTK) sometimes killed the host simulation with "Out of bounds memory access", then "access to a null reference", in the tick callback. It happened only in WebKit, only with other pages loaded, and failed 3 tests per full suite run, in CI and locally. The same wasm never failed in Chromium or Firefox. Earlier WebKit-only oddities (a replicon "scope not registered" panic, a zstd failure) look like the same memory damage.

JavaScriptCore checks wasm memory bounds with a guard-page trap ("fast memory") by default. Results on the local release build:
- Default options: the full WebKit suite failed 3 tests in each of 2 runs.
- `JSC_useOMGJIT=0` (optimizing tier off, confirmed applied with `JSC_dumpOptions`): still failed.
- `JSC_useWasmFastMemory=0` (explicit bounds checks): the full suite passed.

The `webkit` Playwright project sets `JSC_useWasmFastMemory=0`. This changes only the test browser. The cause inside WebKit is UNVERIFIED, and no WebKit bug report was found. Real Safari on macOS and iOS uses its own memory setup; Phase 7 testing on a real Safari must check for this crash.
