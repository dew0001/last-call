# LAST CALL: Build Plan

Browser-based friendslop game. Rust client (Bevy, WebAssembly). The room host's browser runs the authoritative simulation; everyone else connects to the host peer-to-peer. 4 to 8 players own a failing dive bar that doubles as an illegal casino. They run games, serve drinks, gamble their own cut, survive chaos events, and pay off a loan shark to buy the bar.

This document is the source of truth for Claude Code. Follow it in order. Every open decision already has a default below. Do not ask the user to decide anything that this document decides.

Terminology: wherever this document says "server" or "server-side" (sections 4 and 5 especially), it means the host simulation running in the host's browser. There is no dedicated game server.

---

## 0. Non-negotiables

1. Language: Rust for client and host simulation. Shared game logic lives in one crate used by both.
2. Platform: runs in the browser. Chrome, Edge, Firefox, Safari. WebGPU first, WebGL2 fallback.
3. Multiplayer: 4 to 8 players per room. Join by link. Lobby to gameplay in under 15 seconds on a 50 Mbps connection.
4. Host authoritative. The player who creates a room runs the authoritative simulation in a Web Worker inside their game tab. All money, RNG, cards, dice, wheels, reels, and scores resolve in that simulation. Every client, including the host's own, renders, predicts, and sends inputs only. A client never decides a payout. The host can technically cheat; with fake money among friends that is accepted.
5. Performance: 60 FPS on an Intel Iris Xe laptop in Chrome at 1080p. 120 FPS on a GTX 1060 class desktop. First load under 40 MB compressed.
6. Fake currency only. No real money. No cash out. No purchasable currency. Ever.
7. Proximity voice chat in the bar, peer-to-peer over WebRTC.
8. Physics props everywhere. Bottles, chips, cash, stools. The physics are the comedy.
9. $0 to build, host, and play. Free tiers only. No service that needs a credit card. No paid domain. No dedicated game servers, no databases, no Docker.
10. Nothing is installed on or runs on the user's PC. All building and testing happens in a Claude Code cloud session and in GitHub Actions. The only thing that runs on a player's machine is the game tab.

---

## 0.5 Operating rules for Claude Code

The user does one thing: approve permission prompts. Nothing else. These rules make that possible.

1. Never ask a question. If something is unclear, pick the option that keeps section 0 true, write it in `docs/DECISIONS.md`, and continue.
2. Never wait for a manual test. Every acceptance box in section 10 that says "humans" or "manual" is replaced by an automated test with bot clients. Human playtests are optional and happen after the phase closes.
3. Never stop on a failing build. Fix it. If a crate does not support the current Bevy version, switch to the alternative in section 1 or write the missing glue yourself.
4. Install everything yourself, inside the cloud session or CI, never on the user's PC: Rust toolchain targets, `trunk`, `wasm-opt`, `wasm-bindgen-cli`, Node (for Playwright and `wrangler`), browsers for headless tests (Playwright with Chromium, Firefox, WebKit). No Docker anywhere.
5. Run the whole stack in the cloud session with one command: `make dev` starts the signaling Worker with `wrangler dev` and the client with `trunk serve`. There is no game server to start; the first browser tab that creates a room hosts it. `make test` runs everything in section 11. `make build` produces the deployable bundle. The user never types anything.
6. Art: do not wait for an artist. Use CC0 assets pulled by a script (`crates/tools/fetch_assets.rs`) from Kenney and Quaternius packs, plus procedural meshes generated in code for anything missing. Record every asset source and license in `assets/LICENSES.md`. No asset without a license entry.
7. Audio: generate placeholder SFX procedurally (`crates/tools/gen_sfx.rs`, simple synthesis to ogg). Music: five generated loops from a simple sequencer. No downloads that need a license check beyond CC0.
8. Accounts: the user has GitHub. Cloudflare (free plan, no card) is needed for Pages, the signaling Worker, and TURN. Do not block on it. Everything must run in the cloud session with `wrangler dev` first. When a deploy step needs a credential, append one line to `docs/YOU_DO_THIS.md` with the exact link, the exact click path, and which GitHub Actions secret to paste it into. Keep building. The user reads that file once at the end. If a free tier turns out to need a credit card, pick another free option and log it in `docs/DECISIONS.md`.
9. Browser testing is automated: Playwright drives 8 Chromium tabs against `make dev`. Tab 1 creates the room and hosts it; tabs 2 to 8 join by link. They play one full 14-minute shift with scripted inputs at 4x speed (host URL flag `?timescale=4`, debug builds only) and assert the final state hash. The cloud session has no GPU: rendering tests run on WebGL2 through SwiftShader, and GPU frame time is checked through proxies (section 9).
10. Commit after every green checklist box and push to GitHub. Conventional commit messages. Never force push.
11. Work through section 10 top to bottom. Do not skip ahead. Do not polish early.
12. At the end of each phase, write `docs/status/phase_N.md`: what works, what was swapped, budgets measured, and the deployed `*.pages.dev` URL (or "not deployed yet: waiting on `docs/YOU_DO_THIS.md`").

---

## 1. Tech stack

