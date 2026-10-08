//! Authoritative host simulation for one LAST CALL room.
//!
//! A headless Bevy app with lightyear's server plugins. Each call to
//! [`HostSim::tick`] advances exactly one 1/64 s tick: virtual time moves by a
//! fixed step, so the simulation never depends on how late a tick runs.
//! Runners decide when a tick is due (wall clock natively, `setTimeout` in a
//! Web Worker).
//!
//! Peers attach through [`HostSim::connect_peer`] with a byte pipe
//! ([`shared::pipe`]). Native bots use pipe pairs; the browser feeds the same
//! pipes from WebRTC data channels.

use core::net::{IpAddr, Ipv4Addr, SocketAddr};

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use shared::pipe::{PipeIo, PipePlugin};
use shared::protocol::ProtocolPlugin;

pub mod game;
pub mod runner;
#[cfg(target_arch = "wasm32")]
mod web;

/// Number of ticks simulated since the room started.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickCount(pub u64);

fn advance_tick(mut tick: ResMut<TickCount>) {
    tick.0 += 1;
}

/// A headless host simulation.
pub struct HostSim {
    app: App,
    server: Entity,
    next_port: u16,
}

impl Default for HostSim {
    fn default() -> Self {
        Self::new()
    }
}

impl HostSim {
    pub fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, StatesPlugin));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(shared::TICK));
        app.add_plugins(ServerPlugins { tick_duration: shared::TICK });
        app.add_plugins((ProtocolPlugin, PipePlugin));
        app.insert_resource(ReplicationMetadata::new(shared::SNAPSHOT_INTERVAL));
        app.init_resource::<TickCount>();
        app.add_systems(FixedUpdate, advance_tick);
        app.add_plugins(game::GamePlugin);

        let server = app.world_mut().spawn((Name::new("Server"), RawServer)).id();
        app.finish();
        app.cleanup();
        app.world_mut().trigger(Start { entity: server });
        // A raw server becomes `Started` once its entity is `Linked`. There is
        // no listening socket here (peers arrive as ready-made links), so link it now.
        app.world_mut().entity_mut(server).insert(Linked);
        app.world_mut().flush();
        // Bevy's first update has a zero time delta and runs no fixed tick.
        // Spend it here so every `tick()` runs exactly one.
        app.update();
        Self { app, server, next_port: 1 }
    }

    /// Attach a peer over a byte channel. Returns the peer's link entity.
    pub fn connect_peer(&mut self, io: PipeIo) -> Entity {
        let port = self.next_port;
        self.next_port = self.next_port.wrapping_add(1).max(1);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        self.app
            .world_mut()
            .spawn((
                LinkOf { server: self.server },
                Link::default(),
                PeerAddr(addr),
                Linked,
                io,
                ReplicationSender,
                ReplicationReceiver,
            ))
            .id()
    }

    /// Drop a peer's link (its tab closed or its data channel failed).
    pub fn disconnect_peer(&mut self, link: Entity) {
        if let Ok(e) = self.app.world_mut().get_entity_mut(link) {
            e.despawn();
        }
    }

    /// Run exactly one simulation tick.
    pub fn tick(&mut self) {
        self.app.update();
    }

    /// Ticks simulated so far.
    pub fn tick_count(&self) -> u64 {
        self.app.world().resource::<TickCount>().0
    }

    pub fn world(&self) -> &World {
        self.app.world()
    }

    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tick_runs_one_fixed_update() {
        let mut sim = HostSim::new();
        for _ in 0..640 {
            sim.tick();
        }
        assert_eq!(sim.tick_count(), 640);
    }
}
