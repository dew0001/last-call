//! Customer NPCs on the host (plan section 4.4, Phase 2 scope: the bar).
//!
//! During Open a wave of customers comes in every [`Timings::wave`] seconds.
//! Each walks the navmesh from the door to a free stool in front of the
//! counter, sits, and orders a beer. Served, they pay and drink; unserved for
//! [`PATIENCE_SECS`], they leave. Everyone leaves at Last call, or when their
//! cash runs out. Someone who loses their stool (it was knocked over or
//! carried off) leaves too.
//!
//! The navmesh is built once from [`shared::bar::BLOCKS`] and queried
//! synchronously, so the simulation stays deterministic.
//!
//! [`Timings::wave`]: shared::shift::Timings::wave

use avian3d::prelude::{Position, Rotation};
use bevy::prelude::*;
use lightyear::prelude::*;
use shared::bar;
use shared::casino::{self, TableId};
use shared::customers::{self, DOOR, Mood, PATIENCE_SECS, RADIUS, WALK_SPEED};
use shared::protocol::{Customer, HeldBy, NpcPose, PropKind};
use shared::rng::{Draw, ROOM_STREAM, TableRng};
use shared::shift::ShiftPhase;
use vleue_navigator::NavMesh;

use crate::casino::{Audit, PlayerSpots, WantsToLeave};
use crate::shift::{PhaseStarted, RunClock, ShiftConfig, ShiftTimer};

/// Height of a seated customer's feet: on top of a stool.
const SEAT_HEIGHT: f32 = 0.45;
/// A stool that moved more than this from where its customer sat no longer counts.
const SEAT_SLIP: f32 = 0.3;

/// The bar's navmesh, in the floor plane (mesh x = world x, mesh y = world z).
#[derive(Resource)]
pub struct BarNavMesh(pub NavMesh);

/// Build the navmesh: the room inset by the customer radius, minus every block
/// grown by the same radius.
pub fn build_navmesh() -> NavMesh {
    let r = RADIUS;
    let (hx, hz) = (bar::HALF_X - r, bar::HALF_Z - r);
    let edges = vec![Vec2::new(-hx, -hz), Vec2::new(hx, -hz), Vec2::new(hx, hz), Vec2::new(-hx, hz)];
    let obstacles = bar::BLOCKS
        .iter()
        .map(|b| {
            // Grow by the radius, but keep inside the outer edge.
            let x0 = (b.cx - b.hx - r).max(-hx + 0.01);
            let x1 = (b.cx + b.hx + r).min(hx - 0.01);
            let z0 = (b.cz - b.hz - r).max(-hz + 0.01);
            let z1 = (b.cz + b.hz + r).min(hz - 0.01);
            vec![Vec2::new(x0, z0), Vec2::new(x1, z0), Vec2::new(x1, z1), Vec2::new(x0, z1)]
        })
        .collect();
    NavMesh::from_edge_and_obstacles(edges, obstacles)
}

/// A walkable path from `from` to `to` (floor x, z), excluding `from`. Falls
/// back to a straight line when either end is off the mesh.
pub fn route(mesh: &NavMesh, from: Vec2, to: Vec2) -> Vec<Vec2> {
    mesh.path(from, to).map(|p| p.path).unwrap_or_else(|| vec![to])
}

/// The room's RNG stream for customers.
#[derive(Resource)]
pub struct CustomerRng(TableRng);

/// What a customer came to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// Sit at the counter and drink.
    Bar,
    /// Gamble at a table or machine, at this bettor spot.
    Table(TableId, u8),
}

/// How customers pick an activity: (bar, blackjack, roulette, slots).
pub const ACTIVITY_WEIGHTS: [u32; 4] = [2, 3, 3, 2];

/// The room's activity weights (tests send everyone to the bar).
#[derive(Resource, Clone, Copy, Debug)]
pub struct Tastes(pub [u32; 4]);

/// Only the bar: the Phase 2 behaviour.
pub const BAR_ONLY: [u32; 4] = [1, 0, 0, 0];

/// Host-only customer state.
#[derive(Component, Debug)]
pub struct Npc {
    pub cash: i64,
    /// Cash they came in with (walk-away thresholds and bet sizes).
    pub start_cash: i64,
    pub activity: Activity,
    /// The stool this customer is heading to or sitting on, and where it stood
    /// when they claimed it.
    pub seat: Option<(Entity, Vec3)>,
    pub path: Vec<Vec2>,
    /// Ticks left in the current wait (patience, or time until the next order).
    /// Tables use it for patience with the dealer or croupier.
    pub ticks: u32,
}