Verify every crate version against the current Bevy release before the first `cargo build`. Pin versions in `Cargo.toml`. If a crate below does not support the current Bevy version, use the alternative listed, and note the swap in `docs/DECISIONS.md`.

| Layer | Crate or tool | Alternative |
|---|---|---|
| Engine | `bevy` (current stable, 0.19 at time of writing) | none |
| Physics | `avian3d` | `bevy_rapier3d` |
| Netcode | `lightyear` (client prediction, server reconciliation) running over a WebRTC data channel transport. Use lightyear's WebRTC support if the current version has it; otherwise write a lightyear transport adapter over `matchbox_socket` | `bevy_replicon` with a transport adapter over `matchbox_socket` |
| Transport | WebRTC data channels via `matchbox_socket` (one reliable and one unreliable channel). Star topology: every client connects to the host | none |
| Signaling | Cloudflare Worker with one Durable Object per room code (free plan, SQLite-backed). Written in Rust with the `worker` crate. Brokers SDP and ICE only, holds no game state | TypeScript Worker if `worker` lacks Durable Object WebSocket support |
| NAT traversal | Google public STUN. Cloudflare TURN on its free allowance, with short-lived credentials minted by the signaling Worker | Metered Open Relay free tier; if neither is free without a card, ship STUN only and log it |
| Host runtime | The host simulation crate compiled to wasm and run in a dedicated Web Worker in the host's tab. Same crate compiles natively for tests and bots | none |
| Serialization | `serde`, `bincode` | `rkyv` |
| RNG | `rand_chacha` seeded per table from `getrandom` (`wasm_js` backend in the browser) | none |
| Wasm bindings | `wasm-bindgen`, `web-sys`, `js-sys`, `wasm-bindgen-futures` | none |
| Voice | Plain browser WebRTC audio in `web/voice.js`, full mesh between players (7 peer connections each at 8 players). Proximity volume and drunk pitch through Web Audio. No SDK, no media server | none |
| Persistence | `localStorage` for the player UUID, cosmetics, unlocks, and run history. IndexedDB in the host tab for the RNG audit log | none |
| UI | `bevy_ui` with the built-in widgets | `bevy_egui` for debug panels only |
| Audio | `bevy_audio` default backend | `bevy_kira_audio` |
| Asset pipeline | glTF 2.0 models, KTX2 textures with Basis Universal, Brotli on the wire | none |
| Build | `trunk` for the wasm bundle, `wasm-opt` with `-Oz` in release | `wasm-pack` |
| Hosting (static) | Cloudflare Pages, free plan, `*.pages.dev` address | GitHub Pages plus `coi-serviceworker` for the COOP/COEP headers |
| Hosting (game servers) | None. The host's browser is the server | none |
| Dev and test environment | Claude Code cloud session (Linux, no GPU) | none |
| CI | GitHub Actions: fmt, clippy, test, wasm build, Playwright, deploy | none |

Required web server headers for the client: `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`. These enable SharedArrayBuffer for wasm threads. Ship single-threaded first. Turn on threads in Phase 6 after profiling.

Cloudflare Pages rejects any single file over 25 MiB. Keep the uncompressed `.wasm` under that, or move to the GitHub Pages alternative and log it.

---

## 2. Repository layout

```
LastCall/                    repo root
  Cargo.toml                 workspace
  Makefile                   dev, test, build
  crates/
    shared/                  game rules, state types, protocol messages, RNG wrappers, economy math. No Bevy render code. Compiles for wasm and native.
    client/                  Bevy app. wasm target. Rendering, input, prediction, UI, voice bridge, host-worker launcher.
    host/                    Headless Bevy app. Authoritative simulation and room logic. Compiles to wasm (runs in the host's Web Worker) and native (tests, bots, soak runs).
    signal/                  Cloudflare Worker (Rust, `worker` crate). Room code registry, SDP/ICE relay, TURN credentials, /health, /report.
    bots/                    Native bot clients that drive the host sim through the same protocol.
    tools/                   asset bake scripts, lighting bake, texture compress, RNG log replay.
  assets/
    models/                  glTF
    textures/                KTX2
    audio/                   ogg
    fonts/
  web/
    index.html               trunk entry
    host-worker.js           boots the host wasm inside a Web Worker
    voice.js                 WebRTC audio mesh and Web Audio proximity
    styles.css
    _headers                 COOP/COEP for Cloudflare Pages
  tests/e2e/                 Playwright specs
  wrangler.toml              signaling Worker config
  docs/
    DECISIONS.md             every deviation from this plan, with reason
    PROTOCOL.md              generated message list
    YOU_DO_THIS.md           the user's one-time account steps
  .github/workflows/ci.yml
```

Use `cargo workspace` with shared `[workspace.dependencies]`. `shared` must not depend on `client` or `host`.

---

## 3. Architecture

### 3.1 Client

