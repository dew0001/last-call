//! The parking lot (plan sections 5.7 and 5.8): penalties against a player
//! or NPC goalie, field goals from the tee, and the gauntlet.

use bevy::prelude::*;
use shared::gauntlet::{self, Run, RunEnd};
use shared::kicks::{self, KickResult, Save, Shootout};
use shared::protocol::*;
use shared::sports::{aim_error, split_pot};
use shared::world::{Room, room_at};

use super::{GamePlayers, GameQueue, GameRngs, charge, pay, player_id, who};
use crate::casino::Audit;
use crate::shift::ShiftTimer;

/// Most a single bet or stake may be.
pub const STAKE_MAX: i64 = 500;

fn near(pos: &PlayerPos, (x, z): (f32, f32), reach: f32) -> bool {
    (pos.0.x - x).hypot(pos.0.z - z) <= reach
}

fn house(room: &mut Query<&mut RunLedger, With<RoomState>>, amount: i64) {
    if let Ok(mut r) = room.single_mut() {
        r.ledger.house += amount;
    }
}

// ---------- Penalties ----------

/// A kick on its way to the goal.
struct InFlight {
    by: u64,
    x: f32,
    height: f32,
    power: u8,
    kicked_at: u64,
    /// The NPC's dive, or the player goalie's (with ticks after the kick).
    save: Option<Save>,
    counts: bool,
}

#[derive(Component, Default)]
pub struct PenaltyHost {
    shootout: Shootout,
    goalie: Option<(Entity, u64)>,
    flight: Option<InFlight>,
    /// A dive the goalie made before the kick.
    early_dive: Option<kicks::Dive>,
    bets: Vec<(u64, bool, i64)>,
    last: Option<KickView>,
    kicks: u32,
    paid: Vec<(u64, i64)>,
}

