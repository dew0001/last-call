//! Host physics: the room's static colliders, player bodies, physics props,
//! and picking up, carrying, throwing and dropping.
//!
//! Avian runs in `FixedPostUpdate`, once per tick. Hands logic runs in
//! `FixedUpdate`, after movement. After the physics step, each prop's pose is
//! copied into the replicated [`PropPose`] only when it moved, so resting
//! props cost no bandwidth.

use avian3d::prelude::*;
use bevy::prelude::*;
use lightyear::prelude::input::native::ActionState;
use lightyear::prelude::*;
use shared::bar;
use shared::movement::buttons;
use shared::protocol::*;

/// Bottles on the counter.
pub const BOTTLES: usize = 20;
/// Chips on the counter, in stacks of [`CHIPS_PER_STACK`].
pub const CHIPS: usize = 100;
pub const CHIPS_PER_STACK: usize = 10;
/// Stools in front of the counter.
pub const STOOLS: usize = 10;

/// How far from the hand a prop can be picked up, in meters.
pub const REACH: f32 = 0.9;
/// Throw speed range, m/s, from a tap to a full charge.
pub const THROW_MIN: f32 = 4.0;
pub const THROW_MAX: f32 = 12.0;
/// Ticks of holding F for a full charge (1 s).
pub const FULL_CHARGE_TICKS: u16 = 64;

pub struct HostPhysicsPlugin;

impl Plugin for HostPhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsPlugins::default());
        app.add_systems(Startup, (spawn_room, spawn_props).chain().in_set(SpawnRoom));
        app.add_observer(add_player_body);
        app.add_systems(
            FixedUpdate,
            (drive_player_bodies, hands.in_set(HandsSet)).chain().after(crate::game::MovePlayers),
        );
        app.add_systems(FixedPostUpdate, publish_poses.after(PhysicsSystems::Writeback));
    }
}

/// The Startup systems that build the room and its props. Other Startup
/// spawns order themselves after it: entity ids feed the replay hash, and an
/// unordered pair of systems may run in a different order in another build.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpawnRoom;

/// The pick-up, carry, throw and drop system.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HandsSet;

/// Per-player hand state on the host.
#[derive(Component, Default, Debug)]
pub struct Hands {
    pub held: Option<Entity>,
    pub prev_buttons: u16,
    pub charge: u16,
}

fn forward(yaw: f32) -> Vec3 {
    Vec3::from_array(shared::movement::forward(yaw))
}

/// Where a player's hand is (see [`shared::movement::hand_point`]).
pub fn hand_point(pos: Vec3, yaw: f32) -> Vec3 {
    Vec3::from_array(shared::movement::hand_point(pos.to_array(), yaw))
}

fn spawn_room(mut commands: Commands) {
    let (hx, hz, h) = (bar::HALF_X, bar::HALF_Z, bar::WALL_HEIGHT);
    let mut wall = |size: Vec3, at: Vec3| {
        commands.spawn((RigidBody::Static, Collider::cuboid(size.x, size.y, size.z), Transform::from_translation(at)));
    };
    wall(Vec3::new(hx * 2.0, 0.2, hz * 2.0), Vec3::new(0.0, -0.1, 0.0));
    wall(Vec3::new(hx * 2.0, h, 0.2), Vec3::new(0.0, h / 2.0, -hz - 0.1));
    wall(Vec3::new(hx * 2.0, h, 0.2), Vec3::new(0.0, h / 2.0, hz + 0.1));
    wall(Vec3::new(0.2, h, hz * 2.0), Vec3::new(-hx - 0.1, h / 2.0, 0.0));
    wall(Vec3::new(0.2, h, hz * 2.0), Vec3::new(hx + 0.1, h / 2.0, 0.0));
    for b in &bar::BLOCKS {
        wall(Vec3::new(b.hx * 2.0, b.height, b.hz * 2.0), Vec3::new(b.cx, b.height / 2.0, b.cz));
    }
}

/// Collider and mass for each prop kind.
pub fn prop_body(kind: PropKind) -> (Collider, f32) {
    match kind {
        PropKind::Bottle => (Collider::cylinder(0.04, 0.28), 0.6),
        // A flat box stacks far better than a thin cylinder; it still draws as a disc.
        PropKind::Chip => (Collider::cuboid(0.036, 0.012, 0.036), 0.01),
        PropKind::Stool => (Collider::cuboid(0.4, 0.75, 0.4), 5.0),
        PropKind::Glass => (Collider::cylinder(0.045, 0.15), 0.5),
    }
}