- Bevy app with these plugins in order: DefaultPlugins (with wasm window config), AvianPlugins, NetClientPlugin (lightyear client over the WebRTC transport), GameSharedPlugin, ClientRenderPlugin, ClientInputPlugin, ClientUiPlugin, VoicePlugin.
- Fixed timestep 64 Hz for simulation. Render unlocked, vsync on.
- Client predicts: own player movement, own held prop physics, own throw. Everything else is interpolated from host snapshots at 20 Hz.
- Client sends inputs only: movement vector, look yaw and pitch, action button states, interact target entity id, bet amount, minigame input events. Never sends state.
- Entities tagged with a stable `NetId` from the host sim. Client never spawns gameplay entities on its own except for prediction ghosts and VFX.
- The host's own client is an ordinary client. It talks to the host Worker over a `MessageChannel` loopback with the same protocol, so the host gets no special powers in code.

### 3.2 Host simulation

- Headless Bevy app (`crates/host`) compiled to wasm, running in a dedicated Web Worker in the host's tab. Tick 64 Hz. Snapshot broadcast 20 Hz with delta compression (lightyear handles it).
- `RTCPeerConnection` is not available inside Web Workers in every browser, so the host tab's main thread owns the WebRTC data channels and relays bytes to and from the Worker over `postMessage` with transferable buffers.
- Room flow:
  - Create: the host's client generates a room code (5 uppercase letters, no vowels, no 0/O/1/I), registers it with the signaling Worker, starts the host Worker, and shows the join link `/?room=CODE`.
  - Join: a client opens `/?room=CODE`, asks the signaling Worker for the host, and exchanges SDP and ICE through it. After the data channel opens, signaling is no longer involved.
  - The signaling Durable Object for a room is deleted 2 minutes after the host disconnects.
- Host leaves: the room ends. Every client shows a "Host left" screen within 5 seconds. Host migration is out of scope for v1.
- Background throttling: browsers throttle timers in background tabs. The sim runs in a dedicated Worker, and the host tab keeps an active audio context (voice) to stay unthrottled. The Phase 1 e2e test hides the host tab for 60 seconds and asserts the tick rate holds.
- Saved runs (user decision, 2026-10-09): the host saves the run at every shift's Setup in its own IndexedDB and can resume it later in a new room; see `docs/DECISIONS.md`.
- Persistence: no server-side storage. Each client stores its UUID, cosmetics, unlocks, and run summaries in `localStorage`. No accounts in v1.
- All randomness: `ChaCha20Rng` seeded from `getrandom` at room start. One RNG stream per table so a slot machine does not drain the roulette stream. Log every RNG draw with table id and tick behind an `RngLog` trait: IndexedDB in the browser (exportable from the debug menu), a JSONL file in native tests. The replay tool reads either.

### 3.3 Protocol

Define in `shared/src/protocol.rs`. Every message is a Rust enum variant with `serde`. Generate `docs/PROTOCOL.md` from doc comments with a build script.

Client to server:
- `JoinRoom { code, player_uuid, display_name, cosmetic_id }` (the `player_uuid` from `localStorage` is how a refreshed tab gets its old player back)
- `Input { tick, move: Vec2, yaw: f32, pitch: f32, buttons: u16 }`
- `Interact { target: NetId }`
- `PlaceBet { table: NetId, amount: u32, selection: BetSelection }`
- `MinigameInput { game: NetId, event: MinigameEvent }`
- `Consume { item: ItemKind }`
- `BuyUpgrade { id: UpgradeId }`
- `Vote { kind: VoteKind, yes: bool }`

Host to client:
- `Welcome { your_net_id, tick, room_state }`
- Replicated components (lightyear): `Transform`, `Velocity`, `PlayerState`, `PropState`, `TableState`, `ShiftClock`, `Economy`, `BuffState`, `Health`
- `Event` stream: `BetResolved`, `CardDealt`, `WheelResult`, `ReelResult`, `ChaosStarted`, `ChaosEnded`, `ShiftEnded`, `DebtPaid`, `DebtMissed`, `PlayerPassedOut`, `Chat`

### 3.4 Shared game logic

`shared` holds pure functions. Examples: `blackjack::hand_value(cards) -> u8`, `roulette::payout(selection, result) -> u32`, `economy::next_payment(week) -> u32`, `buffs::apply(state, item) -> state`. Unit test every one of these. The host sim calls them. The client calls them only for UI preview.

---

## 4. Core design

### 4.1 Setting

A dive bar called Last Call on a rainy pier town street. Rooms: main bar, back office, back room (poker), basement (fight pit), roof (hoop), kitchen, parking lot (football and soccer), pier (fishing). All rooms and activities are open from the start (user decision, 2026-10-09: no debt-tier unlocks).

### 4.2 The goal

The crew owes the loan shark `$120,000`. Pay it off and the crew buys the bar. That is the win screen. Miss two payments in a row and the enforcers burn the bar down. That is the loss screen. Then the run restarts with a higher starting debt and more chaos (new game plus).

### 4.3 Time structure

- A run is made of weeks. A week is 3 shifts. Each shift is 14 real minutes.
- Shift phases:
  1. Setup (2 min): buy upgrades, stock the bar, talk, place props.
  2. Open (9 min): customers arrive in waves, games run, chaos events fire.
  3. Last call (2 min): customers leave, players count cash and chips, settle personal debts to each other.
  4. Payment (1 min): the loan shark's cut is due at the end of shift 3 each week. Other shifts show the running total.
