//! Fixtures players use by standing next to them: the office drawer (Zeen),
//! the kitchen pass (food), the upgrade terminal, the Lucky Charm Shelf, the
//! jukebox, the basement breaker and each slot machine's service key. The
//! mop (for the inspector) is an ordinary prop that starts in the kitchen.

use serde::{Deserialize, Serialize};

use crate::buffs::Item;
use crate::upgrades::UpgradeId;
use crate::world;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Fixture {
    ZeenDrawer,
    KitchenPass,
    Shop,
    CharmShelf,
    Jukebox,
    Breaker,
    /// The service key on slot machine `n`.
    ServiceKey(u8),
}

/// One-shift items from the Lucky Charm Shelf.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Charm {
    /// Forces one roulette spin to land in the chosen dozen.
    RiggedDie,
    /// Shows the dealer's hole card at blackjack, for the shift.
    MarkedDeck,
    /// Clears 25 drunk and adds 20 Focus.
    ColdBrew,
}

impl Charm {
    pub fn price(self) -> i64 {
        match self {
            Charm::RiggedDie => 150,
            Charm::MarkedDeck => 200,
            Charm::ColdBrew => 10,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Charm::RiggedDie => "Rigged die",
            Charm::MarkedDeck => "Marked deck",
            Charm::ColdBrew => "Cold brew",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixtureAction {
    /// Buy and consume (drawer, kitchen pass).
    Buy(Item),
    /// Buy an upgrade rank (shop, Setup only).
    Upgrade(UpgradeId),
    /// Buy a one-shift item (charm shelf).
    Charm(Charm),
    /// Queue a track (jukebox, once bought).
    Track(u8),
    /// Flip the breaker or turn a service key.
    Use,
}

impl FixtureAction {
    /// A menu line: what it is and what it costs (`upgrades` for the next rank).
    pub fn label(self, upgrades: &crate::upgrades::Upgrades) -> String {
        match self {
            FixtureAction::Buy(i) => format!("{} ${}", i.label(), i.price()),
            FixtureAction::Upgrade(u) => match upgrades.next_cost(u) {
                Some(c) => format!("{} (rank {}/{}) ${c}", u.label(), upgrades.rank(u) + 1, u.max_rank()),
                None => format!("{} (max)", u.label()),
            },
            FixtureAction::Charm(c) => format!("{} ${}", c.label(), c.price()),
            FixtureAction::Track(n) => format!("Play \"{}\"", TRACKS[usize::from(n) % TRACKS.len()]),
            FixtureAction::Use => "Use".into(),
        }
    }
}

/// Jukebox tracks (generated loops, plan section 8).
pub const TRACKS: [&str; 5] = ["Last Orders", "Tide Out", "Neon Rain", "Loan Shark Blues", "Closing Time"];

/// The jukebox, by the bar's east wall: (x, z).
pub const JUKEBOX: (f32, f32) = (9.5, 5.5);
/// The Lucky Charm Shelf, by the front door: (x, z).
pub const CHARM_SHELF: (f32, f32) = (-3.0, 6.5);
/// Where the mop starts, in the kitchen: (x, z).
pub const MOP_CLOSET: (f32, f32) = (-9.4, -14.4);

impl Fixture {
    pub fn position(self) -> (f32, f32) {
        match self {
            Fixture::ZeenDrawer => world::ZEEN_DRAWER,
            Fixture::KitchenPass => world::KITCHEN_PASS,
            Fixture::Shop => world::SHOP,
            Fixture::CharmShelf => CHARM_SHELF,
            Fixture::Jukebox => JUKEBOX,
            Fixture::Breaker => world::BREAKER,
            Fixture::ServiceKey(n) => {
                // Behind the machine's side, toward the room.
                let (x, z) = crate::casino::SLOTS[usize::from(n) % crate::casino::SLOTS.len()];
                (x + 0.6, z - 0.6)
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Fixture::ZeenDrawer => "Office drawer",
            Fixture::KitchenPass => "Kitchen pass",
            Fixture::Shop => "Upgrade terminal",
            Fixture::CharmShelf => "Lucky Charm Shelf",
            Fixture::Jukebox => "Jukebox",
            Fixture::Breaker => "Breaker",
            Fixture::ServiceKey(_) => "Service key",
        }
    }

    pub fn all() -> Vec<Fixture> {
        let mut out = vec![
            Fixture::ZeenDrawer,
            Fixture::KitchenPass,
            Fixture::Shop,
            Fixture::CharmShelf,
            Fixture::Jukebox,
            Fixture::Breaker,
        ];
        out.extend((0..crate::casino::SLOT_MACHINES).map(Fixture::ServiceKey));
        out
    }

    /// What the fixture offers, in key order (key 1 is the first).
    pub fn menu(self) -> Vec<FixtureAction> {
        match self {
            Fixture::ZeenDrawer => vec![FixtureAction::Buy(Item::Zeen)],
            Fixture::KitchenPass => vec![FixtureAction::Buy(Item::Fries), FixtureAction::Buy(Item::Burger)],
            Fixture::Shop => crate::upgrades::ALL.iter().map(|u| FixtureAction::Upgrade(*u)).collect(),
            Fixture::CharmShelf => {
                [Charm::RiggedDie, Charm::MarkedDeck, Charm::ColdBrew].into_iter().map(FixtureAction::Charm).collect()
            }
            Fixture::Jukebox => (0..TRACKS.len() as u8).map(FixtureAction::Track).collect(),
            Fixture::Breaker | Fixture::ServiceKey(_) => vec![FixtureAction::Use],
        }
    }

    /// Can a player at (x, z) use it?
    pub fn in_reach(self, x: f32, z: f32) -> bool {
        world::near(self.position(), x, z)
    }

    /// The fixture a player at (x, z) is at, the nearest in reach.
    pub fn nearest(x: f32, z: f32) -> Option<Fixture> {
        Fixture::all().into_iter().filter(|f| f.in_reach(x, z)).min_by(|a, b| {
            let d = |f: &Fixture| {
                let (fx, fz) = f.position();
                (fx - x).hypot(fz - z)
            };
            d(a).total_cmp(&d(b))
        })
    }
}

/// The roulette result for a rigged spin into `dozen` (0 to 2).
pub fn rigged_spin(dozen: u8, d: &mut impl crate::rng::Draw) -> u8 {
    1 + 12 * (dozen % 3) + d.below(12) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;

    #[test]
    fn every_fixture_has_a_menu_that_fits_the_number_keys() {
        let u = crate::upgrades::Upgrades::default();
        for f in Fixture::all() {
            let menu = f.menu();
            assert!(!menu.is_empty() && menu.len() <= 10, "{f:?}");
            assert!(menu.iter().all(|a| !a.label(&u).is_empty()));
        }
        assert_eq!(FixtureAction::Buy(Item::Zeen).label(&u), "Zeen $3");
    }

    #[test]
    fn every_fixture_stands_in_a_room_clear_of_blocks() {
        for f in Fixture::all() {
            let (x, z) = f.position();
            assert!(world::room_at(x, z).is_some(), "{f:?} at {x},{z}");
            assert_eq!(Fixture::nearest(x, z), Some(f), "{f:?} is the nearest at its own spot");
        }
        assert_eq!(Fixture::nearest(0.0, 0.0), None);
    }

    #[test]
    fn a_rigged_die_lands_in_its_dozen() {
        let mut d = SimRng::new(5);
        for dozen in 0..3u8 {
            for _ in 0..200 {
                let n = rigged_spin(dozen, &mut d);
                assert!(crate::roulette::Bet::Dozen(dozen).covers(n), "{n} not in dozen {dozen}");
            }
        }
    }
}
