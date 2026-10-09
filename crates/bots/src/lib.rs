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
    /// Walk through `route` (x, z waypoints) from its own predicted position,
    /// then tap E: 4 frames down, 4 up, until the script changes.
    Route { route: &'static [(f32, f32)] },
    /// Walk to the front of the tap, then hold E for `hold` frames with look
    /// pitch `pitch`, then let go. With `drink`, then press R to drink it.
    Pour { hold: u32, pitch: f32, drink: bool },
    /// Wait until another player passes out, walk up to the body from the
    /// room side, grab it with E, then walk backward (+Z) dragging it.
    Drag,
    /// Walk to a table and play it: run it from the dealer or croupier spot
    /// (`role`), or bet at bettor spot `spot` (see [`play_tables`]).
    Casino { table: shared::casino::TableId, role: bool, spot: u8 },
}

/// The way from a spawn point to a table spot, around the tables: across
/// the open floor at z = 3, then down the aisle beside the table.
pub fn casino_route(table: shared::casino::TableId, role: bool, spot: u8) -> Vec<(f32, f32)> {
    use shared::casino::{self, TableId};
    if role {
        let (x, z) = casino::role_spot(table).unwrap_or((0.0, 0.0));
        // The aisle on the room's middle side of the table.
        let aisle = match table {
            TableId::Blackjack => x + 2.5,
            _ => x - 2.5,
        };
        return vec![(aisle, 3.0), (aisle, z - 0.4), (x, z - 0.4)];
    }
    let (x, z) = casino::bettor_spots(table)[usize::from(spot)];
    vec![(x, 3.0), (x, z)]
}

/// The planned route of a [`Script::Casino`] bot.
#[derive(Resource, Default)]
struct CasinoPlan(Vec<(f32, f32)>);

/// A dragger's approach to a body, once one is seen.
#[derive(Resource, Default)]
struct DragPlan(Option<[(f32, f32); 2]>);

/// From the main room to the front of the beer tap, between two stools.
pub const ROUTE_TO_TAP: &[(f32, f32)] = &[(4.0, -1.5), (4.0, -3.0)];

/// From the main room, past the roulette table, through the office door, to the safe.
pub const ROUTE_TO_SAFE: &[(f32, f32)] = &[(8.2, 3.0), (8.2, -1.0), (8.2, -3.0), (9.4, -5.55)];

#[derive(Resource, Default)]
struct BotClock(u64);

/// Next waypoint of a route, and the frame the route ended.
#[derive(Resource, Default)]
struct RouteStep(usize, u64);

fn drive(
    script: Res<Script>,
    session: Res<Session>,
    own: Query<(&Player, &PlayerPos), With<Predicted>>,
    others: Query<(&Player, &PlayerPos, &Drunk), With<Interpolated>>,
    mut clock: ResMut<BotClock>,
    mut step: ResMut<RouteStep>,
    mut plan: ResMut<DragPlan>,
    casino_plan: Res<CasinoPlan>,
    mut input: ResMut<LocalInput>,
) {
    clock.0 += 1;
    input.0 = match *script {
        Script::Idle => PlayerInput::default(),
        Script::GrabAndThrow => grab_and_throw(clock.0),
        Script::GrabAndDrop => grab_and_drop(clock.0),
        Script::Circle { phase } => PlayerInput::new(Vec2::new(0.0, 1.0), phase + clock.0 as f32 * 0.02, 0.0, 0),
        Script::Route { route } => {
            let pos = own.iter().find(|(p, _)| Some(p.id) == session.player_id).map(|(_, pos)| pos.0);
            follow_route(route, pos, &mut step.0, clock.0)
        }
        Script::Pour { hold, pitch, drink } => {
            use shared::movement::buttons::{INTERACT, USE};
            let pos = own.iter().find(|(p, _)| Some(p.id) == session.player_id).map(|(_, pos)| pos.0);
            if step.0 < ROUTE_TO_TAP.len() {
                step.1 = clock.0;
                let i = follow_route(ROUTE_TO_TAP, pos, &mut step.0, clock.0);
                // No E at the end of the route; the pour starts a few frames later.
                PlayerInput::new(i.mv(), i.yaw(), 0.0, 0)
            } else {
                let t = clock.0 - step.1;
                let hold = u64::from(hold);
                let b = if (8..8 + hold).contains(&t) {
                    INTERACT
                } else if drink && (8 + hold + 32..8 + hold + 40).contains(&t) {
                    USE
                } else {
                    0
                };
                PlayerInput::new(Vec2::ZERO, 0.0, pitch, b)
            }
        }
        Script::Casino { table, role, .. } => {
            let pos = own.iter().find(|(p, _)| Some(p.id) == session.player_id).map(|(_, pos)| pos.0);
            if step.0 < casino_plan.0.len() {
                let i = follow_route(&casino_plan.0, pos, &mut step.0, clock.0);
                PlayerInput::new(i.mv(), i.yaw(), 0.0, 0)
            } else {
                // Face the table: south of it face -Z, north of it +Z, slots -X.
                let yaw = match (table, role) {
                    (shared::casino::TableId::Slot(_), _) => std::f32::consts::FRAC_PI_2,
                    (_, true) => std::f32::consts::PI,
                    _ => 0.0,
                };
                PlayerInput::new(Vec2::ZERO, yaw, 0.0, 0)
            }
        }
        Script::Drag => {
            use shared::movement::buttons::INTERACT;
            let pos = own.iter().find(|(p, _)| Some(p.id) == session.player_id).map(|(_, pos)| pos.0);
            // Wait until someone else is on the floor, then plan the approach
            // from the room side of the body.
            if plan.0.is_none()
                && let Some((_, body, _)) =
                    others.iter().find(|(p, _, d)| Some(p.id) != session.player_id && d.passed_out)
            {
                plan.0 = Some([(body.0.x, body.0.z + 2.0), (body.0.x, body.0.z + 1.4)]);
                step.0 = 0;
            }
            match plan.0 {
                None => PlayerInput::default(),
                Some(route) if step.0 < route.len() => {
                    step.1 = clock.0;
                    let i = follow_route(&route, pos, &mut step.0, clock.0);
                    PlayerInput::new(i.mv(), i.yaw(), 0.0, 0)
                }
                Some(_) => {
                    let t = clock.0 - step.1;
                    if t < 16 {
                        // Face the body (toward -Z) and grab.
                        PlayerInput::new(Vec2::ZERO, 0.0, 0.0, if t >= 8 { INTERACT } else { 0 })
                    } else if t < 16 + 96 {
                        // Back away for 1.5 s, still facing it.
                        PlayerInput::new(Vec2::new(0.0, -1.0), 0.0, 0.0, 0)
                    } else {
                        PlayerInput::default()
                    }
                }
            }
        }
    };
}

