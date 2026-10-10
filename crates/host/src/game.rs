//! Host-side game rules: joining, players, movement.

use bevy::prelude::*;
use lightyear::input::native::prelude::NativeStateSequence;
use lightyear::input::server::{InputValidationAppExt, authorize_controlled_targets};
use lightyear::prelude::input::native::ActionState;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use shared::protocol::*;

/// The highest hat style (0 is none).
const MAX_HAT: u16 = 3;

/// Seconds a disconnected player waits for a reconnect before removal.
pub const RECONNECT_GRACE_SECS: f32 = 30.0;

pub struct GamePlugin;

/// Player movement for the tick. Hands and physics run after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MovePlayers;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (handle_joins, track_disconnects).chain());
        // A client may only drive the player it controls. Without this, a stale
        // input marker on another client could overwrite a player's input.
        app.add_input_validator(authorize_controlled_targets::<NativeStateSequence<PlayerInput>>);
        app.add_systems(FixedUpdate, move_players.in_set(MovePlayers));
    }
}

/// A player driven directly by host code (replays and tests), with no link.
/// [`crate::HostSim::set_input`] writes its input each tick.
#[derive(Component)]
pub struct LocalPlayer;

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
    // Slots taken, including players spawned earlier in this same pass (their
    // spawn commands have not been applied yet).
    let mut taken: Vec<u8> = players.iter().map(|(_, p)| p.slot).collect();
    let mut spawned: Vec<(u64, Entity)> = Vec::new();
    for (link, remote, mut receiver, mut sender) in &mut links {
        for join in receiver.receive() {
            if join.protocol != PROTOCOL_VERSION {
                sender.send::<Control>(JoinReply::Refused { reason: "version mismatch, reload the page".into() });
                continue;
            }
            let player_id = player_id_from_uuid(&join.player_uuid);
            let peer = remote.0;
            let existing = players
                .iter()
                .find(|(_, p)| p.id == player_id)
                .map(|(e, _)| e)
                .or_else(|| spawned.iter().find(|(id, _)| *id == player_id).map(|(_, e)| *e));
            let entity = match existing {
                Some(e) => e,
                None => {
                    let slot = free_slot(&taken);
                    taken.push(slot);
                    let spawn = shared::bar::spawn_point(usize::from(slot));
                    let name = clean_name(&join.display_name);
                    let e = commands
                        .spawn((
                            Player { id: player_id, name, slot, cosmetic: join.cosmetic_id.min(MAX_HAT) },
                            PlayerPos(Vec3::from_array(spawn)),
                            PlayerYaw(0.0),
                        ))
                        .id();
                    spawned.push((player_id, e));
                    e
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

/// Mark players whose link is gone; remove them after the grace period.
/// A player who rejoins in time (same UUID) gets the same entity back.
fn track_disconnects(
    mut commands: Commands,
    tick: Res<crate::TickCount>,
    players: Query<(Entity, Option<&ControlledBy>, Option<&AwaitingReconnect>), (With<Player>, Without<LocalPlayer>)>,
    links: Query<(), (With<ClientOf>, With<Connected>)>,
) {
    let grace = (RECONNECT_GRACE_SECS * shared::TICK_HZ as f32) as u64;
    for (entity, controlled, waiting) in &players {
        let linked = controlled.is_some_and(|c| links.contains(c.owner));
        match (linked, waiting) {
            (false, None) => {
                commands.entity(entity).insert(AwaitingReconnect { since_tick: tick.0 });
            }
            (false, Some(w)) if tick.0.saturating_sub(w.since_tick) > grace => {
                info!("player entity {entity:?} did not reconnect in time; removing");
                commands.entity(entity).despawn();
            }
            (true, Some(_)) => {
                commands.entity(entity).remove::<AwaitingReconnect>();
            }
            _ => {}
        }
    }
}

/// The lowest slot not in `taken`.
pub fn free_slot(taken: &[u8]) -> u8 {
    (0..=u8::MAX).find(|s| !taken.contains(s)).unwrap_or(0)
}

/// Display names: printable, trimmed, at most 16 characters.
pub fn clean_name(raw: &str) -> String {
    let name: String = raw.chars().filter(|c| !c.is_control()).take(16).collect();
    let name = name.trim();
    if name.is_empty() { "Patron".into() } else { name.into() }
}

fn move_players(
    mut players: Query<(&mut PlayerPos, &mut PlayerYaw, &ActionState<PlayerInput>, Option<&Drunk>), With<Player>>,
) {
    for (mut pos, mut yaw, action, drunk) in &mut players {
        shared::net::apply_action(&mut pos, &mut yaw, action, drunk);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_fill_lowest_first() {
        assert_eq!(free_slot(&[]), 0);
        assert_eq!(free_slot(&[0, 1, 3]), 2);
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Rook\n "), "Rook");
        assert_eq!(clean_name(""), "Patron");
        assert_eq!(clean_name("abcdefghijklmnopqrstuvwxyz").chars().count(), 16);
    }
}