#[derive(Resource, Default)]
struct WaveTimer {
    ticks_left: u32,
    next_id: u32,
}

pub struct CustomersPlugin;

impl Plugin for CustomersPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(BarNavMesh(build_navmesh()));
        app.init_resource::<WaveTimer>();
        app.add_systems(Startup, seed_rng);
        app.add_systems(
            FixedUpdate,
            (phase_changes, spawn_waves, walk, seated, walk_out)
                .chain()
                .after(RunClock)
                .after(crate::casino::CasinoSet),
        );
    }
}

fn seed_rng(mut commands: Commands, seed: Res<crate::RoomSeed>) {
    commands.insert_resource(CustomerRng(TableRng::new(seed.0, ROOM_STREAM)));
}

fn ticks(secs: u32) -> u32 {
    secs * shared::TICK_HZ
}

/// Open starts the waves; Last call sends everyone home; a new Setup clears
/// anyone still walking out.
fn phase_changes(
    mut commands: Commands,
    mut started: MessageReader<PhaseStarted>,
    mut waves: ResMut<WaveTimer>,
    mesh: Res<BarNavMesh>,
    mut npcs: Query<(Entity, &mut Customer, &mut Npc, &NpcPose)>,
) {
    for p in started.read() {
        match p.phase {
            ShiftPhase::Open => waves.ticks_left = 0,
            ShiftPhase::LastCall => {
                for (_, mut c, mut npc, pose) in &mut npcs {
                    leave(&mesh.0, &mut c, &mut npc, pose);
                }
            }
            ShiftPhase::Setup => {
                for (e, ..) in &npcs {
                    commands.entity(e).despawn();
                }
            }
            ShiftPhase::Payment => {}
        }
    }
}

/// Free stools in front of the counter, sorted by x so picks are deterministic.
fn free_seats(
    stools: &Query<(Entity, &PropKind, &Position, &Rotation, &HeldBy)>,
    npcs: &Query<(Entity, &mut Customer, &mut Npc, &NpcPose)>,
) -> Vec<(Entity, Vec3)> {
    let taken: Vec<Entity> = npcs.iter().filter_map(|(_, _, n, _)| n.seat.map(|s| s.0)).collect();
    let mut seats: Vec<(Entity, Vec3)> = stools
        .iter()
        .filter(|(e, kind, pos, rot, held)| {
            **kind == PropKind::Stool
                && held.0.is_none()
                && !taken.contains(e)
                && customers::is_bar_seat(pos.0.x, pos.0.z, (rot.0 * Vec3::Y).y)
        })
        .map(|(e, _, pos, ..)| (e, pos.0))
        .collect();
    seats.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
    seats
}

