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
| `@playwright/test` | 1.64.0 | Newest release. Was 1.56.1 to match the Chromium build already in the cloud session; raised because its WebKit 26.0 build crashed the host (see "WebKit host crash"). The cloud session downloads the 1.64 browsers. |

### `trunk` replaced by a build script

Bevy's `webgpu` feature replaces WebGL2 instead of adding to it. So WebGPU-first with a WebGL2 fallback needs two client wasm bundles, plus a third wasm module for the host Web Worker. `trunk` builds one main bundle per page. `scripts/build-web.sh` runs `cargo build`, `wasm-bindgen` and (in release) `wasm-opt -Oz` for all three, then copies `web/` into `dist/`. `web/boot.js` picks a bundle at load time. `make dev` serves `dist/` with `scripts/serve.mjs`, which sends the same COOP/COEP headers as `web/_headers`.

### Renderer selection and fallback

`web/boot.js` uses WebGPU when `navigator.gpu.requestAdapter()` returns an adapter, else WebGL2. `?gpu=webgl2` or `?gpu=webgpu` forces one. If the auto-picked WebGPU build has not drawn 30 frames after 20 s, the tab remembers that in `sessionStorage` and reloads on WebGL2.

### What the cloud session could and could not verify

The cloud session has no GPU and runs headless Chromium 141 only.

- WebGL2 in Chromium (SwiftShader through ANGLE): verified. The cube renders.
- WebGPU in Chromium: partly verified. The browser gives a SwiftShader WebGPU adapter. The WebGPU bundle loads, wgpu creates a device, and Bevy reports the `BrowserWebGpu` backend. Then the browser loses the device with "A valid external Instance reference no longer exists." Plain WebGPU JavaScript with no Bevy loses the device the same way, with every flag set tried. So actual WebGPU drawing is not verified here. The automatic WebGL2 fallback is verified. With Playwright 1.64 (Chromium 156) the signal changed: the device is destroyed about 20 ms after the first draw to a canvas ("Device was destroyed"), plain JavaScript included, and the old warning appears only when the page closes. The game then stalls after 3 frames. The test now first draws one frame with plain WebGPU JavaScript and skips when that device is lost. It also keeps watching for the old warning during the whole frame wait.
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

### WebKit host crash: investigation log

WebKit (Playwright build 2215, WebKitGTK) sometimes kills the host simulation with "Out of bounds memory access" in the tick callback. Later calls then fail with "access to a null reference", "Out of bounds table access" or "unreachable". It happens only in WebKit and only with other pages loaded. The full WebKit suite fails 3 to 4 tests per run, in CI and locally. The same wasm never fails in Chromium or Firefox.

JavaScriptCore options tried on the release build (each confirmed applied with `JSC_dumpOptions=1`):
- `JSC_useOMGJIT=0`: still crashes.
- `JSC_useWasmFastMemory=0`: passed one full run, then crashed 4 times in the next two. Not the cause.
- `JSC_useWasmOSR=0` with `JSC_freeRetiredWasmCode=0`: still crashes.
- `JSC_useBBQJIT=0`: no memory errors, but wasm runs in the interpreter and is too slow for the tests.
- An 8 MB wasm stack (instead of 1 MB): still crashes, so it is not a stack overflow.
- A host build that keeps function names shows the trap inside avian's `NarrowPhase::update_contacts`, a very large function. A JIT miscompile in WebKit 26.0 fits all results (UNVERIFIED).

Result: Playwright 1.64.0 ships WebKit 27.2 (build 2370). The WebKit e2e job passed in CI on the first run with it (run 37854696748), with no host memory errors. The cause stays UNVERIFIED; the old WebKit build is the only change. Locally, WebKit 2370 opens no WebRTC connection in this container (no ICE candidates), so WebKit runs in CI only.

### Bot pick-up in the props tests

In CI, `props_rest_and_a_bot_throws_a_bottle` failed with "bot never picked anything up". It never failed locally, even under CPU load.

Cause (VERIFIED from the failure output): the scripted bot stood at x = -7, the spawn point of slot 0, past the end of the counter. The host gives each joining player the lowest free slot. `LocalRoom` connected all bots at once, so on a slow runner they could join in any order, and bot 2 did not always get slot 2.

Fix: `LocalRoom::new` connects bots one at a time and waits for each Welcome, so bot `i` always gets slot `i`. Two earlier changes stay because they make the tests more tolerant of slow machines:
- Both props tests watch from the first tick instead of after a 6-second settle phase.
- The grab scripts keep walking into the counter while they tap E 8 times, instead of one press.

