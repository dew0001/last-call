//! Upgrades (plan section 4.5): bought from the house pool during Setup at
//! the office terminal. Each has ranks with a cost and an effect. They last
//! for the run.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum UpgradeId {
    /// Table max bet x2 per rank; customers with more cash sit down.
    Felt,
    /// More beer types, faster pour.
    TapWall,
    /// Shows cheating customers; catching one earns a bonus.
    SecurityCamera,
    /// Brawls end 50% sooner per rank.
    Bouncer,
    /// Customers stay 30% longer; players queue tracks.
    Jukebox,
    /// Sells one-shift items: rigged die, marked deck, cold brew.
    CharmShelf,
    /// Power outages last 10 s instead of 60 s.
    Generator,
    /// A player can put out a kitchen fire.
    Extinguisher,
    /// +1 customer per wave per rank.
    NeonSign,
    /// During a raid, cash can go out the back.
    BackDoor,
}

pub const ALL: [UpgradeId; 10] = [
    UpgradeId::Felt,
    UpgradeId::TapWall,
    UpgradeId::SecurityCamera,
    UpgradeId::Bouncer,
    UpgradeId::Jukebox,
    UpgradeId::CharmShelf,
    UpgradeId::Generator,
    UpgradeId::Extinguisher,
    UpgradeId::NeonSign,
    UpgradeId::BackDoor,
];

impl UpgradeId {
    /// Cost of each rank, in order.
    pub fn costs(self) -> &'static [i64] {
        match self {
            UpgradeId::Felt => &[1_500, 3_000, 6_000],
            UpgradeId::TapWall => &[1_000, 2_000, 4_000],
            UpgradeId::SecurityCamera => &[2_500],
            UpgradeId::Bouncer => &[4_000, 8_000],
            UpgradeId::Jukebox => &[1_200],
            UpgradeId::CharmShelf => &[2_000],
            UpgradeId::Generator => &[3_000],
            UpgradeId::Extinguisher => &[800],
            UpgradeId::NeonSign => &[2_000, 2_000, 2_000],
            UpgradeId::BackDoor => &[5_000],
        }
    }

    pub fn max_rank(self) -> u8 {
        self.costs().len() as u8
    }

    pub fn index(self) -> usize {
        ALL.iter().position(|u| *u == self).expect("listed")
    }

    pub fn label(self) -> &'static str {
        match self {
            UpgradeId::Felt => "Felt Upgrade",
            UpgradeId::TapWall => "Bigger Tap Wall",
            UpgradeId::SecurityCamera => "Security Camera",
            UpgradeId::Bouncer => "Bouncer",
            UpgradeId::Jukebox => "Jukebox",
            UpgradeId::CharmShelf => "Lucky Charm Shelf",
            UpgradeId::Generator => "Generator",
            UpgradeId::Extinguisher => "Fire Extinguisher",
            UpgradeId::NeonSign => "Neon Sign",
            UpgradeId::BackDoor => "Back Door",
        }
    }
}

/// The ranks owned, by [`UpgradeId::index`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Upgrades(pub [u8; 10]);

impl Upgrades {
    pub fn rank(&self, id: UpgradeId) -> u8 {
        self.0[id.index()]
    }

    pub fn has(&self, id: UpgradeId) -> bool {
        self.rank(id) > 0
    }

    /// The price of the next rank, or `None` at the maximum.
    pub fn next_cost(&self, id: UpgradeId) -> Option<i64> {
        id.costs().get(usize::from(self.rank(id))).copied()
    }