- Between runs there is no persistent money. Cosmetics persist.

### 4.4 Economy

Two pools of money:

- House pool (shared): earned from customer losses at the tables, drink sales, and minigame entry fees from NPCs. Pays the loan shark. Buys upgrades. Visible on the office wall as a big LED sign.
- Personal pocket (per player): earned from tips, a 10% dealer commission on house wins at the table you are running, and your own gambling wins. Spent on your own bets, beers, Zeens, and cosmetics.

Players can move money from pocket to house pool at the office safe. Never the other way. Arguments about this are the point.

Payment schedule (per week, `shared::economy::next_payment`):
`week 1: 8,000 | week 2: 11,000 | week 3: 15,000 | week 4: 20,000 | week 5: 26,000 | week 6: 40,000`
Total: 120,000. Remaining balance after week 6 counts as paid if the house pool covers it.

Customer waves (per shift, scales with week):
- Base customers per wave: `4 + week * 2`. Waves every 90 seconds during Open.
- Each customer carries `rand(40, 400) * (1 + 0.15 * week)` cash.
- Customer behavior: pick a table by weighted preference (upgrades change weights), bet until they hit their "walk away" threshold (lose 70% or win 150% of their cash), buy a drink every 2 minutes if a bar stool is free.
- House edge is real and fixed per game (section 5). The house makes money if players deal correctly and keep customers happy.

### 4.5 Progression and upgrades

User decision (2026-10-09): every room and activity below is available from the start of a run. Debt tiers no longer unlock anything; the tier number stays as a progress marker. Upgrades are bought from the house pool during Setup.

| Tier | Paid so far | Unlocks |
|---|---|---|
| 0 | 0 | Main bar: blackjack (1 table), roulette (1 wheel), slots (2 machines), beer taps. Office. |
| 1 | 8,000 | Kitchen (food buffs). Second blackjack table. |
| 2 | 19,000 | Pier: fishing. Fish sell to the kitchen. |
| 3 | 34,000 | Roof: basketball hoop bets. |
| 4 | 54,000 | Parking lot: soccer penalty kicks, football gauntlet. |
| 5 | 80,000 | Basement: FPS fight pit. Back room: high-stakes blackjack. |
| 6 | 120,000 | Win. |

Upgrade list (`shared::upgrades`), each with cost, effect, and max rank:

- Felt Upgrade (3 ranks, 1,500 / 3,000 / 6,000): table max bet x2 per rank. Customers with more cash sit down.
- Bigger Tap Wall (3 ranks, 1,000 / 2,000 / 4,000): more beer types, faster pour.
- Security Camera (1 rank, 2,500): shows cheating customers with a red outline. Catching one earns a 500 bonus.
- Bouncer NPC (2 ranks, 4,000 / 8,000): reduces brawl chaos duration by 50% per rank.
- Jukebox (1 rank, 1,200): customers stay 30% longer. Players can queue tracks.
- Lucky Charm Shelf (1 rank, 2,000): sells one-shift single-use items (rigged die, marked deck, cold brew).
- Generator (1 rank, 3,000): power outage chaos lasts 10 s instead of 60 s.
- Fire Extinguisher (1 rank, 800): kitchen fire chaos can be ended by a player.
- Neon Sign (3 ranks, 2,000 each): +1 customer per wave per rank.
- Back Door (1 rank, 5,000): during a raid, players can carry cash out the back. Cash in hand at raid end is kept. Cash on tables is seized.

Personal unlocks (persist across runs, cosmetic only): hats, lighters, voice lines, chip skins. Earned by achievements: "Hit 21 three times in a shift", "Pass out on the roof", "Catch the boot".

### 4.6 Consumables and buffs

Beer (from tap, costs 5 from pocket, customers pay 8):
- Drunk meter 0 to 100. Each beer +20. Decays 1 per 2 seconds.
- 20 to 40: Courage. Personal max bet x1.5. Slight camera sway.
- 40 to 70: Sloppy. Throw accuracy -40%. Walk speed +10%. Screen blur. Voice pitch shifted down for others (client-side effect flag).
- 70 to 99: Wasted. Random stumble every 8 s. Cannot deal. Bet buttons shuffle positions.
- 100: Pass out. Ragdoll for 45 s. Other players can drag you. Props can be stacked on you.

Zeen (nicotine pouch, costs 3, from the office drawer):
- Focus meter 0 to 100. Each pouch +35. Decays 1 per second.
- 1 to 60: Focus. Card count hint shows on blackjack UI. Aim sway -50%. Fishing tension gauge wider.
- 61 to 100: Buzzed. Hands shake (UI jitter). 20% chance per 10 s of a gag animation that drops held item.
- Beer and Zeen stack. Drunk over 40 plus Focus over 60 triggers "The Spins": camera rolls and the player vomits, clearing both meters, and creating a slip hazard prop.

Kitchen food (Tier 1): fries clear 15 drunk. Burger clears 30 drunk and gives 10% max health for the fight pit. Fish plate (from fishing) gives +10 pocket on sale and a luck buff: one reroll on the next losing roulette spin.

