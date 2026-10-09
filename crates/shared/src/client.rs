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

/// Tap press and release stamps to send to the host (see [`TapEvent`]).
#[derive(Resource, Default, Clone, Debug)]
pub struct OutgoingTaps(pub Vec<TapEvent>);

/// Table requests to send to the host (UI buttons, bot scripts).
#[derive(Resource, Default, Clone, Debug)]
pub struct OutgoingTable(pub Vec<TableRequest>);

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
        app.init_resource::<LocalInput>()
            .init_resource::<Session>()
            .init_resource::<OutgoingTable>()
            .init_resource::<OutgoingTaps>()
            .init_resource::<ConnectAfterFrames>();
        // Margin for jitter: 2x covers about 95% of packets. lightyear's default
        // (4x) put browser clients more than a second ahead of the host.
        app.insert_resource(
            InputTimelineConfig::default().with_sync_config(SyncConfig { jitter_multiple: 2, ..default() }),
        );
        app.add_systems(Update, connect_when_warm);
        app.add_systems(Update, (send_join, read_join_reply, mark_own_player, send_table_requests).chain());
        app.add_systems(FixedPreUpdate, write_input.in_set(InputSystems::WriteClientInputs));
        app.add_systems(FixedUpdate, predict_movement);
    }
}

/// Frames to draw before connecting. A page's first frames stutter while it
/// compiles shaders; pings measured then inflate the round-trip estimate and
/// push the client's timeline far ahead of the host. Native bots use 0. A
/// very slow device connects after [`CONNECT_AFTER_MAX`] anyway.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ConnectAfterFrames(pub u32);

/// Connect after this long even if few frames were drawn.
pub const CONNECT_AFTER_MAX: core::time::Duration = core::time::Duration::from_secs(3);

impl Default for ConnectAfterFrames {
    fn default() -> Self {
        Self(15)
    }
}

fn connect_when_warm(
    wait: Res<ConnectAfterFrames>,
    time: Res<Time<Real>>,
    mut frames: Local<u32>,
    clients: Query<Entity, (With<Client>, Without<Connected>, Without<Connecting>)>,
    mut done: Local<bool>,
    mut commands: Commands,
) {
    if *done {
        return;
    }
    *frames += 1;
    if *frames <= wait.0 && time.elapsed() < CONNECT_AFTER_MAX {
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

fn send_table_requests(
    session: Res<Session>,
    mut out: ResMut<OutgoingTable>,
    mut taps: ResMut<OutgoingTaps>,
    mut links: Query<(&mut MessageSender<TableRequest>, &mut MessageSender<TapEvent>), (With<Client>, With<Connected>)>,
) {
    if session.player_id.is_none() {
        return;
    }
    let Ok((mut tables, mut tap_sender)) = links.single_mut() else { return };
    for request in out.0.drain(..) {
        tables.send::<Control>(request);
    }
    for tap in taps.0.drain(..) {
        tap_sender.send::<Control>(tap);
    }
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

/// Keep the input marker on exactly one entity: the player this client was
/// welcomed as, once it is predicted here. Any other marker (left over when a
/// player's controller changed) is removed, so we never send input for a
/// player we do not own.
fn mark_own_player(
    session: Res<Session>,
    players: Query<(Entity, &Player, Has<Predicted>, Has<InputMarker<PlayerInput>>)>,
    mut commands: Commands,
) {
    for (entity, player, predicted, marked) in &players {
        let mine = predicted && Some(player.id) == session.player_id;
        if mine && !marked {
            commands.entity(entity).insert(InputMarker::<PlayerInput>::default());
        } else if !mine && marked {
            commands.entity(entity).remove::<InputMarker<PlayerInput>>();
        }
    }
}

fn write_input(local: Res<LocalInput>, mut q: Query<&mut ActionState<PlayerInput>, With<InputMarker<PlayerInput>>>) {
    for mut action in &mut q {
        action.0 = local.0;
    }
}

fn predict_movement(
    mut q: Query<
        (&mut PlayerPos, &mut PlayerYaw, &ActionState<PlayerInput>, Option<&crate::protocol::Drunk>),
        (With<Predicted>, With<Player>),
    >,
) {
    for (mut pos, mut yaw, action, drunk) in &mut q {
        crate::net::apply_action(&mut pos, &mut yaw, action, drunk);
    }
}
