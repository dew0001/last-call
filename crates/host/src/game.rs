//! Host-side game rules: joining, players, movement.

use bevy::prelude::*;
use lightyear::prelude::input::native::ActionState;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use shared::protocol::*;

/// Seconds a disconnected player waits for a reconnect before removal.
pub const RECONNECT_GRACE_SECS: f32 = 30.0;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, handle_joins);
        app.add_systems(FixedUpdate, move_players);
    }
}

/// Marks a player whose link is gone, waiting for a reconnect.
#[derive(Component)]
pub struct AwaitingReconnect {
    pub since_tick: u64,
}

fn handle_joins(
    mut commands: Commands,
    mut links: Query<(Entity, &RemoteId, &mut MessageReceiver<Join>, &mut MessageSender<JoinReply>), With<ClientOf>>,
    players: Query<(Entity, &Player)>,
) {
    for (link, remote, mut receiver, mut sender) in &mut links {
        for join in receiver.receive() {
            if join.protocol != PROTOCOL_VERSION {
                sender.send::<Control>(JoinReply::Refused { reason: "version mismatch, reload the page".into() });
                continue;
            }
            let player_id = player_id_from_uuid(&join.player_uuid);
            let peer = remote.0;
            let existing = players.iter().find(|(_, p)| p.id == player_id).map(|(e, _)| e);
            let entity = match existing {
                Some(e) => e,
                None => {
                    let slot = players.iter().count();
                    let spawn = shared::bar::spawn_point(slot);
                    let name = clean_name(&join.display_name);
                    commands
                        .spawn((
                            Player { id: player_id, name, slot: slot as u8 },
                            PlayerPos(Vec3::from_array(spawn)),
                            PlayerYaw(0.0),
                        ))
                        .id()
                }
            };
            commands.entity(entity).remove::<AwaitingReconnect>().insert((
                Replicate::to_clients(NetworkTarget::All),
                PredictionTarget::to_clients(NetworkTarget::Single(peer)),
                InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(peer)),
                ControlledBy { owner: link, lifetime: Lifetime::Persistent },
            ));
            sender.send::<Control>(JoinReply::Welcome { player_id });
            info!("player {player_id:x} joined on {peer:?} (rejoin: {})", existing.is_some());
        }
    }
}

/// Display names: printable, trimmed, at most 16 characters.
pub fn clean_name(raw: &str) -> String {
    let name: String = raw.chars().filter(|c| !c.is_control()).take(16).collect();
    let name = name.trim();
    if name.is_empty() { "Patron".into() } else { name.into() }
}

fn move_players(mut players: Query<(&mut PlayerPos, &mut PlayerYaw, &ActionState<PlayerInput>), With<Player>>) {
    for (mut pos, mut yaw, action) in &mut players {
        shared::net::apply_action(&mut pos, &mut yaw, action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Rook\n "), "Rook");
        assert_eq!(clean_name(""), "Patron");
        assert_eq!(clean_name("abcdefghijklmnopqrstuvwxyz").chars().count(), 16);
    }
}
