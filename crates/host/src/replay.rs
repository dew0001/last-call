//! Determinism replay (plan section 11): a scripted 14-minute shift with four
//! local players and the room seed fixed, then a hash of the whole host state.
//! The same build must give the same hash every run, natively and as wasm.
//!
//! The script is a pure function of the tick and the host's own state, so
//! there is no network timing in it. The players work the bar: one pours and
//! serves, one walks circles, one throws bottles, one pours and spills.

use avian3d::prelude::{Position, Rotation};
use bevy::prelude::*;
use shared::casino::TableAction;
use shared::movement::buttons;
use shared::protocol::*;

use crate::{HostConfig, HostSim};

/// Ticks in one plan-length shift (14 minutes).
pub const SHIFT_TICKS: u64 = 14 * 60 * shared::TICK_HZ as u64;

/// The seed every replay uses.
pub const SEED: [u8; 32] = *b"last call replay seed, v1 ......";

/// One scripted player: a list of steps, run in a loop.
struct Bot {
    steps: &'static [Step],
    at: usize,
    since: u64,
}

#[derive(Clone, Copy)]
enum Step {
    /// Walk to (x, z) at walking pace.
    Walk(f32, f32),
    /// Hold buttons with a yaw and pitch for some ticks.
    Hold { buttons: u16, yaw: f32, pitch: f32, ticks: u64 },
    /// Walk forward while turning, for some ticks.
    Circle(u64),
    /// Run a table for the rest of the shift (see [`run_table`]).
    Run(shared::casino::TableId),
}

const BARTENDER: &[Step] = &[
    Step::Walk(4.0, -1.5),
    Step::Walk(4.0, -3.0),
    Step::Hold { buttons: 0, yaw: 0.0, pitch: -0.4, ticks: 8 },
    Step::Hold { buttons: buttons::INTERACT, yaw: 0.0, pitch: -0.4, ticks: 148 },
    Step::Hold { buttons: 0, yaw: 0.0, pitch: 0.0, ticks: 8 },
    Step::Walk(4.0, -1.5),
    Step::Walk(1.0, -1.5),
    Step::Walk(1.0, -3.05),
    Step::Hold { buttons: 0, yaw: 0.0, pitch: 0.0, ticks: 16 },
    Step::Hold { buttons: buttons::DROP, yaw: 0.0, pitch: 0.0, ticks: 8 },
    Step::Hold { buttons: 0, yaw: 0.0, pitch: 0.0, ticks: 320 },
];

const WALKER: &[Step] = &[Step::Circle(2000), Step::Walk(-6.0, 3.0)];

const THROWER: &[Step] = &[
    Step::Walk(-3.0, -1.5),
    Step::Walk(-3.0, -3.0),
    Step::Hold { buttons: 0, yaw: 0.0, pitch: 0.0, ticks: 8 },
    Step::Hold { buttons: buttons::INTERACT, yaw: 0.0, pitch: 0.0, ticks: 8 },
    Step::Hold { buttons: 0, yaw: std::f32::consts::PI, pitch: 0.0, ticks: 60 },
    Step::Hold { buttons: buttons::THROW, yaw: std::f32::consts::PI, pitch: 0.0, ticks: 32 },
    Step::Hold { buttons: 0, yaw: std::f32::consts::PI, pitch: 0.0, ticks: 600 },
];

const DEALER: &[Step] = &[
    Step::Walk(-3.0, -0.6),
    Step::Walk(-5.0, -0.6),
    Step::Hold { buttons: 0, yaw: std::f32::consts::PI, pitch: 0.0, ticks: 8 },
    Step::Run(shared::casino::TableId::Blackjack),
];

const CROUPIER: &[Step] = &[
    Step::Walk(2.5, -0.2),
    Step::Walk(4.5, -0.2),
    Step::Hold { buttons: 0, yaw: std::f32::consts::PI, pitch: 0.0, ticks: 8 },
    Step::Run(shared::casino::TableId::Roulette),
];

