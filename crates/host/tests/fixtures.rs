//! Fixtures and upgrades on the host: Zeen and food, the upgrade shop, the
//! Lucky Charm Shelf (a rigged die at roulette), the Focus meter and The Spins.

use bevy::prelude::*;
use host::economy::Preset;
use host::{HostConfig, HostSim};
use shared::buffs::Item;
use shared::casino::{self, TableAction, TableId};
use shared::fixtures::{Charm, Fixture, FixtureAction};
use shared::protocol::*;
use shared::roulette::Bet as RBet;
use shared::shift::Timings;
use shared::upgrades::UpgradeId;

/// A room that stays in Setup.
fn setup_room() -> (HostSim, Entity) {
    let timings = Timings { setup: 10_000, ..Timings::PLAN };
    let mut sim = HostSim::with_config(HostConfig {
        timings,
        preset: Preset::Casino,
        manual_chaos: true,
        seed: [3; 32],
        ..Default::default()
    });
    let a = sim.add_local_player(0xa, "a", 0);
    run(&mut sim, 3);
    (sim, a)
}

fn run(sim: &mut HostSim, n: u32) {
    for _ in 0..n {
        sim.tick();
    }
}

fn put(sim: &mut HostSim, e: Entity, (x, z): (f32, f32)) {
    sim.world_mut().get_mut::<PlayerPos>(e).unwrap().0 = Vec3::new(x, 0.0, z);
}

fn use_fixture(sim: &mut HostSim, p: Entity, fixture: Fixture, action: FixtureAction) {
    put(sim, p, fixture.position());
    sim.fixture_request(p, FixtureRequest { fixture, action });
    run(sim, 1);
}

fn pocket(sim: &HostSim, e: Entity) -> i64 {
    sim.world().get::<Pocket>(e).unwrap().0
}

fn room<T: Component + Clone>(sim: &mut HostSim) -> T {
    let w = sim.world_mut();
    w.query_filtered::<&T, With<RoomState>>().single(w).unwrap().clone()
}

fn set_house(sim: &mut HostSim, house: i64) {
    let w = sim.world_mut();
    w.query_filtered::<&mut RunLedger, With<RoomState>>().single_mut(w).unwrap().ledger.house = house;
}

#[test]
fn zeen_and_food_cost_money_and_change_the_meters() {
    let (mut sim, a) = setup_room();
    let start = pocket(&sim, a);
    use_fixture(&mut sim, a, Fixture::ZeenDrawer, FixtureAction::Buy(Item::Zeen));
    assert_eq!(pocket(&sim, a), start - Item::Zeen.price());
    assert_eq!(sim.world().get::<Focus>(a).unwrap().level, shared::buffs::FOCUS_PER_ZEEN);

    sim.world_mut().get_mut::<Drunk>(a).unwrap().level = 30;
    let house = room::<RunLedger>(&mut sim).ledger.house;
    use_fixture(&mut sim, a, Fixture::KitchenPass, FixtureAction::Buy(Item::Burger));
    assert_eq!(sim.world().get::<Drunk>(a).unwrap().level, 0, "a burger clears 30 drunk");
    assert!(sim.world().get::<Inventory>(a).unwrap().well_fed);
    assert_eq!(room::<RunLedger>(&mut sim).ledger.house, house + Item::Burger.price(), "food money goes to the house");

    // Out of reach: nothing happens.
    put(&mut sim, a, (0.0, 0.0));
    sim.fixture_request(a, FixtureRequest { fixture: Fixture::ZeenDrawer, action: FixtureAction::Buy(Item::Zeen) });
    let before = pocket(&sim, a);
    run(&mut sim, 1);
    assert_eq!(pocket(&sim, a), before);
}

