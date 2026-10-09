//! Saving and resuming runs (see [`shared::save`]).
//!
//! At the start of every Setup but the room's first, the host writes a [`RunSave`] into
//! [`PendingSave`]; runners take it with [`crate::HostSim::take_save`] and
//! store it (IndexedDB in the browser, a file natively). A room started with
//! [`crate::HostConfig::resume`] begins at the saved shift's Setup, and a
//! returning player (same player id) gets their saved pocket back.

use std::collections::BTreeMap;

use bevy::prelude::*;
use shared::protocol::{Player, Pocket, RoomState, RoomUpgrades, RunLedger};
use shared::save::RunSave;
use shared::shift::ShiftPhase;

use crate::shift::{PhaseStarted, ShiftTimer};

/// Pockets from a resumed save whose players have not rejoined yet. They
/// carry into later saves until claimed, so a friend who joins late still
/// gets their money. A new run (win or loss) clears them.
#[derive(Resource, Default, Debug)]
pub struct SavedPockets(pub BTreeMap<u64, i64>);

/// The newest save, waiting for the runner to store it.
#[derive(Resource, Default, Debug)]
pub struct PendingSave(pub Option<RunSave>);

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SavedPockets>().init_resource::<PendingSave>();
        app.add_systems(FixedUpdate, write_save.after(crate::economy::EconomySet));
    }
}

/// Save at each Setup, except the room's first: opening a room (by mistake,
/// say) does not replace the saved run until a shift has been played.
fn write_save(
    mut first: Local<bool>,
    mut started: MessageReader<PhaseStarted>,
    timer: Res<ShiftTimer>,
    saved: Res<SavedPockets>,
    room: Query<(&RunLedger, &RoomUpgrades), With<RoomState>>,
    players: Query<(&Player, &Pocket)>,
    mut pending: ResMut<PendingSave>,
) {
    if !started.read().any(|p| p.phase == ShiftPhase::Setup) {
        return;
    }
    if !*first {
        *first = true;
        return;
    }
    let Ok((run, upgrades)) = room.single() else { return };
    let mut pockets = saved.0.clone();
    for (p, pocket) in &players {
        pockets.insert(p.id, pocket.0);
    }
    let mut save = RunSave::new(run.ledger, timer.calendar, pockets);
    save.upgrades = upgrades.0;
    pending.0 = Some(save);
}
