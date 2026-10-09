//! Wire protocol between clients and the host simulation (lightyear).
//!
//! Every replicated component, message and input is registered in
//! [`ProtocolPlugin`]. Both the host and every client add that plugin, in the
//! same order, so registration ids match.

use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use lightyear::prelude::input::InputConfig;
use lightyear::prelude::input::native::InputPlugin;
use lightyear::prelude::*;
use serde::{Deserialize, Serialize};

/// Protocol version. Bump on any breaking change. Sent in [`Join`].
pub const PROTOCOL_VERSION: u16 = 3;

// ---------- Components (host to clients) ----------

/// A player. `id` is stable across reconnects (derived from the player UUID).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Player {
    pub id: u64,
    pub name: String,
    pub slot: u8,
}

/// Player feet position. Predicted for the owner, interpolated for others.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default, Reflect)]
pub struct PlayerPos(pub Vec3);

/// Player facing, radians. Yaw 0 faces -Z.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default, Reflect)]
pub struct PlayerYaw(pub f32);

/// Kind of physics prop.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropKind {
    Bottle,
    Chip,
    Stool,
}

/// Prop pose, copied from the host's physics each tick. Interpolated on clients.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Reflect)]
pub struct PropPose {
    pub pos: Vec3,
    pub rot: Quat,
}

/// The player (by [`Player::id`]) holding this prop, if any.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct HeldBy(pub Option<u64>);

// ---------- Room state (host to clients) ----------

/// Marks the one room-state entity. Room-wide components live on it.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RoomState;

/// The shift clock. The host updates it when the phase changes and once per
/// second; `running` is false while no player is in the room.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ShiftClock {
    pub calendar: crate::shift::Calendar,
    pub phase: crate::shift::ShiftPhase,
    pub seconds_left: u16,
    pub running: bool,
}

impl Ease for PlayerPos {
    fn interpolating_curve_unbounded(start: Self, end: Self) -> impl Curve<Self> {
        FunctionCurve::new(Interval::UNIT, move |t| PlayerPos(start.0.lerp(end.0, t)))
    }
}

impl Ease for PlayerYaw {
    fn interpolating_curve_unbounded(start: Self, end: Self) -> impl Curve<Self> {
        FunctionCurve::new(Interval::UNIT, move |t| PlayerYaw(lerp_angle(start.0, end.0, t)))
    }
}

impl Ease for PropPose {
    fn interpolating_curve_unbounded(start: Self, end: Self) -> impl Curve<Self> {
        FunctionCurve::new(Interval::UNIT, move |t| PropPose {
            pos: start.pos.lerp(end.pos, t),
            rot: start.rot.slerp(end.rot, t),
        })
    }
}

/// Interpolate angles the short way round.
pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let d = (b - a).rem_euclid(tau);
    let d = if d > std::f32::consts::PI { d - tau } else { d };
    a + d * t
}

// ---------- Inputs (client to host, per tick) ----------

/// One tick of player input, quantized to keep upload small (8 bytes).
/// Build it with [`PlayerInput::new`]; read it with the accessors.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
pub struct PlayerInput {
    /// Move vector in the player's frame (x right, y forward), each axis * 127.
    pub mv: [i8; 2],
    /// Yaw as a fraction of a full turn, 0..65536.
    pub yaw: u16,
    /// Pitch as a fraction of a half turn, -32768..32767 maps to -pi/2..pi/2.
    pub pitch: i16,
    /// Button bits, see [`crate::movement::buttons`].
    pub buttons: u16,
}

impl PlayerInput {
    pub fn new(mv: Vec2, yaw: f32, pitch: f32, buttons: u16) -> Self {
        let q = |v: f32| if v.is_finite() { (v.clamp(-1.0, 1.0) * 127.0).round() as i8 } else { 0 };
        let tau = std::f32::consts::TAU;
        let yaw = if yaw.is_finite() { yaw.rem_euclid(tau) / tau } else { 0.0 };
        let half_pi = std::f32::consts::FRAC_PI_2;
        let pitch = if pitch.is_finite() { pitch.clamp(-half_pi, half_pi) / half_pi } else { 0.0 };
        Self {
            mv: [q(mv.x), q(mv.y)],
            yaw: ((yaw * 65536.0).round() as u32 % 65536) as u16,
            pitch: (pitch * 32767.0).round() as i16,
            buttons,
        }
    }