/// Where every prop starts. Deterministic.
pub fn prop_layout() -> Vec<(PropKind, Vec3)> {
    let (_, cz, _, _) = bar::COUNTER;
    let top = bar::COUNTER_HEIGHT;
    let mut out = Vec::new();
    for i in 0..BOTTLES {
        // Mid-counter, clear of the front edge where served glasses land.
        out.push((PropKind::Bottle, Vec3::new(-4.75 + i as f32 * 0.5, top + 0.15, cz - 0.1)));
    }
    for i in 0..CHIPS {
        let (stack, level) = (i / CHIPS_PER_STACK, i % CHIPS_PER_STACK);
        let x = -4.5 + stack as f32 * 1.0;
        out.push((PropKind::Chip, Vec3::new(x, top + 0.007 + level as f32 * 0.0125, cz - 0.35)));
    }
    for i in 0..STOOLS {
        out.push((PropKind::Stool, Vec3::new(-4.5 + i as f32 * 1.0, 0.38, cz + 1.3)));
    }
    out
}

fn spawn_props(mut commands: Commands) {
    for (kind, at) in prop_layout() {
        let (collider, mass) = prop_body(kind);
        let mut e = commands.spawn((
            kind,
            PropPose { pos: at, rot: Quat::IDENTITY },
            HeldBy(None),
            RigidBody::Dynamic,
            collider,
            Mass(mass),
            Transform::from_translation(at),
            Replicate::to_clients(NetworkTarget::All),
            InterpolationTarget::to_clients(NetworkTarget::All),
            // Placed at rest: start asleep so the room does not spend its first
            // seconds settling 130 bodies (a CPU spike, and a burst of pose updates).
            Sleeping,
        ));
        if kind == PropKind::Chip {
            // Thin discs in tall stacks jitter forever at the default sleep
            // threshold. Damp them and let them sleep a little sooner.
            e.insert((SleepThreshold { linear: 0.3, angular: 0.6 }, LinearDamping(0.8), AngularDamping(1.5)));
        }
    }
}

/// Players are kinematic capsules so they push props around.
fn add_player_body(trigger: On<Add, Player>, players: Query<&PlayerPos>, mut commands: Commands) {
    let Ok(pos) = players.get(trigger.entity) else { return };
    let r = bar::PLAYER_RADIUS;
    commands.entity(trigger.entity).insert((
        RigidBody::Kinematic,
        Collider::capsule(r, bar::PLAYER_HEIGHT - 2.0 * r),
        Transform::from_translation(pos.0 + Vec3::Y * bar::PLAYER_HEIGHT / 2.0),
        Hands::default(),
    ));
}

/// Move each player's body to its new position over the coming step.
fn drive_player_bodies(
    mut q: Query<(&PlayerPos, &Position, &mut LinearVelocity), (With<Player>, Without<crate::drunk::PassedOut>)>,
) {
    let dt = shared::TICK.as_secs_f32();
    for (pos, body, mut vel) in &mut q {
        let target = pos.0 + Vec3::Y * bar::PLAYER_HEIGHT / 2.0;
        vel.0 = (target - body.0) / dt;
    }
}

type PropQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Position,
        &'static mut HeldBy,
        &'static mut LinearVelocity,
        &'static mut AngularVelocity,
        Option<&'static mut Beer>,
        Option<&'static ChipValue>,
    ),
    Without<Player>,
>;

type HandPlayers<'w, 's> = Query<
    'w,
    's,
    (
        &'static Player,
        &'static PlayerPos,
        &'static PlayerYaw,
        &'static mut Hands,
        Option<&'static ActionState<PlayerInput>>,
        Option<&'static Drunk>,
        Has<crate::drunk::Dragging>,
        Option<&'static mut Pocket>,
    ),
>;

