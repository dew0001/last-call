//! Zeen, The Spins and cleaning up (plan section 4.6) on the host.
//!
//! - The Focus meter loses a point a second.
//! - Buzzed (over 60): every 10 seconds, a 20% chance of a gag that drops
//!   whatever is in the hands.
//! - The Spins (drunk over 40 and Focus over 60): the camera rolls for 3
//!   seconds, then the player vomits. Both meters clear, and a slippery
//!   vomit puddle stays on the floor until someone mops it.
//! - The mop: held within reach of a puddle or vomit for a second, it
//!   cleans it.

use avian3d::prelude::*;
use bevy::prelude::*;
use shared::buffs::{self, Buffs, FocusTier};
use shared::protocol::*;

use crate::casino::Audit;
use crate::drunk::{PassedOut, PlayerRng};
use crate::physics::{Hands, HandsSet};

/// Host timers for one player's Focus.
#[derive(Component, Default)]
pub struct FocusClock {
    decay: u32,
    gag: u32,
    spins: u32,
}

/// Ticks of mopping per puddle.
const MOP_TICKS: u32 = 64;
/// How close the mop must be to a puddle.
const MOP_REACH: f32 = 1.0;

/// Progress mopping one puddle (on the mop prop).
#[derive(Component, Default)]
pub struct Mopping {
    target: Option<Entity>,
    ticks: u32,
}

pub struct BuffsPlugin;

impl Plugin for BuffsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, (give_buffs, focus, mop).chain().before(HandsSet).after(crate::game::MovePlayers));
    }
}

fn give_buffs(mut commands: Commands, players: Query<Entity, (With<Player>, Without<Focus>)>) {
    for e in &players {
        commands.entity(e).insert((Focus::default(), Inventory::default(), FocusClock::default()));
    }
}

/// Drop whatever the player holds (a gag, The Spins).
pub fn drop_held(commands: &mut Commands, hands: &mut Hands, props: &mut Query<&mut HeldBy>) {
    if let Some(e) = hands.held.take() {
        if let Ok(mut held) = props.get_mut(e) {
            held.0 = None;
        }
        commands.entity(e).insert(RigidBody::Dynamic).remove::<Sleeping>();
    }
}

#[allow(clippy::type_complexity)]
fn focus(
    mut commands: Commands,
    tick: Res<crate::TickCount>,
    mut rng: ResMut<PlayerRng>,
    mut audit: ResMut<Audit>,
    mut players: Query<
        (&PlayerPos, &PlayerYaw, &mut Focus, &mut FocusClock, &mut Drunk, &mut Hands),
        Without<PassedOut>,
    >,
    mut props: Query<&mut HeldBy>,
) {
    for (pos, yaw, mut focus, mut clock, mut drunk, mut hands) in &mut players {
        if focus.spinning {
            clock.spins = clock.spins.saturating_sub(1);
            if clock.spins == 0 {
                // Vomit: both meters clear, and the floor gets a hazard.
                let b = buffs::after_spins(Buffs { drunk: drunk.level, focus: focus.level, ..default() });
                drunk.level = b.drunk;
                *focus = Focus { level: b.focus, spinning: false };
                drop_held(&mut commands, &mut hands, &mut props);
                let f = Vec3::from_array(shared::movement::forward(yaw.0));
                let at = Vec3::new(pos.0.x + f.x * 0.5, 0.01, pos.0.z + f.z * 0.5);
                commands.spawn((
                    Name::new("Vomit"),
                    Vomit,
                    Puddle { pos: at },
                    lightyear::prelude::Replicate::to_clients(lightyear::prelude::NetworkTarget::All),
                ));
            }
            continue;
        }
        if buffs::spins(drunk.level, focus.level) {
            focus.spinning = true;
            clock.spins = buffs::SPINS_SECS * shared::TICK_HZ;
            continue;
        }
        if focus.level > 0 {
            clock.decay += 1;
            if clock.decay >= buffs::FOCUS_DECAY_TICKS {
                clock.decay = 0;
                focus.level -= 1;
            }
        }
        if FocusTier::of(focus.level) == FocusTier::Buzzed {
            clock.gag += 1;
            if clock.gag >= buffs::GAG_EVERY_SECS * shared::TICK_HZ {
                clock.gag = 0;
                // Draw only when something could drop, so idle players do not use the stream.
                if hands.held.is_some() && rng.0.below(100, tick.0, &mut audit.0) < buffs::GAG_PERCENT {
                    drop_held(&mut commands, &mut hands, &mut props);
                }
            }
        } else {
            clock.gag = 0;
        }
    }
}

/// A held mop cleans the nearest puddle or vomit within reach, over a second.
fn mop(
    mut commands: Commands,
    mut mops: Query<(Entity, &PropKind, &HeldBy, &Position, Option<&mut Mopping>)>,
    puddles: Query<(Entity, &Puddle)>,
) {
    for (e, kind, held, pos, mopping) in &mut mops {
        if *kind != PropKind::Mop {
            continue;
        }
        let Some(mut mopping) = mopping else {
            commands.entity(e).insert(Mopping::default());
            continue;
        };
        if held.0.is_none() {
            *mopping = Mopping::default();
            continue;
        }
        let near = puddles
            .iter()
            .map(|(pe, p)| (pe, Vec2::new(p.pos.x - pos.0.x, p.pos.z - pos.0.z).length()))
            .filter(|(_, d)| *d < MOP_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(pe, _)| pe);
        if near != mopping.target {
            *mopping = Mopping { target: near, ticks: 0 };
            continue;
        }
        if let Some(target) = near {
            mopping.ticks += 1;
            if mopping.ticks >= MOP_TICKS {
                commands.entity(target).despawn();
                *mopping = Mopping::default();
            }
        }
    }
}