### 4.7 Chaos events

Fired by the server during Open. One per shift at week 1, up to three per shift by week 6. Random pick, weighted. Each has a start, a duration, a player counter, and a consequence.

| Event | Duration | What happens | Counter | Consequence if ignored |
|---|---|---|---|---|
| Police Raid | 60 s | Siren, lights, cops enter after 20 s | Hide chips in the safe, carry cash out the back door (upgrade), stand customers away from tables | All cash on tables seized |
| Brawl | 45 s | Two customers ragdoll fight, props fly | Grab and throw them out the door (ragdoll carry) | Customers leave, 3 props break |
| Power Outage | 60 s | Darkness, tables pause, slots freeze | Flip breaker in basement stairwell | 60 s of no income |
| Rigged Slot Jam | until fixed | One slot machine pays out 10x | Hit it with a stool (no, that makes it worse) or turn the service key | House loses up to 2,000 |
| Health Inspector | 90 s | NPC walks the bar | Hide beers, clean vomit props with the mop | Fine of 1,500 |
| Loan Shark Visit | 30 s | He sits at a table and plays | Deal him fairly. He tips big if he wins. | He breaks a table if the dealer is drunk |
| Kitchen Fire | 40 s | Smoke, customers cough and leave | Extinguisher (upgrade) or throw beer on it | Kitchen offline next shift |
| Card Counter | until caught | A customer wins every hand | Security Camera reveals him. Throw him out. | He drains the table pool |

---

## 5. Minigames

Every minigame follows one contract in `shared::minigame`:

```rust
pub trait Minigame {
    type Input;    // what the client sends
    type State;    // replicated
    type Outcome;  // resolved on server
    fn start(rng: &mut TableRng, params: &Params) -> Self::State;
    fn apply(state: &mut Self::State, who: PlayerId, input: Self::Input, tick: u64) -> Option<Self::Outcome>;
    fn payout(outcome: &Self::Outcome, bets: &[Bet]) -> Vec<Payout>;
}
```

Server runs the state machine. Client renders state and sends inputs. Customers (NPCs) are server-side bots that call `apply` with scripted inputs.

### 5.1 Blackjack

- Standard rules. 6-deck shoe, reshuffle at 75% penetration. Dealer stands on soft 17. Blackjack pays 3 to 2. Double on any two. Split once. No surrender. Insurance offered when dealer shows an Ace (pays 2 to 1).
- One player is the dealer. Dealer earns 10% of house wins at that table into pocket. Dealer must press Deal, Hit, Stand on behalf of the house by the rules. The server enforces the rules; the dealer's job is speed. A slow dealer makes customers leave. Dealer drunk over 70 cannot deal.
- Up to 5 seats. Players and customers mix at seats.
- Card physics: cards are kinematic on the felt, not dynamic. Chips are dynamic rigid bodies. Dropping a chip stack on the floor is a real loss until someone picks it up.
- House edge target: about 0.6% against perfect basic strategy. Customers play a simple strategy table plus a 15% chance per decision of a mistake, which pushes the real edge against customers higher. That is intended: it is where the house makes its money.
- Zeen Focus shows a running Hi-Lo count on the dealer's UI.

### 5.2 Roulette

- European wheel, single zero. Bets: straight (35 to 1), split (17 to 1), street (11 to 1), corner (8 to 1), line (5 to 1), column and dozen (2 to 1), red/black, odd/even, high/low (1 to 1).
- One player spins. Spin is a server timer with a physics-driven visual on the client. Result decided server-side the moment the spin starts; the client animation lands on it. Minimum 6 s spin.
- The "croupier" earns 10% commission. Must clear losing chips with the rake (physics sweep) before the next spin or customers get impatient.
- Rigged die item from the Lucky Charm Shelf: forces one spin to land in the chosen dozen. Server checks item ownership.

### 5.3 Slots

- 3 reels, 5 symbols each, 1 payline. Symbols: cherry, lemon, bell, bar, seven. Return to player (RTP) 92%. Paytable in `shared::slots::PAYTABLE`.
- Pull lever is a physics interaction. Reel stop is server-decided; client animates.
- No player job. Customers use these. Players can also play. Slots are the quiet income.
- Rigged Slot Jam chaos sets RTP to 1000% on one machine until fixed.

### 5.4 Beer serving

- Pour: hold interact on the tap. A fill gauge rises. Foam gauge rises faster if the glass is tilted wrong (mouse Y controls tilt). Release in the green zone for a perfect pour (customer pays 8 and tips 2). Overflow wastes the beer and makes a puddle prop (slip hazard).
- Carry: glasses are dynamic rigid bodies. Running spills them. Walking is safe. Drunk players spill more.
- Serve: drop the glass on the customer's stool zone. Serve time under 20 s or the customer leaves.
- Later taps (upgrade) add stout (slow pour, pays 12) and the "Boot" (2 liters, pays 30, 2 hands required, pass-out risk for the customer who then ragdolls off the stool).

### 5.5 Fishing (Pier, Tier 2)