fn hands(
    mut players: HandPlayers,
    mut props: PropQuery,
    mut commands: Commands,
    tick: Res<crate::TickCount>,
    mut rng: ResMut<crate::drunk::PlayerRng>,
    mut audit: ResMut<crate::casino::Audit>,
) {
    let dt = shared::TICK.as_secs_f32();
    for (player, pos, yaw, mut hands, action, drunk, dragging, mut pocket) in &mut players {
        if drunk.is_some_and(|d| d.passed_out) {
            continue;
        }
        let b = action.map(|a| a.0.buttons).unwrap_or(0);
        let prev = hands.prev_buttons;
        let pressed = |bit: u16| b & bit != 0 && prev & bit == 0;
        let released = |bit: u16| b & bit == 0 && prev & bit != 0;
        let hand = hand_point(pos.0, yaw.0);
        let fwd = forward(yaw.0);

        // Pick up the nearest free prop in reach. At the tap, E pours instead.
        let at_tap = shared::movement::distance_to_tap(pos.0.to_array()) < bar::TAP_REACH;
        if hands.held.is_none() && pressed(buttons::INTERACT) && !at_tap && !dragging {
            let nearest = props
                .iter()
                .filter(|(_, _, held, ..)| held.0.is_none())
                .map(|(e, p, ..)| (e, p.0.distance(hand)))
                .filter(|(_, d)| *d <= REACH)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((e, _)) = nearest
                && let Ok((_, _, mut held, _, _, _, chips)) = props.get_mut(e)
            {
                // A chip stack with money on it goes straight into the pocket.
                if let Some(value) = chips
                    && let Some(pocket) = pocket.as_mut()
                {
                    pocket.0 += value.0;
                    held.0 = Some(player.id);
                    commands.entity(e).despawn();
                    hands.prev_buttons = b;
                    continue;
                }
                held.0 = Some(player.id);
                commands.entity(e).insert(RigidBody::Kinematic).remove::<Sleeping>();
                hands.held = Some(e);
                hands.charge = 0;
            }
        }

        if let Some(e) = hands.held {
            let Ok((_, p, mut held, mut lin, mut ang, beer, _)) = props.get_mut(e) else {
                hands.held = None;
                continue;
            };
            if b & buttons::THROW != 0 {
                hands.charge = (hands.charge + 1).min(FULL_CHARGE_TICKS);
            }
            if released(buttons::THROW) {
                let t = f32::from(hands.charge) / f32::from(FULL_CHARGE_TICKS);
                let speed = THROW_MIN + (THROW_MAX - THROW_MIN) * t;
                // Sloppy and worse throws go wide.
                let spread = shared::drunk::throw_spread(shared::drunk::Tier::of(drunk.map_or(0, |d| d.level)));
                let fwd = if spread > 0.0 {
                    Quat::from_rotation_y(spread * rng.signed(tick.0, &mut audit)) * fwd
                } else {
                    fwd
                };
                held.0 = None;
                lin.0 = fwd * speed + Vec3::Y * 2.0;
                ang.0 = Vec3::new(4.0, 0.0, 2.0);
                // A thrown glass spills everything.
                if let Some(mut beer) = beer {
                    beer.fill = 0;
                }
                hands.held = None;
                commands.entity(e).insert(RigidBody::Dynamic).remove::<Sleeping>();
            } else if pressed(buttons::DROP) {
                held.0 = None;
                lin.0 = Vec3::ZERO;
                ang.0 = Vec3::ZERO;
                hands.held = None;
                commands.entity(e).insert(RigidBody::Dynamic).remove::<Sleeping>();
            } else {
                // Carry: steer the kinematic prop to the hand over one step.
                lin.0 = (hand - p.0) / dt;
                ang.0 = Vec3::ZERO;
            }
        }
        hands.prev_buttons = b;
    }
}

/// Copy physics poses into the replicated component, only when they moved.
fn publish_poses(mut props: Query<(&Position, &Rotation, &mut PropPose)>) {
    for (pos, rot, mut pose) in &mut props {
        let moved = pos.0.distance_squared(pose.pos) > 1e-6 || rot.0.angle_between(pose.rot) > 0.005;
        if moved {
            pose.pos = pos.0;
            pose.rot = rot.0;
        }
    }
}

/// Bodies that are awake (budget: 300).
pub fn awake_bodies(world: &mut World) -> usize {
    let mut q = world.query_filtered::<&RigidBody, Without<Sleeping>>();
    q.iter(world).filter(|b| b.is_dynamic()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_has_the_planned_props() {
        let layout = prop_layout();
        let count = |k: PropKind| layout.iter().filter(|(kind, _)| *kind == k).count();
        assert_eq!(count(PropKind::Bottle), 20);
        assert_eq!(count(PropKind::Chip), 100);
        assert_eq!(count(PropKind::Stool), 10);
    }

    #[test]
    fn hand_is_in_front() {
        let h = hand_point(Vec3::ZERO, 0.0);
        assert!(h.z < 0.0 && h.y > 1.0);
    }
}
