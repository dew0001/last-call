//! Chaos events (plan section 4.7): each one's counter, and its consequence
//! when nobody counters it.

use bevy::prelude::*;
use host::{HostConfig, HostSim};
use shared::casino::{self, TableAction, TableId};
use shared::chaos::{ChaosKind, Ending};
use shared::fixtures::{Fixture, FixtureAction};
use shared::movement::buttons;
use shared::protocol::*;
use shared::shift::{ShiftPhase, Timings};
use shared::upgrades::UpgradeId;

const HZ: u32 = shared::TICK_HZ;

/// A room in Open with one player, customers only at the bar (so table
/// money does not move), and a long Open.
fn open_room() -> (HostSim, Entity) {
    let timings = Timings { setup: 1, open: 300, last_call: 1, payment: 1, outcome: 1, wave: 10_000 };
    let config =
        HostConfig { timings, tastes: Some(host::customers::BAR_ONLY), manual_chaos: true, ..Default::default() };
    let mut sim = HostSim::with_config(config);
    let a = sim.add_local_player(0xa, "a", 0);
    while clock(&mut sim).phase != ShiftPhase::Open {
        sim.tick();
    }
    (sim, a)
}

fn clock(sim: &mut HostSim) -> ShiftClock {
    let w = sim.world_mut();
    *w.query::<&ShiftClock>().single(w).unwrap()
}

fn state(sim: &mut HostSim) -> ChaosState {
    let w = sim.world_mut();
    w.query_filtered::<&ChaosState, With<RoomState>>().single(w).unwrap().clone()
}

fn house(sim: &mut HostSim) -> i64 {
    let w = sim.world_mut();
    w.query_filtered::<&RunLedger, With<RoomState>>().single(w).unwrap().ledger.house
}

fn run(sim: &mut HostSim, ticks: u32) {
    for _ in 0..ticks {
        sim.tick();
    }
}

fn put(sim: &mut HostSim, p: Entity, (x, z): (f32, f32)) {
    sim.world_mut().get_mut::<PlayerPos>(p).unwrap().0 = Vec3::new(x, 0.0, z);
}

fn buy(sim: &mut HostSim, id: UpgradeId) {
    let w = sim.world_mut();
    let mut up = w.query_filtered::<&mut RoomUpgrades, With<RoomState>>().single_mut(w).unwrap();
    let mut cash = 1_000_000;
    up.0.buy(id, &mut cash).unwrap();
}

fn npcs(sim: &mut HostSim, kind: ChaosKind) -> Vec<(Entity, NpcPose, ChaosNpc)> {
    let w = sim.world_mut();
    let mut q = w.query::<(Entity, &NpcPose, &ChaosNpc)>();
    let mut out: Vec<_> = q.iter(w).filter(|(_, _, c)| c.kind == kind).map(|(e, p, c)| (e, *p, *c)).collect();
    out.sort_by_key(|(e, ..)| *e);
    out
}

/// Grab the NPC (R next to it) and walk it out the front door.
fn haul_out(sim: &mut HostSim, p: Entity, npc: Entity) {
    let at = sim.world().get::<NpcPose>(npc).unwrap().pos;
    put(sim, p, (at.x, at.z - 0.5));
    sim.set_input(p, PlayerInput { buttons: buttons::USE, ..default() });
    sim.tick();
    sim.set_input(p, PlayerInput::default());
    sim.tick();
    // Walk to the door, then through it.
    for i in 0..200 {
        let t = i as f32 / 100.0;
        let (x, z) = if t < 1.0 { (at.x * (1.0 - t), at.z + (6.8 - at.z) * t) } else { (0.0, 6.8 + (t - 1.0) * 2.0) };
        put(sim, p, (x, z));
        sim.tick();
        if sim.world().get_entity(npc).is_err() {
            return;
        }
    }
    panic!("the NPC was never hauled out");
}

#[test]
fn the_breaker_ends_an_outage_and_a_dark_outage_runs_its_course() {
    let (mut sim, a) = open_room();
    sim.force_chaos(ChaosKind::Outage);
    run(&mut sim, 2);
    assert!(state(&mut sim).dark, "the lights are out");
    put(&mut sim, a, shared::world::BREAKER);
    sim.fixture_request(a, FixtureRequest { fixture: Fixture::Breaker, action: FixtureAction::Use });
    run(&mut sim, 2);
    assert!(!state(&mut sim).dark);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::Outage, Ending::Countered)]);

    sim.force_chaos(ChaosKind::Outage);
    run(&mut sim, 61 * HZ);
    assert_eq!(sim.chaos_log()[1], (ChaosKind::Outage, Ending::Consequence));
}