const SPILLER: &[Step] = &[
    Step::Walk(2.0, -1.5),
    Step::Walk(4.6, -1.5),
    Step::Walk(4.6, -3.0),
    Step::Hold { buttons: 0, yaw: 0.0, pitch: 0.3, ticks: 8 },
    Step::Hold { buttons: buttons::INTERACT, yaw: 0.0, pitch: 0.3, ticks: 120 },
    Step::Hold { buttons: 0, yaw: 0.0, pitch: 0.0, ticks: 8 },
    Step::Walk(7.5, -1.0),
    Step::Walk(7.5, 2.0),
    Step::Hold { buttons: buttons::THROW, yaw: 1.0, pitch: 0.0, ticks: 20 },
    Step::Hold { buttons: 0, yaw: 1.0, pitch: 0.0, ticks: 900 },
];

impl Bot {
    fn new(steps: &'static [Step]) -> Self {
        Self { steps, at: 0, since: 0 }
    }

    /// The table this bot runs, once it got there.
    fn running(&self) -> Option<shared::casino::TableId> {
        match self.steps[self.at] {
            Step::Run(t) => Some(t),
            _ => None,
        }
    }

    fn input(&mut self, pos: Vec3) -> PlayerInput {
        loop {
            let step = self.steps[self.at];
            let done = match step {
                Step::Walk(x, z) => Vec2::new(x - pos.x, z - pos.z).length() < 0.15 || self.since > 64 * 20,
                Step::Hold { ticks, .. } | Step::Circle(ticks) => self.since >= ticks,
                Step::Run(_) => false,
            };
            if !done {
                self.since += 1;
                return match step {
                    Step::Walk(x, z) => {
                        let (dx, dz) = (x - pos.x, z - pos.z);
                        PlayerInput::new(Vec2::Y, shared::math::atan2(-dx, -dz), 0.0, 0)
                    }
                    Step::Hold { buttons, yaw, pitch, .. } => PlayerInput::new(Vec2::ZERO, yaw, pitch, buttons),
                    Step::Circle(_) => PlayerInput::new(Vec2::Y, self.since as f32 * 0.02, 0.0, 0),
                    Step::Run(_) => PlayerInput::new(Vec2::ZERO, std::f32::consts::PI, 0.0, 0),
                };
            }
            self.at = (self.at + 1) % self.steps.len();
            self.since = 0;
        }
    }
}

/// FNV-1a over bytes.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, b: &[u8]) {
        for x in b {
            self.0 = (self.0 ^ u64::from(*x)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.bytes(&v.to_bits().to_le_bytes());
    }
    fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
}

/// Hash every piece of game state on the host, exactly (floats by their bits).
pub fn state_hash(world: &mut World) -> u64 {
    let mut h = Fnv::new();
    for (_, part) in state_parts(world) {
        h.u64(part);
    }
    h.0
}

