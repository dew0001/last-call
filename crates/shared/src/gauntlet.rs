//! The football gauntlet in the parking lot (plan section 5.8).
//!
//! One runner with the ball, 3 to 6 NPC tacklers (more each week) in a lane,
//! 20 seconds. The runner moves like any player; a dodge (a double-tapped
//! direction) jumps 1.5 m sideways with a 1-second cooldown, and a stiff arm
//! shoves the nearest tackler back and stuns him for a second. A tackler
//! that touches the runner brings him down. Reach the end zone to double the
//! entry fee.

use serde::{Deserialize, Serialize};

use crate::rng::Draw;

/// The lane, on the east side of the parking lot: x from 3 to 13, from
/// z = 25 (start) to z = 9 (end zone).
pub const LANE_X: (f32, f32) = (3.0, 13.0);
/// The middle of the lane.
pub const LANE_MID: f32 = (LANE_X.0 + LANE_X.1) / 2.0;
pub const START_Z: f32 = 25.0;
pub const END_Z: f32 = 9.0;
pub const SECONDS: u32 = 20;
/// Tackler speed, m/s (a sprinting player makes about 6).
pub const TACKLER_SPEED: f32 = 4.0;
pub const TACKLE_REACH: f32 = 0.6;
pub const DODGE: f32 = 1.5;
pub const DODGE_COOLDOWN_TICKS: u32 = crate::TICK_HZ;
pub const STIFF_ARM_REACH: f32 = 1.5;
pub const STIFF_ARM_PUSH: f32 = 2.0;
pub const STUN_TICKS: u32 = crate::TICK_HZ;
pub const ENTRY: i64 = 20;

