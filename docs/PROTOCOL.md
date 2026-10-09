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

### `Beer` (struct)

The beer in a glass. `fill` is in percent; `perfect` records a perfect pour; `poured_by` is the player who gets the tip.

### `PourGauge` (struct)

A pour in progress at the tap, on the pouring player. Percent.

### `Puddle` (struct)

Spilled beer on the floor (a slip hazard).

### `PropPose` (struct)

Prop pose, copied from the host's physics each tick. Interpolated on clients.

### `HeldBy` (struct)

The player (by [`Player::id`]) holding this prop, if any.

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

### `Control` (struct)

Reliable, ordered control channel (join, replies, votes).

### `ProtocolPlugin` (struct)

Registers the whole protocol. Add after `ClientPlugins` or `ServerPlugins`.
