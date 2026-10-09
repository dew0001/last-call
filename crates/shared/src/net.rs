//! Bevy systems shared by the host and predicting clients.

use bevy::prelude::*;
use lightyear::prelude::input::native::ActionState;

use crate::drunk::{self, Tier};
use crate::movement;
use crate::protocol::{Drunk, PlayerInput, PlayerPos, PlayerYaw};

/// Apply one tick of input to a player. Called in `FixedUpdate` by the host
/// for every player and by a client for its own predicted player. A passed-out
/// player does not move; a Sloppy or worse one walks 10% faster.
pub fn apply_input(pos: &mut PlayerPos, yaw: &mut PlayerYaw, input: &PlayerInput, drunk: Option<&Drunk>) {
    if drunk.is_some_and(|d| d.passed_out) {
        return;
    }
    let dt = crate::TICK.as_secs_f32();
    let mv = input.mv();
    yaw.0 = input.yaw();
    let mult = drunk::walk_multiplier(Tier::of(drunk.map_or(0, |d| d.level)));
    let p = movement::step_scaled(pos.0.to_array(), [mv.x, mv.y], yaw.0, input.buttons, dt, mult);
    pos.0 = Vec3::from_array(p);
}

/// Read the current action state and move.
pub fn apply_action(
    pos: &mut PlayerPos,
    yaw: &mut PlayerYaw,
    action: &ActionState<PlayerInput>,
    drunk: Option<&Drunk>,
) {
    apply_input(pos, yaw, &action.0, drunk);
}