/// The state hash split by part (players, room, customers, props, puddles),
/// to find which part two builds disagree on.
pub fn state_parts(world: &mut World) -> Vec<(&'static str, u64)> {
    let mut parts = Vec::new();
    let mut h = Fnv::new();
    let mut players: Vec<_> = world
        .query::<(&Player, &PlayerPos, &PlayerYaw, Option<&Pocket>, Option<&Drunk>)>()
        .iter(world)
        .map(|(p, pos, yaw, pocket, drunk)| {
            (p.id, pos.0, yaw.0, pocket.map_or(0, |m| m.0), drunk.copied().unwrap_or_default())
        })
        .collect();
    players.sort_by_key(|p| p.0);
    for (id, pos, yaw, pocket, drunk) in players {
        h.u64(id);
        h.vec3(pos);
        h.f32(yaw);
        h.u64(pocket as u64);
        h.bytes(&[drunk.level, u8::from(drunk.passed_out)]);
    }
    parts.push(("players", h.0));
    let mut h = Fnv::new();
    for (clock, run) in world.query::<(&ShiftClock, &RunLedger)>().iter(world) {
        h.bytes(&[clock.calendar.week, clock.calendar.shift, clock.phase as u8]);
        h.u64(u64::from(clock.seconds_left));
        let l = run.ledger;
        for v in [l.house, l.paid, l.carried, run.due] {
            h.u64(v as u64);
        }
        h.bytes(&[l.missed_in_a_row, l.ng, run.outcome as u8]);
    }
    parts.push(("room", h.0));
    let mut h = Fnv::new();
    let mut customers: Vec<_> = world
        .query::<(&Customer, &NpcPose, &crate::customers::Npc)>()
        .iter(world)
        .map(|(c, p, n)| (c.id, c.mood as u8, c.patience, p.pos, p.yaw, n.cash))
        .collect();
    customers.sort_by_key(|c| c.0);
    for (id, mood, patience, pos, yaw, cash) in customers {
        h.u64(u64::from(id));
        h.bytes(&[mood, patience]);
        h.vec3(pos);
        h.f32(yaw);
        h.u64(cash as u64);
    }
    parts.push(("customers", h.0));
    let mut h = Fnv::new();
    let mut props: Vec<_> = world
        .query::<(Entity, &PropKind, &Position, &Rotation, &HeldBy, Option<&Beer>)>()
        .iter(world)
        .map(|(e, k, p, r, held, beer)| (e.index_u32(), *k as u8, p.0, r.0, held.0, beer.copied()))
        .collect();
    props.sort_by_key(|p| p.0);
    for (index, kind, pos, rot, held, beer) in props {
        h.u64(u64::from(index));
        h.bytes(&[kind]);
        h.vec3(pos);
        h.vec3(rot.xyz());
        h.f32(rot.w);
        h.u64(held.unwrap_or(0));
        if let Some(b) = beer {
            h.bytes(&[b.fill, u8::from(b.perfect)]);
        }
    }
    parts.push(("props", h.0));
    let mut h = Fnv::new();
    let mut puddles: Vec<_> = world.query::<&Puddle>().iter(world).map(|p| p.pos).collect();
    puddles.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z)));
    for p in puddles {
        h.vec3(p);
    }
    parts.push(("puddles", h.0));
    let mut h = Fnv::new();
    let mut chips: Vec<i64> = world.query::<&ChipValue>().iter(world).map(|c| c.0).collect();
    chips.sort_unstable();
    for c in chips {
        h.u64(c as u64);
    }
    for v in world.query::<&BlackjackView>().iter(world) {
        h.bytes(&serde_json::to_vec(v).unwrap_or_default());
    }
    for v in world.query::<&RouletteView>().iter(world) {
        h.bytes(&serde_json::to_vec(v).unwrap_or_default());
    }
    let mut slots: Vec<SlotView> = world.query::<&SlotView>().iter(world).cloned().collect();
    slots.sort_by_key(|s| s.machine);
    for v in slots {
        h.bytes(&serde_json::to_vec(&v).unwrap_or_default());
    }
    parts.push(("casino", h.0));
    parts
}

/// What a replay ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayResult {
    pub hash: u64,
    /// The house pool at the end.
    pub house: i64,
    pub puddles: usize,
    /// Blackjack rounds dealt, roulette spins, slot pulls.
    pub rounds: u32,
    pub spins: u32,
    pub pulls: u32,
}

/// A dealer's or croupier's requests for this tick, read from the table's
/// view: take the role, deal when bets are down, play the house hand, spin
/// when bets are down, rake losing chips. Every 16 ticks, like a player
/// reacting about four times a second.
fn run_table(world: &mut World, table: shared::casino::TableId, tick: u64, out: &mut Vec<TableAction>) {
    if !tick.is_multiple_of(16) {
        return;
    }
    out.push(TableAction::TakeRole);
    match table {
        shared::casino::TableId::Blackjack => {
            let Some(v) = world.query::<&BlackjackView>().iter(world).next().cloned() else { return };
            if v.phase == BjPhase::Betting && v.seats.iter().any(|s| s.bet > 0) {
                out.push(TableAction::Deal);
            }
            if let Some(a) = v.dealer_should {
                out.push(TableAction::Dealer(a));
            }
        }
        shared::casino::TableId::Roulette => {
            let Some(v) = world.query::<&RouletteView>().iter(world).next().cloned() else { return };
            if v.to_rake > 0 {
                out.push(TableAction::Rake);
            } else if !v.spinning && !v.bets.is_empty() {
                out.push(TableAction::Spin);
            }
        }
        shared::casino::TableId::Slot(_) => {}
    }
}