#[allow(clippy::too_many_arguments)]
pub fn penalties(
    queue: Res<GameQueue>,
    tick: Res<crate::TickCount>,
    timer: Res<ShiftTimer>,
    mut rngs: ResMut<GameRngs>,
    mut audit: ResMut<Audit>,
    mut goal: Query<(&mut PenaltyHost, &mut PenaltyView)>,
    mut players: GamePlayers,
    mut room: Query<&mut RunLedger, With<RoomState>>,
) {
    let Ok((mut host, mut view)) = goal.single_mut() else { return };
    let host = &mut *host;
    // A goalie who left the goal gives it up.
    if let Some((e, _)) = host.goalie
        && !players.get(e).is_ok_and(|(_, _, p, ..)| near(p, kicks::GOALIE_SPOT, 4.0))
    {
        host.goalie = None;
    }
    for (e, action) in &queue.0 {
        let Ok((_, player, pos, _, mut pocket, drunk, focus)) = players.get_mut(*e) else { continue };
        let me = who(player);
        let in_lot = room_at(pos.0.x, pos.0.z) == Some(Room::ParkingLot);
        match *action {
            GameAction::JoinShootout if in_lot && host.goalie.is_none_or(|g| g.0 != *e) => {
                if charge(&mut pocket, kicks::ENTRY) && !host.shootout.join(me, kicks::ENTRY) {
                    pocket.0 += kicks::ENTRY;
                }
            }
            GameAction::TakeGoal
                if host.goalie.is_none()
                    && near(&pos, kicks::GOALIE_SPOT, kicks::SPOT_REACH)
                    && !host.shootout.entrants.iter().any(|x| x.who == me) =>
            {
                host.goalie = Some((*e, player.id));
            }
            GameAction::StartShootout if host.shootout.entrants.iter().any(|x| x.who == me) => {
                host.shootout.start();
            }
            GameAction::BetKick { goal, amount }
                if host.flight.is_none()
                    && in_lot
                    && amount <= STAKE_MAX
                    && host.goalie.is_none_or(|g| g.0 != *e)
                    && !host.bets.iter().any(|b| b.0 == player.id)
                    && charge(&mut pocket, amount) =>
            {
                host.bets.push((player.id, goal, amount));
            }
            GameAction::Dive { dive } if host.goalie.is_some_and(|g| g.0 == *e) => match &mut host.flight {
                Some(f) if f.save.is_none() => {
                    f.save = Some(Save { dive, at: (tick.0 - f.kicked_at) as u32 });
                }
                None => host.early_dive = Some(dive),
                _ => {}
            },
            GameAction::Kick { kick }
                if host.flight.is_none() && near(&pos, kicks::PENALTY_SPOT, kicks::SPOT_REACH) =>
            {
                let started = host.shootout.started && !host.shootout.finished();
                if started && host.shootout.kicker() != Some(me) {
                    continue;
                }
                let kick = kick.clamped();
                let err = aim_error(drunk.map_or(0, |d| d.level), focus.map_or(0, |f| f.level));
                let mut d = rngs.kicks.at(tick.0, &mut audit.0);
                let (x, height) = kicks::penalty_flight(kick, err, &mut d);
                let save = match host.goalie {
                    Some(_) => host.early_dive.take().map(|dive| Save { dive, at: 0 }),
                    None => Some(kicks::npc_save(x, kick.power, timer.calendar.week, &mut d)),
                };
                host.flight = Some(InFlight {
                    by: player.id,
                    x,
                    height,
                    power: kick.power,
                    kicked_at: tick.0,
                    save,
                    counts: started,
                });
            }
            _ => {}
        }
    }

    // The ball reaches the line.
    if let Some(f) = &host.flight
        && tick.0 >= f.kicked_at + u64::from(kicks::flight_ticks(f.power))
    {
        let f = host.flight.take().unwrap();
        let result = kicks::resolve_penalty(f.x, f.height, f.power, f.save);
        let goal = result == KickResult::Goal;
        host.last = Some(KickView { by: f.by, x: f.x, height: f.height, result, dive: f.save.map(|s| s.dive) });
        host.kicks += 1;
        if f.counts {
            host.shootout.record(goal);
        }
        let mut to_house = 0;
        for (id, bet_goal, amount) in std::mem::take(&mut host.bets) {
            if bet_goal == goal && pay(&mut players, id, amount * 2) {
                to_house -= amount;
            } else {
                to_house += amount;
            }
        }
        house(&mut room, to_house);
        // A player goalie earns his keep: a save is worth the entry fee.
        if result == KickResult::Saved
            && let Some((_, id)) = host.goalie
        {
            pay(&mut players, id, kicks::ENTRY);
            house(&mut room, -kicks::ENTRY);
        }
    }
    if host.shootout.finished() {
        let s = std::mem::take(&mut host.shootout);
        host.paid = split_pot(s.pot, &s.winners()).into_iter().map(|(w, a)| (player_id(w), a)).collect();
        for (id, amount) in &host.paid {
            pay(&mut players, *id, *amount);
        }
    }
    let mut shootout = host.shootout.clone();
    shootout.goalie = host.goalie.map(|g| shared::minigame::Who::Player(g.1));
    let next = PenaltyView {
        shootout,
        in_flight: host.flight.is_some(),
        last: host.last,
        kicks: host.kicks,
        bets: host.bets.clone(),
        paid: host.paid.clone(),
    };
    view.set_if_neq(next);
}

// ---------- Field goals ----------