/// Free bettor spots at a table: not held by a player, not claimed by a customer.
fn free_spots(
    table: TableId,
    players: &PlayerSpots,
    npcs: &Query<(Entity, &mut Customer, &mut Npc, &NpcPose)>,
) -> Vec<(Activity, Vec3)> {
    casino::bettor_spots(table)
        .into_iter()
        .enumerate()
        .map(|(i, (x, z))| (i as u8, x, z))
        .filter(|(i, ..)| !players.0.contains(&(table, *i)))
        .filter(|(i, ..)| {
            !npcs.iter().any(|(_, c, n, _)| c.mood != Mood::Leaving && n.activity == Activity::Table(table, *i))
        })
        .map(|(i, x, z)| (Activity::Table(table, i), Vec3::new(x, 0.0, z)))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn spawn_waves(
    mut commands: Commands,
    config: Res<ShiftConfig>,
    timer: Res<ShiftTimer>,
    tick: Res<crate::TickCount>,
    mesh: Res<BarNavMesh>,
    mut rng: ResMut<CustomerRng>,
    mut audit: ResMut<Audit>,
    tastes: Res<Tastes>,
    player_spots: Res<PlayerSpots>,
    mut waves: ResMut<WaveTimer>,
    stools: Query<(Entity, &PropKind, &Position, &Rotation, &HeldBy)>,
    npcs: Query<(Entity, &mut Customer, &mut Npc, &NpcPose)>,
) {
    if timer.phase != ShiftPhase::Open || timer.frozen {
        return;
    }
    if waves.ticks_left > 0 {
        waves.ticks_left -= 1;
        return;
    }
    waves.ticks_left = ticks(config.timings.wave);
    let week = timer.calendar.week;
    let mut seats = free_seats(&stools, &npcs);
    let mut blackjack = free_spots(TableId::Blackjack, &player_spots, &npcs);
    let mut roulette = free_spots(TableId::Roulette, &player_spots, &npcs);
    let mut slots: Vec<(Activity, Vec3)> =
        (0..casino::SLOT_MACHINES).flat_map(|m| free_spots(TableId::Slot(m), &player_spots, &npcs)).collect();
    let door = Vec2::new(DOOR.0, DOOR.1);
    let mut d = rng.0.at(tick.0, &mut audit.0);
    for _ in 0..customers::wave_size(week) {
        let weights = tastes.0;
        let open = [!seats.is_empty(), !blackjack.is_empty(), !roulette.is_empty(), !slots.is_empty()];
        let total: u32 = (0..4).filter(|i| open[*i]).map(|i| weights[i]).sum();
        if total == 0 {
            break;
        }
        let mut roll = d.below(total);
        let mut kind = 0;
        for i in 0..4 {
            if !open[i] || weights[i] == 0 {
                continue;
            }
            if roll < weights[i] {
                kind = i;
                break;
            }
            roll -= weights[i];
        }
        let (activity, seat, at) = match kind {
            0 => {
                let (stool, at) = seats.remove(d.below(seats.len() as u32) as usize);
                (Activity::Bar, Some((stool, at)), at)
            }
            k => {
                let list = match k {
                    1 => &mut blackjack,
                    2 => &mut roulette,
                    _ => &mut slots,
                };
                let (activity, at) = list.remove(d.below(list.len() as u32) as usize);
                (activity, None, at)
            }
        };
        let cash = customers::starting_cash(d.below(361), week);
        waves.next_id += 1;
        commands.spawn((
            Name::new("Customer"),
            Customer { id: waves.next_id, mood: Mood::Entering, patience: 0 },
            NpcPose { pos: Vec3::new(DOOR.0, 0.0, DOOR.1), yaw: 0.0 },
            Npc { cash, start_cash: cash, activity, seat, path: route(&mesh.0, door, Vec2::new(at.x, at.z)), ticks: 0 },
            Replicate::to_clients(NetworkTarget::All),
            InterpolationTarget::to_clients(NetworkTarget::All),
        ));
    }
}

/// Customers a table sent away walk out.
fn walk_out(
    mut commands: Commands,
    mesh: Res<BarNavMesh>,
    mut npcs: Query<(Entity, &mut Customer, &mut Npc, &NpcPose), With<WantsToLeave>>,
) {
    for (e, mut c, mut npc, pose) in &mut npcs {
        leave(&mesh.0, &mut c, &mut npc, pose);
        commands.entity(e).remove::<WantsToLeave>();
    }
}

/// Start walking out: off the stool, then to the door.
fn leave(mesh: &NavMesh, c: &mut Customer, npc: &mut Npc, pose: &NpcPose) {
    if c.mood == Mood::Leaving {
        return;
    }
    c.mood = Mood::Leaving;
    c.patience = 0;
    npc.seat = None;
    // Step back from the counter first, so the path starts on the navmesh.
    let off = Vec2::new(pose.pos.x, pose.pos.z + 0.6);
    let mut path = vec![off];
    path.extend(route(mesh, off, Vec2::new(DOOR.0, DOOR.1)));
    npc.path = path;
}

/// Walk along the path; arrive at the stool or out the door.
fn walk(mut commands: Commands, mut npcs: Query<(Entity, &mut Customer, &mut Npc, &mut NpcPose)>) {
    let step = WALK_SPEED * shared::TICK.as_secs_f32();
    for (e, mut c, mut npc, mut pose) in &mut npcs {
        if npc.path.is_empty() {
            continue;
        }
        let here = Vec2::new(pose.pos.x, pose.pos.z);
        let target = npc.path[0];
        let to = target - here;
        let d = to.length();
        let mut next = *pose;
        if d <= step {
            next.pos = Vec3::new(target.x, 0.0, target.y);
            npc.path.remove(0);
        } else {
            let dir = to / d;
            next.pos = Vec3::new(here.x + dir.x * step, 0.0, here.y + dir.y * step);
            // Yaw 0 faces -Z: forward = (-sin yaw, -cos yaw).
            next.yaw = shared::math::atan2(-dir.x, -dir.y);
        }
        if npc.path.is_empty() {
            match c.mood {
                Mood::Entering => match npc.activity {
                    Activity::Bar => {
                        // Sit down facing the counter and order.
                        if let Some((_, at)) = npc.seat {
                            next.pos = Vec3::new(at.x, SEAT_HEIGHT, at.z);
                        }
                        next.yaw = 0.0;
                        c.mood = Mood::Waiting;
                        npc.ticks = ticks(PATIENCE_SECS);
                    }
                    Activity::Table(table, _) => {
                        // Face the table (slot machines stand to the west).
                        next.yaw = if matches!(table, TableId::Slot(_)) { std::f32::consts::FRAC_PI_2 } else { 0.0 };
                        c.mood = Mood::Gambling;
                        npc.ticks = ticks(casino::TABLE_PATIENCE_SECS);
                    }
                },
                Mood::Leaving => {
                    commands.entity(e).despawn();
                    continue;
                }
                Mood::Waiting | Mood::Drinking | Mood::Gambling => {}
            }
        }
        pose.set_if_neq(next);
    }
}

/// Seated customers: patience runs out, drinks finish, stools get taken away.
fn seated(
    mesh: Res<BarNavMesh>,
    stools: Query<(&Position, &Rotation, &HeldBy)>,
    mut npcs: Query<(&mut Customer, &mut Npc, &NpcPose)>,
) {
    for (mut c, mut npc, pose) in &mut npcs {
        if !matches!(c.mood, Mood::Waiting | Mood::Drinking) {
            continue;
        }
        let lost_seat = match npc.seat {
            Some((stool, at)) => stools.get(stool).map_or(true, |(pos, rot, held)| {
                held.0.is_some() || pos.0.distance(at) > SEAT_SLIP || (rot.0 * Vec3::Y).y < 0.9
            }),
            None => true,
        };
        npc.ticks = npc.ticks.saturating_sub(1);
        if lost_seat || (c.mood == Mood::Waiting && npc.ticks == 0) {
            leave(&mesh.0, &mut c, &mut npc, pose);
            continue;
        }
        if c.mood == Mood::Drinking && npc.ticks == 0 {
            if npc.cash >= customers::BEER_PRICE {
                c.mood = Mood::Waiting;
                npc.ticks = ticks(PATIENCE_SECS);
            } else {
                leave(&mesh.0, &mut c, &mut npc, pose);
                continue;
            }
        }
        let patience = if c.mood == Mood::Waiting { npc.ticks.div_ceil(shared::TICK_HZ) as u8 } else { 0 };
        if c.patience != patience {
            c.patience = patience;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crosses_block(a: Vec2, b: Vec2) -> bool {
        // Sample the segment; any sample inside a block (not grown) is a crossing.
        (0..=50).any(|i| {
            let p = a.lerp(b, i as f32 / 50.0);
            bar::BLOCKS.iter().any(|k| (p.x - k.cx).abs() < k.hx && (p.y - k.cz).abs() < k.hz)
        })
    }

    #[test]
    fn the_navmesh_routes_around_the_counter() {
        let mesh = build_navmesh();
        let door = Vec2::new(DOOR.0, DOOR.1);
        let (_, cz, _, hz) = bar::COUNTER;
        let seat = Vec2::new(2.5, cz + hz + 0.8);
        let path = route(&mesh, door, seat);
        assert!(mesh.path(door, seat).is_some(), "door to stool has a path");
        assert_eq!(*path.last().unwrap(), seat);
        let mut from = door;
        for p in &path {
            assert!(!crosses_block(from, *p), "segment {from} -> {p} crosses a block");
            from = *p;
        }
        // Behind the counter: the path must go round its end.
        let behind = Vec2::new(0.0, cz - hz - 0.6);
        let around = mesh.path(door, behind).expect("a path behind the counter");
        assert!(around.length > (door - behind).length() + 1.0);
        let mut from = door;
        for p in &around.path {
            assert!(!crosses_block(from, *p), "segment {from} -> {p} crosses a block");
            from = *p;
        }
    }

    #[test]
    fn the_office_is_reachable() {
        let mesh = build_navmesh();
        let inside = Vec2::new(8.0, -5.0);
        assert!(mesh.path(Vec2::new(DOOR.0, DOOR.1), inside).is_some());
    }
}
