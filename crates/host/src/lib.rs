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

use bevy::app::TaskPoolOptions;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use shared::pipe::{PipeIo, PipePlugin};
use shared::protocol::ProtocolPlugin;

pub mod beer;
pub mod casino;
pub mod customers;
pub mod drunk;
pub mod economy;
pub mod game;
pub mod physics;
pub mod replay;
pub mod runner;
pub mod save;
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
    /// A test or demo start (`?preset=`).
    pub preset: economy::Preset,
    /// How customers pick what to do (bar, blackjack, roulette, slots).
    /// `None` uses [`customers::ACTIVITY_WEIGHTS`].
    pub tastes: Option<[u32; 4]>,
    /// Resume a saved run: its ledger, calendar and pockets replace the
    /// preset's.
    pub resume: Option<shared::save::RunSave>,
}

/// Run every schedule on one thread, in its fixed topological order. The
/// browser host is single-threaded anyway; a native build with Bevy's
/// `multi_threaded` feature would otherwise run unordered systems in an order
/// that varies with thread timing, and the same inputs could give a different
/// result (the replay test caught it).
fn deterministic_schedules(app: &mut App) {
    let mut schedules = app.world_mut().resource_mut::<Schedules>();
    for (_, schedule) in schedules.iter_mut() {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    }
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
        // One thread: a parallel task pool can visit items in a different
        // order from run to run (see `deterministic_schedules`).
        let one_thread = bevy::app::TaskPoolPlugin { task_pool_options: TaskPoolOptions::with_num_threads(1) };
        app.add_plugins((MinimalPlugins.set(one_thread), TransformPlugin, StatesPlugin));
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
        app.insert_resource(customers::Tastes(config.tastes.unwrap_or(customers::ACTIVITY_WEIGHTS)));
        app.insert_resource(casino::Audit(shared::audit::AuditLog::new(&config.seed)));
        let mut start = config.preset.start();
        if let Some(save) = &config.resume {
            start.ledger = save.ledger;
            start.calendar = save.calendar;
            app.insert_resource(save::SavedPockets(save.pockets_by_id()));
        }
        app.insert_resource(start);
        app.add_plugins((
            game::GamePlugin,
            physics::HostPhysicsPlugin,
            shift::ShiftPlugin,
            economy::EconomyPlugin,
            customers::CustomersPlugin,
            beer::BeerPlugin,
            drunk::DrunkPlugin,
            casino::CasinoPlugin,
            save::SavePlugin,
        ));

        deterministic_schedules(&mut app);
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

    /// Add a player driven by host code instead of a client (replays and
    /// tests). It joins like a networked player at `slot`'s spawn point.
    pub fn add_local_player(&mut self, id: u64, name: &str, slot: u8) -> Entity {
        let spawn = shared::bar::spawn_point(usize::from(slot));
        self.app
            .world_mut()
            .spawn((
                shared::protocol::Player { id, name: name.into(), slot },
                shared::protocol::PlayerPos(Vec3::from_array(spawn)),
                shared::protocol::PlayerYaw(0.0),
                lightyear::prelude::input::native::ActionState::<shared::protocol::PlayerInput>::default(),
                game::LocalPlayer,
            ))
            .id()
    }

    /// Set a local player's input for the coming ticks.
    pub fn set_input(&mut self, player: Entity, input: shared::protocol::PlayerInput) {
        if let Some(mut action) = self
            .app
            .world_mut()
            .get_mut::<lightyear::prelude::input::native::ActionState<shared::protocol::PlayerInput>>(player)
        {
            action.0 = input;
        }
    }

    /// Queue a table request from a local player (tests and replays), as if
    /// it had come over the network.
    pub fn table_request(&mut self, player: Entity, request: shared::protocol::TableRequest) {
        self.app.world_mut().resource_mut::<casino::TableQueue>().0.push((player, request));
    }

    /// A tap stamp from a local player (tests), as if it had come over the network.
    pub fn tap(&mut self, player: Entity, tap: shared::protocol::TapEvent) {
        self.app.world_mut().resource_mut::<beer::TapQueue>().0.push((player, tap));
    }

    /// The host's lightyear tick (the tick inputs and tap stamps are numbered by).
    pub fn net_tick(&self) -> u32 {
        self.app.world().resource::<lightyear::prelude::LocalTimeline>().tick().0
    }

    /// Take the newest run save, if one was written since the last call
    /// (one is written at the start of every Setup).
    pub fn take_save(&mut self) -> Option<shared::save::RunSave> {
        self.app.world_mut().resource_mut::<save::PendingSave>().0.take()
    }

    /// Take the audit log lines written since the last call.
    pub fn drain_audit(&mut self) -> Vec<shared::audit::Entry> {
        self.app.world_mut().resource_mut::<casino::Audit>().0.drain()
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