#[test]
fn the_shop_sells_upgrades_from_the_house_pool() {
    let (mut sim, a) = setup_room();
    set_house(&mut sim, 2_000);
    use_fixture(&mut sim, a, Fixture::Shop, FixtureAction::Upgrade(UpgradeId::Felt));
    assert_eq!(room::<RoomUpgrades>(&mut sim).0.rank(UpgradeId::Felt), 1);
    assert_eq!(room::<RunLedger>(&mut sim).ledger.house, 500);
    use_fixture(&mut sim, a, Fixture::Shop, FixtureAction::Upgrade(UpgradeId::Felt));
    assert_eq!(room::<RoomUpgrades>(&mut sim).0.rank(UpgradeId::Felt), 1, "rank 2 costs 3,000");
    run(&mut sim, 1);
    // The table max doubled.
    let max = room::<RoomUpgrades>(&mut sim).0.table_max(casino::BLACKJACK_MAX);
    assert_eq!(max, casino::BLACKJACK_MAX * 2);
}

#[test]
fn a_rigged_die_lands_the_ball_in_its_dozen() {
    let (mut sim, a) = setup_room();
    let croupier = sim.add_local_player(0xc, "c", 1);
    run(&mut sim, 2);
    set_house(&mut sim, 10_000);
    use_fixture(&mut sim, a, Fixture::Shop, FixtureAction::Upgrade(UpgradeId::CharmShelf));
    for _ in 0..3 {
        use_fixture(&mut sim, a, Fixture::CharmShelf, FixtureAction::Charm(Charm::RiggedDie));
    }
    assert_eq!(sim.world().get::<Inventory>(a).unwrap().rigged_dice, 3);

    put(&mut sim, croupier, casino::role_spot(TableId::Roulette).unwrap());
    put(&mut sim, a, casino::bettor_spots(TableId::Roulette)[2]);
    sim.table_request(croupier, TableRequest { table: TableId::Roulette, action: TableAction::TakeRole });
    run(&mut sim, 1);
    for dozen in 0..3u8 {
        let ask =
            |sim: &mut HostSim, e, action| sim.table_request(e, TableRequest { table: TableId::Roulette, action });
        ask(&mut sim, a, TableAction::RouletteBet(RBet::Dozen(dozen), 10));
        ask(&mut sim, a, TableAction::RiggedDie(dozen));
        run(&mut sim, 1);
        ask(&mut sim, croupier, TableAction::Spin);
        run(&mut sim, 1);
        let w = sim.world_mut();
        let result = w.query::<&RouletteView>().single(w).unwrap().result.unwrap();
        assert!((1 + 12 * dozen..=12 + 12 * dozen).contains(&result), "dozen {dozen}: {result}");
        // Let the spin end and rake the losers.
        run(&mut sim, 8 * 64);
        ask(&mut sim, croupier, TableAction::Rake);
        run(&mut sim, 2 * 64);
    }
    assert_eq!(sim.world().get::<Inventory>(a).unwrap().rigged_dice, 0);
    // The audit log re-derives the rigged spins too.
    let lines: Vec<String> = sim.drain_audit().iter().map(|e| e.to_line()).collect();
    let report = shared::audit::verify(lines.iter().map(String::as_str)).expect("the log replays");
    assert!(report.spins >= 3);
}

#[test]
fn beer_and_zeen_together_bring_on_the_spins() {
    let (mut sim, a) = setup_room();
    sim.world_mut().get_mut::<Drunk>(a).unwrap().level = 50;
    sim.world_mut().get_mut::<Focus>(a).unwrap().level = 70;
    run(&mut sim, 2);
    assert!(sim.world().get::<Focus>(a).unwrap().spinning);
    run(&mut sim, shared::buffs::SPINS_SECS * shared::TICK_HZ + 2);
    let focus = *sim.world().get::<Focus>(a).unwrap();
    assert!(!focus.spinning);
    assert_eq!(focus.level, 0);
    let w = sim.world_mut();
    assert_eq!(w.query::<&Vomit>().iter(w).count(), 1, "a slippery mess on the floor");
}