#[test]
fn the_service_key_fixes_a_jammed_slot() {
    let (mut sim, a) = open_room();
    sim.force_chaos(ChaosKind::SlotJam);
    run(&mut sim, 2);
    let m = state(&mut sim).active[0].target.expect("a jammed machine");
    let key = Fixture::ServiceKey(m);
    put(&mut sim, a, key.position());
    sim.fixture_request(a, FixtureRequest { fixture: key, action: FixtureAction::Use });
    run(&mut sim, 2);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::SlotJam, Ending::Countered)]);
}

#[test]
fn a_jammed_machine_pays_ten_times_up_to_the_cap() {
    let mut fx = host::chaos::TableEffects { jam: Some(1), ..default() };
    assert_eq!(fx.jam_extra(0, 100), 0, "only the jammed machine");
    assert_eq!(fx.jam_extra(1, 100), 900);
    assert_eq!(fx.jam_extra(1, 1_000), 1_100, "the house loses at most 2,000");
    assert_eq!(fx.jam_extra(1, 1_000), 0);
}

#[test]
fn the_inspector_fines_a_mess_and_passes_a_clean_bar() {
    let (mut sim, _) = open_room();
    let before = house(&mut sim);
    sim.force_chaos(ChaosKind::Inspector);
    run(&mut sim, 91 * HZ);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::Inspector, Ending::Countered)]);
    assert_eq!(house(&mut sim), before);
    assert!(npcs(&mut sim, ChaosKind::Inspector).is_empty(), "he left");

    sim.world_mut().spawn(Puddle { pos: Vec3::new(0.0, 0.01, 0.0) });
    sim.force_chaos(ChaosKind::Inspector);
    run(&mut sim, 91 * HZ);
    assert_eq!(sim.chaos_log()[1], (ChaosKind::Inspector, Ending::Consequence));
    assert_eq!(house(&mut sim), before - shared::chaos::INSPECTOR_FINE);
}

#[test]
fn a_raid_seizes_chips_in_the_bar_but_not_in_the_office() {
    let (mut sim, _) = open_room();
    {
        let w = sim.world_mut();
        let mut c = w.commands();
        host::casino::pay_chips(&mut c, Vec3::new(0.0, 0.1, 0.0), 300);
        host::casino::pay_chips(&mut c, Vec3::new(8.0, 0.1, -4.0), 200);
        w.flush();
    }
    run(&mut sim, 2);
    let chips = |sim: &mut HostSim| {
        let w = sim.world_mut();
        let mut v: Vec<i64> = w.query::<&ChipValue>().iter(w).map(|c| c.0).collect();
        v.sort();
        v
    };
    assert_eq!(chips(&mut sim), vec![200, 300]);
    sim.force_chaos(ChaosKind::Raid);
    run(&mut sim, 21 * HZ);
    assert!(state(&mut sim).active[0].cops_in);
    assert_eq!(npcs(&mut sim, ChaosKind::Raid).len(), 2, "two cops");
    assert_eq!(chips(&mut sim), vec![200], "the office chips are safe");
    run(&mut sim, 40 * HZ);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::Raid, Ending::Consequence)]);
    assert!(npcs(&mut sim, ChaosKind::Raid).is_empty());
}

#[test]
fn hauling_both_brawlers_out_ends_a_brawl() {
    let (mut sim, a) = open_room();
    sim.force_chaos(ChaosKind::Brawl);
    run(&mut sim, 6 * HZ);
    let brawlers = npcs(&mut sim, ChaosKind::Brawl);
    assert_eq!(brawlers.len(), 2);
    for (e, ..) in brawlers {
        haul_out(&mut sim, a, e);
    }
    run(&mut sim, 2);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::Brawl, Ending::Countered)]);
}

#[test]
fn an_unchecked_brawl_breaks_three_props() {
    let (mut sim, _) = open_room();
    let breakable = |sim: &mut HostSim| {
        let w = sim.world_mut();
        w.query::<&PropKind>().iter(w).filter(|k| matches!(k, PropKind::Bottle | PropKind::Glass)).count()
    };
    let before = breakable(&mut sim);
    sim.force_chaos(ChaosKind::Brawl);
    run(&mut sim, 46 * HZ);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::Brawl, Ending::Consequence)]);
    assert_eq!(breakable(&mut sim), before - shared::chaos::BRAWL_BREAKS);
}

