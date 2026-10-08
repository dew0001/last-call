//! Native bot driver. Phase 0: boots a host simulation in-process, runs a
//! number of ticks, and checks a bot can encode a join request.
//! Real bot clients arrive with the netcode in Phase 1.

use host::HostSim;
use shared::protocol::{ClientMsg, decode, encode};

fn main() {
    let ticks: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(640);
    let mut sim = HostSim::new();
    for _ in 0..ticks {
        sim.tick();
    }
    let join = ClientMsg::JoinRoom {
        code: "BCDFG".into(),
        player_uuid: [1; 16],
        display_name: "bot-1".into(),
        cosmetic_id: 0,
    };
    let bytes = encode(&join);
    assert_eq!(decode::<ClientMsg>(&bytes).expect("join decodes"), join);
    println!("bots: host ran {} ticks, join message is {} bytes", sim.tick_count(), bytes.len());
}
