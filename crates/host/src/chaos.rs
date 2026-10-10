//! Chaos events on the host (plan section 4.7).
//!
//! At the start of Open the host draws how many events this shift gets and
//! when each starts ([`shared::chaos::schedule`]); at each start it picks one
//! by weight from those not running. Every event has a counter players can
//! pull off and a consequence if they do not:
//!
//! - Raid: cops come in after 20 s and seize every chip stack in the bar
//!   (the office safe is out of reach; with the Back Door, chips in a hand
//!   are kept). Tables pause while the cops are in.
//! - Brawl: two brawlers fling props. Haul both out the front door (R next
//!   to one), or every customer leaves and three props break.
//! - Outage: dark, tables and slots stop. Flip the breaker.
//! - Slot jam: one machine pays 10x until its service key is turned, up to
//!   a 2,000 loss for the house.
//! - Inspector: walks the bar; loose beers or puddles at the end cost 1,500.
//! - Loan shark: sits at blackjack and tips the dealer when he wins. A
//!   dealer at Sloppy or worse while he sits there and he breaks the table
//!   for the shift.
//! - Kitchen fire: customers leave one by one. Throw a beer on it, or use
//!   the extinguisher (upgrade); else the kitchen is closed next shift.
//! - Card counter: a blackjack customer who wins every hand until he is
//!   hauled out (+500 with the Security Camera, which outlines him).
//!
//! The tables read [`TableEffects`] for pauses, the jam, the shark and the
//! counter. Every draw comes from the chaos stream and goes into the audit log.

use avian3d::prelude::*;
use bevy::prelude::*;
use lightyear::prelude::input::native::ActionState;
use lightyear::prelude::*;
use shared::casino::{self, SLOT_MACHINES, TableId};
use shared::chaos::{self, ChaosKind, Ending};
use shared::customers::{DOOR, Mood, WALK_SPEED};
use shared::movement::buttons;
use shared::protocol::*;
use shared::rng::{Draw, StreamId, TableRng};
use shared::shift::ShiftPhase;
use shared::upgrades::UpgradeId;
use shared::world::{self, Room};

use crate::casino::{Audit, PlayerSpots, WantsToLeave};
use crate::customers::{Activity, BarNavMesh, Npc, route};
use crate::fixtures::{FixtureUsed, Owned};
use crate::physics::Hands;
use crate::shift::{PhaseStarted, ShiftConfig, ShiftTimer};

/// The chaos RNG stream.
pub const CHAOS_STREAM: StreamId = StreamId(50);
/// How close a player must stand to haul a chaos NPC or use the extinguisher.
pub const HAUL_REACH: f32 = 1.2;
/// A thrown beer within this distance of the fire puts it out.
pub const DOUSE_REACH: f32 = 1.5;
/// Where the brawl happens.
pub const BRAWL_SPOT: (f32, f32) = (0.0, 3.0);
/// Where the kitchen fire burns.
pub const FIRE_AT: (f32, f32) = (-6.0, -12.0);
/// Seconds between customers leaving because of the smoke.
const SMOKE_EVERY_SECS: u32 = 6;
/// Ticks between props a brawl flings.
const FLING_TICKS: u32 = 40;
/// Customer ids for chaos NPCs start here (wave customers count from 1).
const FIRST_ID: u32 = 1_000_000;

/// What the tables must know about running chaos. Rebuilt every tick.
#[derive(Resource, Default, Debug)]
pub struct TableEffects {
    /// Outage, or the cops are in: tables and slots stop.
    pub paused: bool,
    /// The jammed slot machine.
    pub jam: Option<u8>,
    /// What the jam has cost the house so far (the slots add to it).
    pub jam_cost: i64,
    pub blackjack_broken: bool,
    pub shark: Option<Entity>,
    pub counter: Option<Entity>,
}

impl TableEffects {
    /// Extra a jammed machine pays on top of `returned`, within the cap.
    pub fn jam_extra(&mut self, machine: u8, returned: i64) -> i64 {
        if self.jam != Some(machine) {
            return 0;
        }
        let extra = (returned * (chaos::JAM_MULTIPLIER - 1)).min(chaos::JAM_CAP - self.jam_cost).max(0);
        self.jam_cost += extra;
        extra
    }
}

#[derive(Resource)]
struct ChaosRng(TableRng);