#[allow(clippy::too_many_arguments)]
pub fn field_goals(
    queue: Res<GameQueue>,
    tick: Res<crate::TickCount>,
    mut rngs: ResMut<GameRngs>,
    mut audit: ResMut<Audit>,
    mut views: Query<&mut FieldGoalView>,
    mut players: GamePlayers,
    mut room: Query<&mut RunLedger, With<RoomState>>,
) {
    let Ok(mut view) = views.single_mut() else { return };
    for (e, action) in &queue.0 {
        let GameAction::FieldGoal { yards, stake, kick } = *action else { continue };
        let Ok((_, player, pos, _, mut pocket, drunk, focus)) = players.get_mut(*e) else { continue };
        if !near(&pos, kicks::TEE, kicks::SPOT_REACH)
            || !kicks::FIELD_GOALS.iter().any(|(y, _)| *y == yards)
            || stake > STAKE_MAX
            || !charge(&mut pocket, stake)
        {
            continue;
        }
        let err = aim_error(drunk.map_or(0, |d| d.level), focus.map_or(0, |f| f.level));
        let mut d = rngs.kicks.at(tick.0, &mut audit.0);
        let (lateral, cleared) = kicks::field_goal_flight(yards, kick, err, &mut d);
        let good = kicks::field_goal_good(lateral, cleared);
        let returned = kicks::field_goal_return(yards, stake, good);
        pocket.0 += returned;
        house(&mut room, stake - returned);
        view.last = Some(FieldGoalResult { by: player.id, yards, lateral, cleared, good, returned });
        view.kicks += 1;
    }
}

// ---------- The gauntlet ----------

#[derive(Component, Default)]
pub struct GauntletHost {
    runner: Option<(Entity, u64)>,
    run: Option<Run>,
    stake: i64,
    last: Option<(u64, RunEnd)>,
    runs: u32,
}

#[allow(clippy::too_many_arguments)]
pub fn gauntlet(
    queue: Res<GameQueue>,
    tick: Res<crate::TickCount>,
    timer: Res<ShiftTimer>,
    mut rngs: ResMut<GameRngs>,
    mut audit: ResMut<Audit>,
    mut lane: Query<(&mut GauntletHost, &mut GauntletView)>,
    mut players: GamePlayers,
    mut room: Query<&mut RunLedger, With<RoomState>>,
) {
    let Ok((mut host, mut view)) = lane.single_mut() else { return };
    let host = &mut *host;
    for (e, action) in &queue.0 {
        let Ok((_, player, mut pos, _, mut pocket, ..)) = players.get_mut(*e) else { continue };
        let runner = host.runner.is_some_and(|r| r.0 == *e);
        match *action {
            GameAction::Run { stake }
                if host.runner.is_none()
                    && stake <= STAKE_MAX
                    && near(&pos, (gauntlet::LANE_MID, gauntlet::START_Z), 2.0)
                    && charge(&mut pocket, stake) =>
            {
                let mut d = rngs.gauntlet.at(tick.0, &mut audit.0);
                host.run = Some(Run::start(timer.calendar.week, &mut d));
                host.runner = Some((*e, player.id));
                host.stake = stake;
                host.runs += 1;
            }
            GameAction::Dodge { right } if runner => {
                if let Some(run) = &mut host.run
                    && let Some(to) = run.dodge([pos.0.x, pos.0.z], if right { 1.0 } else { -1.0 })
                {
                    pos.0.x = to[0];
                }
            }
            GameAction::StiffArm if runner => {
                if let Some(run) = &mut host.run {
                    run.stiff_arm([pos.0.x, pos.0.z]);
                }
            }
            _ => {}
        }
    }
    if let (Some((e, id)), Some(run)) = (host.runner, &mut host.run) {
        let at = players.get(e).ok().map(|(_, _, p, ..)| [p.0.x, p.0.z]);
        let end = match at {
            // Out of the lane (or gone): the run is forfeit.
            None => Some(RunEnd::TimeUp),
            Some(a)
                if a[0] < gauntlet::LANE_X.0 - 1.0
                    || a[0] > gauntlet::LANE_X.1 + 1.0
                    || a[1] > gauntlet::START_Z + 2.0 =>
            {
                Some(RunEnd::TimeUp)
            }
            Some(a) => run.step(a, 1.0),
        };
        if let Some(end) = end {
            if end == RunEnd::Scored {
                pay(&mut players, id, host.stake * 2);
                house(&mut room, -host.stake);
            } else {
                house(&mut room, host.stake);
            }
            host.last = Some((id, end));
            host.runner = None;
            host.run = None;
        }
    }
    let next = GauntletView {
        runner: host.runner.map(|r| r.1),
        run: host.run.clone(),
        stake: host.stake,
        last: host.last,
        runs: host.runs,
    };
    view.set_if_neq(next);
}
