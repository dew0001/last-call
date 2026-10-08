//! Wire protocol between clients and the host simulation.
//!
//! Phase 0 holds the room handshake and tick report only. Gameplay messages
//! from section 3.3 of the plan are added in Phase 1 onward.

use serde::{Deserialize, Serialize};

/// Protocol version. Bump on any breaking change.
pub const PROTOCOL_VERSION: u16 = 1;

/// Messages a client sends to the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// Ask to join a room. `player_uuid` comes from `localStorage`, so a refreshed
    /// tab gets its old player back.
    JoinRoom { code: String, player_uuid: [u8; 16], display_name: String, cosmetic_id: u16 },
}

/// Messages the host sends to a client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HostMsg {
    /// Sent once after a successful join.
    Welcome { your_net_id: u32, tick: u64 },
    /// Periodic health report of the host simulation.
    TickReport { tick: u64, ticks_per_second: f32 },
}

fn config() -> impl bincode::config::Config {
    bincode::config::standard()
}

/// Encode a message to bytes.
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    bincode::serde::encode_to_vec(msg, config()).expect("protocol messages always encode")
}

/// Decode a message from bytes.
pub fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, bincode::error::DecodeError> {
    bincode::serde::decode_from_slice(bytes, config()).map(|(msg, _)| msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_client_msg() {
        let msg = ClientMsg::JoinRoom {
            code: "BCDFG".into(),
            player_uuid: [9; 16],
            display_name: "Rook".into(),
            cosmetic_id: 2,
        };
        assert_eq!(decode::<ClientMsg>(&encode(&msg)).unwrap(), msg);
    }

    #[test]
    fn round_trip_host_msg() {
        let msg = HostMsg::TickReport { tick: 640, ticks_per_second: 64.0 };
        assert_eq!(decode::<HostMsg>(&encode(&msg)).unwrap(), msg);
    }

    #[test]
    fn garbage_does_not_panic() {
        assert!(decode::<HostMsg>(&[0xff, 0xff, 0xff]).is_err());
    }
}