/// One running event (host side).
#[derive(Debug)]
struct Running {
    kind: ChaosKind,
    ticks_left: Option<u32>,
    elapsed: u32,
    /// Customers this event brought in (they walk out when it ends).
    npcs: Vec<Entity>,
    /// Cops and the inspector (despawned when it ends).
    staff: Vec<Entity>,
    target: Option<u8>,
    fire: Option<Entity>,
    cops_in: bool,
    seized: i64,
    hauled: u32,
    countered: bool,
    consequence: bool,
}

impl Running {
    fn new(kind: ChaosKind, owned: &shared::upgrades::Upgrades) -> Self {
        Self {
            kind,
            ticks_left: kind.duration(owned).map(|s| s * shared::TICK_HZ),
            elapsed: 0,
            npcs: Vec::new(),
            staff: Vec::new(),
            target: None,
            fire: None,
            cops_in: false,
            seized: 0,
            hauled: 0,
            countered: false,
            consequence: false,
        }
    }
}

/// The shift's chaos: the schedule, running events and carried effects.
#[derive(Resource, Default, Debug)]
pub struct Chaos {
    /// Seconds into Open each event starts.
    starts: Vec<u32>,
    next: usize,
    running: Vec<Running>,
    /// NPCs hauled out this tick.
    hauled: Vec<Entity>,
    /// The extinguisher was used on the fire this tick.
    extinguished: bool,
    kitchen_offline: bool,
    kitchen_offline_next: bool,
    blackjack_broken: bool,
    next_id: u32,
    /// Endings so far, in order (tests and bots read it).
    pub log: Vec<(ChaosKind, Ending)>,
    /// Only forced events run (tests).
    pub manual: bool,
}

impl Chaos {
    /// `manual`: only forced events run.
    pub fn new(manual: bool) -> Self {
        Self { manual, ..default() }
    }
}

/// A queued forced event (from tests): started on the next tick in Open.
#[derive(Resource, Default)]
pub struct ForcedChaos(pub Vec<ChaosKind>);

/// A player hauling a chaos NPC, and their last buttons.
#[derive(Component, Default)]
pub struct Hauling {
    prev: u16,
    npc: Option<Entity>,
}

/// The inspector's walk.
#[derive(Component)]
struct Patrol {
    path: Vec<Vec2>,
    leg: usize,
}

/// The chaos systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChaosSet;

pub struct ChaosPlugin;

impl Plugin for ChaosPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Chaos>().init_resource::<TableEffects>().init_resource::<ForcedChaos>();
        let seed = app.world().resource::<crate::RoomSeed>().0;
        app.insert_resource(ChaosRng(TableRng::new(seed, CHAOS_STREAM)));
        app.add_systems(
            FixedUpdate,
            (phases, start_events, haul, run_events)
                .chain()
                .in_set(ChaosSet)
                .after(crate::fixtures::FixturesSet)
                .before(crate::casino::CasinoSet),
        );
    }
}

type Customers<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Customer, &'static mut Npc, &'static mut NpcPose, Option<&'static ChaosNpc>),
    Without<Patrol>,
>;

/// Open draws the schedule; Last call ends everything; Setup carries the
/// kitchen and table damage into the new shift.
#[allow(clippy::too_many_arguments)]
fn phases(
    mut commands: Commands,
    mut started: MessageReader<PhaseStarted>,
    config: Res<ShiftConfig>,
    tick: Res<crate::TickCount>,
    mut rng: ResMut<ChaosRng>,
    mut audit: ResMut<Audit>,
    mut chaos: ResMut<Chaos>,
    room: Query<&RunLedger, With<RoomState>>,
    mut players: Query<&mut Hauling>,
) {
    for p in started.read() {
        match p.phase {
            ShiftPhase::Open => {
                let ng = room.single().map_or(0, |r| r.ledger.ng);
                let n = chaos::events_per_shift(p.calendar.week, ng);
                let mut d = rng.0.at(tick.0, &mut audit.0);
                chaos.starts = chaos::schedule(n, config.timings.open, &mut d);
                chaos.next = 0;
                if chaos.manual {
                    chaos.starts.clear();
                }
            }
            ShiftPhase::LastCall => {
                for run in std::mem::take(&mut chaos.running) {
                    cleanup(&mut commands, &run);
                    chaos.log.push((run.kind, Ending::Expired));
                }
                chaos.starts.clear();
                for mut h in &mut players {
                    h.npc = None;
                }
            }
            ShiftPhase::Setup => {
                chaos.kitchen_offline = std::mem::take(&mut chaos.kitchen_offline_next);
                chaos.blackjack_broken = false;
            }
            ShiftPhase::Payment => {}
        }
    }
}