On failure the throw test prints the bot's host position and the distance from its hand to the nearest prop.

### Host tick budget in the 8-tab test: Chromium only

The 8-tab test checks that the host's worst 1-second average tick stays under 6 ms. With WebKit 27.2 in CI, one run passed and the next measured 15.5 ms. Eight WebKit processes share the runner's 4 cores, so the wall-clock tick time mostly measures CPU contention. The test still runs in WebKit and checks that all 8 clients join, see each other and move. The 6 ms budget is asserted in Chromium only; WebKit records its figure as a test annotation. Real tick profiling is Phase 7 work.

## Phase 2

### Shift clock

- The host owns the clock (`crates/host/src/shift.rs`) and counts ticks: 64 per second, so a phase lasts exactly its length in simulated time. Clients get a replicated `ShiftClock` on one room-state entity. It changes once per second, so it costs almost no bandwidth.
- The clock runs only while at least one player is in the room (OPINION; the plan does not say). An empty room, or one where everyone is reconnecting, keeps its time.
- `?fast=N` on a host page divides every phase length by N (at least 1 second each). Tests use `?fast=60`: a 14-second shift. Native tests use `Timings::scaled_down` through `HostConfig`.
- The room seed comes from `crypto.getRandomValues` in the host Worker. Native tests fix it, so runs can replay.
- The clock shows as a `bevy_ui` text line for now. The plan wants a diegetic bar clock and wall sign (section 7); those come with the art pass in Phase 6. `bevy_ui`, `bevy_text` and the default font added 1.06 MB Brotli to the first load (6.01 to 7.07 MB; budget 12 MB).

### A lightyear debug assertion after long client stalls

A client that misses more than 256 replication updates in a row hits a `debug_assert!` in lightyear 0.30.1 ("missing authoritative checkpoint mapping for completed mutate tick"): its checkpoint map keeps the last 256 entries. Release builds, which the web uses, log an error and continue (VERIFIED in `lightyear_replication/src/client.rs`). Native tests hit it only when the host ticked for 45 seconds without updating the bot, so tests now update bots every tick.

### Economy rules the plan leaves open

The plan fixes the payment schedule, tiers and the two-misses loss rule. These details are my choices (OPINION), in `crates/shared/src/economy.rs`:
- A missed payment takes nothing and carries into next week's payment.
- From week 6 on, the whole remaining balance is due. A run that missed week 6 but not twice in a row continues into week 7 and later.
- After a win or a loss, the screen shows for 30 seconds, then a new run starts at the next new game plus level. Each level adds 25% to the debt, every payment and every tier threshold. No money carries over (plan section 4.3).
- Each press of E at the office safe moves $100 (or what is left) from the pocket to the house pool.
- Money is whole dollars in `i64`.

### Office and shared blocks

The office is the back-right corner (x 6 to 10, z -7 to -2) with a doorway in its front wall and the safe in the back corner. `shared::bar::BLOCKS` lists every solid box (counter, office walls, safe). Player collision, the host's physics colliders and the client's meshes all read it, so they cannot disagree.

### Test presets

`?preset=lastweek` starts at week 6 with 80,000 paid, 45,000 in the house and 300 in each pocket. `?preset=broke` starts with one payment missed and nothing in the house. With `?fast=60`, browser tests reach the win and loss screens in under a minute.

### Third-person camera stays inside the room

The camera sat 3.5 m behind the player. At the spawn line that put it outside the back wall, so the screen showed only the wall's outside. It is now clamped inside the room. Found from a screenshot during Phase 2.

### Customers and the navmesh

- `vleue_navigator` 0.16.0 (VERIFIED: it requires Bevy 0.19.1 and avian3d 0.7, the versions we use). The host builds one navmesh at start with `NavMesh::from_edge_and_obstacles`: the room inset by the customer radius, minus every block in `shared::bar::BLOCKS` grown by the same radius. Paths come from the synchronous `path()` call, not the async updater, so the simulation stays deterministic. Default features (gizmos) are off.
- Customers sit on the stool props: any upright stool in the strip in front of the counter is a seat. If someone knocks over or carries off a customer's stool, the customer leaves (OPINION; the plan does not cover it).
- Customers have no physics body yet. They walk the navmesh and pass through players and props. Bodies come with the brawl chaos event, which needs ragdolls anyway.
- The wave interval (90 s) scales with `?fast`; patience (20 s), drink interval (120 s) and walking speed do not, so fast test rooms still play like the real game inside a phase.
- Customers draw from the room's RNG stream (stream 0) with the room seed: the same seed brings the same customers with the same cash (tested).
- Ten walking customers cost 4 KB/s of download per client (budget 40 KB/s).

