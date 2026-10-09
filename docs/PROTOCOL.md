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

### `PropPose` (struct)

Prop pose, copied from the host's physics each tick. Interpolated on clients.

### `HeldBy` (struct)

The player (by [`Player::id`]) holding this prop, if any.

## Room state (host to clients)

### `RoomState` (struct)

Marks the one room-state entity. Room-wide components live on it.

### `ShiftClock` (struct)

The shift clock. The host updates it when the phase changes and once per second; `running` is false while no player is in the room.

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