/// Despawn what an event brought that is not a customer; customers walk out.
fn cleanup(commands: &mut Commands, run: &Running) {
    for &e in &run.npcs {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_insert(WantsToLeave);
        }
    }
    for &e in &run.staff {
        commands.entity(e).try_despawn();
    }
    if let Some(f) = run.fire {
        commands.entity(f).try_despawn();
    }
}

/// Seconds into Open, from the shift timer (it stops while nobody is in).
fn open_secs(timer: &ShiftTimer, config: &ShiftConfig) -> u32 {
    let total = config.timings.open * shared::TICK_HZ;
    total.saturating_sub(timer.ticks_left) / shared::TICK_HZ
}

/// A blackjack seat for a visitor: a free one, else one a customer gives up.
fn blackjack_seat(commands: &mut Commands, spots: &PlayerSpots, customers: &Customers) -> Option<u8> {
    let mut taken = [None; shared::blackjack::SEATS];
    for (e, c, n, ..) in customers.iter() {
        if c.mood != Mood::Leaving
            && let Activity::Table(TableId::Blackjack, s) = n.activity
        {
            taken[usize::from(s)] = Some(e);
        }
    }
    let open: Vec<u8> =
        (0..shared::blackjack::SEATS as u8).filter(|s| !spots.0.contains(&(TableId::Blackjack, *s))).collect();
    if let Some(s) = open.iter().find(|s| taken[usize::from(**s)].is_none()) {
        return Some(*s);
    }
    let s = *open.first()?;
    if let Some(e) = taken[usize::from(s)] {
        commands.entity(e).insert(WantsToLeave);
    }
    Some(s)
}

#[allow(clippy::too_many_arguments)]
fn start_events(
    mut commands: Commands,
    config: Res<ShiftConfig>,
    timer: Res<ShiftTimer>,
    tick: Res<crate::TickCount>,
    owned: Res<Owned>,
    mesh: Res<BarNavMesh>,
    spots: Res<PlayerSpots>,
    mut rng: ResMut<ChaosRng>,
    mut audit: ResMut<Audit>,
    mut chaos: ResMut<Chaos>,
    mut forced: ResMut<ForcedChaos>,
    mut effects: ResMut<TableEffects>,
    customers: Customers,
) {
    if timer.phase != ShiftPhase::Open || timer.frozen {
        return;
    }
    let now = open_secs(&timer, &config);
    let mut due: Vec<Option<ChaosKind>> = forced.0.drain(..).map(Some).collect();
    while chaos.starts.get(chaos.next).is_some_and(|s| *s <= now) {
        chaos.next += 1;
        due.push(None);
    }
    for want in due {
        let running: Vec<ChaosKind> = chaos.running.iter().map(|r| r.kind).collect();
        let mut d = rng.0.at(tick.0, &mut audit.0);
        let kind = match want {
            Some(k) if !running.contains(&k) => k,
            Some(_) => continue,
            None => match chaos::pick(&running, &mut d) {
                Some(k) => k,
                None => continue,
            },
        };
        let mut run = Running::new(kind, &owned.0);
        let door = Vec2::new(DOOR.0, DOOR.1);
        let visitor = |chaos: &mut Chaos, commands: &mut Commands, mood, activity, to: Vec2, cash, outlined| {
            chaos.next_id += 1;
            let npc = Npc { cash, start_cash: cash, activity, seat: None, path: route(&mesh.0, door, to), ticks: 0 };
            let e = crate::customers::spawn(commands, FIRST_ID + chaos.next_id, mood, npc);
            commands.entity(e).insert(ChaosNpc { kind, outlined });
            e
        };
        match kind {
            ChaosKind::Raid | ChaosKind::Outage => {}
            ChaosKind::Brawl => {
                for side in [-0.5, 0.5] {
                    let to = Vec2::new(BRAWL_SPOT.0 + side, BRAWL_SPOT.1);
                    let e = visitor(&mut chaos, &mut commands, Mood::Trouble, Activity::Bar, to, 0, false);
                    run.npcs.push(e);
                }
            }
            ChaosKind::SlotJam => {
                let m = d.below(u32::from(SLOT_MACHINES)) as u8;
                run.target = Some(m);
                effects.jam_cost = 0;
            }
            ChaosKind::Inspector => {
                let stops = [(-7.0, 3.0), (-7.0, -4.0), (7.0, -4.0), (7.0, 3.0), (DOOR.0, DOOR.1 - 1.0)];
                let mut path = Vec::new();
                let mut from = door;
                for (x, z) in stops {
                    let to = Vec2::new(x, z);
                    path.extend(route(&mesh.0, from, to));
                    from = to;
                }
                let e = commands
                    .spawn((
                        Name::new("Inspector"),
                        ChaosNpc { kind, outlined: false },
                        NpcPose { pos: Vec3::new(DOOR.0, 0.0, DOOR.1), yaw: 0.0 },
                        Patrol { path, leg: 0 },
                        Replicate::to_clients(NetworkTarget::All),
                        InterpolationTarget::to_clients(NetworkTarget::All),
                    ))
                    .id();
                run.staff.push(e);
            }
            ChaosKind::LoanShark | ChaosKind::CardCounter => {
                let seat = blackjack_seat(&mut commands, &spots, &customers);
                run.target = seat;
                let (activity, to) = match seat {
                    Some(s) => {
                        let (x, z) = casino::bettor_spots(TableId::Blackjack)[usize::from(s)];
                        (Activity::Table(TableId::Blackjack, s), Vec2::new(x, z))
                    }
                    None => (Activity::Bar, Vec2::new(BRAWL_SPOT.0, BRAWL_SPOT.1)),
                };
                let mood = if seat.is_some() { Mood::Entering } else { Mood::Trouble };
                let (cash, outlined) = match kind {
                    ChaosKind::LoanShark => (chaos::SHARK_BET * 8, false),
                    _ => (400, owned.0.has(UpgradeId::SecurityCamera)),
                };
                let e = visitor(&mut chaos, &mut commands, mood, activity, to, cash, outlined);
                run.npcs.push(e);
            }
            ChaosKind::KitchenFire => {
                let pos = Vec3::new(FIRE_AT.0, 0.0, FIRE_AT.1);
                let f = commands
                    .spawn((Name::new("Kitchen fire"), Fire { pos }, Replicate::to_clients(NetworkTarget::All)))
                    .id();
                run.fire = Some(f);
            }
        }
        chaos.running.push(run);
    }
}

