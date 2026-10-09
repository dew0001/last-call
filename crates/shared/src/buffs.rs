//! Consumables and buffs (plan section 4.6): Zeen and the Focus meter, The
//! Spins, and kitchen food. The drunk meter itself is in [`crate::drunk`].

use serde::{Deserialize, Serialize};

/// A Zeen pouch costs this much, from the office drawer.
pub const ZEEN_COST: i64 = 3;
/// Each pouch adds this to the Focus meter.
pub const FOCUS_PER_ZEEN: u8 = 35;
/// The Focus meter loses a point every this many ticks (1 per second).
pub const FOCUS_DECAY_TICKS: u32 = 64;
/// Buzzed: every 10 seconds, this chance of a gag that drops what is held.
pub const GAG_EVERY_SECS: u32 = 10;
pub const GAG_PERCENT: u32 = 20;
/// The Spins: drunk over this and Focus over [`SPINS_FOCUS`].
pub const SPINS_DRUNK: u8 = 40;
pub const SPINS_FOCUS: u8 = 60;
/// How long the camera rolls before the player vomits, seconds.
pub const SPINS_SECS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub enum FocusTier {
    #[default]
    None,
    /// 1 to 60: card count hint, steadier aim, a wider fishing gauge.
    Focus,
    /// 61 to 100: shaky hands, gags that drop what is held.
    Buzzed,
}

impl FocusTier {
    pub fn of(level: u8) -> Self {
        match level {
            0 => FocusTier::None,
            1..=60 => FocusTier::Focus,
            _ => FocusTier::Buzzed,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FocusTier::None => "none",
            FocusTier::Focus => "focus",
            FocusTier::Buzzed => "buzzed",
        }
    }
}

pub fn after_zeen(focus: u8) -> u8 {
    focus.saturating_add(FOCUS_PER_ZEEN).min(100)
}

/// Does this player get The Spins?
pub fn spins(drunk: u8, focus: u8) -> bool {
    drunk > SPINS_DRUNK && focus > SPINS_FOCUS
}

/// Does the card count hint show (blackjack)?
pub fn shows_count(focus: u8) -> bool {
    FocusTier::of(focus) >= FocusTier::Focus
}

/// Aim sway multiplier (fight pit): Focus halves it.
pub fn aim_sway(focus: u8) -> f32 {
    if FocusTier::of(focus) == FocusTier::Focus { 0.5 } else { 1.0 }
}

/// Fishing tension gauge width multiplier: Focus widens it.
pub fn tension_width(focus: u8) -> f32 {
    if FocusTier::of(focus) == FocusTier::Focus { 1.5 } else { 1.0 }
}

/// Screen jitter in pixels for the HUD when Buzzed (client effect).
pub fn ui_jitter(focus: u8) -> f32 {
    if FocusTier::of(focus) == FocusTier::Buzzed { 2.0 + f32::from(focus - 60) / 10.0 } else { 0.0 }
}

/// Things a player can consume.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Item {
    Zeen,
    /// Kitchen: clears 15 drunk.
    Fries,
    /// Kitchen: clears 30 drunk, +10% max health in the fight pit this shift.
    Burger,
    /// Kitchen, from a caught fish: +10 pocket on sale, one roulette reroll.
    FishPlate,
}

impl Item {
    pub fn price(self) -> i64 {
        match self {
            Item::Zeen => ZEEN_COST,
            Item::Fries => 4,
            Item::Burger => 8,
            Item::FishPlate => 12,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Item::Zeen => "Zeen",
            Item::Fries => "Fries",
            Item::Burger => "Burger",
            Item::FishPlate => "Fish plate",
        }
    }
}

/// A player's buff state, as the rules see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Buffs {
    pub drunk: u8,
    pub focus: u8,
    /// Burger: +10% max health in the fight pit this shift.
    pub well_fed: bool,
    /// Fish plate: one reroll of the next losing roulette spin.
    pub lucky: bool,
}

/// Consume an item (plan: `buffs::apply(state, item) -> state`).
pub fn apply(mut b: Buffs, item: Item) -> Buffs {
    match item {
        Item::Zeen => b.focus = after_zeen(b.focus),
        Item::Fries => b.drunk = b.drunk.saturating_sub(15),
        Item::Burger => {
            b.drunk = b.drunk.saturating_sub(30);
            b.well_fed = true;
        }
        Item::FishPlate => b.lucky = true,
    }
    b
}

/// What The Spins leave: both meters cleared.
pub fn after_spins(mut b: Buffs) -> Buffs {
    b.drunk = 0;
    b.focus = 0;
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_tiers() {
        assert_eq!(FocusTier::of(0), FocusTier::None);
        assert_eq!(FocusTier::of(1), FocusTier::Focus);
        assert_eq!(FocusTier::of(60), FocusTier::Focus);
        assert_eq!(FocusTier::of(61), FocusTier::Buzzed);
        assert_eq!(after_zeen(0), 35);
        assert_eq!(after_zeen(35), 70);
        assert_eq!(after_zeen(90), 100);
    }

    #[test]
    fn focus_effects() {
        assert!(!shows_count(0) && shows_count(35) && shows_count(70));
        assert_eq!(aim_sway(35), 0.5);
        assert_eq!(aim_sway(70), 1.0);
        assert_eq!(tension_width(35), 1.5);
        assert_eq!(ui_jitter(35), 0.0);
        assert!(ui_jitter(70) > 2.0);
    }

    #[test]
    fn the_spins_need_both_meters_high() {
        assert!(!spins(40, 70), "drunk must be over 40");
        assert!(!spins(50, 60), "focus must be over 60");
        assert!(spins(41, 61));
        assert_eq!(after_spins(Buffs { drunk: 60, focus: 70, ..Default::default() }), Buffs::default());
    }

    #[test]
    fn food() {
        let b = Buffs { drunk: 50, ..Default::default() };
        assert_eq!(apply(b, Item::Fries).drunk, 35);
        let burger = apply(b, Item::Burger);
        assert_eq!(burger.drunk, 20);
        assert!(burger.well_fed);
        assert_eq!(apply(Buffs::default(), Item::Fries).drunk, 0, "no underflow");
        assert!(apply(b, Item::FishPlate).lucky);
        assert_eq!(apply(b, Item::Zeen).focus, 35);
    }
}
