//! Shift timing (plan section 4.3). Pure data and rules: the host drives the
//! clock, clients only display it.
//!
//! A run is made of weeks, a week is [`SHIFTS_PER_WEEK`] shifts, and a shift
//! runs Setup, Open, Last call and Payment in that order.

use serde::{Deserialize, Serialize};

/// Shifts in one week. The loan shark collects at the end of the last one.
pub const SHIFTS_PER_WEEK: u8 = 3;

/// Weeks in a run (the payment schedule has six entries).
pub const WEEKS_PER_RUN: u8 = 6;

/// One phase of a shift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ShiftPhase {
    /// Buy upgrades, stock the bar, place props.
    #[default]
    Setup,
    /// Customers arrive in waves; games run; chaos fires.
    Open,
    /// Customers leave; players count cash and settle up.
    LastCall,
    /// The loan shark's cut is due (last shift of the week only).
    Payment,
}

impl ShiftPhase {
    /// The phase after this one, or `None` after Payment (the shift is over).
    pub fn next(self) -> Option<Self> {
        match self {
            Self::Setup => Some(Self::Open),
            Self::Open => Some(Self::LastCall),
            Self::LastCall => Some(Self::Payment),
            Self::Payment => None,
        }
    }

    /// Name shown on the clock.
    pub fn label(self) -> &'static str {
        match self {
            Self::Setup => "SETUP",
            Self::Open => "OPEN",
            Self::LastCall => "LAST CALL",
            Self::Payment => "PAYMENT",
        }
    }
}

/// Phase lengths in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timings {
    pub setup: u32,
    pub open: u32,
    pub last_call: u32,
    pub payment: u32,
    /// How long the win or loss screen shows before the next run starts.
    pub outcome: u32,
}

impl Timings {
    /// The plan's timings: 2 + 9 + 2 + 1 minutes = a 14-minute shift.
    pub const PLAN: Self = Self { setup: 120, open: 540, last_call: 120, payment: 60, outcome: 30 };

    /// Length of one phase.
    pub fn seconds(&self, phase: ShiftPhase) -> u32 {
        match phase {
            ShiftPhase::Setup => self.setup,
            ShiftPhase::Open => self.open,
            ShiftPhase::LastCall => self.last_call,
            ShiftPhase::Payment => self.payment,
        }
    }

    /// Length of a whole shift.
    pub fn shift_seconds(&self) -> u32 {
        self.setup + self.open + self.last_call + self.payment
    }

    /// Every phase divided by `divisor`, at least one second each. Tests and
    /// `?fast` rooms use it to run a shift in seconds.
    pub fn scaled_down(self, divisor: u32) -> Self {
        let d = divisor.max(1);
        let s = |v: u32| (v / d).max(1);
        Self {
            setup: s(self.setup),
            open: s(self.open),
            last_call: s(self.last_call),
            payment: s(self.payment),
            outcome: s(self.outcome),
        }
    }
}

impl Default for Timings {
    fn default() -> Self {
        Self::PLAN
    }
}

/// Where the run is: week 1..=6, shift 0..3 within the week.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Calendar {
    pub week: u8,
    pub shift: u8,
}

impl Default for Calendar {
    fn default() -> Self {
        Self { week: 1, shift: 0 }
    }
}

impl Calendar {
    /// The loan shark collects at the end of this shift.
    pub fn is_payment_shift(&self) -> bool {
        self.shift + 1 == SHIFTS_PER_WEEK
    }

    /// The next shift (the week rolls over after the payment shift).
    pub fn next_shift(self) -> Self {
        if self.is_payment_shift() {
            Self { week: self.week.saturating_add(1), shift: 0 }
        } else {
            Self { week: self.week, shift: self.shift + 1 }
        }
    }
}

/// Format seconds as `m:ss` for the clock.
pub fn clock_text(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_shift_is_fourteen_minutes() {
        assert_eq!(Timings::PLAN.shift_seconds(), 14 * 60);
    }

    #[test]
    fn phases_run_in_order_and_end_after_payment() {
        let mut p = ShiftPhase::Setup;
        let mut seen = vec![p];
        while let Some(n) = p.next() {
            seen.push(n);
            p = n;
        }
        assert_eq!(seen, [ShiftPhase::Setup, ShiftPhase::Open, ShiftPhase::LastCall, ShiftPhase::Payment]);
    }

    #[test]
    fn scaled_timings_keep_every_phase() {
        let t = Timings::PLAN.scaled_down(60);
        assert_eq!(t, Timings { setup: 2, open: 9, last_call: 2, payment: 1, outcome: 1 });
        let tiny = Timings::PLAN.scaled_down(10_000);
        assert!([tiny.setup, tiny.open, tiny.last_call, tiny.payment, tiny.outcome].iter().all(|&s| s == 1));
        assert_eq!(Timings::PLAN.scaled_down(0), Timings::PLAN);
    }

    #[test]
    fn calendar_rolls_weeks_after_three_shifts() {
        let mut c = Calendar::default();
        let mut payments = 0;
        for _ in 0..(3 * 6) {
            if c.is_payment_shift() {
                payments += 1;
            }
            c = c.next_shift();
        }
        assert_eq!(payments, 6);
        assert_eq!(c, Calendar { week: 7, shift: 0 });
    }

    #[test]
    fn clock_text_pads_seconds() {
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(65), "1:05");
        assert_eq!(clock_text(540), "9:00");
    }
}