/// R next to a brawler or the card counter grabs them; R again lets go. A
/// hauled NPC follows the player, and is gone once out the front door. R by
/// the fire with the extinguisher puts it out.
#[allow(clippy::type_complexity)]
fn haul(
    mut commands: Commands,
    owned: Res<Owned>,
    mut chaos: ResMut<Chaos>,
    mut players: Query<
        (Entity, &PlayerPos, &PlayerYaw, &ActionState<PlayerInput>, &Hands, Option<&mut Hauling>),
        Without<crate::drunk::PassedOut>,
    >,
    mut npcs: Query<(Entity, &ChaosNpc, &mut Customer, &mut Npc, &mut NpcPose)>,
    fires: Query<&Fire>,
) {
    let busy: Vec<Entity> = players.iter().filter_map(|(.., h)| h.and_then(|h| h.npc)).collect();
    for (entity, pos, yaw, action, hands, hauling) in &mut players {
        let Some(mut h) = hauling else {
            commands.entity(entity).insert(Hauling::default());
            continue;
        };
        let b = action.0.buttons;
        let pressed = b & buttons::USE != 0 && h.prev & buttons::USE == 0;
        h.prev = b;
        if let Some(e) = h.npc {
            let Ok((_, _, _, mut npc, mut pose)) = npcs.get_mut(e) else {
                h.npc = None;
                continue;
            };
            if pressed {
                h.npc = None;
                continue;
            }
            let f = Vec3::from_array(shared::movement::forward(yaw.0));
            let at = Vec3::new(pos.0.x - f.x * 0.9, 0.0, pos.0.z - f.z * 0.9);
            npc.path.clear();
            pose.set_if_neq(NpcPose { pos: at, yaw: yaw.0 });
            if world::room_at(at.x, at.z).is_none_or(|r| r == Room::ParkingLot) {
                commands.entity(e).despawn();
                chaos.hauled.push(e);
                h.npc = None;
            }
            continue;
        }
        if !pressed || hands.held.is_some() {
            continue;
        }
        let near = npcs
            .iter()
            .filter(|(e, c, ..)| matches!(c.kind, ChaosKind::Brawl | ChaosKind::CardCounter) && !busy.contains(e))
            .map(|(e, _, _, _, p)| (e, Vec2::new(p.pos.x - pos.0.x, p.pos.z - pos.0.z).length()))
            .filter(|(_, d)| *d < HAUL_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((e, _)) = near
            && let Ok((_, _, mut c, mut npc, _)) = npcs.get_mut(e)
        {
            c.mood = Mood::Trouble;
            npc.activity = Activity::Bar;
            npc.seat = None;
            npc.path.clear();
            h.npc = Some(e);
            continue;
        }
        if owned.0.has(UpgradeId::Extinguisher)
            && fires.iter().any(|f| Vec2::new(f.pos.x - pos.0.x, f.pos.z - pos.0.z).length() < HAUL_REACH * 2.0)
        {
            chaos.extinguished = true;
        }
    }
}

type Props<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PropKind,
        &'static Position,
        &'static HeldBy,
        &'static mut LinearVelocity,
        Option<&'static ChipValue>,
        Option<&'static Beer>,
    ),
