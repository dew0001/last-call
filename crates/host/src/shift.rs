//! The shift clock (plan section 4.3). The host owns it: it counts ticks,
//! moves through Setup, Open, Last call and Payment, and rolls into the next
//! shift. Clients read the replicated [`ShiftClock`].
//!
//! The clock runs only while at least one player is in the room, so an empty
//! room (everyone refreshing, or the host alone in the lobby) does not lose
//! time. It also stops while the win or loss screen shows.

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::protocol::{Player, RoomState, ShiftClock};
use shared::shift::{Calendar, ShiftPhase, Timings};

use crate::game::AwaitingReconnect;

/// Phase lengths for this room. Tests and `?fast` rooms shorten them.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct ShiftConfig {
    pub timings: Timings,
}

/// Host-side clock state.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ShiftTimer {
    pub calendar: Calendar,
    pub phase: ShiftPhase,
    pub ticks_left: u32,
    /// Stopped while the win or loss screen shows.
    pub frozen: bool,
}

/// Sent on the tick a phase starts (including the first Setup).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseStarted {
    pub calendar: Calendar,
    pub phase: ShiftPhase,
}

/// Runs after player movement in the fixed tick.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunClock;

pub struct ShiftPlugin;

impl Plugin for ShiftPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShiftConfig>();
        app.add_message::<PhaseStarted>();
        app.add_systems(Startup, start_clock);
        app.add_systems(FixedUpdate, run_clock.in_set(RunClock).after(crate::game::MovePlayers));
    }
}

fn ticks(seconds: u32) -> u32 {
    seconds * shared::TICK_HZ
}

/// Create the room-state entity and start the first Setup.
pub fn start_clock(
    mut commands: Commands,
    config: Res<ShiftConfig>,
    start: Option<Res<crate::economy::RunStart>>,
    mut started: MessageWriter<PhaseStarted>,
) {
    let calendar = start.map(|s| s.calendar).unwrap_or_default();
    let phase = ShiftPhase::Setup;
    let seconds = config.timings.seconds(phase);
    commands.insert_resource(ShiftTimer { calendar, phase, ticks_left: ticks(seconds), frozen: false });
    commands.spawn((
        Name::new("Room"),
        RoomState,
        ShiftClock { calendar, phase, seconds_left: seconds as u16, running: false },
        Replicate::to_clients(NetworkTarget::All),
    ));
    started.write(PhaseStarted { calendar, phase });
}

fn run_clock(
    config: Res<ShiftConfig>,
    timer: Option<ResMut<ShiftTimer>>,
    players: Query<(), (With<Player>, Without<AwaitingReconnect>)>,
    mut clock: Query<&mut ShiftClock, With<RoomState>>,
    mut started: MessageWriter<PhaseStarted>,
) {
    let (Some(mut timer), Ok(mut clock)) = (timer, clock.single_mut()) else { return };
    let running = !players.is_empty() && !timer.frozen;
    if running {
        timer.ticks_left = timer.ticks_left.saturating_sub(1);
        if timer.ticks_left == 0 {
            match timer.phase.next() {
                Some(next) => timer.phase = next,
                None => {
                    timer.calendar = timer.calendar.next_shift();
                    timer.phase = ShiftPhase::Setup;
                }
            }
            timer.ticks_left = ticks(config.timings.seconds(timer.phase));
            started.write(PhaseStarted { calendar: timer.calendar, phase: timer.phase });
        }
    }
    let seconds_left = timer.ticks_left.div_ceil(shared::TICK_HZ) as u16;
    // Write only on change, so replication sends at most one update a second.
    clock.set_if_neq(ShiftClock { calendar: timer.calendar, phase: timer.phase, seconds_left, running });
}