pub fn tacklers_for_week(week: u8) -> usize {
    (3 + usize::from(week.saturating_sub(1)) * 3 / 5).min(6)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tackler {
    pub pos: [f32; 2],
    pub stunned: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunEnd {
    Scored,
    Tackled,
    TimeUp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub tacklers: Vec<Tackler>,
    pub ticks_left: u32,
    pub dodge_cooldown: u32,
}

impl Run {
    /// Tacklers spread across the lane between the runner and the end zone.
    pub fn start(week: u8, d: &mut impl Draw) -> Self {
        let n = tacklers_for_week(week);
        let tacklers = (0..n)
            .map(|i| {
                let z = END_Z + 3.0 + (START_Z - END_Z - 8.0) * i as f32 / n as f32;
                let x = LANE_X.0 + 1.0 + d.below(1001) as f32 / 1000.0 * (LANE_X.1 - LANE_X.0 - 2.0);
                Tackler { pos: [x, z], stunned: 0 }
            })
            .collect();
        Self { tacklers, ticks_left: SECONDS * crate::TICK_HZ, dodge_cooldown: 0 }
    }

    /// One tick with the runner at `runner`.
    pub fn step(&mut self, runner: [f32; 2], speed_scale: f32) -> Option<RunEnd> {
        if runner[1] <= END_Z {
            return Some(RunEnd::Scored);
        }
        let dt = 1.0 / crate::TICK_HZ as f32;
        for t in &mut self.tacklers {
            if t.stunned > 0 {
                t.stunned -= 1;
                continue;
            }
            let (dx, dz) = (runner[0] - t.pos[0], runner[1] - t.pos[1]);
            let d = (dx * dx + dz * dz).sqrt();
            if d <= TACKLE_REACH {
                return Some(RunEnd::Tackled);
            }
            let step = (TACKLER_SPEED * speed_scale * dt).min(d);
            t.pos = [t.pos[0] + dx / d * step, t.pos[1] + dz / d * step];
        }
        self.dodge_cooldown = self.dodge_cooldown.saturating_sub(1);
        self.ticks_left = self.ticks_left.saturating_sub(1);
        if self.ticks_left == 0 {
            return Some(RunEnd::TimeUp);
        }
        None
    }

    /// A dodge to the left (-1) or right (1): where the runner lands, if allowed.
    pub fn dodge(&mut self, runner: [f32; 2], side: f32) -> Option<[f32; 2]> {
        if self.dodge_cooldown > 0 {
            return None;
        }
        self.dodge_cooldown = DODGE_COOLDOWN_TICKS;
        let x = (runner[0] + side.signum() * DODGE).clamp(LANE_X.0, LANE_X.1);
        Some([x, runner[1]])
    }

    /// Shove the nearest tackler in reach back and stun him.
    pub fn stiff_arm(&mut self, runner: [f32; 2]) -> bool {
        let near = self
            .tacklers
            .iter_mut()
            .map(|t| {
                let d = ((t.pos[0] - runner[0]).powi(2) + (t.pos[1] - runner[1]).powi(2)).sqrt();
                (t, d)
            })
            .filter(|(_, d)| *d < STIFF_ARM_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((t, d)) = near else { return false };
        let (dx, dz) = (t.pos[0] - runner[0], t.pos[1] - runner[1]);
        let d = d.max(0.01);
        t.pos = [t.pos[0] + dx / d * STIFF_ARM_PUSH, t.pos[1] + dz / d * STIFF_ARM_PUSH];
        t.stunned = STUN_TICKS;
        true
    }
}

/// A scripted runner for bots and tests: run for the end zone, stepping
/// away from the closest tackler ahead, dodging and stiff-arming when close.
pub fn bot_run_dir(run: &Run, at: [f32; 2]) -> [f32; 2] {
    let ahead = run
        .tacklers
        .iter()
        .filter(|t| t.pos[1] < at[1] + 0.5 && t.stunned == 0)
        .min_by(|a, b| (at[1] - a.pos[1]).total_cmp(&(at[1] - b.pos[1])));
    let mut dir = [0.0f32, -1.0];
    if let Some(t) = ahead {
        let dx = at[0] - t.pos[0];
        if (at[1] - t.pos[1]) < 4.0 {
            dir[0] = if dx.abs() < 0.01 { 1.0 } else { dx.signum() } * 1.2;
        }
    }
    if at[0] < LANE_X.0 + 1.0 {
        dir[0] = 1.0;
    } else if at[0] > LANE_X.1 - 1.0 {
        dir[0] = -1.0;
    }
    let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    [dir[0] / len, dir[1] / len]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;

    #[test]
    fn more_tacklers_each_week() {
        assert_eq!(tacklers_for_week(1), 3);
        assert_eq!(tacklers_for_week(6), 6);
    }

    #[test]
    fn standing_still_gets_you_tackled_and_running_wide_open_scores() {
        let mut d = SimRng::new(2);
        let mut run = Run::start(1, &mut d);
        let mut end = None;
        for _ in 0..(SECONDS * 64) {
            if let Some(e) = run.step([LANE_MID, START_Z], 1.0) {
                end = Some(e);
                break;
            }
        }
        assert_eq!(end, Some(RunEnd::Tackled));

        let mut run = Run::start(1, &mut d);
        for t in &mut run.tacklers {
            t.stunned = 10_000;
        }
        let mut at = [LANE_MID, START_Z];
        let mut end = None;
        for _ in 0..(SECONDS * 64) {
            at[1] -= 6.0 / 64.0;
            if let Some(e) = run.step(at, 1.0) {
                end = Some(e);
                break;
            }
        }
        assert_eq!(end, Some(RunEnd::Scored));
    }

    #[test]
    fn dodges_cool_down_and_stay_in_the_lane() {
        let mut d = SimRng::new(2);
        let mut run = Run::start(1, &mut d);
        assert_eq!(run.dodge([12.0, 20.0], 1.0), Some([13.0, 20.0]));
        assert_eq!(run.dodge([12.0, 20.0], 1.0), None, "cooling down");
        for _ in 0..64 {
            run.step([LANE_MID, 30.0], 0.0);
        }
        assert_eq!(run.dodge([8.0, 20.0], -1.0), Some([6.5, 20.0]));
    }

    #[test]
    fn a_stiff_arm_shoves_and_stuns() {
        let mut d = SimRng::new(2);
        let mut run = Run::start(1, &mut d);
        run.tacklers[0].pos = [8.0, 19.0];
        assert!(run.stiff_arm([8.0, 20.0]));
        assert!((run.tacklers[0].pos[1] - 17.0).abs() < 1e-5);
        assert_eq!(run.tacklers[0].stunned, STUN_TICKS);
        assert!(!run.stiff_arm([LANE_X.1, 26.5]), "nobody in reach");
    }

    #[test]
    fn the_scripted_runner_beats_week_one_most_of_the_time() {
        let mut scored = 0;
        for seed in 0..40 {
            let mut d = SimRng::new(seed);
            let mut run = Run::start(1, &mut d);
            let mut at = [LANE_MID, START_Z];
            loop {
                let near = run
                    .tacklers
                    .iter()
                    .any(|t| t.stunned == 0 && ((t.pos[0] - at[0]).powi(2) + (t.pos[1] - at[1]).powi(2)).sqrt() < 1.2);
                if near
                    && !run.stiff_arm(at)
                    && let Some(p) = run.dodge(at, if at[0] > LANE_MID { -1.0 } else { 1.0 })
                {
                    at = p;
                }
                let dir = bot_run_dir(&run, at);
                at = [at[0] + dir[0] * 6.0 / 64.0, at[1] + dir[1] * 6.0 / 64.0];
                if let Some(e) = run.step(at, 1.0) {
                    scored += usize::from(e == RunEnd::Scored);
                    break;
                }
            }
        }
        assert!(scored >= 20, "{scored} of 40");
    }
}
