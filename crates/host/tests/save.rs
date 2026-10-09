//! Saving and resuming a run: the host saves at each Setup, and a room
//! resumed from that save starts at the same shift with the same money.

use host::economy::Preset;
use host::{HostConfig, HostSim};
use shared::protocol::{Pocket, RoomState, RunLedger, ShiftClock};
use shared::shift::{Calendar, ShiftPhase, Timings};

/// One-second phases, so a shift passes in a few seconds of ticks.
fn fast() -> Timings {
    Timings { setup: 1, open: 1, last_call: 1, payment: 1, outcome: 1, wave: 10_000 }
}

fn ledger(sim: &mut HostSim) -> RunLedger {
    let w = sim.world_mut();
    *w.query_filtered::<&RunLedger, bevy::prelude::With<RoomState>>().single(w).unwrap()
}

fn clock(sim: &mut HostSim) -> ShiftClock {
    let w = sim.world_mut();
    *w.query::<&ShiftClock>().single(w).unwrap()
}

#[test]
fn a_saved_run_resumes_with_the_same_money_and_shift() {
    let config = HostConfig { timings: fast(), preset: Preset::Casino, ..Default::default() };
    let mut sim = HostSim::with_config(config.clone());
    let a = sim.add_local_player(0xa, "a", 0);
    for _ in 0..4 {
        sim.tick();
    }
    assert!(sim.take_save().is_none(), "opening a room does not replace the saved run");

    // Money moves during the shift.
    sim.world_mut().get_mut::<Pocket>(a).unwrap().0 = 777;
    let w = sim.world_mut();
    w.query_filtered::<&mut RunLedger, bevy::prelude::With<RoomState>>().single_mut(w).unwrap().ledger.house = 1_234;

    // Run into the next shift's Setup.
    let mut save = None;
    for _ in 0..(10 * 64) {
        sim.tick();
        if let Some(s) = sim.take_save() {
            save = Some(s);
            break;
        }
    }
    let save = save.expect("a save at the next Setup");
    assert_eq!(save.calendar, Calendar { week: 1, shift: 1 });
    assert_eq!(save.ledger.house, 1_234);
    assert_eq!(save.pockets_by_id().get(&0xa), Some(&777));

    // A new room from the save (through JSON, as the browser stores it).
    let save = shared::save::RunSave::from_json(&save.to_json()).unwrap();
    let mut resumed = HostSim::with_config(HostConfig { resume: Some(save), ..config });
    let a2 = resumed.add_local_player(0xa, "a", 0);
    let b = resumed.add_local_player(0xb, "b", 1);
    for _ in 0..4 {
        resumed.tick();
    }
    assert_eq!(resumed.world().get::<Pocket>(a2).unwrap().0, 777, "the same player gets their pocket back");
    assert_eq!(resumed.world().get::<Pocket>(b).unwrap().0, 1_000, "a new player gets the starting pocket");
    assert_eq!(ledger(&mut resumed).ledger.house, 1_234);
    let c = clock(&mut resumed);
    assert_eq!(c.calendar, Calendar { week: 1, shift: 1 });
    assert_eq!(c.phase, ShiftPhase::Setup);
}

#[test]
fn a_player_who_has_not_rejoined_keeps_their_pocket_in_later_saves() {
    let ledger = shared::economy::Ledger { house: 50, ..Default::default() };
    let save = shared::save::RunSave::new(ledger, Calendar { week: 2, shift: 0 }, [(0xa, 10), (0xc, 99)]);
    let config = HostConfig { timings: fast(), resume: Some(save), ..Default::default() };
    let mut sim = HostSim::with_config(config);
    sim.add_local_player(0xa, "a", 0);
    let mut later = None;
    for _ in 0..(10 * 64) {
        sim.tick();
        if let Some(s) = sim.take_save()
            && s.calendar.shift == 1
        {
            later = Some(s);
            break;
        }
    }
    let pockets = later.expect("a later save").pockets_by_id();
    assert_eq!(pockets.get(&0xc), Some(&99), "absent player's money is kept");
    assert_eq!(pockets.get(&0xa), Some(&10));
}