/// A [`Script::Casino`] bot's table requests, four times a second once it
/// stands at its spot: run the table by the rules, or bet and play simply
/// (hit below 17, never insure).
#[allow(clippy::too_many_arguments)]
fn play_tables(
    script: Res<Script>,
    session: Res<Session>,
    clock: Res<BotClock>,
    step: Res<RouteStep>,
    plan: Res<CasinoPlan>,
    bj: Query<&BlackjackView>,
    wheel: Query<&RouletteView>,
    mut out: ResMut<shared::client::OutgoingTable>,
) {
    use shared::casino::{TableAction, TableId};
    use shared::minigame::Who;
    let Script::Casino { table, role, .. } = *script else { return };
    let Some(id) = session.player_id else { return };
    if step.0 < plan.0.len() || !clock.0.is_multiple_of(16) {
        return;
    }
    let me = Who::Player(id);
    let mut send = |action| out.0.push(TableRequest { table, action });
    match (table, role) {
        (TableId::Blackjack, true) => {
            send(TableAction::TakeRole);
            let Some(v) = bj.iter().next() else { return };
            if v.phase == BjPhase::Betting && v.seats.iter().any(|s| s.bet > 0) {
                send(TableAction::Deal);
            }
            if let Some(a) = v.dealer_should {
                send(TableAction::Dealer(a));
            }
        }
        (TableId::Blackjack, false) => {
            let Some(v) = bj.iter().next() else { return };
            let mine = v.seats.iter().position(|s| s.who == Some(me));
            match (v.phase, mine) {
                (_, None) => send(TableAction::Bet(10)),
                (BjPhase::Insurance, Some(s)) if v.seats[s].insurance.is_none() => send(TableAction::Insure(false)),
                (BjPhase::Players { seat, hand }, Some(s)) if usize::from(seat) == s => {
                    let cards = v.seats[s].hands.get(usize::from(hand)).map_or(&[][..], |h| &h.cards[..]);
                    let hit = shared::blackjack::hand_value(cards).0 < 17;
                    send(TableAction::Play(if hit {
                        shared::blackjack::Action::Hit
                    } else {
                        shared::blackjack::Action::Stand
                    }));
                }
                _ => {}
            }
        }
        (TableId::Roulette, true) => {
            send(TableAction::TakeRole);
            let Some(v) = wheel.iter().next() else { return };
            if v.to_rake > 0 {
                send(TableAction::Rake);
            } else if !v.spinning && !v.bets.is_empty() {
                send(TableAction::Spin);
            }
        }
        (TableId::Roulette, false) => {
            let Some(v) = wheel.iter().next() else { return };
            if !v.spinning && !v.bets.iter().any(|(w, ..)| *w == me) {
                send(TableAction::RouletteBet(shared::roulette::Bet::Red, 5));
            }
        }
        (TableId::Slot(_), _) => send(TableAction::Pull(1)),
    }
}