>;

#[allow(clippy::too_many_arguments)]
fn run_events(
    mut commands: Commands,
    tick: Res<crate::TickCount>,
    owned: Res<Owned>,
    mut rng: ResMut<ChaosRng>,
    mut audit: ResMut<Audit>,
    mut chaos: ResMut<Chaos>,
    mut effects: ResMut<TableEffects>,
    mut used: MessageReader<FixtureUsed>,
    mut room: Query<(&mut RunLedger, &mut ChaosState), With<RoomState>>,
    customers: Customers,
    mut patrols: Query<(&mut NpcPose, &mut Patrol), Without<Customer>>,
    mut props: Props,
    puddles: Query<&Puddle>,
    players: Query<(&Player, &Drunk)>,
    views: Query<&BlackjackView>,
) {
    let used: Vec<FixtureUsed> = used.read().copied().collect();
    let hauled = std::mem::take(&mut chaos.hauled);
    let extinguished = std::mem::take(&mut chaos.extinguished);
    let Ok((mut run_ledger, mut state)) = room.single_mut() else { return };
    let mut running = std::mem::take(&mut chaos.running);
    let mut ended = Vec::new();
    for run in &mut running {
        run.elapsed += 1;
        if let Some(t) = &mut run.ticks_left {
            *t = t.saturating_sub(1);
        }
        let mut d = rng.0.at(tick.0, &mut audit.0);
        match run.kind {
            ChaosKind::Raid => {
                if !run.cops_in && run.elapsed >= chaos::RAID_COPS_AFTER * shared::TICK_HZ {
                    run.cops_in = true;
                    for side in [-1.5, 1.5] {
                        let e = commands
                            .spawn((
                                Name::new("Cop"),
                                ChaosNpc { kind: ChaosKind::Raid, outlined: false },
                                NpcPose { pos: Vec3::new(DOOR.0 + side, 0.0, DOOR.1 - 1.0), yaw: 0.0 },
                                Patrol { path: Vec::new(), leg: 0 },
                                Replicate::to_clients(NetworkTarget::All),
                                InterpolationTarget::to_clients(NetworkTarget::All),
                            ))
                            .id();
                        run.staff.push(e);
                    }
                    let back_door = owned.0.has(UpgradeId::BackDoor);
                    let mut seized: Vec<(Entity, i64)> = props
                        .iter()
                        .filter_map(|(e, _, p, held, _, chip, _)| {
                            let in_bar = world::room_at(p.0.x, p.0.z) == Some(Room::Bar);
                            let kept = held.0.is_some() && back_door;
                            chip.filter(|_| in_bar && !kept).map(|c| (e, c.0))
                        })
                        .collect();
                    seized.sort();
                    for (e, value) in seized {
                        commands.entity(e).despawn();
                        run.seized += value;
                    }
                }
            }
            ChaosKind::Brawl => {
                run.hauled += hauled.iter().filter(|e| run.npcs.contains(e)).count() as u32;
                if run.hauled >= 2 {
                    run.countered = true;
                } else if run.elapsed.is_multiple_of(FLING_TICKS) {
                    let mut near: Vec<Entity> = props
                        .iter()
                        .filter(|(_, k, p, held, ..)| {
                            **k != PropKind::Stool
                                && held.0.is_none()
                                && Vec2::new(p.0.x - BRAWL_SPOT.0, p.0.z - BRAWL_SPOT.1).length() < 4.0
                        })
                        .map(|(e, ..)| e)
                        .collect();
                    near.sort();
                    if !near.is_empty() {
                        let e = near[d.below(near.len() as u32) as usize];
                        let a = d.below(360) as f32 * std::f32::consts::PI / 180.0;
                        let speed = 3.0 + d.below(3) as f32;
                        if let Ok((.., mut v, _, _)) = props.get_mut(e) {
                            v.0 = Vec3::new(a.cos() * speed, 3.0, a.sin() * speed);
                            commands.entity(e).remove::<Sleeping>();
                        }
                    }
                }
            }
            ChaosKind::Outage => {
                run.countered |= used.iter().any(|u| u.fixture == shared::fixtures::Fixture::Breaker);
            }
            ChaosKind::SlotJam => {
                let m = run.target.unwrap_or(0);
                run.countered |= used.iter().any(|u| u.fixture == shared::fixtures::Fixture::ServiceKey(m));
                run.consequence |= effects.jam_cost >= chaos::JAM_CAP;
            }
            ChaosKind::Inspector => {
                for &e in &run.staff {
                    if let Ok((mut pose, mut patrol)) = patrols.get_mut(e) {
                        walk(&mut pose, &mut patrol, WALK_SPEED * 0.7);
                    }
                }
            }
            ChaosKind::LoanShark => {
                let shark = run.npcs.first().copied();
                let seated =
                    shark.and_then(|e| customers.get(e).ok()).is_some_and(|(_, c, ..)| c.mood == Mood::Gambling);
                let dealer = views.single().ok().and_then(|v| v.dealer);
                let drunk_dealer = dealer
                    .and_then(|id| players.iter().find(|(p, _)| p.id == id))
                    .is_some_and(|(_, d)| shared::drunk::Tier::of(d.level) >= shared::drunk::Tier::Sloppy);
                if seated && drunk_dealer {
                    run.consequence = true;
                }
            }
            ChaosKind::KitchenFire => {
                let fire = Vec2::new(FIRE_AT.0, FIRE_AT.1);
                let mut doused: Vec<Entity> = props
                    .iter()
                    .filter(|(_, _, p, held, _, _, beer)| {
                        beer.is_some()
                            && held.0.is_none()
                            && p.0.y < 2.0
                            && Vec2::new(p.0.x, p.0.z).distance(fire) < DOUSE_REACH
                    })
                    .map(|(e, ..)| e)
                    .collect();
                doused.sort();
                if let Some(e) = doused.first() {
                    commands.entity(*e).despawn();
                    run.countered = true;
                }
                run.countered |= extinguished;
                if !run.countered && run.elapsed.is_multiple_of(SMOKE_EVERY_SECS * shared::TICK_HZ) {
                    let mut stay: Vec<(u32, Entity)> = customers
                        .iter()
                        .filter(|(_, c, _, _, x)| x.is_none() && !matches!(c.mood, Mood::Leaving | Mood::Entering))
                        .map(|(e, c, ..)| (c.id, e))
                        .collect();
                    stay.sort();
                    if !stay.is_empty() {
                        let (_, e) = stay[d.below(stay.len() as u32) as usize];
                        commands.entity(e).insert(WantsToLeave);
                    }
                }
            }
            ChaosKind::CardCounter => {
                if hauled.iter().any(|e| run.npcs.contains(e)) {
                    run.countered = true;
                } else if run
                    .npcs
                    .first()
                    .is_none_or(|e| customers.get(*e).ok().is_none_or(|(_, c, ..)| c.mood == Mood::Leaving))
                {
                    // He walked out with his winnings.
                    run.consequence = true;
                }
            }
        }

        // How it ends.
        let timed_out = run.ticks_left == Some(0);
        let ending = if run.countered {
            Some(Ending::Countered)
        } else if run.consequence {
            Some(Ending::Consequence)
        } else if timed_out {
            Some(match run.kind {
                ChaosKind::Raid if run.seized == 0 => Ending::Countered,
                ChaosKind::LoanShark => Ending::Countered,
                ChaosKind::Inspector => {
                    let loose_beer = props.iter().any(|(_, _, _, held, _, _, beer)| beer.is_some() && held.0.is_none());
                    let mess = puddles.iter().any(|p| world::room_at(p.pos.x, p.pos.z).is_some_and(|r| !r.outdoors()));
                    if loose_beer || mess { Ending::Consequence } else { Ending::Countered }
                }
                _ => Ending::Consequence,
            })
        } else {
            None
        };
        let Some(ending) = ending else { continue };
        match (run.kind, ending) {
            (ChaosKind::Brawl, Ending::Consequence) => {
                for (e, c, _, _, x) in customers.iter() {
                    if x.is_none() && c.mood != Mood::Leaving {
                        commands.entity(e).insert(WantsToLeave);
                    }
                }
                let mut breakable: Vec<Entity> = props
                    .iter()
                    .filter(|(_, k, p, held, ..)| {
                        matches!(k, PropKind::Bottle | PropKind::Glass)
                            && held.0.is_none()
                            && world::room_at(p.0.x, p.0.z) == Some(Room::Bar)
                    })
                    .map(|(e, ..)| e)
                    .collect();
                breakable.sort();
                for _ in 0..chaos::BRAWL_BREAKS.min(breakable.len()) {
                    let e = breakable.remove(d.below(breakable.len() as u32) as usize);
                    commands.entity(e).despawn();
                }
            }
            (ChaosKind::Inspector, Ending::Consequence) => run_ledger.ledger.house -= chaos::INSPECTOR_FINE,
            (ChaosKind::LoanShark, Ending::Consequence) => chaos.blackjack_broken = true,
            (ChaosKind::KitchenFire, Ending::Consequence) => chaos.kitchen_offline_next = true,
            (ChaosKind::CardCounter, Ending::Countered) if owned.0.has(UpgradeId::SecurityCamera) => {
                run_ledger.ledger.house += shared::upgrades::CATCH_BONUS;
            }
            _ => {}
        }
        cleanup(&mut commands, run);
        ended.push((run.kind, ending));
    }
    running.retain(|r| !ended.iter().any(|(k, _)| *k == r.kind));
    chaos.running = running;
    chaos.log.extend(ended.iter().copied());

    // Publish.
    let fire = chaos.running.iter().any(|r| r.kind == ChaosKind::KitchenFire);
    let mut recent = state.recent.clone();
    recent.extend(ended);
    let keep = recent.len().saturating_sub(4);
    recent.drain(..keep);
    let next = ChaosState {
        active: chaos
            .running
            .iter()
            .map(|r| ActiveChaos {
                kind: r.kind,
                seconds_left: r.ticks_left.map(|t| t.div_ceil(shared::TICK_HZ) as u16),
                target: r.target,
                cops_in: r.cops_in,
            })
            .collect(),
        recent,
        kitchen_offline: chaos.kitchen_offline || fire,
        blackjack_broken: chaos.blackjack_broken,
        dark: chaos.running.iter().any(|r| r.kind == ChaosKind::Outage),
    };
    state.set_if_neq(next);
    effects.paused = chaos.running.iter().any(|r| r.kind == ChaosKind::Outage || r.cops_in);
    effects.jam = chaos.running.iter().find(|r| r.kind == ChaosKind::SlotJam).and_then(|r| r.target);
    effects.blackjack_broken = chaos.blackjack_broken;
    let npc_of = |k: ChaosKind| chaos.running.iter().find(|r| r.kind == k).and_then(|r| r.npcs.first().copied());
    effects.shark = npc_of(ChaosKind::LoanShark);
    effects.counter = npc_of(ChaosKind::CardCounter);
}

/// Move a patrolling NPC one tick along its path, looping back to the start.
fn walk(pose: &mut NpcPose, patrol: &mut Patrol, speed: f32) {
    if patrol.path.is_empty() {
        return;
    }
    let step = speed * shared::TICK.as_secs_f32();
    let here = Vec2::new(pose.pos.x, pose.pos.z);
    let to = patrol.path[patrol.leg % patrol.path.len()];
    let d = to - here;
    if d.length() <= step {
        pose.pos = Vec3::new(to.x, 0.0, to.y);
        patrol.leg = (patrol.leg + 1) % patrol.path.len();
    } else {
        let dir = d / d.length();
        pose.pos = Vec3::new(here.x + dir.x * step, 0.0, here.y + dir.y * step);
        pose.yaw = shared::math::atan2(-dir.x, -dir.y);
    }
}