- Cast: hold to charge, release. Distance maps to depth zone. Three zones with different fish tables.
- Bite: server timer with rng (5 to 25 s). Client shows a bobber dip. Press within 600 ms to hook.
- Reel: tension gauge. Hold reel to raise tension, release to lower. Keep tension in the band for N seconds (N by fish size). Fish pulls in server-scripted patterns. Line snaps at 100%.
- Fish table: minnow (sells 5), bass (20), catfish (45), shark (300, 1% chance, 90 s fight), boot (0, achievement).
- Bets: other players can bet pocket money on "lands it" vs "snaps" at 1 to 1 while a fight is on.
- Focus buff widens the tension band.

### 5.6 Basketball (Roof, Tier 3)

- One hoop, one ball (dynamic). Shot: hold to charge arc height, aim with look, release. Server simulates the ball with the same avian settings as the client. Client predicts the ball, server corrects.
- Contests: "3 of 5" (entry fee from pocket, pot to best). "HORSE" for 2 to 4 players. "Around the World" solo for customers.
- Customers bet on players. A player sinking 5 of 5 draws a crowd and raises drink sales 20% for 2 minutes.
- Drunk over 40: aim sway. Wind on the roof scales with week.

### 5.7 Soccer (Parking lot, Tier 4)

- Penalty shootout only. Goal, ball, a goalie (player or NPC).
- Kicker: aim with look, hold for power, release. Power over 80% adds error. Curve from mouse X at release.
- Goalie: pick a dive direction and timing. NPC goalie is drunk-weighted: dives late at week 1, reads well by week 6.
- Rounds of 5. Pot from entry fees. Side bets open from spectators.

### 5.8 Football (Parking lot, Tier 4)

- Two modes.
- Field Goal: ball on a tee, kick with the soccer controls, uprights at 20, 30, 40 yards. Payout 1 to 1, 2 to 1, 4 to 1.
- Gauntlet: one runner with the ball, 3 to 6 NPC tacklers in a lane, 20 s. Runner uses dodge (double tap direction, 1 s cooldown) and stiff arm (push, physics impulse). Reach the end zone to double the entry fee. Tacklers ragdoll the runner on contact.

### 5.9 Fight Pit FPS (Basement, Tier 5)

- Airsoft arena. 60 s rounds. 2 to 8 players. Free for all or 2v2 to 4v4 by vote.
- Weapons on wall racks: pistol (semi, 12 rounds, 2 hit kill), SMG (auto, 30 rounds, 4 hits), pump shotgun (6 shells, 1 hit close, spread), foam bat (melee, knockback).
- Hitscan with server-side lag compensation (lightyear interpolation delay rewind, 200 ms max). Headshots 1.5x.
- "Hit" means the player ragdolls for 3 s, then respawns at a corner. Kills score 1. Most kills wins the pot. Ties split.
- Spectators above the pit can throw beers down. A beer hit gives the target +20 drunk.
- Drunk sway and Zeen focus both apply to aim.

---

## 6. Controls

Keyboard and mouse. Gamepad in Phase 7.

- WASD move, Shift sprint (spills drinks), Space jump (low), Ctrl crouch, E interact / pick up, F throw (hold to charge), Q drop, R use consumable, Tab scoreboard and house pool, 1 to 4 hotbar, V push to talk override (voice is open mic by default), Esc menu.
- Mouse look. Left click: primary action at a table or fire in the pit. Right click: secondary (double down, aim down sights).

---

## 7. Visual style

- Low-poly stylized, chunky proportions, 1.5 heads tall characters with big hands. Think toy figures in a real bar.
- Palette: warm amber bar interior, cold blue rain outside, neon pink and cyan accents. One accent color per room.
- Lighting: baked lightmaps for static geometry (tool in `crates/tools`). Real-time: 1 directional light outside, up to 8 point lights per room, shadows only from the directional. Emissive neon with bloom.
- Post: bloom, slight vignette, chromatic aberration tied to drunk meter, screen blur tied to drunk meter, film grain at 0.05.
- Characters: one base mesh, 12 blend-shape faces, hats as attached meshes. Ragdoll with 11 bodies.
- Props: every prop under 300 triangles. Every room under 60k triangles total. Max 4 draw calls per prop. Use GPU instancing for chips, cards, bottles.
- Texture budget: 2048 atlas per room, KTX2 compressed. No 4k textures.
- UI: diegetic where possible. Bet chips are physical. The house pool is a wall sign. The shift clock is a bar clock. Minimal screen UI: hotbar, drunk and focus meters as two beer-glass icons, crosshair.

---

## 8. Audio

- Voice: WebRTC audio full mesh between the players in a room (every pair gets one audio peer connection, set up through the same signaling Worker). Opus at 24 to 32 kbps per stream. Client sets per-remote-player volume from 3D distance: full at 2 m, zero at 14 m, occluded by walls at -60% (raycast once per 100 ms). Drunk effect: pitch shift applied by the listening client through Web Audio.
- Music: jukebox tracks, 5 original loops, spatialized from the jukebox. Royalty-free or original only.
- SFX: chip clink, card snap, wheel tick, reel spin and stop, tap pour, glass break, ragdoll thud, siren, cash register. Pooled, max 32 voices.

