//! The beer tap minigame on the host (plan section 5.4): pour, carry, serve.
//!
//! - Pour: hold E at the tap with empty hands. The fill rises; foam rises
//!   slowly at a good tilt (look pitch) and fast at a bad one. Release to get
//!   a glass in the hand. Hold too long and it overflows: no glass, a puddle.
//! - Carry: the glass is a physics prop. Sprinting spills it; throwing empties it.
//! - Serve: a glass resting on the counter in front of a waiting customer is
//!   served. The customer pays the house; a perfect pour also tips the pourer.

use avian3d::prelude::*;
use bevy::prelude::*;
use lightyear::prelude::input::native::ActionState;
use lightyear::prelude::*;
use shared::beer::{self, Pour, PourResult};
use shared::customers::{self, Mood};
use shared::movement::{buttons, distance_to_tap};
use shared::protocol::*;

use crate::customers::Npc;
use crate::physics::{Hands, HandsSet, hand_point, prop_body};

/// Seconds a puddle stays on the floor.
pub const PUDDLE_SECS: u32 = 90;
/// Seconds an unserved glass may sit around before it is cleared away.
pub const LOOSE_GLASS_SECS: u32 = 45;
/// A glass moving slower than this (m/s) counts as put down.
const AT_REST: f32 = 0.3;

/// A pour in progress (host side, exact).
#[derive(Component, Default)]
struct Pouring(Pour);

/// Set after an overflow: no new pour until E is let go.
#[derive(Component)]
struct PourLock;

/// Exact fill of a glass; [`Beer::fill`] is the replicated percent.
#[derive(Component)]
pub struct GlassFill(pub f32);

/// Ticks a glass has been loose, or a puddle has existed.
#[derive(Component, Default)]
struct Age(u32);

pub struct BeerPlugin;

impl Plugin for BeerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (pour.before(HandsSet), (carry, serve, tidy).chain().after(HandsSet)).after(crate::game::MovePlayers),
        );
    }
}

fn percent(v: f32) -> u8 {
    (v * 100.0).round().clamp(0.0, 255.0) as u8
}

type PourPlayers<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Player,
        &'static PlayerPos,
        &'static PlayerYaw,
        &'static ActionState<PlayerInput>,
        &'static mut Hands,
        Option<&'static mut Pouring>,
        Has<PourLock>,
        Option<&'static Drunk>,
    ),
>;

fn pour(mut commands: Commands, mut players: PourPlayers) {
    let dt = shared::TICK.as_secs_f32();
    for (entity, player, pos, yaw, action, mut hands, pouring, locked, drunk) in &mut players {
        let input = action.0;
        if drunk.is_some_and(|d| d.passed_out) {
            continue;
        }
        let at_tap = distance_to_tap(pos.0.to_array()) < shared::bar::TAP_REACH;
        let holding_e = input.buttons & buttons::INTERACT != 0;
        if locked && !holding_e {
            commands.entity(entity).remove::<PourLock>();
        }
        let can_pour = holding_e && at_tap && hands.held.is_none() && !locked;
        match (pouring, can_pour) {
            (None, true) => {
                commands.entity(entity).insert((Pouring::default(), PourGauge::default()));
            }
            (Some(mut p), true) => {
                if p.0.step(input.pitch(), dt) {
                    // Overflow: the beer is wasted on the floor.
                    commands.entity(entity).remove::<(Pouring, PourGauge)>().insert(PourLock);
                    let f = Vec3::from_array(shared::movement::forward(yaw.0));
                    let at = Vec3::new(pos.0.x + f.x * 0.4, 0.01, pos.0.z + f.z * 0.4);
                    commands.spawn((
                        Name::new("Puddle"),
                        Puddle { pos: at },
                        Age::default(),
                        Replicate::to_clients(NetworkTarget::All),
                    ));
                } else {
                    commands.entity(entity).insert(PourGauge { fill: percent(p.0.fill), foam: percent(p.0.foam) });
                }
            }
            (Some(p), false) => {
                // Released (or walked away): the glass goes into the hand.
                commands.entity(entity).remove::<(Pouring, PourGauge)>();
                let perfect = p.0.result() == PourResult::Perfect;
                let at = hand_point(pos.0, yaw.0);
                let glass = commands.spawn(glass(at, p.0.fill, perfect, player.id, Some(player.id))).id();
                hands.held = Some(glass);
                hands.charge = 0;
            }
            (None, false) => {}
        }
    }
}

