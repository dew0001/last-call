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

/// Ticks of pour history the host keeps, to honor a late tap stamp (1 s).
const HISTORY: usize = 64;
/// Ticks the host waits for a tap stamp before settling a pour: after the
/// input shows E let go, and after an overflow (a stamped release from
/// before the overflow still wins).
const SETTLE_TICKS: u32 = 8;
const OVERFLOW_GRACE_TICKS: u32 = 32;
/// The most ticks a late press stamp can add to a pour.
const MAX_CATCH_UP: u32 = 32;

/// A pour in progress (host side, exact).
#[derive(Component, Default)]
struct Pouring {
    pour: Pour,
    /// The pour after each tick's step: (tick, state).
    history: std::collections::VecDeque<(u32, Pour)>,
    /// The tick of the first step.
    started: u32,
    /// E let go (or the player left the tap): ticks left to wait for a stamp.
    settling: Option<u32>,
    /// Overflowed at this tick: ticks left to wait for an earlier stamp.
    overflowed: Option<(u32, u32)>,
    /// The inputs have shown E held during this pour. A pour started by a
    /// late press stamp keeps pouring until they do (or a release stamp comes).
    seen_down: bool,
}

impl Pouring {
    /// `speed` is the Tap Wall multiplier.
    fn step(&mut self, tick: u32, pitch: f32, speed: f32) -> bool {
        let over = self.pour.step(pitch, shared::TICK.as_secs_f32() * speed);
        self.history.push_back((tick, self.pour));
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
        over
    }

    /// The pour as it stood when E went up at `tick`: after the step of the
    /// tick before. `None` when that is outside the history.
    fn at_release(&self, tick: u32) -> Option<Pour> {
        if tick <= self.started {
            return Some(Pour::default());
        }
        self.history.iter().rev().find(|(t, _)| *t == tick - 1).map(|(_, p)| *p)
    }
}

/// Set after an overflow: no new pour until E is let go.
#[derive(Component)]
struct PourLock;

/// Exact fill of a glass; [`Beer::fill`] is the replicated percent.
#[derive(Component)]
pub struct GlassFill(pub f32);

/// Ticks a glass has been loose, or a puddle has existed.
#[derive(Component, Default)]
struct Age(u32);

/// Tap stamps waiting for the host to reach their tick: (player entity,
/// stamp). Local players add theirs with [`crate::HostSim::tap`].
#[derive(Resource, Default)]
pub struct TapQueue(pub Vec<(Entity, TapEvent)>);

pub struct BeerPlugin;

impl Plugin for BeerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TapQueue>();
        app.add_systems(
            FixedUpdate,
            ((collect_taps, pour).chain().before(HandsSet), (carry, serve, tidy).chain().after(HandsSet))
                .after(crate::game::MovePlayers),
        );
    }
}

fn percent(v: f32) -> u8 {
    (v * 100.0).round().clamp(0.0, 255.0) as u8
}

