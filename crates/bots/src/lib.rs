//! Native bot clients. A bot is a full lightyear client app with no renderer:
//! it joins through the same protocol as a browser, predicts its own movement,
//! and follows a scripted input pattern.

use core::net::{IpAddr, Ipv4Addr, SocketAddr};

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use host::HostSim;
use lightyear::prelude::client::*;
use lightyear::prelude::*;
use shared::client::{ClientNetPlugin, Identity, LocalInput, Session};
use shared::pipe::{PipeIo, PipeStats};
use shared::protocol::*;
use std::sync::Arc;

/// How a bot moves.
#[derive(Resource, Clone, Copy, Debug)]
pub enum Script {
    /// Stand still.
    Idle,
    /// Walk forward while turning, so the bot circles. `phase` offsets bots.
    Circle { phase: f32 },
    /// Walk to the counter, pick up a prop, turn around, and throw it.
    GrabAndThrow,
    /// Walk to the counter, pick up a prop, turn around, and drop it.
    GrabAndDrop,
}

#[derive(Resource, Default)]
struct BotClock(u64);

fn drive(script: Res<Script>, mut clock: ResMut<BotClock>, mut input: ResMut<LocalInput>) {
    clock.0 += 1;
    input.0 = match *script {
        Script::Idle => PlayerInput::default(),
        Script::GrabAndThrow => grab_and_throw(clock.0),
        Script::GrabAndDrop => grab_and_drop(clock.0),
        Script::Circle { phase } => PlayerInput::new(Vec2::new(0.0, 1.0), phase + clock.0 as f32 * 0.02, 0.0, 0),
    };
}

/// The [`Script::GrabAndThrow`] timeline, by frame (one frame per tick).
fn grab_and_throw(frame: u64) -> PlayerInput {
    use shared::movement::buttons::{INTERACT, THROW};
    let half_turn = std::f32::consts::PI;
    match frame {
        // Wait to join and sync.
        0..200 => PlayerInput::default(),
        // Walk forward (-Z) into the counter.
        200..400 => PlayerInput::new(Vec2::Y, 0.0, 0.0, 0),
        // Press E.
        400..410 => PlayerInput::new(Vec2::ZERO, 0.0, 0.0, INTERACT),
        410..420 => PlayerInput::new(Vec2::ZERO, 0.0, 0.0, 0),
        // Turn around.
        420..480 => PlayerInput::new(Vec2::ZERO, half_turn * (frame - 420) as f32 / 60.0, 0.0, 0),
        // Charge the throw for half a second, then release.
        480..512 => PlayerInput::new(Vec2::ZERO, half_turn, 0.0, THROW),
        _ => PlayerInput::new(Vec2::ZERO, half_turn, 0.0, 0),
    }
}

/// The [`Script::GrabAndDrop`] timeline: like [`grab_and_throw`], then Q.
fn grab_and_drop(frame: u64) -> PlayerInput {
    use shared::movement::buttons::DROP;
    let half_turn = std::f32::consts::PI;
    match frame {
        0..480 => grab_and_throw(frame),
        480..490 => PlayerInput::new(Vec2::ZERO, half_turn, 0.0, DROP),
        _ => PlayerInput::new(Vec2::ZERO, half_turn, 0.0, 0),
    }
}

/// Build a bot client app connected through `io`. Call [`App::update`] once per frame.
pub fn bot_app(io: PipeIo, index: u8, script: Script) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, StatesPlugin));
    app.add_plugins(ClientPlugins { tick_duration: shared::TICK });
    app.add_plugins(ProtocolPlugin);
    app.add_plugins(ClientNetPlugin);
    app.insert_resource(PredictionManager::default());
    app.insert_resource(script);
    app.init_resource::<BotClock>();
    app.add_systems(FixedPreUpdate, drive.before(lightyear::prelude::client::input::InputSystems::WriteClientInputs));
    let mut uuid = [0u8; 16];
    uuid[0] = 0xb0;
    uuid[1] = index;
    app.insert_resource(Identity {
        code: "BOTSS".into(),
        player_uuid: uuid,
        display_name: format!("bot-{index}"),
        cosmetic_id: 0,
    });
    let local = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40_000 + u16::from(index));
    let server = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5000);
    let client = app
        .world_mut()
        .spawn((
            Name::new("Client"),
            Client,
            RawClient,
            io,
            LocalAddr(local),
            PeerAddr(server),
            ReplicationReceiver,
            ReplicationSender,
        ))
        .id();
    // Native bots have no shader warm-up; connect on the first frame.
    app.insert_resource(shared::client::ConnectAfterFrames(0));
    let _ = client;
    app.finish();
    app.cleanup();
    app
}