#[test]
fn a_thrown_beer_puts_out_the_fire_and_an_unchecked_fire_closes_the_kitchen() {
    let (mut sim, a) = open_room();
    sim.force_chaos(ChaosKind::KitchenFire);
    run(&mut sim, 2);
    assert!(state(&mut sim).kitchen_offline, "no food while it burns");
    let (x, z) = host::chaos::FIRE_AT;
    sim.world_mut().spawn(host::beer::glass(Vec3::new(x + 0.5, 0.2, z), 0.9, false, 0xa, None));
    run(&mut sim, 2);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::KitchenFire, Ending::Countered)]);
    assert!(!state(&mut sim).kitchen_offline);

    sim.force_chaos(ChaosKind::KitchenFire);
    run(&mut sim, 41 * HZ);
    assert_eq!(sim.chaos_log()[1], (ChaosKind::KitchenFire, Ending::Consequence));
    // Into the next shift: the kitchen stays closed.
    while clock(&mut sim).phase != ShiftPhase::Setup {
        sim.tick();
    }
    run(&mut sim, 2);
    assert!(state(&mut sim).kitchen_offline);
    put(&mut sim, a, shared::world::KITCHEN_PASS);
    let pocket = sim.world().get::<Pocket>(a).unwrap().0;
    sim.fixture_request(
        a,
        FixtureRequest { fixture: Fixture::KitchenPass, action: FixtureAction::Buy(shared::buffs::Item::Fries) },
    );
    run(&mut sim, 2);
    assert_eq!(sim.world().get::<Pocket>(a).unwrap().0, pocket, "the kitchen refuses orders");
}

#[test]
fn the_extinguisher_puts_out_the_fire() {
    let (mut sim, a) = open_room();
    buy(&mut sim, UpgradeId::Extinguisher);
    sim.force_chaos(ChaosKind::KitchenFire);
    run(&mut sim, 2);
    put(&mut sim, a, (host::chaos::FIRE_AT.0 + 1.0, host::chaos::FIRE_AT.1));
    sim.set_input(a, PlayerInput { buttons: buttons::USE, ..default() });
    run(&mut sim, 2);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::KitchenFire, Ending::Countered)]);
}

#[test]
fn the_camera_outlines_the_card_counter_and_catching_him_pays() {
    let (mut sim, a) = open_room();
    buy(&mut sim, UpgradeId::SecurityCamera);
    run(&mut sim, 2);
    sim.force_chaos(ChaosKind::CardCounter);
    run(&mut sim, 10 * HZ);
    let counter = npcs(&mut sim, ChaosKind::CardCounter);
    assert_eq!(counter.len(), 1);
    assert!(counter[0].2.outlined);
    let mood = sim.world().get::<Customer>(counter[0].0).unwrap().mood;
    assert_eq!(mood, shared::customers::Mood::Gambling, "he sits at blackjack");
    let before = house(&mut sim);
    haul_out(&mut sim, a, counter[0].0);
    run(&mut sim, 2);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::CardCounter, Ending::Countered)]);
    assert_eq!(house(&mut sim), before + shared::upgrades::CATCH_BONUS);
}

#[test]
fn a_drunk_dealer_gets_the_table_broken_by_the_loan_shark() {
    let (mut sim, a) = open_room();
    put(&mut sim, a, casino::role_spot(TableId::Blackjack).unwrap());
    sim.table_request(a, TableRequest { table: TableId::Blackjack, action: TableAction::TakeRole });
    run(&mut sim, 2);
    sim.world_mut().get_mut::<Drunk>(a).unwrap().level = 60;
    sim.force_chaos(ChaosKind::LoanShark);
    run(&mut sim, 12 * HZ);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::LoanShark, Ending::Consequence)]);
    assert!(state(&mut sim).blackjack_broken);
}

#[test]
fn a_sober_dealer_sees_the_loan_shark_off() {
    let (mut sim, _) = open_room();
    sim.force_chaos(ChaosKind::LoanShark);
    run(&mut sim, 31 * HZ);
    assert_eq!(sim.chaos_log(), vec![(ChaosKind::LoanShark, Ending::Countered)]);
    assert!(!state(&mut sim).blackjack_broken);
}

#[test]
fn a_shift_schedules_its_own_events() {
    let timings = Timings { setup: 1, open: 60, last_call: 1, payment: 1, outcome: 1, wave: 10_000 };
    let mut sim = HostSim::with_config(HostConfig { timings, ..Default::default() });
    sim.add_local_player(0xa, "a", 0);
    let mut seen = false;
    for _ in 0..(64 * HZ) {
        sim.tick();
        seen |= !state(&mut sim).active.is_empty();
    }
    assert!(seen, "week 1 has one event per shift");
    assert_eq!(sim.chaos_log().len(), 1);
}