---

## 9. Performance budgets

| Item | Budget |
|---|---|
| Client frame time | 8 ms at 1080p on Iris Xe |
| Draw calls per frame | 400 |
| Triangles on screen | 250k |
| Physics bodies awake | 300 |
| Network up (game data) | 8 KB/s per client |
| Network down (game data) | 40 KB/s per client |
| Host network up (game data) | 300 KB/s at 8 players |
| Voice | 32 kbps per stream, 7 streams each way at 8 players |
| Host sim tick | 64 Hz, under 6 ms per tick at 8 players, measured as wasm in a headless Chromium Worker |
| Host main thread | Client frame plus relay work still inside the client frame budget |
| wasm binary | 12 MB Brotli, under 25 MiB uncompressed (Cloudflare Pages file limit) |
| Total first load | 40 MB |
| Time to lobby from link | 15 s |

Profile every phase with `bevy::diagnostic` overlays and Chrome Performance. Fail the phase if a budget is missed. Add `tracy` support for native profiling builds.

The cloud session has no GPU, so GPU frame time cannot be measured there. Automated runs enforce the proxies instead: draw calls, triangles on screen, awake physics bodies, and main-thread CPU time per frame (8 ms) in headless Chromium on SwiftShader. The real 60 FPS on Iris Xe target stays a design target, kept honest by the draw-call and triangle budgets. Record measured numbers in each phase status file.

---

## 10. Build phases

Each phase ends with a demo build at a URL and a checklist. Do not start the next phase until every box is checked.

### Phase 0: Skeleton (week 1)
- [x] Workspace compiles for `wasm32-unknown-unknown` and native.
- [x] Bevy window renders a cube in headless Chromium, Firefox, and WebKit through Playwright. The WebGPU path is compiled in and feature-detected; it is tested wherever the cloud browser exposes a WebGPU adapter, and the WebGL2 fallback is tested everywhere. Log in `docs/DECISIONS.md` which paths could and could not be verified in the cloud.
- [x] Host sim boots headless natively and inside a Web Worker in Chromium, and reports its tick rate.
- [x] Signaling Worker runs under `wrangler dev` and answers `/health`.
- [x] CI runs fmt, clippy with `-D warnings`, tests, wasm build, native build, Worker build.
- [x] `docs/DECISIONS.md` created with crate versions chosen.
- [x] `docs/YOU_DO_THIS.md` lists the Cloudflare account and API token steps. Deploy jobs in CI skip cleanly while the secrets are missing.

### Phase 1: Lobby and netcode (weeks 2 to 3)
- [x] Create Room returns a code and a link. Opening `/?room=CODE` joins the host over WebRTC.
- [x] 8 clients (host plus 7) walk around a gray-box bar with capsule bodies. Client prediction on own movement, interpolation on others.
- [x] Physics props: 20 bottles, 100 chips, 10 stools. Pick up, carry, throw, drop. Host-owned, client-predicted while held.
- [x] Voice works between two tabs using Chromium's fake media device. Proximity falloff verified automatically: a received-level meter (Web Audio `AnalyserNode`) drops as the bot walks away.
- [x] Reconnect: refresh a non-host tab and rejoin the same room as the same player.
- [x] Host leaves: closing the host tab shows "Host left" on every client within 5 s.
- [x] Host tab hidden for 60 s keeps a 64 Hz tick.
- [x] Budgets: time to lobby under 15 s. Net up and down within budget with 8 bots.

### Phase 2: Shift loop and economy (week 4)
- [x] Shift phases with timers and server-driven transitions.
- [x] House pool, pockets, safe transfer, loan shark payment, win and loss screens.
- [x] Customer NPC: spawn, walk to a stool, buy a drink, leave. Navmesh with `vleue_navigator` or `oxidized_navigation` (verify Bevy support).
- [x] Beer tap minigame complete. Pour, carry, serve, tips.
- [x] Drunk meter and all four drunk tiers including pass-out ragdoll and drag.

### Phase 3: Casino core (weeks 5 to 7)
- [x] Blackjack full rules, server shoe, dealer role, customer bots with mistake rate, chip physics.
- [x] Roulette full bet grid, spin, rake sweep, payouts.
- [x] Slots with paytable and lever.
- [x] RNG audit log (IndexedDB in the browser, JSONL natively). Replay tool in `crates/tools` that re-derives every outcome from the log.
- [x] House edge test: 100,000 simulated hands or spins per game in a unit test. Blackjack edge between 0.4% and 0.9% against a perfect basic-strategy bot. Roulette edge 2.7%. Slots RTP 91% to 93%. Also report (do not gate on) the blackjack edge against the 15%-mistake customer bot.

### Phase 4: Chaos and upgrades (week 8)
- [x] All eight chaos events with counters and consequences.
- [x] Upgrade shop in the office. All upgrades functional.
- [x] Zeen consumable with Focus tiers and The Spins.
- [x] Room doors. No tier gates: every room is open from the start (user decision, 2026-10-09).

