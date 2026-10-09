//! A hash of the replicated game state, for checking that clients agree with
//! the host (plan section 11). It covers the discrete state every client
//! receives as is: players' money and drunk meters, the shift clock and
//! ledger, customers, glasses and puddles. Positions are left out: clients
//! predict or interpolate them, so they differ from the host's on purpose.

use bevy::prelude::*;

use crate::protocol::*;

struct Fnv(u64);

impl Fnv {
    fn bytes(&mut self, b: &[u8]) {
        for x in b {
            self.0 = (self.0 ^ u64::from(*x)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn i64(&mut self, v: i64) {
        self.bytes(&v.to_le_bytes());
    }
}

/// Hash the replicated game state in `world` (a host's or a client's).
pub fn replicated_hash(world: &mut World) -> u64 {
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    let mut players: Vec<(u64, i64, u8, bool)> = world
        .query::<(&Player, Option<&Pocket>, Option<&Drunk>)>()
        .iter(world)
        .map(|(p, m, d)| (p.id, m.map_or(0, |m| m.0), d.map_or(0, |d| d.level), d.is_some_and(|d| d.passed_out)))
        .collect();
    players.sort_unstable();
    players.dedup();
    for (id, pocket, level, out) in players {
        h.i64(id as i64);
        h.i64(pocket);
        h.bytes(&[level, u8::from(out)]);
    }
    for (clock, run) in world.query::<(&ShiftClock, Option<&RunLedger>)>().iter(world) {
        h.bytes(&[clock.calendar.week, clock.calendar.shift, clock.phase as u8, u8::from(clock.running)]);
        h.i64(i64::from(clock.seconds_left));
        if let Some(run) = run {
            let l = run.ledger;
            for v in [l.house, l.paid, l.carried, run.due] {
                h.i64(v);
            }
            h.bytes(&[l.missed_in_a_row, l.ng, run.outcome as u8]);
        }
    }
    let mut customers: Vec<(u32, u8, u8)> =
        world.query::<&Customer>().iter(world).map(|c| (c.id, c.mood as u8, c.patience)).collect();
    customers.sort_unstable();
    customers.dedup();
    for (id, mood, patience) in customers {
        h.i64(i64::from(id));
        h.bytes(&[mood, patience]);
    }
    let mut glasses: Vec<(u64, u8, bool, u64)> = world
        .query::<(&Beer, &HeldBy)>()
        .iter(world)
        .map(|(b, held)| (b.poured_by, b.fill, b.perfect, held.0.unwrap_or(0)))
        .collect();
    glasses.sort_unstable();
    for (by, fill, perfect, held) in glasses {
        h.i64(by as i64);
        h.bytes(&[fill, u8::from(perfect)]);
        h.i64(held as i64);
    }
    let puddles = world.query::<&Puddle>().iter(world).count();
    h.i64(puddles as i64);
    h.0
}