/// A beer glass prop: in a hand (kinematic) when `held` is set, else loose.
pub fn glass(at: Vec3, fill: f32, perfect: bool, poured_by: u64, held: Option<u64>) -> impl Bundle {
    let (collider, mass) = prop_body(PropKind::Glass);
    let body = if held.is_some() { RigidBody::Kinematic } else { RigidBody::Dynamic };
    (
        (PropKind::Glass, Beer { fill: percent(fill), perfect, poured_by }, GlassFill(fill), Age::default()),
        (PropPose { pos: at, rot: Quat::IDENTITY }, HeldBy(held), Transform::from_translation(at)),
        (body, collider, Mass(mass)),
        (Replicate::to_clients(NetworkTarget::All), InterpolationTarget::to_clients(NetworkTarget::All)),
    )
}

/// Sprinting with a glass spills it.
fn carry(
    players: Query<(&Player, &ActionState<PlayerInput>, Option<&Drunk>)>,
    mut glasses: Query<(&HeldBy, &mut GlassFill, &mut Beer)>,
) {
    let dt = shared::TICK.as_secs_f32();
    for (held, mut fill, mut beer) in &mut glasses {
        // A throw empties the glass (see the hands system).
        if beer.fill == 0 {
            fill.0 = 0.0;
        }
        let Some(id) = held.0 else { continue };
        let Some((_, action, drunk)) = players.iter().find(|(p, ..)| p.id == id) else { continue };
        let input = action.0;
        let sprinting = input.buttons & buttons::SPRINT != 0 && input.mv() != Vec2::ZERO;
        let mult = shared::drunk::spill_multiplier(shared::drunk::Tier::of(drunk.map_or(0, |d| d.level)));
        fill.0 = beer::carry(fill.0, sprinting, mult, dt);
        let p = percent(fill.0);
        if beer.fill != p {
            beer.fill = p;
        }
    }
}

/// A glass at rest on the counter in front of a waiting customer is served.
fn serve(
    mut commands: Commands,
    glasses: Query<(Entity, &Position, &LinearVelocity, &HeldBy, &GlassFill, &Beer)>,
    mut npcs: Query<(&mut Customer, &mut Npc)>,
    mut pockets: Query<(&Player, &mut Pocket)>,
    mut room: Query<&mut RunLedger, With<RoomState>>,
) {
    let Ok(mut run) = room.single_mut() else { return };
    for (glass, pos, vel, held, fill, beer) in &glasses {
        if held.0.is_some() || vel.0.length() > AT_REST {
            continue;
        }
        let Some((mut customer, mut npc)) = npcs.iter_mut().find(|(c, n)| {
            c.mood == Mood::Waiting && n.seat.is_some_and(|(_, at)| customers::in_serve_zone(at.x, pos.0.to_array()))
        }) else {
            continue;
        };
        let Some((paid, tip)) = beer::serve(fill.0, beer.perfect) else { continue };
        let paid = paid.min(npc.cash);
        npc.cash -= paid;
        run.ledger.house += paid;
        if tip > 0
            && let Some((_, mut pocket)) = pockets.iter_mut().find(|(p, _)| p.id == beer.poured_by)
        {
            pocket.0 += tip;
        }
        customer.mood = Mood::Drinking;
        customer.patience = 0;
        npc.ticks = customers::DRINK_EVERY_SECS * shared::TICK_HZ;
        commands.entity(glass).despawn();
    }
}

/// Clear loose glasses and old puddles.
fn tidy(
    mut commands: Commands,
    mut glasses: Query<(Entity, &HeldBy, &mut Age), With<GlassFill>>,
    mut puddles: Query<(Entity, &mut Age), (With<Puddle>, Without<GlassFill>)>,
) {
    for (e, held, mut age) in &mut glasses {
        age.0 = if held.0.is_some() { 0 } else { age.0 + 1 };
        if age.0 > LOOSE_GLASS_SECS * shared::TICK_HZ {
            commands.entity(e).despawn();
        }
    }
    for (e, mut age) in &mut puddles {
        age.0 += 1;
        if age.0 > PUDDLE_SECS * shared::TICK_HZ {
            commands.entity(e).despawn();
        }
    }
}