/// A host plus bots wired by crossbeam channels, all in one thread.
pub struct LocalRoom {
    pub host: HostSim,
    pub bots: Vec<App>,
    /// Traffic counters of each bot's end of its pipe.
    pub bot_stats: Vec<Arc<PipeStats>>,
    /// Each bot's link entity on the host.
    pub links: Vec<bevy::ecs::entity::Entity>,
    scripts: Vec<Script>,
}

impl LocalRoom {
    pub fn new(bots: u8, script: impl Fn(u8) -> Script) -> Self {
        let mut host = HostSim::new();
        let mut bot_stats = Vec::new();
        let mut links = Vec::new();
        let scripts: Vec<Script> = (0..bots).map(&script).collect();
        let bots = (0..bots)
            .map(|i| {
                let (client_io, server_io) = PipeIo::pair();
                bot_stats.push(client_io.stats.clone());
                links.push(host.connect_peer(server_io));
                bot_app(client_io, i, scripts[usize::from(i)])
            })
            .collect();
        Self { host, bots, bot_stats, links, scripts }
    }

    /// Drop a bot's connection, as when its tab closes or refreshes.
    pub fn disconnect(&mut self, bot: usize) {
        self.host.disconnect_peer(self.links[bot]);
    }

    /// Start a fresh client for a bot with the same identity (a refreshed tab).
    pub fn reconnect(&mut self, bot: usize) {
        let (client_io, server_io) = PipeIo::pair();
        self.bot_stats[bot] = client_io.stats.clone();
        self.links[bot] = self.host.connect_peer(server_io);
        self.bots[bot] = bot_app(client_io, bot as u8, self.scripts[bot]);
    }

    /// Run one host tick and one frame on every bot.
    pub fn step(&mut self) {
        self.host.tick();
        for bot in &mut self.bots {
            bot.update();
        }
    }

    /// Players the host knows about: (id, position).
    pub fn host_players(&mut self) -> Vec<(u64, Vec3)> {
        let world = self.host.world_mut();
        let mut q = world.query::<(&Player, &PlayerPos)>();
        q.iter(world).map(|(p, pos)| (p.id, pos.0)).collect()
    }

    pub fn session(&self, bot: usize) -> Session {
        self.bots[bot].world().resource::<Session>().clone()
    }

    /// Distinct player ids a bot can see.
    pub fn players_seen_by(&mut self, bot: usize) -> Vec<u64> {
        let world = self.bots[bot].world_mut();
        let mut q = world.query::<&Player>();
        let mut ids: Vec<u64> = q.iter(world).map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Run `seconds` of real time at the tick rate.
    pub fn run_realtime(&mut self, seconds: f32) {
        let start = std::time::Instant::now();
        let ticks = (seconds * shared::TICK_HZ as f32) as u32;
        for t in 1..=ticks {
            self.step();
            if let Some(wait) = (shared::TICK * t).checked_sub(start.elapsed()) {
                std::thread::sleep(wait);
            }
        }
    }

    /// Host-side player position by id.
    pub fn host_pos(&mut self, id: u64) -> Option<Vec3> {
        self.host_players().into_iter().find(|(pid, _)| *pid == id).map(|(_, p)| p)
    }

    /// Every prop on the host: (entity, kind, pose, held by). Track a prop by
    /// its entity: query order changes when props fall asleep or wake.
    pub fn host_props(&mut self) -> Vec<(bevy::ecs::entity::Entity, PropKind, PropPose, HeldBy)> {
        let world = self.host.world_mut();
        let mut q = world.query::<(bevy::ecs::entity::Entity, &PropKind, &PropPose, &HeldBy)>();
        q.iter(world).map(|(e, k, p, h)| (e, *k, *p, *h)).collect()
    }

    /// One prop by entity.
    pub fn host_prop(&mut self, e: bevy::ecs::entity::Entity) -> Option<(PropKind, PropPose, HeldBy)> {
        self.host_props().into_iter().find(|p| p.0 == e).map(|(_, k, p, h)| (k, p, h))
    }

    /// The bot's own predicted position, if it has one yet.
    pub fn predicted_pos(&mut self, bot: usize) -> Option<Vec3> {
        let world = self.bots[bot].world_mut();
        let mut q = world.query_filtered::<&PlayerPos, (With<Predicted>, With<Player>)>();
        q.iter(world).next().map(|p| p.0)
    }
}