    pub fn mv(&self) -> Vec2 {
        Vec2::new(f32::from(self.mv[0]), f32::from(self.mv[1])) / 127.0
    }

    pub fn yaw(&self) -> f32 {
        f32::from(self.yaw) / 65536.0 * std::f32::consts::TAU
    }

    pub fn pitch(&self) -> f32 {
        f32::from(self.pitch) / 32767.0 * std::f32::consts::FRAC_PI_2
    }
}

impl MapEntities for PlayerInput {
    fn map_entities<M: EntityMapper>(&mut self, _entity_mapper: &mut M) {}
}

// ---------- Messages ----------

/// First message from a client. `player_uuid` comes from `localStorage`, so a
/// refreshed tab gets its old player back.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Join {
    pub protocol: u16,
    pub code: String,
    pub player_uuid: [u8; 16],
    pub display_name: String,
    pub cosmetic_id: u16,
}

/// Host reply to [`Join`].
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum JoinReply {
    Welcome { player_id: u64 },
    Refused { reason: String },
}

/// Reliable, ordered control channel (join, replies, votes).
pub struct Control;

/// Stable player id from a UUID (FNV-1a, 64 bit).
pub fn player_id_from_uuid(uuid: &[u8; 16]) -> u64 {
    uuid.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// Registers the whole protocol. Add after `ClientPlugins` or `ServerPlugins`.
pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        app.add_channel::<Control>(ChannelSettings {
            mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
            ..default()
        })
        .add_direction(NetworkDirection::Bidirectional);

        app.register_message::<Join>().add_direction(NetworkDirection::ClientToServer);
        app.register_message::<JoinReply>().add_direction(NetworkDirection::ServerToClient);

        // Send inputs every 2 ticks (32 Hz); each packet repeats the last 4
        // sends, which covers about 125 ms of packet loss. Keeps upload under
        // the 8 KB/s budget.
        app.add_plugins(InputPlugin::<PlayerInput> {
            config: InputConfig { packet_redundancy: 4, send_interval: crate::TICK * 2, ..default() },
        });

        app.component::<Player>().replicate();
        app.component::<PlayerPos>().replicate().predict().add_linear_interpolation();
        app.component::<PlayerYaw>().replicate().predict().add_linear_interpolation();
        app.component::<PropKind>().replicate();
        app.component::<PropPose>().replicate().add_linear_interpolation();
        app.component::<HeldBy>().replicate();
        app.component::<RoomState>().replicate();
        app.component::<ShiftClock>().replicate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_lerp_takes_short_way() {
        let a = 3.0;
        let b = -3.0;
        let mid = lerp_angle(a, b, 0.5);
        assert!((mid.rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI).abs() < 0.01);
    }

    #[test]
    fn input_quantization_round_trips() {
        let i = PlayerInput::new(Vec2::new(0.5, -1.0), 1.0, -0.3, 5);
        assert!((i.mv() - Vec2::new(0.5, -1.0)).length() < 0.01);
        assert!((i.yaw() - 1.0).abs() < 1e-3);
        assert!((i.pitch() + 0.3).abs() < 1e-3);
        assert_eq!(i.buttons, 5);
        let nan = PlayerInput::new(Vec2::NAN, f32::NAN, f32::INFINITY, 0);
        assert_eq!(nan.mv, [0, 0]);
        assert_eq!(nan.yaw, 0);
    }

    #[test]
    fn player_id_is_stable_and_spread() {
        assert_eq!(player_id_from_uuid(&[1; 16]), player_id_from_uuid(&[1; 16]));
        assert_ne!(player_id_from_uuid(&[1; 16]), player_id_from_uuid(&[2; 16]));
    }
}
