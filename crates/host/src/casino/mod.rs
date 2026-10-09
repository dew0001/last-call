//! The casino floor on the host (plan section 5, Phase 3): blackjack,
//! roulette and slots.
//!
//! Players ask a table to act with a [`TableRequest`] message (or, for local
//! players, [`crate::HostSim::table_request`]). Each tick the requests are
//! queued, then each table's system handles the ones for it, plays its
//! customers, and publishes a view for clients.
//!
//! Every table draws from its own RNG stream, and every draw from every
//! stream goes into the [`Audit`] log with the outcomes it decided.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::*;
use bevy::prelude::*;
use lightyear::prelude::*;
use shared::audit::{AuditLog, Derived};
use shared::casino::TableId;
use shared::drunk::Tier;
use shared::protocol::*;
use shared::rng::{Logged, TableRng};

pub mod blackjack;
pub mod roulette;
pub mod slots;

pub struct CasinoPlugin;

impl Plugin for CasinoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TableQueue>().init_resource::<PlayerSpots>();
        let seed = app.world().resource::<crate::RoomSeed>().0;
        let rngs = TableId::all().into_iter().map(|t| (t, TableRng::new(seed, t.stream()))).collect();
        app.insert_resource(TableRngs(rngs));
        app.add_systems(Startup, (blackjack::spawn, roulette::spawn, slots::spawn));
        app.add_systems(
            FixedUpdate,
            (collect_requests, blackjack::run, roulette::run, slots::run, cash_out_lost_chips, clear_queue)
                .chain()
                .in_set(CasinoSet)
                .after(crate::shift::RunClock)
                .after(crate::physics::HandsSet),
        );
    }
}

/// The table systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CasinoSet;

/// The room's RNG audit log. Runners drain it to storage.
#[derive(Resource)]
pub struct Audit(pub AuditLog);

/// One RNG stream per table.
#[derive(Resource)]
pub struct TableRngs(pub BTreeMap<TableId, TableRng>);

impl TableRngs {
    /// Draw from `table`'s stream at `tick`, logging every value.
    pub fn draw<'a>(&'a mut self, table: TableId, tick: u64, audit: &'a mut Audit) -> Logged<'a> {
        self.0.get_mut(&table).expect("every table has a stream").at(tick, &mut audit.0)
    }
}

/// Run `f` with `table`'s stream and record that its draws decided an outcome.
pub fn decide<T>(
    rngs: &mut TableRngs,
    audit: &mut Audit,
    table: TableId,
    tick: u64,
    f: impl FnOnce(&mut Logged) -> T,
    record: impl FnOnce(&T) -> Option<Derived>,
) -> T {
    let first = audit.0.mark(table.stream());
    let out = f(&mut rngs.draw(table, tick, audit));
    if let Some(what) = record(&out) {
        audit.0.outcome(table.stream(), tick, first, what);
    }
    out
}

/// This tick's table requests: (player entity, request).
#[derive(Resource, Default)]
pub struct TableQueue(pub Vec<(Entity, TableRequest)>);

impl TableQueue {
    pub fn for_table(&self, table: TableId) -> impl Iterator<Item = (Entity, shared::casino::TableAction)> + '_ {
        self.0.iter().filter(move |(_, r)| r.table == table).map(|(e, r)| (*e, r.action))
    }
}

/// Spots players hold (blackjack seats, slot machines), so customers pick others.
#[derive(Resource, Default, Debug)]
pub struct PlayerSpots(pub BTreeSet<(TableId, u8)>);

/// Requests from networked players. The link that sent one must control the player.
fn collect_requests(
    mut queue: ResMut<TableQueue>,
    mut links: Query<(Entity, &mut MessageReceiver<TableRequest>)>,
    players: Query<(Entity, &ControlledBy), With<Player>>,
) {
    for (link, mut receiver) in &mut links {
        let player = players.iter().find(|(_, c)| c.owner == link).map(|(e, _)| e);
        for request in receiver.receive() {
            if let Some(p) = player {
                queue.0.push((p, request));
            }
        }
    }
}

fn clear_queue(mut queue: ResMut<TableQueue>) {
    queue.0.clear();
}

/// Marks a customer a table wants gone (walked away, out of money, out of
/// patience). The customers plugin walks them out.
#[derive(Component)]
pub struct WantsToLeave;

/// The players query every table uses.
pub type Players<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Player, &'static PlayerPos, &'static mut Pocket, Option<&'static Drunk>),
    Without<crate::drunk::PassedOut>,
>;

pub fn tier(drunk: Option<&Drunk>) -> Tier {
    Tier::of(drunk.map_or(0, |d| d.level))
}

/// A chip stack paid out on a table: a dynamic chip prop worth `value`. The
/// first player to pick it up gets the money.
pub fn pay_chips(commands: &mut Commands, at: Vec3, value: i64) {
    let (collider, mass) = crate::physics::prop_body(PropKind::Chip);
    commands.spawn((
        Name::new("Chip stack"),
        PropKind::Chip,
        ChipValue(value),
        PropPose { pos: at, rot: Quat::IDENTITY },
        HeldBy(None),
        RigidBody::Dynamic,
        collider,
        Mass(mass),
        Transform::from_translation(at),
        LinearDamping(0.8),
        AngularDamping(1.5),
        Replicate::to_clients(NetworkTarget::All),
        InterpolationTarget::to_clients(NetworkTarget::All),
    ));
}

/// A chip that fell out of the room is gone; keep the money in play by
/// returning it to the house (it cannot be picked up any more).
fn cash_out_lost_chips(
    mut commands: Commands,
    chips: Query<(Entity, &Position, &ChipValue)>,
    mut room: Query<&mut RunLedger, With<RoomState>>,
) {
    for (e, pos, value) in &chips {
        if pos.0.y < -2.0 {
            if let Ok(mut run) = room.single_mut() {
                run.ledger.house += value.0;
            }
            commands.entity(e).despawn();
        }
    }
}

/// Move house money: `net` won (or lost, if negative) at a table, less the
/// commission paid to whoever ran it.
pub fn book(room: &mut Query<&mut RunLedger, With<RoomState>>, net: i64, commission: i64) {
    if let Ok(mut run) = room.single_mut() {
        run.ledger.house += net - commission;
    }
}
