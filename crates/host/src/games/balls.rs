//! What every ball needs: drag on the floor (the physics has no rolling
//! resistance, so a ball would roll on forever), a way home when it leaves
//! its area, and a throw along the look for whoever holds it.

use avian3d::prelude::*;
use bevy::prelude::*;
use shared::protocol::*;

use super::{GamePlayers, GameQueue};
use crate::physics::{BallHome, Hands};

/// A rolling ball keeps this much of its speed each tick.
const ROLL_DRAG: f32 = 0.99;

/// The area a ball belongs in (with a margin): outside it, it has fallen
/// off the roof or rolled into the street.
fn bounds(kind: PropKind) -> (f32, f32, f32, f32) {
    match kind {
        PropKind::Basketball => (9.0, 25.0, -6.0, 10.0),
        _ => (-15.0, 15.0, 6.0, 28.0),
    }
}

/// Where a ball rests on the floor, by its kind.
pub fn radius(kind: PropKind) -> f32 {
    match kind {
        PropKind::Basketball => shared::hoops::BALL_RADIUS,
        PropKind::SoccerBall => shared::kicks::SOCCER_RADIUS,
        _ => shared::kicks::FOOTBALL_RADIUS,
    }
}

pub fn care(
    mut balls: Query<
        (&PropKind, &mut Position, &mut LinearVelocity, &mut AngularVelocity, &HeldBy, &BallHome),
        Without<Player>,
    >,
) {
    for (kind, mut pos, mut lin, mut ang, held, home) in &mut balls {
        if held.0.is_some() {
            continue;
        }
        // (Only a moving ball: writing the velocity wakes a sleeping one.)
        let rolling = pos.0.y < radius(*kind) + 0.03 && lin.0.y.abs() < 0.5;
        let moving = lin.0.length_squared() > 1e-4 || ang.0.length_squared() > 1e-3;
        if rolling && moving {
            lin.0.x *= ROLL_DRAG;
            lin.0.z *= ROLL_DRAG;
            ang.0 *= ROLL_DRAG;
        }
        let (x0, x1, z0, z1) = bounds(*kind);
        let off = pos.0.y < -1.0 || !(x0..=x1).contains(&pos.0.x) || !(z0..=z1).contains(&pos.0.z);
        if off {
            pos.0 = home.0;
            lin.0 = Vec3::ZERO;
            ang.0 = Vec3::ZERO;
        }
    }
}

/// A lot ball in hand: the left button throws it along the look (a pass).
/// The roof's basketball shot is [`super::hoops`].
pub fn throw(
    mut commands: Commands,
    queue: Res<GameQueue>,
    players: GamePlayers,
    mut hands: Query<&mut Hands>,
    mut balls: Query<
        (&PropKind, &mut Position, &mut LinearVelocity, &mut AngularVelocity, &mut HeldBy),
        Without<Player>,
    >,
) {
    for (e, action) in &queue.0 {
        let GameAction::Shoot { power, yaw, pitch } = *action else { continue };
        let Ok((_, _, pos, ..)) = players.get(*e) else { continue };
        let Ok(mut h) = hands.get_mut(*e) else { continue };
        let Some(ball) = h.held else { continue };
        let Ok((kind, mut bpos, mut lin, mut ang, mut held)) = balls.get_mut(ball) else { continue };
        if !matches!(kind, PropKind::SoccerBall | PropKind::Football) {
            continue;
        }
        let k = shared::kicks::Kick {
            power: (power.min(1000) / 10) as u8,
            yaw,
            pitch: pitch - shared::kicks::LOFT,
            curve: 0.0,
        };
        let v = Vec3::from_array(k.velocity()) * 0.75;
        bpos.0 = Vec3::new(pos.0.x, 1.7, pos.0.z) + v.normalize_or_zero() * 0.4;
        lin.0 = v;
        // A spiral about the throw.
        ang.0 = v.normalize_or_zero() * 20.0;
        held.0 = None;
        h.held = None;
        commands.entity(ball).insert(RigidBody::Dynamic).remove::<Sleeping>();
    }
}
