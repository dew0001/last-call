//! Money on the host (plan section 4.4): the house pool, pockets, the office
//! safe, the loan shark's collection, the win and loss screens, and new runs.
//!
//! The room entity's [`RunLedger`] is the source of truth; replication sends
//! it to clients whenever it changes.

use bevy::prelude::*;
use lightyear::prelude::input::native::ActionState;
use shared::economy::{self, Ledger, Outcome};
use shared::movement::{buttons, distance_to_safe};
use shared::protocol::{Drunk, Player, PlayerInput, PlayerPos, Pocket, RoomState, RunLedger};
use shared::shift::{Calendar, ShiftPhase};

use crate::shift::{PhaseStarted, RunClock, ShiftConfig, ShiftTimer};

/// A room set up for a test or a demo, chosen with `?preset=` on the host page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Preset {
    #[default]
    None,
    /// Week 6 with 80,000 paid and 45,000 in the house: the run is won at the
    /// end of the week. Players start with 300 in their pockets.
    LastWeek,
    /// One payment already missed and an empty house: the run is lost at the
    /// end of the first week.
    Broke,
    /// Players join Sloppy (45 on the drunk meter) with 300 in their pockets.
    Tipsy,
    /// Players join Wasted (90 on the drunk meter) with 300 in their pockets:
    /// one beer passes them out.
    Wasted,
    /// Players join with 1,000 in their pockets, to play the tables.
    Casino,
}

impl Preset {
    pub fn parse(s: &str) -> Self {
        match s {
            "lastweek" => Self::LastWeek,
            "broke" => Self::Broke,
            "tipsy" => Self::Tipsy,
            "wasted" => Self::Wasted,
            "casino" => Self::Casino,
            _ => Self::None,
        }
    }

    /// Where a run starts with this preset.
    pub fn start(self) -> RunStart {
        match self {
            Self::None => RunStart::default(),
            Self::LastWeek => RunStart {
                ledger: Ledger { house: 45_000, paid: 80_000, ..Ledger::default() },
                calendar: Calendar { week: 6, shift: 0 },
                pocket: 300,
                ..RunStart::default()
            },
            Self::Broke => RunStart {
                ledger: Ledger { missed_in_a_row: 1, carried: 8_000, ..Ledger::default() },
                calendar: Calendar::default(),
                ..RunStart::default()
            },
            Self::Tipsy => RunStart { pocket: 300, drunk: 45, ..RunStart::default() },
            Self::Wasted => RunStart { pocket: 300, drunk: 90, ..RunStart::default() },
            Self::Casino => RunStart { pocket: 1_000, ..RunStart::default() },
        }
    }
}

/// Money and calendar at the start of the first run.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct RunStart {
    pub ledger: Ledger,
    pub calendar: Calendar,
    /// What a joining player finds in their pocket.
    pub pocket: i64,
    /// A joining player's drunk meter.
    pub drunk: u8,
}

/// Present while the win or loss screen shows.
#[derive(Resource, Clone, Copy, Debug)]
pub struct RunOver {
    pub outcome: Outcome,
    pub ticks_left: u32,
}

/// Last tick's buttons, for press edges at the safe.
#[derive(Component, Default)]
struct SafeButtons(u16);

pub struct EconomyPlugin;

/// The economy systems (saves run after them).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EconomySet;

impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RunStart>();
        app.add_systems(Startup, open_books.after(crate::shift::start_clock));
        app.add_systems(
            FixedUpdate,
            (give_pockets, safe_deposits, collect_payments, end_run, update_due)
                .chain()
                .in_set(EconomySet)
                .after(RunClock),
        );
    }
}

fn open_books(mut commands: Commands, start: Res<RunStart>, room: Query<Entity, With<RoomState>>) {
    for room in &room {
        let ledger = start.ledger;
        commands.entity(room).insert(RunLedger { ledger, due: ledger.due(start.calendar.week), ..default() });
    }
}

