//! Bevy systems shared by the host and predicting clients.

use bevy::prelude::*;
use lightyear::prelude::input::native::ActionState;

use crate::movement;
use crate::protocol::{PlayerInput, PlayerPos, PlayerYaw};

/// Apply one tick of input to a player. Called in `FixedUpdate` by the host
/// for every player and by a client for its own predicted player.
pub fn apply_input(pos: &mut PlayerPos, yaw: &mut PlayerYaw, input: &PlayerInput) {
    let dt = crate::TICK.as_secs_f32();
    let mv = input.mv();
    yaw.0 = input.yaw();
    let p = movement::step(pos.0.to_array(), [mv.x, mv.y], yaw.0, input.buttons, dt);
    pos.0 = Vec3::from_array(p);
}

/// Read the current action state and move.
pub fn apply_action(pos: &mut PlayerPos, yaw: &mut PlayerYaw, action: &ActionState<PlayerInput>) {
    apply_input(pos, yaw, &action.0);
}
