# Phase 5 status: Side minigames

Date: 2026-10-10. Branch: `main-ygr4w2`.

## What works

- **Fishing** (pier, two spots). Hold C and let go to cast; the charge picks the depth zone and its fish table (minnow, bass, catfish, shark at 1%, the boot). The bite comes in 5 to 25 s; strike (B) within 600 ms. Hold N to reel: keep the tension in the band (wider with Focus) until the fish tires; full tension snaps the line, 3 s of slack lets it go. Others bet "lands it" or "snaps" at 1 to 1. A landed fish sells at once, and can become a fish plate at the kitchen (+$10 and a lucky roulette reroll).
- **Basketball** (roof). Shots fly under gravity and the roof wind (stronger each week), with aim error from drink, halved by Focus. 3 of 5 and HORSE with a $20 entry pot; a 5 of 5 draws a crowd that buys drinks 20% better for two minutes.
- **Soccer penalties** (parking lot). Aim with the look, charge with C, curve with A or D at release; power over 80 adds error. An NPC goalie that dives late in week 1 and reads kickers by week 6, or a player in goal diving left, center or right. Shootouts of five kicks; spectators bet goal or miss.
- **Football** (parking lot). Field goals from 20, 30 and 40 yards paying 1, 2 and 4 to 1. The gauntlet: 3 to 6 tacklers by week, 20 s, dodge (double-tap A or D, 1 s cooldown) and stiff arm (G); reach the end zone to double the stake.
- **Fight pit** (basement). 60 s rounds for 2 to 8, free for all or teams by vote. Pistol, SMG, pump shotgun (eight pellets with falloff) and foam bat (knockback) from wall racks. Hitscan with lag compensation (rewind up to 200 ms), headshots 1.5x, down for 3 s then up at a corner, most takedowns take the pot. A thrown beer that hits any player adds 20 drunk.
- Every game draws from its own RNG stream, logged in the audit log.

## Tests

- **Shared** (136 tests in the crate): each game's rules as a `Minigame` or contest: zones and fish tables, bite and hook timing, a steady hand landing every fish, snaps and escapes, bets; shot flights from all over the court, wind and aim error, 3 of 5 and HORSE; kicks, the goalie by week, shootouts, field goal power and payouts; the gauntlet's tackles, dodges, stiff arms and a scripted runner beating week 1; ray hits on bodies and heads, hits to take down per weapon, rounds, teams, the 60 s clock.
- **Host** (`crates/host/tests/games.rs`, 11 tests): a fisher landing a fish with a winning bettor, walking off the pier; 5 of 5 winning the pot and a crowd; HORSE; a shootout against the NPC; a player goalie's save with bets; field goals by distance; the gauntlet tackling a runner who stands still and paying one who scores; the pit hitting a target that moved since the shooter saw it, refusing a rewind older than 200 ms, a takedown, the respawn and the pot; a thrown beer adding drunk. Plus the fish plate and the lucky reroll (`fixtures.rs`).
- **Bots** (`crates/bots/tests/games.rs`): eight bots walk to the stations and play every game for 70 s (two fishers, a shooter, a kicker, a field goal kicker, a runner, two fighters). Every game makes progress and pays out, and every 10 s each client's replicated state matches the host's (56 checks).
- **Browser** (`tests/e2e/phase_5.spec.ts`, 4 tests): a fish hooked and fought with an in-page reel; five perfect shots winning 3 of 5 and drawing a crowd; a field goal, a penalty and a gauntlet run; a pit hit from another tab across WebRTC, with lag compensation.

## What was swapped or deferred

See `docs/DECISIONS.md`, Phase 5 section. Main items:
- Balls are deterministic arithmetic shared by host and clients, not avian bodies with client prediction.
- Not built: customers betting on basketball players and their "Around the World"; a ragdoll on a gauntlet tackle.
- Field goal distances are notional (the lot is shorter than 40 yards).

## Budgets measured

| Item | Budget | Measured |
|---|---|---|
| First load (WebGL2 variant), Brotli | 12 MB | 7.76 MB |