/// Tap stamps from networked players. The link that sent one must control the player.
fn collect_taps(
    mut queue: ResMut<TapQueue>,
    mut links: Query<(Entity, &mut MessageReceiver<TapEvent>)>,
    players: Query<(Entity, &ControlledBy), With<Player>>,
) {
    for (link, mut receiver) in &mut links {
        let player = players.iter().find(|(_, c)| c.owner == link).map(|(e, _)| e);
        for tap in receiver.receive() {
            if let Some(p) = player {
                queue.0.push((p, tap));
            }
        }
    }
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

/// Pour, by each player's inputs. A client also stamps the ticks its pour
/// started and ended ([`TapEvent`]): when its inputs arrive late (a stalled
/// tab), the host has already reused the last known input for those ticks,
/// and the stamps put the pour right. A release stamp ends the pour at the
/// fill it had at that tick; a press stamp from before the pour started
/// here adds the missed steps. Bots send no stamps and pour by inputs alone.
fn pour(
    mut commands: Commands,
    timeline: Res<LocalTimeline>,
    owned: Res<crate::fixtures::Owned>,
    mut queue: ResMut<TapQueue>,
    mut players: PourPlayers,
) {
    let now = timeline.tick().0;
    let speed = owned.0.pour_speed();
    // A client runs a few ticks ahead of the host, so a stamp often arrives
    // before its tick: keep it until the host gets there. Drop stale ones.
    queue.0.retain(|(_, t)| now.saturating_sub(t.tick) < HISTORY as u32);
    let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut queue.0).into_iter().partition(|(_, t)| t.tick <= now);
    queue.0 = later;
    for (entity, player, pos, yaw, action, mut hands, pouring, locked, drunk) in &mut players {
        let input = action.0;
        if drunk.is_some_and(|d| d.passed_out) {
            continue;
        }
        let stamps = due.iter().filter(|(e, _)| *e == entity).map(|(_, t)| *t);
        let release =
            stamps.clone().filter(|t| !t.down && now.saturating_sub(t.tick) < HISTORY as u32).map(|t| t.tick).min();
        let press = stamps.filter(|t| t.down && now - t.tick <= MAX_CATCH_UP).map(|t| t.tick).min();
        let at_tap = distance_to_tap(pos.0.to_array()) < shared::bar::TAP_REACH;
        let holding_e = input.buttons & buttons::INTERACT != 0;
        if locked && !holding_e {
            commands.entity(entity).remove::<PourLock>();
        }
        let can_pour = holding_e && at_tap && hands.held.is_none() && !locked;
        let Some(mut p) = pouring else {
            if can_pour || press.is_some_and(|_| at_tap && hands.held.is_none() && !locked) {
                let mut new = Pouring { started: press.unwrap_or(now), seen_down: can_pour, ..default() };
                // A late press: the steps this host missed, then this tick's.
                for t in new.started..=now {
                    new.step(t, input.pitch(), speed);
                }
                commands
                    .entity(entity)
                    .insert((PourGauge { fill: percent(new.pour.fill), foam: percent(new.pour.foam) }, new));
            }
            continue;
        };

        // A stamped release ends the pour at that tick's fill, even after an overflow
        // here, as long as the release came first.
        let stamped = release.and_then(|t| {
            let before_overflow = p.overflowed.is_none_or(|(at, _)| t <= at);
            before_overflow.then(|| p.at_release(t)).flatten()
        });
        if let Some(state) = stamped {
            finish(&mut commands, entity, player.id, pos.0, yaw.0, &mut hands, state);
            continue;
        }
        if let Some((at, wait)) = p.overflowed {
            if wait == 0 {
                // Overflow: the beer is wasted on the floor.
                commands.entity(entity).remove::<(Pouring, PourGauge)>().insert(PourLock);
                let f = Vec3::from_array(shared::movement::forward(yaw.0));
                let spot = Vec3::new(pos.0.x + f.x * 0.4, 0.01, pos.0.z + f.z * 0.4);
                commands.spawn((
                    Name::new("Puddle"),
                    Puddle { pos: spot },
                    Age::default(),
                    Replicate::to_clients(NetworkTarget::All),
                ));
            } else {
                p.overflowed = Some((at, wait - 1));
            }
            continue;
        }
        if let Some(wait) = p.settling {
            if wait == 0 {
                let state = p.pour;
                finish(&mut commands, entity, player.id, pos.0, yaw.0, &mut hands, state);
            } else {
                p.settling = Some(wait - 1);
            }
            continue;
        }
        let stale = !p.seen_down && now - p.started <= MAX_CATCH_UP && at_tap && hands.held.is_none();
        if can_pour || stale {
            p.seen_down |= can_pour;
            if p.step(now, input.pitch(), speed) {
                p.overflowed = Some((now, OVERFLOW_GRACE_TICKS));
            }
            let gauge = PourGauge { fill: percent(p.pour.fill), foam: percent(p.pour.foam) };
            commands.entity(entity).insert(gauge);
        } else {
            // Released (or walked away): settle, giving a late stamp a moment to arrive.
            p.settling = Some(SETTLE_TICKS);
        }
    }
}

/// End a pour: the glass goes into the hand.
fn finish(commands: &mut Commands, entity: Entity, id: u64, pos: Vec3, yaw: f32, hands: &mut Hands, state: Pour) {
    commands.entity(entity).remove::<(Pouring, PourGauge)>();
    let perfect = state.result() == PourResult::Perfect;
    let glass = commands.spawn(glass(hand_point(pos, yaw), state.fill, perfect, id, Some(id))).id();
    hands.held = Some(glass);
    hands.charge = 0;
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
