//! With nobody in the room, every prop comes to rest and falls asleep within
//! 6 seconds, so resting props cost no CPU and no bandwidth.

use avian3d::prelude::*;
use bevy::prelude::Has;
use host::HostSim;
use shared::protocol::{PropKind, PropPose};

#[test]
fn props_settle_and_sleep() {
    let mut sim = HostSim::new();
    for _ in 0..(64 * 6) {
        sim.tick();
    }
    let world = sim.world_mut();
    let mut q = world.query::<(&PropKind, &PropPose, Has<Sleeping>)>();
    let rows: Vec<_> = q.iter(world).map(|(k, p, s)| (*k, *p, s)).collect();
    assert_eq!(rows.len(), 130);
    for (kind, pose, sleeping) in &rows {
        assert!(*sleeping, "{kind:?} at {:?} is still awake", pose.pos);
        // Nothing fell off the counter: bottles and chips rest on top of it.
        if *kind != PropKind::Stool {
            assert!(pose.pos.y > shared::bar::COUNTER_HEIGHT - 0.05, "{kind:?} fell to {:?}", pose.pos);
        }
    }
}
