# Protocol

Generated from `crates/shared/src/protocol.rs` by `cargo run -p last_call_tools -- protocol_doc`. Do not edit by hand.

### `PROTOCOL_VERSION` (const)

Protocol version. Bump on any breaking change. Sent in [`Join`].

## Components (host to clients)

### `Player` (struct)

A player. `id` is stable across reconnects (derived from the player UUID).

### `PlayerPos` (struct)

Player feet position. Predicted for the owner, interpolated for others.

### `PlayerYaw` (struct)

Player facing, radians. Yaw 0 faces -Z.

### `PropKind` (enum)

Kind of physics prop.
- `Bottle`
- `Chip`
- `Stool`
- `Glass`: A beer glass from the tap. Carries a [`Beer`].
- `Mop`: The kitchen mop: held near vomit or a puddle, it cleans it up.

### `Beer` (struct)

The beer in a glass. `fill` is in percent; `perfect` records a perfect pour; `poured_by` is the player who gets the tip.

### `PourGauge` (struct)

A pour in progress at the tap, on the pouring player. Percent.

### `Drunk` (struct)

A player's drunk meter, 0 to 100 (see [`crate::drunk`]). `passed_out` stays set for the whole pass-out, while the meter keeps decaying.

### `Focus` (struct)

A player's Focus meter, 0 to 100 (see [`crate::buffs`]). `spinning` is set while The Spins roll the camera.

### `Inventory` (struct)

A player's one-shift items and food effects.

### `Vomit` (struct)

Vomit on the floor (from The Spins). Also a [`Puddle`] (it is slippery); it stays until someone mops it.

### `Puddle` (struct)

Spilled beer on the floor (a slip hazard).

### `PropPose` (struct)

Prop pose, copied from the host's physics each tick. Interpolated on clients.

### `HeldBy` (struct)

The player (by [`Player::id`]) holding this prop, if any.

### `ChipValue` (struct)

Money on a chip prop: a payout waiting on the felt (or dropped on the floor). Whoever picks it up gets it.

## Casino tables (host to clients)

### `HIDDEN_CARD` (const)

A hidden card (the dealer's hole card) in a view.

### `HandView` (struct)


### `SeatView` (struct)


### `BjPhase` (enum)

Where a blackjack round stands, for drawing.
- `Betting`: Between rounds: bets go down, the dealer deals.
- `Insurance`
- `Players`
- `Dealer`

### `BlackjackView` (struct)

The blackjack table as every client sees it.

### `RouletteView` (struct)

The roulette table as every client sees it.

### `SlotView` (struct)

One slot machine as every client sees it.

### `RoomUpgrades` (struct)

The upgrades the crew owns (on the room entity).

### `JukeboxState` (struct)

The jukebox (on the room entity): the playing track, if any.

### `ActiveChaos` (struct)

One chaos event in progress.

### `ChaosState` (struct)

Chaos on the room entity: what is running, how the last ones ended, and lasting consequences.

### `ChaosNpc` (struct)

A chaos NPC: a cop, a brawler, the inspector, the loan shark, the card counter (also carries [`Customer`]-like pose via [`NpcPose`]).

### `Fire` (struct)

The kitchen fire (a hazard area).

## Side games (host to clients)

### `FishPhase` (enum)

Where a line at a fishing spot is.
- `Idle`
- `Waiting`
- `Biting`
- `Reeling`

### `FishingView` (struct)

A fishing spot on the pier.

### `ShotView` (struct)

A basketball shot as released, for clients to draw the flight.

### `HoopsView` (struct)

The roof court.

### `KickView` (struct)

A penalty as it crossed the line.

### `PenaltyView` (struct)

The penalty spot and goal.

### `FieldGoalResult` (struct)

A field goal attempt's result.

### `FieldGoalView` (struct)


### `GauntletView` (struct)

The gauntlet lane.

### `Tracer` (struct)

A shot's line for clients to draw.

### `PitView` (struct)

The fight pit.

## Room state (host to clients)

### `RoomState` (struct)

Marks the one room-state entity. Room-wide components live on it.

### `ShiftClock` (struct)

The shift clock. The host updates it when the phase changes and once per second; `running` is false while no player is in the room.

### `RunLedger` (struct)

The run's money and standing. `due` is the payment due at the end of the current week; `last` is the most recent collection.

### `Pocket` (struct)

A player's personal money.

### `Customer` (struct)

A customer NPC. `id` counts up per room. `patience` is the seconds left before a waiting customer gives up.

### `NpcPose` (struct)

NPC feet position and facing. Interpolated on clients.

## Inputs (client to host, per tick)

### `PlayerInput` (struct)

One tick of player input, quantized to keep upload small (8 bytes). Build it with [`PlayerInput::new`]; read it with the accessors.

## Messages

### `Join` (struct)

First message from a client. `player_uuid` comes from `localStorage`, so a refreshed tab gets its old player back.

### `JoinReply` (enum)

Host reply to [`Join`].
- `Welcome`
- `Refused`

### `TableRequest` (struct)

A player asks a table to do something. The host checks everything: where the player stands, the rules, the player's money and drunk tier.

### `TapEvent` (struct)

The tick a player pressed (`down`) or let go of E at the beer tap, by the client's own prediction. Inputs from a client that stalls reach the host after it has simulated those ticks, and the host then reuses the last known input; this stamp lets the host end (or start) the pour at the right tick anyway. The host still computes the pour itself.

### `FixtureRequest` (struct)

A player uses a fixture (drawer, kitchen pass, shop, breaker...). The host checks where the player stands, the money and the rules.

### `GameRequest` (struct)

A player acts in a side game (plan sections 5.5 to 5.9). The host finds the station by where the player stands and checks the rules and money.

### `GameAction` (enum)

- `Cast`: Fishing: cast from the pier spot the player stands at (charge 0 to 100).
- `Hook`: Fishing: strike when the bobber dips.
- `Reel`: Fishing: the reel is held or let go.
- `BetCatch`: Fishing: bet on the fight at a spot.
- `JoinHoops`: Basketball: enter the next contest (pays the entry fee).
- `StartHoops`
- `Shoot`: Basketball: a shot with this charge (0 to 1000) and look.
- `JoinShootout`: Penalties: enter the shootout, or stand in goal.
- `TakeGoal`
- `StartShootout`
- `Kick`: Penalties: the kicker's kick from the spot.
- `Dive`: Penalties: the goalie dives.
- `BetKick`: Penalties: a spectator bets on the next kick (goal or not), 1 to 1.
- `FieldGoal`: Football: a field goal from the tee, at this distance, for this stake.
- `Run`: Football: start a gauntlet run for this stake.
- `Dodge`
- `StiffArm`
- `JoinPit`: Fight pit: enter the next round (voting for teams or not).
- `StartPit`
- `Pick`: Fight pit: take a weapon from the rack the player stands at.
- `Fire`: Fight pit: fire along this look, as the shooter saw the others at `view_tick` (their interpolated tick), for lag compensation.

### `Control` (struct)

Reliable, ordered control channel (join, replies, votes).

### `ProtocolPlugin` (struct)

Registers the whole protocol. Add after `ClientPlugins` or `ServerPlugins`.
