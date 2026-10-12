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
    let mut solid = |size: Vec3, at: Vec3| {
        commands.spawn((RigidBody::Static, Collider::cuboid(size.x, size.y, size.z), Transform::from_translation(at)));
    };
    // A floor slab under every room, walls along their edges (door gaps
    // open), and the bar's blocks.
    for a in shared::world::AREAS.iter().filter(|a| a.room != shared::world::Room::Office) {
        let (w, d) = (a.x1 - a.x0, a.z1 - a.z0);
        solid(Vec3::new(w, 0.2, d), Vec3::new(a.x0 + w / 2.0, -0.1, a.z0 + d / 2.0));
    }
    for b in bar::BLOCKS.iter().chain(shared::world::walls()) {
        solid(Vec3::new(b.hx * 2.0, b.height, b.hz * 2.0), Vec3::new(b.cx, b.height / 2.0, b.cz));
    }
    spawn_hoop(&mut commands);
    spawn_lot_goals(&mut commands);
}

/// The parking lot's goals for the real balls: the soccer goal (posts, bar,
/// and a dead net that stops the ball), the field goal posts, and the
/// goalkeeper's body (moved by [`crate::games::field`]).
fn spawn_lot_goals(commands: &mut Commands) {
    use shared::kicks::{
        BAR, CROSSBAR, GOAL, GOAL_HALF_WIDTH, GOALIE_SPOT, NET_DEPTH, UPRIGHT_HALF, UPRIGHT_HEIGHT, UPRIGHTS,
    };
    let mut solid = |size: Vec3, at: Vec3, bounce: f32| {
        let r = if bounce > 0.0 {
            Restitution::new(bounce).with_combine_rule(CoefficientCombine::Max)
        } else {
            Restitution::new(0.0).with_combine_rule(CoefficientCombine::Min)
        };
        commands.spawn((
            RigidBody::Static,
            Collider::cuboid(size.x, size.y, size.z),
            r,
            Transform::from_translation(at),
        ));
    };
    let (gx, gz) = GOAL;
    let post = 0.12;
    for side in [-1.0, 1.0] {
        solid(Vec3::new(post, BAR, post), Vec3::new(gx + side * (GOAL_HALF_WIDTH + post / 2.0), BAR / 2.0, gz), 0.5);
        // The net's sides.
        solid(
            Vec3::new(0.04, BAR, NET_DEPTH),
            Vec3::new(gx + side * (GOAL_HALF_WIDTH + post / 2.0), BAR / 2.0, gz + NET_DEPTH / 2.0),
            0.0,
        );
    }
    let width = 2.0 * GOAL_HALF_WIDTH + 2.0 * post;
    solid(Vec3::new(width, post, post), Vec3::new(gx, BAR + post / 2.0, gz), 0.5);
    // The net's back and roof.
    solid(Vec3::new(width, BAR, 0.04), Vec3::new(gx, BAR / 2.0, gz + NET_DEPTH), 0.0);
    solid(Vec3::new(width, 0.04, NET_DEPTH), Vec3::new(gx, BAR + 0.02, gz + NET_DEPTH / 2.0), 0.0);
    // Field goal posts: the post, the crossbar, two uprights.
    let (ux, uz) = UPRIGHTS;
    solid(Vec3::new(0.15, CROSSBAR, 0.15), Vec3::new(ux, CROSSBAR / 2.0, uz + 0.6), 0.4);
    solid(Vec3::new(2.0 * UPRIGHT_HALF, 0.1, 0.1), Vec3::new(ux, CROSSBAR, uz), 0.4);
    for side in [-1.0, 1.0] {
        solid(
            Vec3::new(0.1, UPRIGHT_HEIGHT, 0.1),
            Vec3::new(ux + side * UPRIGHT_HALF, CROSSBAR + UPRIGHT_HEIGHT / 2.0, uz),
            0.4,
        );
    }
    // The keeper: a body the size of a goalie, standing in the goal.
    let (kx, kz) = GOALIE_SPOT;
    commands.spawn((
        crate::games::field::Keeper,
        RigidBody::Kinematic,
        Collider::capsule(0.32, 1.2),
        Restitution::new(0.3),
        Transform::from_xyz(kx, 0.92, kz),
    ));
}

