# Phase 3 status: Casino core

Date: 2026-10-09. Branch: `main-ygr4w2`.

## What works

- **Blackjack.** One table with five seats. 6-deck shoe, reshuffle at 75%, dealer stands on soft 17, blackjack pays 3 to 2, double on the first two cards, split once, insurance 2 to 1, no surrender. A player runs the table from the dealer spot: T takes the deal, Enter deals, H and G hit or stand for the house. The host accepts only the press the rules call for. The dealer earns 10% of the house's win on each round. A Wasted player cannot deal. Players bet by standing at the table; customers walk to free seats, play the strategy table with a 15% mistake rate, and leave at the walk-away thresholds or after 30 s without a dealer.
- **Roulette.** European, single zero, the full bet grid (straight, split, street, corner, line, column, dozen, red and black, odd and even, high and low). The croupier spins; the result is drawn at once and the wheel lands on it after 6 s. Losing bets leave chips on the layout, and the croupier rakes them off before the next spin. The croupier earns 10% of the house's win.
- **Slots.** Two machines, 3 reels, 5 symbols, 1 payline, the paytable in `shared::slots`. The stops are drawn when the lever is pulled; the reels spin 2 s on clients. Coins go straight into the pocket.
- **Chip physics.** A player's table winnings land on the felt as a chip stack (a dynamic prop worth the payout). Whoever picks it up gets the money. Losing roulette chips are real props the rake sweeps.
- **Drunk tiers at the tables.** Courage and worse bet 1.5 times the table maximum. Wasted cannot deal or run the wheel, and the bet buttons shuffle once a second.
- **RNG audit log.** Every draw from every stream, with its tick, plus each shuffle, spin and slot pull. The browser host stores it in IndexedDB (the host page's "RNG log" button exports it); `host-native --audit FILE` writes JSONL. `tools replay FILE` rebuilds every stream from the seed and derives every outcome again; a changed draw or outcome fails.
- **Determinism.** The 14-minute replay now has a dealer and a croupier serving customers (123 blackjack rounds, 68 spins, 287 slot pulls) and hashes the same natively and in the browser's wasm host.

## Tests

- **House edge** (`crates/shared`): blackjack 0.64% over 1,000,000 hands of basic strategy (gate 0.4% to 0.9%); the 15%-mistake customer gives 10.4% (reported). Roulette exactly 1/37 over every bet on the layout, plus 100,000 simulated spins. Slots RTP exactly 91.91% over all 8,000 stop combinations (gate 91% to 93%), plus 100,000 simulated pulls. Property test: every roulette bet returns 36 stakes over 37 pockets.
- **Rules** (`crates/shared`): hand values, soft 17, peeking, insurance, doubles, splits (split aces take one card), turn order, the strategy table, customer mistake rate, every roulette payout and invalid bet, the paytable, table reach and spots, walk-away thresholds, audit log tampering.
- **Host** (`crates/host/tests/casino.rs`, local players): dealing ten rounds with money conserved; winnings waiting on the felt until picked up; a Wasted player cannot deal; Courage bets $150; a roulette spin with payouts, losing chips, the rake and the spin lock; 20 slot pulls; customers at every table; a busy room's audit log replays and a tampered line fails.
- **Bots** (`crates/bots/tests/consistency.rs`): 8 bot clients over the in-memory transport play the casino (a dealer, a croupier, two blackjack players, two roulette players, two slot players, plus customers) for 1,000 ticks; every 100 ticks each client's replicated state hash matches the host's. All 80 checks pass. The bar test from Phase 2 passes too.
- **Browser** (`tests/e2e/phase_3.spec.ts`, 6 tests): blackjack with money conservation and a chip pick-up; roulette with the result known at spin start, payouts and the rake; slots against the paytable; Wasted cannot deal and Courage bets $150; customers at every game, with the audit log exported from IndexedDB and replayed in wasm; table keys (T, 2, Enter).

## What was swapped or deferred

See `docs/DECISIONS.md`, Phase 3 section. Main items:
- Gray-box table UI: a text panel and blank card tiles. Physical chips for bets, card faces and the diegetic UI belong to the art pass (Phase 6).
- No double after a split (the plan's "double on any two" read as the first two cards).
- The blackjack edge test runs 1,000,000 hands instead of 100,000, so the gate is not at the mercy of sampling noise.
- `Minigame::apply` takes the table's RNG instead of a tick.
- Spawn points moved so every lane straight ahead is clear of the tables.

## Budgets measured

| Item | Budget | Measured |
|---|---|---|
| Client wasm, release | 25 MiB raw, 12 MB Brotli | 22.95 MB raw, 5.21 MB Brotli |
| Host wasm, release | | 11.28 MB raw, 2.32 MB Brotli |
| First load, release | 40 MB | 7.56 MB Brotli |
| Host tick, wasm release in a Chromium Worker, customers at every table | 6 ms | avg 1.80 ms; worst single tick 15.05 ms |
| Blackjack edge test, 1,000,000 hands | | under 0.5 s |

The average tick is well inside the 6 ms budget. The worst single tick (15 ms, over 20 one-second reports) is above the 10 ms soak target of Phase 7. Phase 2 measured 9.7 ms. Not profiled yet; candidates are the shoe shuffle (312 draws, each logged) and serializing the table views. Profiling belongs to Phases 6 and 7.

## Known gaps

- The worst single host tick (15 ms) is over the Phase 7 soak target (see Budgets).
- Customers do not drink at the tables (only bar customers order beer).
- One blackjack table and one wheel; the second blackjack table and the high-stakes room come with tier unlocks (Phase 4).
- The audit log holds the room seed: whoever has the log can predict the room's later draws. It stays in the host's browser unless exported.

## Deployed URLs

- Client (branch preview): https://main-ygr4w2.last-call-21u.pages.dev
- Signaling Worker: https://last-call-signal.drewduncanjr.workers.dev/health
- CI deploys every push and runs the Chromium suite against the new deployment.
