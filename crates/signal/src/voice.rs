//! Full-mesh signaling for proximity voice.
//!
//! Every voice peer connects to every other (plan section 8). A peer names
//! itself with its game player id, so the page can match voices to player
//! positions. On join, every peer already in the room gets `NewPeer` and makes
//! the offer. Same matchbox JSON shapes as the game relay.

use serde_json::{Value, json};

use crate::relay::Out;

/// Voice peers per room (one per player).
pub const MAX_VOICE: usize = 8;

/// A voice peer id: 16 lowercase hex digits (the game player id).
pub fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Who must hear about a new voice peer. `peers` excludes `new`.
pub fn on_join(peers: &[String], new: &str) -> Vec<Out> {
    let mut out = vec![Out::Send { to: new.to_string(), text: json!({ "IdAssigned": new }).to_string() }];
    for p in peers {
        out.push(Out::Send { to: p.clone(), text: json!({ "NewPeer": new }).to_string() });
    }
    out
}

/// Relay a signal to any other voice peer in the room.
pub fn on_message(peers: &[String], from: &str, text: &str) -> Vec<Out> {
    let Ok(msg) = serde_json::from_str::<Value>(text) else { return vec![] };
    let Some(signal) = msg.get("Signal") else { return vec![] };
    let (Some(receiver), Some(data)) = (signal.get("receiver").and_then(Value::as_str), signal.get("data")) else {
        return vec![];
    };
    if receiver == from || !peers.iter().any(|p| p == receiver) {
        return vec![];
    }
    vec![Out::Send {
        to: receiver.to_string(),
        text: json!({ "Signal": { "sender": from, "data": data } }).to_string(),
    }]
}

/// Tell everyone else a voice peer left. `peers` excludes `left`.
pub fn on_leave(peers: &[String], left: &str) -> Vec<Out> {
    let text = json!({ "PeerLeft": left }).to_string();
    peers.iter().map(|p| Out::Send { to: p.clone(), text: text.clone() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "00000000000000aa";
    const B: &str = "00000000000000bb";
    const C: &str = "00000000000000cc";

    #[test]
    fn ids_are_player_ids() {
        assert!(valid_id(A));
        assert!(!valid_id("short"));
        assert!(!valid_id("00000000000000AA"));
    }

    #[test]
    fn existing_peers_offer_to_the_newcomer() {
        let out = on_join(&[A.into(), B.into()], C);
        assert_eq!(out.len(), 3);
        assert!(out.contains(&Out::Send { to: A.into(), text: format!(r#"{{"NewPeer":"{C}"}}"#) }));
    }

    #[test]
    fn signals_reach_any_peer_but_not_self() {
        let peers = [A.to_string(), B.to_string()];
        let msg = format!(r#"{{"Signal":{{"receiver":"{B}","data":1}}}}"#);
        assert_eq!(on_message(&peers, A, &msg).len(), 1);
        let to_self = format!(r#"{{"Signal":{{"receiver":"{A}","data":1}}}}"#);
        assert!(on_message(&peers, A, &to_self).is_empty());
    }

    #[test]
    fn leaving_tells_everyone() {
        assert_eq!(on_leave(&[A.into(), B.into()], C).len(), 2);
    }
}