### Phase 5: Side minigames (weeks 9 to 12)
- [ ] Fishing with bets.
- [ ] Basketball with 3 of 5 and HORSE.
- [ ] Soccer penalties.
- [ ] Football field goal and gauntlet.
- [ ] Fight pit with four weapons, lag compensation, spectator beers.
- [ ] Each minigame has a unit-tested `Minigame` impl and a bot that can play it.

### Phase 6: Art and performance pass (weeks 13 to 15)
- [ ] Replace gray box with final meshes per section 7.
- [ ] Baked lighting pipeline in `crates/tools`. One command bakes all rooms.
- [ ] KTX2 pipeline. Asset size report in CI.
- [ ] Post-processing stack tied to drunk and focus.
- [ ] Enable wasm threads behind the COOP/COEP headers. Measure. Keep only if frame time improves.
- [ ] All budgets in section 9 green (GPU frame time through its proxies) in the Playwright run in the cloud session with Chromium, Firefox, and WebKit. Record the cloud machine specs in `docs/status/phase_6.md`.

### Phase 7: Polish and release (weeks 16 to 18)
- [ ] Gamepad. Settings menu: sensitivity, volume, voice mode, graphics preset (Low disables shadows and bloom).
- [ ] Achievements and cosmetics with `localStorage` UUID persistence.
- [ ] New game plus scaling.
- [ ] Tutorial shift: scripted first shift with prompts.
- [ ] Crash reporting: wasm panics to the signaling Worker's `/report` endpoint, stored in Workers KV on the free plan (capped, oldest dropped).
- [ ] Deploy from CI: Cloudflare Pages for the client, `wrangler deploy` for the signaling Worker.
- [ ] Load tests: 50 rooms of 8 bots joining through the signaling Worker under `wrangler dev`, all connected within budget. Soak: one host sim with 8 bots for 1 hour, natively and as wasm in headless Chromium. No tick over 10 ms.

---

## 11. Testing rules

- Every function in `shared` has unit tests. Payout math has property tests with `proptest`.
- Every minigame has a headless integration test: native host sim plus 8 bot clients over an in-memory transport, 1,000 ticks, no panics, state stays consistent (hash of replicated state on each client equals the host hash every 100 ticks).
- Every phase has an automated Playwright test in `tests/e2e/phase_N.spec.ts` that drives real browser tabs against `make dev` in the cloud session, and runs again in CI. No human test is required to close a phase.
- Determinism: the host simulation must be deterministic given the same inputs and RNG seed. Add a replay test that runs a recorded 14-minute shift twice and compares final state hashes. Run it natively and as wasm, and assert both produce the same hash.

---

## 12. Deployment

Everything deploys from GitHub Actions. Nothing deploys from the user's PC.

- Client: `trunk build --release`, `wasm-opt -Oz`, upload `dist/` to Cloudflare Pages with `wrangler pages deploy`. Cloudflare compresses with Brotli at the edge. Set COOP/COEP headers in `web/_headers`.
- Signaling: `wrangler deploy` for `crates/signal`. One Durable Object per room code on the free plan.
- TURN: the signaling Worker mints short-lived Cloudflare TURN credentials per join. Verify the free allowance and that it needs no card before relying on it.
- Game servers: none. The host's browser runs each room.
- Secrets: `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`, and the TURN key, as GitHub Actions secrets and Worker secrets only. Never in the repo.
- Address: the free `*.pages.dev` subdomain (for example `last-call.pages.dev`, or the closest available name). No paid domain.
- Free-tier limits to stay inside (verify current numbers when deploying, log them in `docs/DECISIONS.md`): Pages file size 25 MiB, Workers free plan requests per day, Durable Object duration per day, Workers KV writes per day, GitHub Actions minutes (unlimited for a public repo, 2,000 per month for a private one).

---

## 13. Decisions already made

Claude Code must not re-open these.

- Rust and Bevy on wasm. No other engine.
- $0 total. Free tiers only, no credit card anywhere, no paid domain.
- Nothing installed or running on the user's PC. Build, test, and deploy from the Claude Code cloud session and GitHub Actions. No Docker.
- Host authoritative: the room host's browser runs the simulation in a Web Worker. No dedicated game servers. Client prediction for movement and held props only.
- WebRTC data channels (star, through the host) for game traffic. Cloudflare Worker plus Durable Objects for signaling only. No WebTransport, no WebSocket game servers.
- Peer-to-peer WebRTC audio mesh for voice. No LiveKit or other media server.
- No database. `localStorage` and IndexedDB only.
- European roulette, single zero.
- 6-deck blackjack, dealer stands on soft 17.
- 8 players max per room.
- 14-minute shifts, 3 shifts per week, 6 weeks per run, 120,000 debt.
- No accounts, no real money, no purchasable currency.
- Nicotine product is called "Zeen". Beer brands are invented: "Pier Light", "Dock Stout", "The Boot".
- No licensed music, no licensed sports team names or logos.
- Low-poly stylized art.
- Gray-box everything first. Art is Phase 6.

Anything else that comes up: pick the simplest option that keeps section 0 true, write the choice in `docs/DECISIONS.md`, and continue.
