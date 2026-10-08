//! Star-topology signaling for one room, speaking the matchbox JSON protocol.
//!
//! Pure logic with no Worker types, so it is unit-tested natively. The host is
//! the offerer for every player. Players never learn about each other: game
//! traffic is a star through the host. (Voice uses its own mesh signaling.)

use serde_json::{Value, json};

/// Most players in a room besides the host.
pub const MAX_GUESTS: usize = 7;

/// Close codes sent to a socket that is refused or dropped.
pub mod close {
    pub const HOST_LEFT: u16 = 4000;
    pub const NO_HOST: u16 = 4404;
    pub const CODE_IN_USE: u16 = 4409;
    pub const ROOM_ENDED: u16 = 4410;
    pub const ROOM_FULL: u16 = 4429;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Host,
    Player,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Host => "host",
            Role::Player => "player",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "host" => Some(Role::Host),
            "player" => Some(Role::Player),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub id: String,
    pub role: Role,
}

/// Something the Durable Object must do on a socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    Send { to: String, text: String },
    Close { to: String, code: u16, reason: &'static str },
}

/// Why a connection was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: u16,
    pub reason: &'static str,
}

fn host(peers: &[Peer]) -> Option<&Peer> {
    peers.iter().find(|p| p.role == Role::Host)
}

/// Check whether a new connection may join. `peers` are the connections
/// already in the room. `ended` is true after the host left.
pub fn admit(peers: &[Peer], role: Role, ended: bool) -> Result<(), Refusal> {
    if ended {
        return Err(Refusal { code: close::ROOM_ENDED, reason: "host left" });
    }
    match role {
        Role::Host if host(peers).is_some() => Err(Refusal { code: close::CODE_IN_USE, reason: "room code in use" }),
        Role::Host => Ok(()),
        Role::Player if host(peers).is_none() => Err(Refusal { code: close::NO_HOST, reason: "no such room" }),
        Role::Player if peers.iter().filter(|p| p.role == Role::Player).count() >= MAX_GUESTS => {
            Err(Refusal { code: close::ROOM_FULL, reason: "room full" })
        }
        Role::Player => Ok(()),
    }
}

/// Messages after `new` was admitted. `peers` excludes `new`.
pub fn on_join(peers: &[Peer], new: &Peer) -> Vec<Out> {
    let mut out = vec![Out::Send { to: new.id.clone(), text: json!({ "IdAssigned": new.id }).to_string() }];
    if new.role == Role::Player
        && let Some(h) = host(peers)
    {
        out.push(Out::Send { to: h.id.clone(), text: json!({ "NewPeer": new.id }).to_string() });
    }
    out
}

/// Relay a request from `from`. Only host-to-player and player-to-host
/// signals pass; everything else is dropped.
pub fn on_message(peers: &[Peer], from: &Peer, text: &str) -> Vec<Out> {
    let Ok(msg) = serde_json::from_str::<Value>(text) else { return vec![] };
    let Some(signal) = msg.get("Signal") else { return vec![] };
    let (Some(receiver), Some(data)) = (signal.get("receiver").and_then(Value::as_str), signal.get("data")) else {
        return vec![];
    };
    let Some(to) = peers.iter().find(|p| p.id == receiver) else { return vec![] };
    if to.role == from.role {
        return vec![];
    }
    vec![Out::Send { to: to.id.clone(), text: json!({ "Signal": { "sender": from.id, "data": data } }).to_string() }]
}

/// Messages after `left` disconnected. `peers` excludes `left`.
pub fn on_leave(peers: &[Peer], left: &Peer) -> Vec<Out> {
    let left_msg = json!({ "PeerLeft": left.id }).to_string();
    match left.role {
        Role::Host => peers
            .iter()
            .flat_map(|p| {
                [
                    Out::Send { to: p.id.clone(), text: left_msg.clone() },
                    Out::Close { to: p.id.clone(), code: close::HOST_LEFT, reason: "host left" },
                ]
            })
            .collect(),
        Role::Player => host(peers).map(|h| Out::Send { to: h.id.clone(), text: left_msg }).into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str, role: Role) -> Peer {
        Peer { id: id.into(), role }
    }

    #[test]
    fn player_needs_a_host() {
        assert_eq!(admit(&[], Role::Player, false).unwrap_err().code, close::NO_HOST);
        assert!(admit(&[peer("h", Role::Host)], Role::Player, false).is_ok());
    }

    #[test]
    fn second_host_is_refused() {
        assert_eq!(admit(&[peer("h", Role::Host)], Role::Host, false).unwrap_err().code, close::CODE_IN_USE);
    }

    #[test]
    fn ended_room_refuses_everyone() {
        assert_eq!(admit(&[], Role::Host, true).unwrap_err().code, close::ROOM_ENDED);
    }

    #[test]
    fn room_caps_at_eight() {
        let mut peers = vec![peer("h", Role::Host)];
        for i in 0..MAX_GUESTS {
            assert!(admit(&peers, Role::Player, false).is_ok());
            peers.push(peer(&format!("p{i}"), Role::Player));
        }
        assert_eq!(admit(&peers, Role::Player, false).unwrap_err().code, close::ROOM_FULL);
    }

    #[test]
    fn join_tells_only_the_host() {
        let peers = vec![peer("h", Role::Host), peer("p1", Role::Player)];
        let out = on_join(&peers, &peer("p2", Role::Player));
        assert_eq!(
            out,
            vec![
                Out::Send { to: "p2".into(), text: r#"{"IdAssigned":"p2"}"#.into() },
                Out::Send { to: "h".into(), text: r#"{"NewPeer":"p2"}"#.into() },
            ]
        );
    }

    #[test]
    fn signals_pass_between_host_and_player_only() {
        let peers = vec![peer("h", Role::Host), peer("p1", Role::Player), peer("p2", Role::Player)];
        let msg = r#"{"Signal":{"receiver":"h","data":{"Offer":"sdp"}}}"#;
        let out = on_message(&peers, &peers[1], msg);
        assert_eq!(
            out,
            vec![Out::Send { to: "h".into(), text: r#"{"Signal":{"data":{"Offer":"sdp"},"sender":"p1"}}"#.into() }]
        );
        let p2p = r#"{"Signal":{"receiver":"p2","data":1}}"#;
        assert!(on_message(&peers, &peers[1], p2p).is_empty());
        assert!(on_message(&peers, &peers[1], "\"KeepAlive\"").is_empty());
        assert!(on_message(&peers, &peers[1], "not json").is_empty());
    }

    #[test]
    fn host_leaving_closes_every_player() {
        let peers = vec![peer("p1", Role::Player), peer("p2", Role::Player)];
        let out = on_leave(&peers, &peer("h", Role::Host));
        assert_eq!(out.len(), 4);
        assert!(out.contains(&Out::Close { to: "p2".into(), code: close::HOST_LEFT, reason: "host left" }));
    }

    #[test]
    fn player_leaving_tells_host() {
        let peers = vec![peer("h", Role::Host)];
        assert_eq!(
            on_leave(&peers, &peer("p1", Role::Player)),
            vec![Out::Send { to: "h".into(), text: r#"{"PeerLeft":"p1"}"#.into() }]
        );
    }
}