/// Run the scripted shift for `ticks` ticks and hash the final state.
/// What a soak run measured: host tick times over the run.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SoakResult {
    pub ticks: u64,
    /// The slowest tick in the first second (room start-up, wasm warm-up).
    pub warmup_worst_ms: f64,
    /// The slowest tick after the first second.
    pub worst_ms: f64,
    pub p99_ms: f64,
    pub mean_ms: f64,
    /// Ticks over the 10 ms soak budget.
    pub over_10ms: u64,
    /// Which ticks were over 10 ms (the first 1000). The simulation is
    /// deterministic, so a tick that is slow in its own right is slow in every
    /// run; one the OS stalled is not.
    pub over_10ms_ticks: Vec<u64>,
    /// The five slowest ticks: (tick, ms).
    pub slowest: Vec<(u64, f64)>,
    /// Natively: the slowest tick after the first second in thread CPU time.
    /// Wall time also counts the times the OS runs something else; on a
    /// shared VM even an empty loop sees 10 ms stalls. None in wasm.
    pub cpu_worst_ms: Option<f64>,
    pub house: i64,
}

/// This thread's CPU time in ms (Unix), for [`soak`].
#[cfg(unix)]
fn thread_cpu_ms() -> Option<f64> {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `t` is a valid timespec for clock_gettime to write.
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) } == 0;
    ok.then(|| t.tv_sec as f64 * 1e3 + t.tv_nsec as f64 / 1e6)
}

#[cfg(not(unix))]
fn thread_cpu_ms() -> Option<f64> {
    None
}

/// The soak test (plan section 10, Phase 7): the replay's six scripted
/// players plus two walkers (eight in all) play `ticks` ticks of plan-length
/// shifts, with customers, tables and chaos, and every tick is timed. The
/// first second (room start-up and, in the browser, wasm tier-up) is
/// reported apart from the rest.
pub fn soak(ticks: u64) -> SoakResult {
    use bevy::platform::time::Instant;
    let mut sim = HostSim::with_config(HostConfig { seed: SEED, ..Default::default() });
    let mut bots = [
        Bot::new(BARTENDER),
        Bot::new(WALKER),
        Bot::new(THROWER),
        Bot::new(SPILLER),
        Bot::new(DEALER),
        Bot::new(CROUPIER),
        Bot::new(WALKER),
        Bot::new(WALKER),
    ];
    let players: Vec<Entity> =
        (0..bots.len()).map(|i| sim.add_local_player(0x50a6_0000 + i as u64, "soak", i as u8)).collect();
    let mut times: Vec<f32> = Vec::with_capacity(ticks as usize);
    let mut cpu_worst: Option<f64> = None;
    for i in 0..ticks {
        for (bot, &player) in bots.iter_mut().zip(&players) {
            let pos = sim.world().get::<PlayerPos>(player).map_or(Vec3::ZERO, |p| p.0);
            let input = bot.input(pos);
            sim.set_input(player, input);
            if let Some(table) = bot.running() {
                let mut actions = Vec::new();
                let tick = sim.tick_count();
                run_table(sim.world_mut(), table, tick, &mut actions);
                for action in actions {
                    sim.table_request(player, TableRequest { table, action });
                }
            }
        }
        let t = Instant::now();
        let cpu = thread_cpu_ms();
        sim.tick();
        times.push(t.elapsed().as_secs_f32() * 1000.0);
        if let (Some(a), Some(b)) = (cpu, thread_cpu_ms())
            && i >= u64::from(shared::TICK_HZ)
        {
            cpu_worst = Some(cpu_worst.unwrap_or(0.0).max(b - a));
        }
        sim.drain_audit();
    }
    let warm = (shared::TICK_HZ as usize).min(times.len());
    let warmup_worst = times[..warm].iter().copied().fold(0.0f32, f32::max);
    let times: Vec<f32> = times[warm..].to_vec();
    let mut times = times;
    let n = times.len().max(1);
    let mean = times.iter().map(|t| f64::from(*t)).sum::<f64>() / n as f64;
    let over = times.iter().filter(|t| **t > 10.0).count() as u64;
    let over_ticks: Vec<u64> =
        times.iter().enumerate().filter(|(_, t)| **t > 10.0).map(|(i, _)| (i + warm) as u64).take(1000).collect();
    let mut ranked: Vec<(u64, f64)> =
        times.iter().enumerate().map(|(i, t)| ((i + warm) as u64, f64::from(*t))).collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    ranked.truncate(5);
    times.sort_by(f32::total_cmp);
    let world = sim.world_mut();
    let house = world.query::<&RunLedger>().iter(world).next().map_or(0, |r| r.ledger.house);
    SoakResult {
        ticks,
        warmup_worst_ms: f64::from(warmup_worst),
        worst_ms: f64::from(times.last().copied().unwrap_or(0.0)),
        p99_ms: f64::from(times.get(n * 99 / 100).copied().unwrap_or(0.0)),
        mean_ms: mean,
        over_10ms: over,
        over_10ms_ticks: over_ticks,
        slowest: ranked,
        cpu_worst_ms: cpu_worst,
        house,
    }
}

