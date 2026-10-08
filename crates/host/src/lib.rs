//! Authoritative host simulation for one LAST CALL room.
//!
//! The simulation advances in fixed ticks of 1/64 s. Runners decide when a tick
//! is due (wall clock natively, `setTimeout` in a Web Worker) and call
//! [`HostSim::tick`]. The simulation itself never reads a clock, which keeps it
//! deterministic for replays.

use bevy::app::App;
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::ScheduleLabel;

pub mod runner;
#[cfg(target_arch = "wasm32")]
mod web;

/// The schedule that runs once per simulation tick.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SimTick;

/// Number of ticks simulated since the room started.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickCount(pub u64);

fn advance_tick(mut tick: ResMut<TickCount>) {
    tick.0 += 1;
}

/// A headless host simulation.
pub struct HostSim {
    app: App,
}

impl Default for HostSim {
    fn default() -> Self {
        Self::new()
    }
}

impl HostSim {
    pub fn new() -> Self {
        let mut app = App::empty();
        app.init_resource::<TickCount>();
        app.add_schedule(Schedule::new(SimTick));
        app.add_systems(SimTick, advance_tick);
        Self { app }
    }

    /// Run exactly one simulation tick.
    pub fn tick(&mut self) {
        self.app.world_mut().run_schedule(SimTick);
    }

    /// Ticks simulated so far.
    pub fn tick_count(&self) -> u64 {
        self.app.world().resource::<TickCount>().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_advance_one_at_a_time() {
        let mut sim = HostSim::new();
        for _ in 0..640 {
            sim.tick();
        }
        assert_eq!(sim.tick_count(), 640);
    }
}