/// The roof hoop for the real ball: the backboard, a ring of small spheres
/// for the rim (balls bounce off it and roll round it), and the pole.
fn spawn_hoop(commands: &mut Commands) {
    use shared::hoops::{BOARD, BOARD_X, BOARD_Y, HOOP, RIM_RADIUS, RIM_TUBE, RIM_Y};
    let bouncy = Restitution::new(0.55).with_combine_rule(CoefficientCombine::Max);
    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(BOARD.0, BOARD.1, BOARD.2),
        bouncy,
        Transform::from_xyz(HOOP.0 + BOARD_X + BOARD.0 / 2.0, BOARD_Y, HOOP.1),
    ));
    const RIM_POINTS: usize = 32;
    for i in 0..RIM_POINTS {
        let a = i as f32 / RIM_POINTS as f32 * std::f32::consts::TAU;
        let (s, c) = shared::math::sin_cos(a);
        commands.spawn((
            RigidBody::Static,
            Collider::sphere(RIM_TUBE),
            bouncy,
            Transform::from_xyz(HOOP.0 + c * RIM_RADIUS, RIM_Y, HOOP.1 + s * RIM_RADIUS),
        ));
    }
    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(0.15, RIM_Y, 0.15),
        Transform::from_xyz(HOOP.0 + 0.9, RIM_Y / 2.0, HOOP.1),
    ));
}

/// Where a basketball goes back to when it leaves the roof.
#[derive(Component, Clone, Copy, Debug)]
pub struct BallHome(pub Vec3);

/// Collider and mass for each prop kind.
pub fn prop_body(kind: PropKind) -> (Collider, f32) {
    match kind {
        PropKind::Bottle => (Collider::cylinder(0.04, 0.28), 0.6),
        // A flat box stacks far better than a thin cylinder; it still draws as a disc.
        PropKind::Chip => (Collider::cuboid(0.036, 0.012, 0.036), 0.01),
        PropKind::Stool => (Collider::cuboid(0.4, 0.75, 0.4), 5.0),
        PropKind::Glass => (Collider::cylinder(0.045, 0.15), 0.5),
        PropKind::Mop => (Collider::cuboid(0.06, 1.3, 0.06), 1.2),
        PropKind::Basketball => (Collider::sphere(shared::hoops::BALL_RADIUS), 0.6),
        PropKind::SoccerBall => (Collider::sphere(shared::kicks::SOCCER_RADIUS), 0.43),
        PropKind::Football => (Collider::sphere(shared::kicks::FOOTBALL_RADIUS), 0.42),
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
    let (mx, mz) = shared::fixtures::MOP_CLOSET;
    out.push((PropKind::Mop, Vec3::new(mx, 0.66, mz)));
    for (x, z) in shared::hoops::RACK {
        out.push((PropKind::Basketball, Vec3::new(x, shared::hoops::BALL_RADIUS, z)));
    }
    let (px, pz) = shared::kicks::PENALTY_SPOT;
    out.push((PropKind::SoccerBall, Vec3::new(px, shared::kicks::SOCCER_RADIUS, pz)));
    let (tx, tz) = shared::kicks::TEE;
    out.push((PropKind::Football, Vec3::new(tx, shared::kicks::FOOTBALL_RADIUS, tz)));
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
        if kind.is_ball() {
            let bounce = match kind {
                PropKind::Basketball => 0.78,
                PropKind::SoccerBall => 0.6,
                _ => 0.45,
            };
            // A lively bounce and a little roll resistance; no air drag, so
            // it flies like [`shared::hoops::fly`].
            e.insert((
                Restitution::new(bounce).with_combine_rule(CoefficientCombine::Max),
                Friction::new(0.6),
                AngularDamping(0.6),
                SweptCcd::default(),
                // No look-ahead contacts: with a bounce they fire before the
                // ball touches, and a clean shot bounced off thin air above
                // the rim. The swept collision catches what this misses.
                SpeculativeMargin(0.0),
                BallHome(at),
            ));
            e.remove::<Sleeping>();
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
        Option<&'static PropKind>,
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
                .map(|(e, p, _, _, _, _, _, kind)| {
                    // A ball on the floor: reach down for it.
                    let d = if kind.is_some_and(|k| k.is_ball()) {
                        Vec2::new(p.0.x - pos.0.x, p.0.z - pos.0.z).length().max(p.0.y - 2.2) * 0.8
                    } else {
                        p.0.distance(hand)
                    };
                    (e, d)
                })
                .filter(|(_, d)| *d <= REACH)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((e, _)) = nearest
                && let Ok((_, _, mut held, _, _, _, chips, _)) = props.get_mut(e)
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
            let Ok((_, p, mut held, mut lin, mut ang, beer, _, _)) = props.get_mut(e) else {
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
