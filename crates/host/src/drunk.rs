//! The drunk meter on the host (plan section 4.6).
//!
//! - R with a beer in hand drinks it: +20 on the meter, $5 from the pocket.
//! - The meter decays 1 point every 2 seconds.
//! - Wasted players stumble every 8 seconds. Anyone sprinting over a puddle,
//!   or walking over one while Sloppy or worse, slips.
//! - At 100 a player passes out for 45 seconds: their body becomes a
//!   dynamic capsule lying on the floor. Others drag it with E (Q lets go),
//!   and props can be stacked on it.
//!
//! Throw spread and faster spilling read the meter in the hands and beer systems.

use avian3d::prelude::*;
use bevy::prelude::*;
use lightyear::prelude::input::native::ActionState;
use shared::drunk::{self, Tier};
use shared::movement::buttons;
use shared::protocol::*;
use shared::rng::{StreamId, TableRng};

use crate::beer::GlassFill;
use crate::physics::{Hands, HandsSet};

/// RNG stream for player effects (stumbles, throw spread).
pub const PLAYER_STREAM: StreamId = StreamId(1);
/// How close the dragger's hand must be to a passed-out body to grab it.
pub const DRAG_REACH: f32 = 1.2;
/// Mass of a passed-out body (kg).
const BODY_MASS: f32 = 70.0;

/// The room's RNG stream for player effects.
#[derive(Resource)]
pub struct PlayerRng(pub TableRng);

impl PlayerRng {
    /// A uniform value in [-1, 1].
    pub fn signed(&mut self, tick: u64, audit: &mut crate::casino::Audit) -> f32 {
        self.0.below(20_001, tick, &mut audit.0) as f32 / 10_000.0 - 1.0
    }
}

/// Host-side timers for one player's meter.
#[derive(Component, Default)]
pub struct DrunkClock {
    decay: u32,
    stumble: u32,
    passed_out: u32,
    prev_buttons: u16,
    on_puddle: bool,
}

/// A stumble in progress: the player is carried `dir` for a few ticks.
#[derive(Component)]
pub struct Stumble {
    pub dir: Vec2,
    pub ticks_left: u32,
}

/// Marks a passed-out player's body (dynamic, lying down).
#[derive(Component)]
pub struct PassedOut;

/// On a player dragging a passed-out body.
#[derive(Component)]
pub struct Dragging(pub Entity);

pub struct DrunkPlugin;

impl Plugin for DrunkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, seed_rng);
        app.add_systems(
            FixedUpdate,
            (
                (give_meters, drink, meters, slips).chain().before(HandsSet),
                stumble.after(crate::game::MovePlayers).before(HandsSet),
                drag.before(HandsSet),
            )
                .chain()
                .after(crate::game::MovePlayers),
        );
        app.add_systems(FixedPostUpdate, follow_bodies.after(PhysicsSystems::Writeback));
    }
}

fn seed_rng(mut commands: Commands, seed: Res<crate::RoomSeed>) {
    commands.insert_resource(PlayerRng(TableRng::new(seed.0, PLAYER_STREAM)));
}

fn give_meters(
    mut commands: Commands,
    start: Res<crate::economy::RunStart>,
    players: Query<Entity, (With<Player>, Without<Drunk>)>,
) {
    for e in &players {
        commands.entity(e).insert((Drunk { level: start.drunk, passed_out: false }, DrunkClock::default()));
    }
}

/// R with a beer in hand: drink it.
fn drink(
    mut commands: Commands,
    mut players: Query<(Entity, &ActionState<PlayerInput>, &mut Hands, &mut Pocket, &mut Drunk, &mut DrunkClock)>,
    glasses: Query<&GlassFill>,
) {
    for (entity, action, mut hands, mut pocket, mut drunk, mut clock) in &mut players {
        let b = action.0.buttons;
        let pressed = b & buttons::USE != 0 && clock.prev_buttons & buttons::USE == 0;
        clock.prev_buttons = b;
        if !pressed || drunk.passed_out || pocket.0 < shared::customers::PLAYER_BEER_COST {
            continue;
        }
        let Some(glass) = hands.held else { continue };
        let Ok(fill) = glasses.get(glass) else { continue };
        if fill.0 < shared::beer::EMPTY_BELOW {
            continue;
        }
        commands.entity(glass).despawn();
        hands.held = None;
        pocket.0 -= shared::customers::PLAYER_BEER_COST;
        drunk.level = drunk::after_beer(drunk.level);
        if drunk.level >= 100 {
            pass_out(&mut commands, entity, &mut drunk, &mut clock);
        }
    }
}

fn pass_out(commands: &mut Commands, entity: Entity, drunk: &mut Drunk, clock: &mut DrunkClock) {
    drunk.passed_out = true;
    clock.passed_out = drunk::PASS_OUT_SECS * shared::TICK_HZ;
    // The body falls over: a dynamic capsule lying on its side, kept from
    // rolling. Its pose becomes the player's position (see `follow_bodies`).
    commands.entity(entity).remove::<Stumble>().insert((
        PassedOut,
        RigidBody::Dynamic,
        Mass(BODY_MASS),
        LockedAxes::ROTATION_LOCKED,
        Rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
    ));
}

fn wake_up(commands: &mut Commands, entity: Entity, pos: Vec3) {
    commands.entity(entity).remove::<(PassedOut, LockedAxes)>().insert((
        RigidBody::Kinematic,
        Rotation::default(),
        Position(Vec3::new(pos.x, shared::bar::PLAYER_HEIGHT / 2.0, pos.z)),
        LinearVelocity::ZERO,
    ));
}