pub fn run(ticks: u64) -> ReplayResult {
    run_with(ticks, |_| {})
}

/// [`run`], then let `inspect` look at the final world (debugging).
pub fn run_with(ticks: u64, inspect: impl FnOnce(&mut World)) -> ReplayResult {
    let mut sim = HostSim::with_config(HostConfig { seed: SEED, ..Default::default() });
    let mut bots = [
        Bot::new(BARTENDER),
        Bot::new(WALKER),
        Bot::new(THROWER),
        Bot::new(SPILLER),
        Bot::new(DEALER),
        Bot::new(CROUPIER),
    ];
    let players: Vec<Entity> =
        (0..bots.len()).map(|i| sim.add_local_player(0xba5e_0000 + i as u64, "replay", i as u8)).collect();
    for _ in 0..ticks {
        for (bot, &player) in bots.iter_mut().zip(&players) {
            let pos = sim.world().get::<PlayerPos>(player).map_or(Vec3::ZERO, |p| p.0);
            let input = bot.input(pos);
            sim.set_input(player, input);
            if let Some(table) = bot.running() {
                let mut actions = Vec::new();
                let tick = sim.tick_count();
                run_table(sim.world_mut(), table, tick, &mut actions);
                for action in actions {
                    sim.table_request(player, TableRequest { table, action });
                }
            }
        }
        sim.tick();
        // The log is checked by its own tests; keep memory flat here.
        sim.drain_audit();
    }
    let world = sim.world_mut();
    let house = world.query::<&RunLedger>().iter(world).next().map_or(0, |r| r.ledger.house);
    let puddles = world.query::<&Puddle>().iter(world).count();
    let rounds = world.query::<&BlackjackView>().iter(world).next().map_or(0, |v| v.rounds);
    let spins = world.query::<&RouletteView>().iter(world).next().map_or(0, |v| v.spins);
    let pulls = world.query::<&SlotView>().iter(world).map(|v| v.pulls).sum();
    let hash = state_hash(world);
    inspect(world);
    ReplayResult { hash, house, puddles, rounds, spins, pulls }
}

/// Text listing of the state parts and every player's and prop's position
/// (floats by their bits), for comparing two builds line by line.
pub fn describe(world: &mut World) -> String {
    let mut out = String::new();
    for (name, h) in state_parts(world) {
        out += &format!("{name} {h:016x}\n");
    }
    let mut players: Vec<_> =
        world.query::<(&Player, &PlayerPos)>().iter(world).map(|(p, pos)| (p.id, pos.0)).collect();
    players.sort_by_key(|p| p.0);
    for (id, p) in players {
        out += &format!("player {id:x} {:08x} {:08x} {:08x}\n", p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
    }
    let mut props: Vec<_> = world
        .query::<(Entity, &PropKind, &Position, &Rotation, &HeldBy)>()
        .iter(world)
        .map(|(e, k, p, r, h)| (e.index_u32(), *k, p.0, r.0, h.0))
        .collect();
    props.sort_by_key(|p| p.0);
    for (i, k, p, r, h) in props {
        out += &format!(
            "prop {i} {k:?} {:08x} {:08x} {:08x} rot {:08x} {:08x} {:08x} {:08x} held {h:?}\n",
            p.x.to_bits(),
            p.y.to_bits(),
            p.z.to_bits(),
            r.x.to_bits(),
            r.y.to_bits(),
            r.z.to_bits(),
            r.w.to_bits()
        );
    }
    out
}
