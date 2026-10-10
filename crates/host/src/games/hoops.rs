//! Basketball on the roof (plan section 5.6).

use bevy::prelude::*;
use shared::hoops::{self, Contest, Mode, Shot};
use shared::protocol::*;
use shared::sports::{aim_error, signed_unit, split_pot};
use shared::world::{Room, room_at};

use super::{Crowd, GamePlayers, GameQueue, GameRngs, charge, pay, player_id, who};
use crate::casino::Audit;
use crate::shift::ShiftTimer;

/// Better drink sales after a 5 of 5: two minutes.
pub const CROWD_TICKS: u32 = 120 * shared::TICK_HZ;

#[derive(Component, Default)]
pub struct HoopsHost {
    contest: Option<Contest>,
    last: Option<ShotView>,
    shots: u32,
    paid: Vec<(u64, i64)>,
}

pub fn on_court(x: f32, z: f32) -> bool {
    let (x0, x1, z0, z1) = hoops::COURT;
    (x0..=x1).contains(&x) && (z0..=z1).contains(&z)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    queue: Res<GameQueue>,
    tick: Res<crate::TickCount>,
    timer: Res<ShiftTimer>,
    mut rngs: ResMut<GameRngs>,
    mut audit: ResMut<Audit>,
    mut crowd: ResMut<Crowd>,
    mut court: Query<(&mut HoopsHost, &mut HoopsView)>,
    mut players: GamePlayers,
) {
    let Ok((mut host, mut view)) = court.single_mut() else { return };
    let host = &mut *host;
    for (e, action) in &queue.0 {
        let Ok((_, player, pos, _, mut pocket, drunk, focus)) = players.get_mut(*e) else { continue };
        let me = who(player);
        let on_roof = room_at(pos.0.x, pos.0.z) == Some(Room::Roof);
        match *action {
            GameAction::JoinHoops { mode } if on_roof => {
                let contest = host.contest.get_or_insert_with(|| Contest::new(mode));
                if contest.mode == mode
                    && !contest.started
                    && charge(&mut pocket, hoops::ENTRY)
                    && !contest.join(me, hoops::ENTRY)
                {
                    pocket.0 += hoops::ENTRY;
                }
            }
            GameAction::StartHoops => {
                if let Some(c) = &mut host.contest
                    && c.entrants.iter().any(|x| x.who == me)
                {
                    c.start();
                }
            }
            GameAction::Shoot { power, yaw, pitch } if on_court(pos.0.x, pos.0.z) => {
                let from = [pos.0.x, pos.0.z];
                let in_contest = host.contest.as_ref().is_some_and(|c| c.started);
                if in_contest && !host.contest.as_ref().is_some_and(|c| c.may_shoot(me, from)) {
                    continue;
                }
                let err = aim_error(drunk.map_or(0, |d| d.level), focus.map_or(0, |f| f.level));
                let mut d = rngs.hoops.at(tick.0, &mut audit.0);
                let shot = Shot {
                    from,
                    power: power.min(1000),
                    yaw: yaw + signed_unit(&mut d) * err,
                    pitch: pitch + signed_unit(&mut d) * err,
                    wind: hoops::wind(timer.calendar.week),
                };
                let made = hoops::fly(&shot).made;
                host.last = Some(ShotView { by: player.id, shot, made });
                host.shots += 1;
                if in_contest && let Some(c) = &mut host.contest {
                    c.record(from, made);
                }
            }
            _ => {}
        }
    }

    // A shooter who left the room misses their turn.
    if let Some(c) = &mut host.contest
        && let Some(shooter) = c.shooter()
        && !players.iter().any(|(_, p, ..)| who(p) == shooter)
    {
        c.record([0.0, 0.0], false);
    }

    // A finished contest pays out.
    if let Some(c) = &host.contest
        && c.finished()
    {
        let c = host.contest.take().unwrap();
        host.paid = split_pot(c.pot, &c.winners()).into_iter().map(|(w, a)| (player_id(w), a)).collect();
        for (id, amount) in &host.paid {
            pay(&mut players, *id, *amount);
        }
        if c.mode == Mode::ThreeOfFive && c.entrants.iter().any(|e| e.score == 5) {
            crowd.0 = CROWD_TICKS;
        }
    }
    let next = HoopsView {
        contest: host.contest.clone(),
        last: host.last,
        shots: host.shots,
        paid: host.paid.clone(),
        crowd_secs: crowd.0.div_ceil(shared::TICK_HZ) as u16,
    };
    view.set_if_neq(next);
}
