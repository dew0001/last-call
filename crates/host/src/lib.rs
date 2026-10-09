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
pub mod physics;
pub mod runner;
pub mod shift;
#[cfg(target_arch = "wasm32")]
mod web;

/// Number of ticks simulated since the room started.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickCount(pub u64);

fn advance_tick(mut tick: ResMut<TickCount>) {
    tick.0 += 1;
}

/// Room settings chosen when the host starts.
#[derive(Clone, Debug, Default)]
pub struct HostConfig {
    /// Shift phase lengths. Tests and `?fast` rooms shorten them.
    pub timings: shared::shift::Timings,
    /// Seed for every RNG stream in the room. The browser host draws it from
    /// `crypto.getRandomValues`; tests fix it so runs replay exactly.
    pub seed: [u8; 32],
}

/// The room's RNG seed.
#[derive(Resource, Clone, Copy, Debug)]
pub struct RoomSeed(pub [u8; 32]);

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
    /// A room with the plan's timings and a zero seed.
    pub fn new() -> Self {
        Self::with_config(HostConfig::default())
    }

    pub fn with_config(config: HostConfig) -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, StatesPlugin));
        // Native debugging: LASTCALL_LOG="lightyear_inputs=debug,info" prints logs.
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(filter) = std::env::var("LASTCALL_LOG") {
            app.add_plugins(bevy::log::LogPlugin { filter, ..default() });
        }
        app.insert_resource(TimeUpdateStrategy::ManualDuration(shared::TICK));
        app.add_plugins(ServerPlugins { tick_duration: shared::TICK });
        app.add_plugins((ProtocolPlugin, PipePlugin));
        app.insert_resource(ReplicationMetadata::new(shared::SNAPSHOT_INTERVAL));
        app.init_resource::<TickCount>();
        app.add_systems(FixedUpdate, advance_tick);
        app.insert_resource(shift::ShiftConfig { timings: config.timings });
        app.insert_resource(RoomSeed(config.seed));
        app.add_plugins((game::GamePlugin, physics::HostPhysicsPlugin, shift::ShiftPlugin));

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

    /// Every player's authoritative position: (id, position).
    pub fn player_positions(&mut self) -> Vec<(u64, Vec3)> {
        let world = self.app.world_mut();
        let mut q = world.query::<(&shared::protocol::Player, &shared::protocol::PlayerPos)>();
        q.iter(world).map(|(p, pos)| (p.id, pos.0)).collect()
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
