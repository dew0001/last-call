//! Client-side netcode shared by the browser client and native bots.
//!
//! The app writes the wanted input into [`LocalInput`] every frame (keyboard
//! in the browser, a script in a bot). This plugin joins the room once the link
//! is connected, marks the controlled player for input, copies [`LocalInput`]
//! into lightyear's input buffer each tick, and predicts the own player's
//! movement with the same code the host runs.

use bevy::prelude::*;
use lightyear::prelude::SyncConfig;
use lightyear::prelude::client::input::InputSystems;
use lightyear::prelude::client::{Connect, Connecting, InputTimelineConfig};
use lightyear::prelude::input::native::{ActionState, InputMarker};
use lightyear::prelude::*;

use crate::protocol::*;

/// What this client wants to do this tick.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub struct LocalInput(pub PlayerInput);

/// Who this client is. Set by the app before connecting.
#[derive(Resource, Clone, Debug)]
pub struct Identity {
    pub code: String,
    pub player_uuid: [u8; 16],
    pub display_name: String,
    pub cosmetic_id: u16,
}

/// Session state the UI and tests read.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct Session {
    pub player_id: Option<u64>,
    pub refused: Option<String>,
    pub join_sent: bool,
}

pub struct ClientNetPlugin;

impl Plugin for ClientNetPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::pipe::PipePlugin>() {
            app.add_plugins(crate::pipe::PipePlugin);
        }
        app.init_resource::<LocalInput>().init_resource::<Session>().init_resource::<ConnectAfterFrames>();
        // Margin for jitter: 2x covers about 95% of packets. lightyear's default
        // (4x) put browser clients more than a second ahead of the host.
        app.insert_resource(
            InputTimelineConfig::default().with_sync_config(SyncConfig { jitter_multiple: 2, ..default() }),
        );
        app.add_systems(Update, connect_when_warm);
        app.add_systems(Update, (send_join, read_join_reply));
        app.add_systems(FixedPreUpdate, write_input.in_set(InputSystems::WriteClientInputs));
        app.add_systems(FixedUpdate, predict_movement);
        app.add_observer(mark_controlled);
    }
}

/// Frames to draw before connecting. A page's first frames stutter while it
/// compiles shaders; pings measured then inflate the round-trip estimate and
/// push the client's timeline far ahead of the host. Native bots use 0.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ConnectAfterFrames(pub u32);

impl Default for ConnectAfterFrames {
    fn default() -> Self {
        Self(15)
    }
}

fn connect_when_warm(
    wait: Res<ConnectAfterFrames>,
    mut frames: Local<u32>,
    clients: Query<Entity, (With<Client>, Without<Connected>, Without<Connecting>)>,
    mut done: Local<bool>,
    mut commands: Commands,
) {
    if *done {
        return;
    }
    *frames += 1;
    if *frames <= wait.0 {
        return;
    }
    for entity in &clients {
        commands.trigger(Connect { entity });
    }
    *done = true;
}

fn send_join(
    identity: Option<Res<Identity>>,
    mut session: ResMut<Session>,
    mut links: Query<&mut MessageSender<Join>, (With<Client>, With<Connected>)>,
) {
    let Some(identity) = identity else { return };
    if session.join_sent {
        return;
    }
    let Ok(mut sender) = links.single_mut() else { return };
    sender.send::<Control>(Join {
        protocol: PROTOCOL_VERSION,
        code: identity.code.clone(),
        player_uuid: identity.player_uuid,
        display_name: identity.display_name.clone(),
        cosmetic_id: identity.cosmetic_id,
    });
    session.join_sent = true;
}

fn read_join_reply(mut session: ResMut<Session>, mut links: Query<&mut MessageReceiver<JoinReply>, With<Client>>) {
    for mut receiver in &mut links {
        for reply in receiver.receive() {
            match reply {
                JoinReply::Welcome { player_id } => session.player_id = Some(player_id),
                JoinReply::Refused { reason } => session.refused = Some(reason),
            }
        }
    }
}

fn mark_controlled(trigger: On<Add, Controlled>, players: Query<(), With<Player>>, mut commands: Commands) {
    if players.contains(trigger.entity) {
        commands.entity(trigger.entity).insert(InputMarker::<PlayerInput>::default());
    }
}

fn write_input(local: Res<LocalInput>, mut q: Query<&mut ActionState<PlayerInput>, With<InputMarker<PlayerInput>>>) {
    for mut action in &mut q {
        action.0 = local.0;
    }
}

fn predict_movement(
    mut q: Query<(&mut PlayerPos, &mut PlayerYaw, &ActionState<PlayerInput>), (With<Predicted>, With<Player>)>,
) {
    for (mut pos, mut yaw, action) in &mut q {
        crate::net::apply_action(&mut pos, &mut yaw, action);
    }
}