    /// Buy the next rank from `house`. Returns the price paid.
    pub fn buy(&mut self, id: UpgradeId, house: &mut i64) -> Result<i64, &'static str> {
        let cost = self.next_cost(id).ok_or("already at the top rank")?;
        if *house < cost {
            return Err("the house pool cannot cover it");
        }
        *house -= cost;
        self.0[id.index()] += 1;
        Ok(cost)
    }

    // ---------- Effects ----------

    /// Table maximum bets: x2 per Felt rank.
    pub fn table_max(&self, base: i64) -> i64 {
        base << self.rank(UpgradeId::Felt)
    }

    /// Customers' starting cash: +50% per Felt rank (richer gamblers sit down).
    pub fn customer_cash(&self, cash: i64) -> i64 {
        cash * (2 + i64::from(self.rank(UpgradeId::Felt))) / 2
    }

    /// Pour speed multiplier: +25% per Tap Wall rank.
    pub fn pour_speed(&self) -> f32 {
        1.0 + 0.25 * f32::from(self.rank(UpgradeId::TapWall))
    }

    /// Beers on tap (names in [`BEERS`]).
    pub fn beer_types(&self) -> usize {
        1 + usize::from(self.rank(UpgradeId::TapWall)).min(2)
    }

    /// Brawl duration multiplier: halves per Bouncer rank.
    pub fn brawl_duration(&self, secs: u32) -> u32 {
        secs >> self.rank(UpgradeId::Bouncer)
    }

    /// Customer patience multiplier, percent: the Jukebox adds 30%.
    pub fn patience_percent(&self) -> u32 {
        if self.has(UpgradeId::Jukebox) { 130 } else { 100 }
    }

    /// Power outage length, seconds.
    pub fn outage_secs(&self) -> u32 {
        if self.has(UpgradeId::Generator) { 10 } else { 60 }
    }

    /// Extra customers per wave.
    pub fn extra_customers(&self) -> u32 {
        u32::from(self.rank(UpgradeId::NeonSign))
    }
}

/// The house brews (invented brands, plan section 13).
pub const BEERS: [&str; 3] = ["Pier Light", "Dock Stout", "The Boot"];

/// Bonus for catching a cheating customer with the Security Camera.
pub const CATCH_BONUS: i64 = 500;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_costs_and_ranks() {
        assert_eq!(UpgradeId::Felt.costs(), &[1_500, 3_000, 6_000]);
        assert_eq!(UpgradeId::NeonSign.max_rank(), 3);
        assert_eq!(UpgradeId::Extinguisher.costs(), &[800]);
        let total: i64 = ALL.iter().flat_map(|u| u.costs()).sum();
        assert_eq!(total, 10_500 + 7_000 + 2_500 + 12_000 + 1_200 + 2_000 + 3_000 + 800 + 6_000 + 5_000);
    }

    #[test]
    fn buying_ranks_from_the_house() {
        let mut u = Upgrades::default();
        let mut house = 2_000;
        assert_eq!(u.buy(UpgradeId::Felt, &mut house), Ok(1_500));
        assert_eq!(house, 500);
        assert!(u.buy(UpgradeId::Felt, &mut house).is_err(), "rank 2 costs 3,000");
        let mut house = 100_000;
        u.buy(UpgradeId::Felt, &mut house).unwrap();
        u.buy(UpgradeId::Felt, &mut house).unwrap();
        assert_eq!(u.rank(UpgradeId::Felt), 3);
        assert!(u.buy(UpgradeId::Felt, &mut house).is_err(), "max rank");
    }

    #[test]
    fn effects() {
        let mut u = Upgrades::default();
        assert_eq!(u.table_max(100), 100);
        assert_eq!(u.outage_secs(), 60);
        assert_eq!(u.brawl_duration(45), 45);
        let mut house = 100_000;
        for id in ALL {
            while u.next_cost(id).is_some() {
                u.buy(id, &mut house).unwrap();
            }
        }
        assert_eq!(u.table_max(100), 800);
        assert_eq!(u.customer_cash(100), 250);
        assert_eq!(u.pour_speed(), 1.75);
        assert_eq!(u.beer_types(), 3);
        assert_eq!(u.brawl_duration(45), 11);
        assert_eq!(u.patience_percent(), 130);
        assert_eq!(u.outage_secs(), 10);
        assert_eq!(u.extra_customers(), 3);
    }
}
