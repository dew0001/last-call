# Phase 4 status: Chaos and upgrades

Date: 2026-10-10. Branch: `main-ygr4w2`.

## What works

- **Rooms and doors.** Nine rooms on one floor plane (`shared::world`): the bar, office, kitchen, back room, basement stairwell, basement, roof deck, parking lot and pier. Walls follow each room's edges with door gaps, so each room is reached through its door. Every room is open from the start (user decision, 2026-10-09). The camera stays inside the player's room; indoor rooms have lamps, outdoor areas a cold moonlight.
- **Zeen and The Spins.** The office drawer sells Zeen ($3, +35 Focus). Focus decays a point a second. Buzzed players shake (UI jitter) and may gag and drop what they hold. Drunk over 40 with Focus over 60 brings on The Spins: the camera rolls for 3 s, both meters clear, and a vomit puddle stays until someone mops it (the mop starts in the kitchen).
- **Kitchen food.** Fries (clear 15 drunk) and burgers (clear 30, well fed) at the kitchen pass. The money goes to the house.
- **Upgrade shop.** All ten upgrades at the office terminal, bought from the house pool during Setup. Every one has its effect: table max and customer cash (Felt), pour speed (Tap Wall), the cheat outline and catch bonus (Security Camera), shorter brawls (Bouncer), patience and tracks (Jukebox), charms (Lucky Charm Shelf), a 10 s outage (Generator), putting out the fire (Extinguisher), bigger waves (Neon), keeping chips in hand in a raid (Back Door). Upgrades are saved with the run.
- **Charms.** Rigged die (forces a roulette dozen; logged and replayed as a `RiggedSpin`), marked deck (shows its holder the hole card), cold brew.
- **Chaos events.** All eight, each with its counter and consequence: raid, brawl, power outage, rigged slot jam, health inspector, loan shark, kitchen fire, card counter. One per shift in week 1, up to three by week 6, picked by weight from the chaos RNG stream. Players haul brawlers and the card counter out the front door with R. A banner shows what is running and what to do; the outage turns the lights off; the fire, cops, inspector and visitors have their own colors.
- **Hi-Lo count.** With Focus, the blackjack panel shows the running count of the cards this client has seen since the shuffle.

## Tests

- **Shared** (106 unit and property tests): the world map (every room reachable through its door, no walk leaves the map), Focus tiers and The Spins, food, upgrade costs and effects, fixture menus, chaos weights, schedules and durations, the Hi-Lo values.
- **Host** (`crates/host/tests/chaos.rs`, 13 tests): each event's counter and its consequence: the breaker, the service key, the jam cap, the inspector's fine and clean pass, a raid seizing bar chips but not office chips, hauling both brawlers out, an unchecked brawl breaking three props, a thrown beer and the extinguisher on the fire, the kitchen closed next shift, the card counter outlined and caught for the bonus, the loan shark breaking the table for a drunk dealer and leaving a sober one alone, and a shift scheduling its own event.
- **Host** (`crates/host/tests/fixtures.rs`, 4 tests): Zeen and food, the shop, a rigged die landing in each dozen with the audit log replaying, The Spins with vomit.
- **Bots:** the Phase 2 and 3 bot tests pass; the casino consistency test runs with chaos off.
- **Browser** (`tests/e2e/phase_4.spec.ts`, 3 tests): two pouches of Zeen on top of beer bring on The Spins; the terminal sells a Felt Upgrade from the house pool; a forced outage ends at the breaker.
- **Replay:** the 14-minute replay (now with its scheduled chaos) has a new golden hash.

## What was swapped or deferred

See `docs/DECISIONS.md`, Phase 4 section. Main items:
- Rooms are on one floor plane: no stairs yet, so the basement and roof are at floor level.
- Not modeled: customers standing away from tables in a raid; hitting the jammed machine with a stool; extra beer types on tap (with the tap wall mesh in the art pass).
- The marked deck's hole card is replicated to every client (only its holder's UI shows it).
- Gray-box fixtures (colored posts and text menus); the art pass brings meshes and diegetic UI.

## Budgets measured

| Item | Budget | Measured |
|---|---|---|
| Client wasm (WebGL2), release | 25 MiB raw, 12 MB Brotli | 23.13 MB raw, 4.57 MB Brotli |
| Host wasm, release | | 11.57 MB raw, 2.07 MB Brotli |
