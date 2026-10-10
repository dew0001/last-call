//! Fixtures on the host: the office drawer (Zeen), the kitchen pass (food),
//! the upgrade terminal, the Lucky Charm Shelf, the jukebox, the breaker and
//! the slot machines' service keys.
//!
//! Requests arrive like table requests; the host checks the player stands
//! at the fixture and can pay. Food money goes to the house pool. The
//! breaker and service keys report to the chaos system ([`FixtureUsed`]).

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::buffs::{self, Buffs, Item};
use shared::fixtures::{Charm, Fixture, FixtureAction};
use shared::protocol::*;
use shared::shift::ShiftPhase;

use crate::drunk::PassedOut;
use crate::shift::ShiftTimer;

/// This tick's fixture requests: (player entity, request).
#[derive(Resource, Default)]
pub struct FixtureQueue(pub Vec<(Entity, FixtureRequest)>);

/// A player flipped the breaker or turned a service key.
#[derive(Message, Clone, Copy, Debug)]
pub struct FixtureUsed {
    pub player: Entity,
    pub fixture: Fixture,
}

/// The upgrades owned, copied from the room entity at the start of each tick
/// so every system reads them the same way.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Owned(pub shared::upgrades::Upgrades);

/// The run's new game plus level, copied like [`Owned`].
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct RunNg(pub u8);

impl RunNg {
    /// Customer patience, percent, with the jukebox's bonus.
    pub fn patience(&self, owned: &Owned) -> u32 {
        owned.0.patience_percent() * shared::economy::ng_patience_percent(self.0) / 100
    }
}

fn sync_upgrades(
    mut owned: ResMut<Owned>,
    mut ng: ResMut<RunNg>,
    room: Query<(&RoomUpgrades, Option<&RunLedger>), With<RoomState>>,
) {
    if let Ok((u, run)) = room.single() {
        if owned.0 != u.0 {
            owned.0 = u.0;
        }
        let level = run.map_or(0, |r| r.ledger.ng);
        if ng.0 != level {
            ng.0 = level;
        }
    }
}

/// The fixtures system.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FixturesSet;

pub struct FixturesPlugin;

impl Plugin for FixturesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FixtureQueue>()
            .init_resource::<Owned>()
            .init_resource::<RunNg>()
            .add_message::<FixtureUsed>();
        app.add_systems(FixedPreUpdate, sync_upgrades);
        app.add_systems(
            FixedUpdate,
            (collect, use_fixtures, new_shift).chain().in_set(FixturesSet).after(crate::shift::RunClock),
        );
    }
}

fn collect(
    mut queue: ResMut<FixtureQueue>,
    mut links: Query<(Entity, &mut MessageReceiver<FixtureRequest>)>,
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

type FixturePlayers<'w, 's> = Query<
    'w,
    's,
    (&'static PlayerPos, &'static mut Pocket, &'static mut Drunk, &'static mut Focus, &'static mut Inventory),
    Without<PassedOut>,
>;

#[allow(clippy::too_many_arguments)]
fn use_fixtures(
    mut queue: ResMut<FixtureQueue>,
    timer: Res<ShiftTimer>,
    mut players: FixturePlayers,
    mut room: Query<(&mut RunLedger, &mut RoomUpgrades, &mut JukeboxState, &ChaosState), With<RoomState>>,
    mut used: MessageWriter<FixtureUsed>,
) {
    let Ok((mut run, mut upgrades, mut jukebox, chaos)) = room.single_mut() else {
        queue.0.clear();
        return;
    };
    for (player, req) in std::mem::take(&mut queue.0) {
        let Ok((pos, mut pocket, mut drunk, mut focus, mut inv)) = players.get_mut(player) else { continue };
        if !req.fixture.in_reach(pos.0.x, pos.0.z) {
            continue;
        }
        match (req.fixture, req.action) {
            (Fixture::ZeenDrawer, FixtureAction::Buy(Item::Zeen))
            | (Fixture::KitchenPass, FixtureAction::Buy(Item::Fries | Item::Burger)) => {
                let item = match req.action {
                    FixtureAction::Buy(i) => i,
                    _ => unreachable!(),
                };
                if req.fixture == Fixture::KitchenPass && chaos.kitchen_offline {
                    continue;
                }
                if pocket.0 < item.price() {
                    continue;
                }
                pocket.0 -= item.price();
                if item != Item::Zeen {
                    run.ledger.house += item.price();
                }
                let b = buffs::apply(
                    Buffs { drunk: drunk.level, focus: focus.level, well_fed: inv.well_fed, lucky: inv.lucky },
                    item,
                );
                drunk.level = b.drunk;
                focus.level = b.focus;
                inv.well_fed = b.well_fed;
            }
            (Fixture::KitchenPass, FixtureAction::Buy(Item::FishPlate)) => {
                // Cooked from a fish the player caught: +$10 on the sale, and luck.
                if chaos.kitchen_offline || inv.fish == 0 {
                    continue;
                }
                inv.fish -= 1;
                pocket.0 += 10;
                inv.lucky = true;
            }
            (Fixture::Shop, FixtureAction::Upgrade(id)) => {
                if timer.phase != ShiftPhase::Setup {
                    continue;
                }
                let mut house = run.ledger.house;
                if upgrades.0.buy(id, &mut house).is_ok() {
                    run.ledger.house = house;
                }
            }
            (Fixture::CharmShelf, FixtureAction::Charm(charm)) => {
                if !upgrades.0.has(shared::upgrades::UpgradeId::CharmShelf) || pocket.0 < charm.price() {
                    continue;
                }
                pocket.0 -= charm.price();
                run.ledger.house += charm.price();
                match charm {
                    Charm::RiggedDie => inv.rigged_dice = inv.rigged_dice.saturating_add(1),
                    Charm::MarkedDeck => inv.marked_deck = true,
                    Charm::ColdBrew => {
                        drunk.level = drunk.level.saturating_sub(25);
                        focus.level = focus.level.saturating_add(20).min(100);
                    }
                }
            }
            (Fixture::Jukebox, FixtureAction::Track(n)) => {
                if upgrades.0.has(shared::upgrades::UpgradeId::Jukebox)
                    && usize::from(n) < shared::fixtures::TRACKS.len()
                {
                    jukebox.track = Some(n);
                }
            }
            (Fixture::Breaker | Fixture::ServiceKey(_), FixtureAction::Use) => {
                used.write(FixtureUsed { player, fixture: req.fixture });
            }
            _ => {}
        }
    }
}

/// A new shift: one-shift items and food effects wear off.
fn new_shift(mut started: MessageReader<crate::shift::PhaseStarted>, mut players: Query<&mut Inventory>) {
    if started.read().any(|p| p.phase == ShiftPhase::Setup) {
        for mut inv in &mut players {
            inv.set_if_neq(Inventory::default());
        }
    }
}