/// Decay, the pass-out timer, and Wasted stumbles.
fn meters(
    mut commands: Commands,
    tick: Res<crate::TickCount>,
    mut rng: ResMut<PlayerRng>,
    mut audit: ResMut<crate::casino::Audit>,
    mut players: Query<(Entity, &PlayerPos, &mut Drunk, &mut DrunkClock, Has<Stumble>)>,
) {
    for (entity, pos, mut drunk, mut clock, stumbling) in &mut players {
        clock.decay += 1;
        if clock.decay >= drunk::DECAY_TICKS {
            clock.decay = 0;
            if drunk.level > 0 {
                drunk.level -= 1;
            }
        }
        if drunk.level >= 100 && !drunk.passed_out {
            pass_out(&mut commands, entity, &mut drunk, &mut clock);
        }
        if drunk.passed_out {
            clock.passed_out = clock.passed_out.saturating_sub(1);
            if clock.passed_out == 0 {
                drunk.passed_out = false;
                wake_up(&mut commands, entity, pos.0);
            }
            continue;
        }
        if Tier::of(drunk.level) == Tier::Wasted {
            clock.stumble += 1;
            if clock.stumble >= drunk::STUMBLE_EVERY_SECS * shared::TICK_HZ && !stumbling {
                clock.stumble = 0;
                let dir =
                    Vec2::new(rng.signed(tick.0, &mut audit), rng.signed(tick.0, &mut audit)).normalize_or(Vec2::X);
                commands.entity(entity).insert(Stumble { dir, ticks_left: drunk::STUMBLE_TICKS });
            }
        } else {
            clock.stumble = 0;
        }
    }
}

/// Puddles: sprinting over one, or walking over one while Sloppy or worse,
/// makes the player slide on in the direction they were going.
fn slips(
    mut commands: Commands,
    puddles: Query<&Puddle>,
    mut players: Query<
        (Entity, &PlayerPos, &PlayerYaw, &ActionState<PlayerInput>, &Drunk, &mut DrunkClock),
        Without<Stumble>,
    >,
) {
    for (entity, pos, yaw, action, drunk, mut clock) in &mut players {
        let on = puddles.iter().any(|p| Vec2::new(p.pos.x - pos.0.x, p.pos.z - pos.0.z).length() < 0.5);
        let entering = on && !clock.on_puddle;
        clock.on_puddle = on;
        if !entering || drunk.passed_out {
            continue;
        }
        let input = action.0;
        let moving = input.mv() != Vec2::ZERO;
        let sprinting = moving && input.buttons & buttons::SPRINT != 0;
        if sprinting || (moving && Tier::of(drunk.level) >= Tier::Sloppy) {
            let f = shared::movement::forward(yaw.0);
            commands.entity(entity).insert(Stumble { dir: Vec2::new(f[0], f[2]), ticks_left: drunk::STUMBLE_TICKS });
        }
    }
}

/// Carry stumbling players along, inside the walls.
fn stumble(mut commands: Commands, mut players: Query<(Entity, &mut PlayerPos, &mut Stumble)>) {
    let step = drunk::STUMBLE_DISTANCE / drunk::STUMBLE_TICKS as f32;
    for (entity, mut pos, mut s) in &mut players {
        let p = shared::movement::collide([pos.0.x + s.dir.x * step, pos.0.y, pos.0.z + s.dir.y * step]);
        pos.0 = Vec3::from_array(p);
        s.ticks_left = s.ticks_left.saturating_sub(1);
        if s.ticks_left == 0 {
            commands.entity(entity).remove::<Stumble>();
        }
    }
}

/// E near a passed-out body grabs it; Q lets go. The body is pulled toward a
/// point in front of the dragger.
fn drag(
    mut commands: Commands,
    draggers: Query<(Entity, &PlayerPos, &PlayerYaw, &ActionState<PlayerInput>, &Hands, Option<&Dragging>)>,
    mut bodies: Query<(Entity, &Position, &mut LinearVelocity), With<PassedOut>>,
) {
    let dt = shared::TICK.as_secs_f32();
    for (entity, pos, yaw, action, hands, dragging) in &draggers {
        let b = action.0.buttons;
        let f = Vec3::from_array(shared::movement::forward(yaw.0));
        let grip = Vec3::new(pos.0.x + f.x * 1.0, shared::bar::PLAYER_RADIUS, pos.0.z + f.z * 1.0);
        match dragging {
            Some(d) => {
                let Ok((_, body, mut vel)) = bodies.get_mut(d.0) else {
                    commands.entity(entity).remove::<Dragging>();
                    continue;
                };
                if b & buttons::DROP != 0 {
                    commands.entity(entity).remove::<Dragging>();
                    continue;
                }
                // Pull: close part of the gap each step, horizontally.
                let to = grip - body.0;
                vel.0 = Vec3::new(to.x, 0.0, to.z) * (0.3 / dt);
            }
            None => {
                if b & buttons::INTERACT == 0 || hands.held.is_some() {
                    continue;
                }
                let hand = crate::physics::hand_point(pos.0, yaw.0);
                let target = bodies
                    .iter()
                    .filter(|(e, ..)| *e != entity)
                    .map(|(e, p, _)| (e, Vec2::new(p.0.x - hand.x, p.0.z - hand.z).length()))
                    .filter(|(_, d)| *d < DRAG_REACH)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((body, _)) = target {
                    commands.entity(entity).insert(Dragging(body));
                }
            }
        }
    }
}

/// A passed-out body's position is the player's position.
fn follow_bodies(mut players: Query<(&Position, &mut PlayerPos), With<PassedOut>>) {
    for (body, mut pos) in &mut players {
        let at = Vec3::new(body.0.x, 0.0, body.0.z);
        if pos.0.distance_squared(at) > 1e-6 {
            pos.0 = at;
        }
    }
}
