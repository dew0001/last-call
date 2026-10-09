//! A stalled client's inputs reach the host after it has simulated those
//! ticks; the host reuses the last known input meanwhile. The client's tap
//! stamps put the pour right: a stamped release ends it at that tick's fill.

use bevy::prelude::*;
use host::HostSim;
use shared::movement::buttons::INTERACT;
use shared::protocol::{Beer, PlayerInput, PlayerPos, TapEvent};

fn at_tap(sim: &mut HostSim) -> Entity {
    let p = sim.add_local_player(0xbee, "pourer", 0);
    sim.tick();
    let (x, z) = shared::bar::TAP;
    sim.world_mut().get_mut::<PlayerPos>(p).unwrap().0 = Vec3::new(x, 0.0, z + 0.9);
    sim.tick();
    p
}

fn glass(sim: &mut HostSim) -> Option<Beer> {
    let w = sim.world_mut();
    w.query::<&Beer>().iter(w).next().copied()
}

/// Hold E at a good tilt for `ticks` ticks; return the tick of the last step.
fn hold(sim: &mut HostSim, p: Entity, ticks: u32) {
    sim.set_input(p, PlayerInput::new(Vec2::ZERO, 0.0, -0.4, INTERACT));
    for _ in 0..ticks {
        sim.tick();
    }
}

#[test]
fn a_late_release_input_still_pours_to_the_stamped_tick() {
    let mut sim = HostSim::new();
    let p = at_tap(&mut sim);
    // 90% at 0.4 per second: 144 ticks of pouring.
    hold(&mut sim, p, 144);
    let release_tick = sim.net_tick() + 1;
    // The client let go here, but its inputs are 0.4 s late: the host keeps
    // pouring on the last known input (E held), past the green zone.
    hold(&mut sim, p, 26);
    sim.tap(p, TapEvent { tick: release_tick, down: false });
    sim.tick();
    let beer = glass(&mut sim).expect("the stamp ended the pour");
    assert!((89..=91).contains(&beer.fill), "{beer:?}");
    assert!(beer.perfect, "{beer:?}");
}

#[test]
fn a_stamp_from_before_an_overflow_still_saves_the_glass() {
    let mut sim = HostSim::new();
    let p = at_tap(&mut sim);
    hold(&mut sim, p, 150);
    let release_tick = sim.net_tick() + 1;
    // Late enough that the host overflows (105%) before the stamp arrives.
    hold(&mut sim, p, 40);
    sim.tap(p, TapEvent { tick: release_tick, down: false });
    sim.tick();
    let beer = glass(&mut sim).expect("the release came before the overflow");
    assert!(beer.perfect, "{beer:?}");
    let w = sim.world_mut();
    assert_eq!(w.query::<&shared::protocol::Puddle>().iter(w).count(), 0, "no puddle");
}

#[test]
fn without_stamps_inputs_alone_decide() {
    let mut sim = HostSim::new();
    let p = at_tap(&mut sim);
    hold(&mut sim, p, 144);
    sim.set_input(p, PlayerInput::new(Vec2::ZERO, 0.0, -0.4, 0));
    for _ in 0..20 {
        sim.tick();
    }
    let beer = glass(&mut sim).expect("a glass after the settle wait");
    assert!(beer.perfect, "{beer:?}");
    // Held through the overflow grace: a puddle, no glass.
    let mut sim = HostSim::new();
    let p = at_tap(&mut sim);
    hold(&mut sim, p, 300);
    assert!(glass(&mut sim).is_none());
    let w = sim.world_mut();
    assert_eq!(w.query::<&shared::protocol::Puddle>().iter(w).count(), 1);
}

#[test]
fn an_early_stamp_waits_for_its_tick() {
    // The client runs ahead of the host: its release stamp arrives first.
    let mut sim = HostSim::new();
    let p = at_tap(&mut sim);
    hold(&mut sim, p, 100);
    let release_tick = sim.net_tick() + 45;
    sim.tap(p, TapEvent { tick: release_tick, down: false });
    // The inputs say E stays down well past the stamp.
    hold(&mut sim, p, 70);
    let beer = glass(&mut sim).expect("the stamp ended the pour at its tick");
    assert!((89..=91).contains(&beer.fill), "{beer:?}");
    assert!(beer.perfect);
}