/// A joining player gets the run's starting pocket, or their own pocket from
/// a resumed save.
fn give_pockets(
    mut commands: Commands,
    start: Res<RunStart>,
    mut saved: ResMut<crate::save::SavedPockets>,
    players: Query<(Entity, &Player), Without<Pocket>>,
) {
    for (entity, player) in &players {
        let pocket = saved.0.remove(&player.id).unwrap_or(start.pocket);
        commands.entity(entity).insert((Pocket(pocket), SafeButtons::default()));
    }
}

/// E at the office safe moves money from the pocket to the house pool.
fn safe_deposits(
    mut players: Query<(&PlayerPos, &ActionState<PlayerInput>, &mut Pocket, &mut SafeButtons, Option<&Drunk>)>,
    mut room: Query<&mut RunLedger, With<RoomState>>,
) {
    let Ok(mut run) = room.single_mut() else { return };
    for (pos, action, mut pocket, mut prev, drunk) in &mut players {
        let b = action.0.buttons;
        let pressed = b & buttons::INTERACT != 0 && prev.0 & buttons::INTERACT == 0;
        prev.0 = b;
        if drunk.is_some_and(|d| d.passed_out) {
            continue;
        }
        if pressed && distance_to_safe(pos.0.to_array()) < shared::bar::SAFE_REACH && pocket.0 > 0 {
            let mut ledger = run.ledger;
            let mut money = pocket.0;
            economy::deposit(&mut money, &mut ledger);
            pocket.0 = money;
            run.ledger = ledger;
        }
    }
}

/// The loan shark collects when the last shift of the week reaches Payment.
fn collect_payments(
    mut commands: Commands,
    config: Res<ShiftConfig>,
    mut started: MessageReader<PhaseStarted>,
    mut timer: ResMut<ShiftTimer>,
    mut room: Query<&mut RunLedger, With<RoomState>>,
) {
    let Ok(mut run) = room.single_mut() else { return };
    for p in started.read() {
        if p.phase != ShiftPhase::Payment || !p.calendar.is_payment_shift() {
            continue;
        }
        let collection = run.ledger.collect(p.calendar.week);
        run.last = Some(collection);
        run.outcome = run.ledger.outcome();
        info!("week {} payment: {collection:?}; outcome {:?}", p.calendar.week, run.outcome);
        if run.outcome != Outcome::Playing {
            timer.frozen = true;
            let ticks_left = config.timings.outcome * shared::TICK_HZ;
            commands.insert_resource(RunOver { outcome: run.outcome, ticks_left });
        }
    }
}

/// After the win or loss screen, start a new run one new game plus level up.
/// No money carries over.
fn end_run(
    mut commands: Commands,
    config: Res<ShiftConfig>,
    over: Option<ResMut<RunOver>>,
    mut timer: ResMut<ShiftTimer>,
    mut room: Query<&mut RunLedger, With<RoomState>>,
    mut pockets: Query<&mut Pocket>,
    mut saved: ResMut<crate::save::SavedPockets>,
    mut started: MessageWriter<PhaseStarted>,
) {
    let Some(mut over) = over else { return };
    over.ticks_left = over.ticks_left.saturating_sub(1);
    if over.ticks_left > 0 {
        return;
    }
    commands.remove_resource::<RunOver>();
    let Ok(mut run) = room.single_mut() else { return };
    let ng = run.ledger.ng.saturating_add(1);
    *run = RunLedger { ledger: Ledger::new_run(ng), ..default() };
    for mut pocket in &mut pockets {
        pocket.0 = 0;
    }
    saved.0.clear();
    let calendar = Calendar::default();
    *timer = ShiftTimer {
        calendar,
        phase: ShiftPhase::Setup,
        ticks_left: config.timings.setup * shared::TICK_HZ,
        frozen: false,
    };
    started.write(PhaseStarted { calendar, phase: ShiftPhase::Setup });
    info!("new run, new game plus level {ng}");
}

fn update_due(timer: Res<ShiftTimer>, mut room: Query<&mut RunLedger, With<RoomState>>) {
    let Ok(mut run) = room.single_mut() else { return };
    let due = run.ledger.due(timer.calendar.week);
    if run.due != due {
        run.due = due;
    }
}
