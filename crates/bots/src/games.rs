//! Bots for the side games (plan section 5): each walks from its spawn
//! point to its station and plays there with simple, steady rules.

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::client::{OutgoingGame, Session};
use shared::fishing;
use shared::gauntlet;
use shared::hoops::{self, Mode};
use shared::kicks::{self, Kick};
use shared::minigame::Who;
use shared::pit::{self, Weapon};
use shared::protocol::*;

/// Which side game a bot plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideGame {
    /// Fish from pier spot `n`, reeling with a steady hand.
    Fish(u8),
    /// Enter 3 of 5 on the roof and shoot with the perfect aim.
    Hoops,
    /// Enter the penalty shootout and kick for the corner.
    Kicks,
    /// Kick 20-yard field goals for $10.
    FieldGoal,
    /// Run the gauntlet for $10, steering like the scripted runner.
    Gauntlet,
    /// Take the rack's weapon in the pit and shoot the nearest fighter.
    Pit(Weapon),
}

/// Through the bar's open floor (z = 3.4) to the front door, out to the lot.
const OUT_FRONT: [(f32, f32); 2] = [(0.0, 3.4), (0.0, 8.5)];

/// The walk from a spawn point to the station.
pub fn route(game: SideGame) -> Vec<(f32, f32)> {
    let mut r: Vec<(f32, f32)> = Vec::new();
    match game {
        SideGame::Fish(n) => {
            let (x, z) = fishing::SPOTS[usize::from(n)];
            r.extend(OUT_FRONT);
            r.extend([(0.0, 26.0), (x, z - 1.0), (x, z)]);
        }
        SideGame::Hoops => r.extend([(0.0, 3.4), (9.0, 3.4), (11.5, 3.4), (14.0, 2.0)]),
        SideGame::Kicks => {
            r.extend(OUT_FRONT);
            r.push(kicks::PENALTY_SPOT);
        }
        SideGame::FieldGoal => {
            r.extend(OUT_FRONT);
            r.push(kicks::TEE);
        }
        SideGame::Gauntlet => {
            r.extend(OUT_FRONT);
            r.extend([(gauntlet::LANE_MID, 12.0), (gauntlet::LANE_MID, gauntlet::START_Z)]);
        }
        SideGame::Pit(w) => {
            let (_, x, z) = *pit::RACKS.iter().find(|(r, ..)| *r == w).expect("a rack per weapon");
            r.extend([(-8.0, 3.4), (-12.0, 3.4), (-15.5, 3.4), (-20.0, 0.0), (x, z * 0.85)]);
        }
    }
    r
}

/// Steering during a gauntlet run: (move, yaw) toward the end zone, around tacklers.
pub fn gauntlet_steer(view: &GauntletView, me: u64, at: Vec3) -> Option<(Vec2, f32)> {
    if view.runner != Some(me) {
        return None;
    }
    let run = view.run.as_ref()?;
    let d = gauntlet::bot_run_dir(run, [at.x, at.z]);
    Some((Vec2::Y, shared::math::atan2(-d[0], -d[1])))
}