/// Steer toward the current waypoint; at the end, tap E.
fn follow_route(route: &[(f32, f32)], pos: Option<Vec3>, step: &mut usize, frame: u64) -> PlayerInput {
    use shared::movement::buttons::INTERACT;
    let Some(pos) = pos else { return PlayerInput::default() };
    while let Some(&(x, z)) = route.get(*step) {
        let (dx, dz) = (x - pos.x, z - pos.z);
        if dx.hypot(dz) > 0.15 {
            // Forward is (-sin yaw, -cos yaw).
            return PlayerInput::new(Vec2::Y, (-dx).atan2(-dz), 0.0, 0);
        }
        *step += 1;
    }
    let tap = if (frame / 4).is_multiple_of(2) { INTERACT } else { 0 };
    PlayerInput::new(Vec2::ZERO, 0.0, 0.0, tap)
}

/// The [`Script::GrabAndThrow`] timeline, by frame (one frame per tick).
///
/// Pick-up happens on the press edge of E, and only with a prop in reach.
/// The bot keeps walking into the counter while it taps E several times, so
/// a slow machine that delays some walk inputs still gets a press in reach.
/// A tap while already holding does nothing.
fn grab_and_throw(frame: u64) -> PlayerInput {
    use shared::movement::buttons::{INTERACT, THROW};
    let half_turn = std::f32::consts::PI;
    match frame {
        // Wait to join and sync.
        0..200 => PlayerInput::default(),
        // Walk forward (-Z) into the counter.
        200..400 => PlayerInput::new(Vec2::Y, 0.0, 0.0, 0),
        // Keep walking and tap E: 4 frames down, 4 up.
        400..464 => PlayerInput::new(Vec2::Y, 0.0, 0.0, if (frame / 4).is_multiple_of(2) { INTERACT } else { 0 }),
        // Turn around.
        464..524 => PlayerInput::new(Vec2::ZERO, half_turn * (frame - 464) as f32 / 60.0, 0.0, 0),
        // Charge the throw for half a second, then release.
        524..556 => PlayerInput::new(Vec2::ZERO, half_turn, 0.0, THROW),
        _ => PlayerInput::new(Vec2::ZERO, half_turn, 0.0, 0),
    }
}

/// The [`Script::GrabAndDrop`] timeline: like [`grab_and_throw`], then Q.
fn grab_and_drop(frame: u64) -> PlayerInput {
    use shared::movement::buttons::DROP;
    let half_turn = std::f32::consts::PI;
    match frame {
        0..524 => grab_and_throw(frame),
        524..534 => PlayerInput::new(Vec2::ZERO, half_turn, 0.0, DROP),
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
    app.init_resource::<BotClock>().init_resource::<RouteStep>().init_resource::<DragPlan>();
    app.add_systems(FixedPreUpdate, drive.before(lightyear::prelude::client::input::InputSystems::WriteClientInputs));
    app.insert_resource(CasinoPlan(match script {
        Script::Casino { table, role, spot } => casino_route(table, role, spot),
        _ => Vec::new(),
    }));
    app.add_systems(Update, play_tables);
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
        Self::with_config(bots, host::HostConfig::default(), script)
    }

    /// Like [`LocalRoom::new`], with room settings (shorter shifts, a seed).
    pub fn with_config(bots: u8, config: host::HostConfig, script: impl Fn(u8) -> Script) -> Self {
        let host = HostSim::with_config(config);
        let bot_stats = Vec::new();
        let links = Vec::new();
        let scripts: Vec<Script> = (0..bots).map(&script).collect();
        let mut room = Self { host, bots: Vec::new(), bot_stats, links, scripts };
        // Connect bots one at a time and wait for each Welcome. The host gives
        // the lowest free slot, so bot `i` then always gets slot `i` and its
        // spawn point, however slow the machine is.
        for i in 0..bots {
            let (client_io, server_io) = PipeIo::pair();
            room.bot_stats.push(client_io.stats.clone());
            room.links.push(room.host.connect_peer(server_io));
            room.bots.push(bot_app(client_io, i, room.scripts[usize::from(i)]));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while room.session(usize::from(i)).player_id.is_none() {
                assert!(std::time::Instant::now() < deadline, "bot {i} did not join within 30 s");
                room.step();
                std::thread::sleep(shared::TICK);
            }
        }
        room
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