### Beer tap

Plan section 5.4 sets the shape; these numbers are mine (OPINION), in `crates/shared/src/beer.rs`:
- Fill rises 40% per second while E is held at the tap (2.5 s to full). The green zone is 85 to 100%. Past 100% a pour is "overfull" (pays, no tip); past 105% it overflows: no glass, and a puddle on the floor. After an overflow, E must be let go before the next pour.
- Tilt is the look pitch: between -0.6 and -0.2 radians (looking a little down) foam rises 3% per second, outside it 40%. A perfect pour has at most 25% foam.
- A served glass pays the customer's $8 to the house. A perfect pour still in the green zone also tips $2 to the pourer's pocket. Short, foamy or overfull pours pay but do not tip. A glass under 60% is refused.
- Carrying: sprinting spills 30% per second; walking spills nothing; a thrown glass spills everything. The drunk multiplier hooks in with the drunk meter.
- Serving: a glass at rest anywhere on the counter top within 0.55 m (along the counter) of a waiting customer's stool. The zone spans the whole counter depth, so a server can reach it from either side.
- The hand point rose from 1.15 m to 1.3 m above the feet, so a held glass clears the counter top. Bottles moved to mid-counter and chips to the back edge, clear of the front edge where glasses land.
- serde-wasm-bindgen turns `None` into `undefined` in `window.__lastCall`, not `null`. Tests treat both as absent.

### Client-predicted pour gauge

The host's pour gauge reaches the client about a round trip late, so players (and CI's slow Chromium) released too late and overfilled. The client now runs the same `Pour::step` on its own inputs every tick, on its predicted timeline. Lightyear stamps inputs with the client's tick, so a release lands on the host at the tick the gauge showed. The HUD and `window.__lastCall.game.pour` show the predicted fill while E is held.

When a client stalls, lightyear keeps the last known input for the missing ticks (VERIFIED: `decay_tick` is a no-op for native inputs). A stall just as E goes down therefore starts the pour late on the host. Native bot tests check what does not depend on timing; the browser test checks a perfect pour end to end.

### Drunk meter

Rules in `crates/shared/src/drunk.rs`; numbers beyond the plan's are mine (OPINION):
- R with a beer in hand drinks it: +20, $5 from the pocket (not possible with less than $5). The meter decays 1 point per 2 s, also while passed out.
- Effects build up: a Wasted player also has the Sloppy and Courage effects.
- Courage (20+): camera sway on the client. The x1.5 max bet waits for the tables (Phase 3); `max_bet_multiplier` is ready.
- Sloppy (40+): walk and sprint +10% (in shared movement, so prediction matches the host); throws go up to 36 degrees off aim; carried beer spills 1.5 times faster; screen blur; other players hear the voice at 0.8 pitch.
- Wasted (70+): a stumble every 8 s that carries the player 0.9 m in a random direction; spills 2.5 times faster. "Cannot deal" and shuffled bet buttons wait for the tables; `can_deal` is ready.
- 100: passed out for 45 s. Whenever the meter reaches 100, the player passes out.
- Puddles: sprinting over one, or walking over one while Sloppy or worse, makes the player slide 0.9 m on.

Pass-out ragdoll: the player's capsule becomes a dynamic body lying on its side, with rotation locked so it does not roll. Props stack on it. Another player grabs it with E (with empty hands, within 1.2 m of the hand) and pulls it along; Q lets go. The plan's 11-body ragdoll (section 7) belongs to the art pass. Stumbles, slips and drags are host-only; the owner's prediction is corrected by rollback.

Screen blur: Bevy 0.19 turns depth of field off on WebGL2 (VERIFIED in `bevy_post_process/src/dof/mod.rs`: "depth textures aren't supported correctly"). The page blurs the canvas with a CSS filter instead, up to 5 px, from the strength the game reports. It also blurs the HUD; the art pass can move the HUD outside the canvas if needed.

Drunk voice: each listener runs remote voices through an AudioWorklet pitch shifter (`web/pitch-worklet.js`, two cross-faded delay taps). The game reports a pitch factor per player (`game.voicePitch`). In Firefox the test measured 818 Hz drunk against 991 Hz sober (ratio 0.83 for a target of 0.8, with 47 Hz analyser bins).

### Presets for drunk tests

`?preset=tipsy` starts players at 45 (Sloppy) and `?preset=wasted` at 90, both with $300 in their pockets.