/// The bot's game requests, four times a second once at its station.
#[allow(clippy::too_many_arguments)]
pub fn play(
    game: SideGame,
    session: &Session,
    at: Vec3,
    frame: u64,
    view_tick: u32,
    fishing: &Query<&FishingView>,
    hoops_v: &Query<&HoopsView>,
    pens: &Query<&PenaltyView>,
    lane: &Query<&GauntletView>,
    pit_v: &Query<&PitView>,
    others: &Query<(&Player, &PlayerPos), With<Interpolated>>,
    out: &mut OutgoingGame,
) {
    let Some(id) = session.player_id else { return };
    let me = Who::Player(id);
    let mut send = |a| out.0.push(a);
    match game {
        SideGame::Fish(n) => {
            let Some(v) = fishing.iter().find(|v| v.spot == n) else { return };
            match v.phase {
                FishPhase::Idle if frame.is_multiple_of(64) => send(GameAction::Cast { power: 60 }),
                FishPhase::Biting if v.fisher == Some(id) => send(GameAction::Hook),
                FishPhase::Reeling if v.fisher == Some(id) => {
                    send(GameAction::Reel { held: v.tension < (v.band.0 + v.band.1) / 2 });
                }
                _ => {}
            }
        }
        SideGame::Hoops => {
            if !frame.is_multiple_of(32) {
                return;
            }
            let v = hoops_v.iter().next();
            let c = v.and_then(|v| v.contest.as_ref());
            match c {
                None => send(GameAction::JoinHoops { mode: Mode::ThreeOfFive }),
                Some(c) if !c.started => send(GameAction::StartHoops),
                Some(c) if c.shooter() == Some(me) => {
                    let (yaw, pitch, power) = hoops::perfect_aim([at.x, at.z]);
                    send(GameAction::Shoot { power, yaw, pitch });
                }
                _ => {}
            }
        }
        SideGame::Kicks => {
            if !frame.is_multiple_of(32) {
                return;
            }
            let Some(v) = pens.iter().next() else { return };
            if !v.shootout.started {
                if v.shootout.entrants.iter().any(|e| e.who == me) {
                    send(GameAction::StartShootout);
                } else {
                    send(GameAction::JoinShootout);
                }
            } else if v.shootout.kicker() == Some(me) && !v.in_flight {
                let aim = if (frame / 32).is_multiple_of(2) { 0.85 } else { -0.85 };
                send(GameAction::Kick { kick: Kick { aim, power: 65, curve: 0.0 } });
            }
        }
        SideGame::FieldGoal => {
            if frame.is_multiple_of(64) {
                send(GameAction::FieldGoal { yards: 20, stake: 10, kick: Kick { aim: 0.0, power: 45, curve: 0.0 } });
            }
        }
        SideGame::Gauntlet => {
            let Some(v) = lane.iter().next() else { return };
            if v.runner.is_none() && frame.is_multiple_of(64) {
                send(GameAction::Run { stake: 10 });
            } else if v.runner == Some(id)
                && frame.is_multiple_of(8)
                && let Some(run) = &v.run
                && run
                    .tacklers
                    .iter()
                    .any(|t| t.stunned == 0 && Vec2::new(t.pos[0] - at.x, t.pos[1] - at.z).length() < 1.2)
            {
                send(GameAction::StiffArm);
                send(GameAction::Dodge { right: at.x < gauntlet::LANE_MID });
            }
        }
        SideGame::Pit(w) => {
            let Some(v) = pit_v.iter().next() else { return };
            let mine = v.round.fighters.iter().find(|f| f.who == me);
            match mine {
                None if !v.round.started && frame.is_multiple_of(32) => send(GameAction::JoinPit { teams: false }),
                Some(f) if f.weapon.is_none() && frame.is_multiple_of(16) => send(GameAction::Pick { weapon: w }),
                Some(_) if !v.round.started && frame.is_multiple_of(32) => send(GameAction::StartPit),
                Some(f) if v.round.started && f.down == 0 && frame.is_multiple_of(4) => {
                    // The nearest other fighter, as this client sees him.
                    let target = others
                        .iter()
                        .filter(|(p, _)| {
                            p.id != id && v.round.fighters.iter().any(|g| g.who == Who::Player(p.id) && g.down == 0)
                        })
                        .map(|(_, p)| p.0)
                        .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)));
                    if let Some(t) = target {
                        let d = t - at;
                        let yaw = shared::math::atan2(-d.x, -d.z);
                        let flat = Vec2::new(d.x, d.z).length().max(0.1);
                        let pitch = shared::math::atan2(1.3 - pit::EYE_HEIGHT, flat);
                        send(GameAction::Fire { yaw, pitch, view_tick });
                    }
                }
                _ => {}
            }
        }
    }
}
