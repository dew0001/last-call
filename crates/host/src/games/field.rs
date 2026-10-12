//! The parking lot (plan sections 5.7 and 5.8): real balls. Penalties
//! against a player goalie or the house's keeper, field goals from the
//! tees, and the gauntlet.

use avian3d::prelude::*;
use bevy::prelude::*;
use shared::gauntlet::{self, Run, RunEnd};
use shared::kicks::{self, Kick, KickResult, Shootout};
use shared::protocol::*;
use shared::sports::{aim_error, signed_unit, split_pot};
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

// ---------- Kicks ----------

/// What a kick is for: a penalty from the spot (counting in a shootout or
/// not), a field goal from a tee (with a stake), or nothing (a free kick).
#[derive(Clone, Copy, Debug, PartialEq)]
enum KickFor {
    Penalty { counts: bool },
    FieldGoal { yards: u8, stake: i64 },
    Free,
}

/// A lot ball after a kick, until it is judged.
#[derive(Component, Debug)]
pub struct Kicked {
    by: u64,
    what: KickFor,
    ticks: u32,
    prev: Vec3,
    /// The curve's sideways pull, m/s².
    curve: [f32; 2],
}

/// A judged ball goes back to its spot (the penalty spot, or the tee it was
/// kicked from) at this tick, unless someone holds it.
#[derive(Component, Debug)]
pub struct ReturnAt(u64);

/// The goalkeeper's body (see `physics::spawn_lot_goals`).
#[derive(Component)]
pub struct Keeper;

/// A judged ball rests this long before it goes back (1.5 s).
const RETURN_TICKS: u64 = 96;

type LotBalls<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PropKind,
        &'static mut Position,
        &'static mut LinearVelocity,
        &'static mut AngularVelocity,
        &'static HeldBy,
        Option<&'static mut Kicked>,
        Option<&'static ReturnAt>,
        &'static mut crate::physics::BallHome,
    ),
    (Without<Player>, Without<Keeper>),
>;

/// Kick the lot ball at this player's feet, if there is one in front of them.
#[allow(clippy::too_many_arguments)]
fn kick_ball(
    commands: &mut Commands,
    balls: &mut LotBalls,
    by: u64,
    at: Vec3,
    kick: Kick,
    err: f32,
    d: &mut impl shared::rng::Draw,
    what: impl Fn(PropKind, Vec3) -> Option<KickFor>,
) -> Option<(Entity, KickFor)> {
    let fwd = Vec2::from_array([-shared::math::sin_cos(kick.yaw).0, -shared::math::sin_cos(kick.yaw).1]);
    let (ball, kind, _) = balls
        .iter()
        .filter(|(_, k, p, _, _, held, ..)| {
            matches!(k, PropKind::SoccerBall | PropKind::Football) && held.0.is_none() && p.0.y < 0.6
        })
        .map(|(e, k, p, ..)| (e, *k, Vec2::new(p.0.x - at.x, p.0.z - at.z)))
        .filter(|(_, _, off)| off.length() <= kicks::KICK_REACH && off.dot(fwd) > -0.2)
        .min_by(|a, b| a.2.length().total_cmp(&b.2.length()))?;
    let Ok((_, _, mut pos, mut lin, mut ang, _, _, _, mut home)) = balls.get_mut(ball) else { return None };
    let what = what(kind, pos.0)?;
    let kick = kick.clamped();
    let wild = kicks::power_error(kick.power);
    let kick =
        Kick { yaw: kick.yaw + signed_unit(d) * (err + wild), pitch: kick.pitch + signed_unit(d) * err * 0.5, ..kick };
    lin.0 = Vec3::from_array(kick.velocity());
    // Topspin for the look of it; the curve is a pull, not a spin.
    ang.0 = Quat::from_rotation_y(kick.yaw) * Vec3::new(-15.0, kick.curve * 10.0, 0.0);
    // Lift it off the ground a touch so it does not drag on the first step.
    pos.0.y = pos.0.y.max(super::balls::radius(kind) + 0.01);
    if let KickFor::FieldGoal { .. } = what
        && let Some((_, x, z, _)) =
            kicks::TEES.iter().find(|(y, ..)| kicks::tee_at(pos.0.x, pos.0.z).is_some_and(|t| t.0 == *y))
    {
        home.0 = Vec3::new(*x, kicks::FOOTBALL_RADIUS, *z);
    }
    let prev = pos.0;
    commands
        .entity(ball)
        .remove::<(Sleeping, ReturnAt)>()
        .insert((RigidBody::Dynamic, Kicked { by, what, ticks: 0, prev, curve: kick.curve_accel() }));
    Some((ball, what))
}

// ---------- Penalties ----------

#[derive(Component, Default)]
pub struct PenaltyHost {
    shootout: Shootout,
    goalie: Option<(Entity, u64)>,
    /// A penalty kick on its way (the bets ride on it).
    pending: bool,
    /// The keeper's dive: which way, and ticks since it began.
    dive: Option<(kicks::Dive, u32)>,
    /// The house keeper's plan for the kick in the air: dive this way at this tick.
    npc_plan: Option<(kicks::Dive, u64)>,
    bets: Vec<(u64, bool, i64)>,
    last: Option<KickView>,
    kicks: u32,
    paid: Vec<(u64, i64)>,
}

/// Ticks for a dive to reach full stretch, to hold it, and to get back up.
const DIVE_OUT: u32 = 18;
const DIVE_HOLD: u32 = 40;
const DIVE_BACK: u32 = 20;

/// How far out a dive is (0 to 1).
fn dive_progress(t: u32) -> f32 {
    if t < DIVE_OUT {
        t as f32 / DIVE_OUT as f32
    } else if t < DIVE_OUT + DIVE_HOLD {
        1.0
    } else {
        1.0 - ((t - DIVE_OUT - DIVE_HOLD) as f32 / DIVE_BACK as f32).min(1.0)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn penalties(
    mut commands: Commands,
    queue: Res<GameQueue>,
    tick: Res<crate::TickCount>,
    timer: Res<ShiftTimer>,
    mut rngs: ResMut<GameRngs>,
    mut audit: ResMut<Audit>,
    mut goal: Query<(&mut PenaltyHost, &mut PenaltyView)>,
    mut fg_views: Query<&mut FieldGoalView>,
    mut players: GamePlayers,
    mut room: Query<&mut RunLedger, With<RoomState>>,
    mut balls: LotBalls,
    mut keeper: Query<(&mut Position, &mut Rotation, &mut LinearVelocity), (With<Keeper>, Without<PropKind>)>,
) {
    let Ok((mut host, mut view)) = goal.single_mut() else { return };
    let Ok(mut fg) = fg_views.single_mut() else { return };
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
        let err = aim_error(drunk.map_or(0, |d| d.level), focus.map_or(0, |f| f.level));
        match *action {
            GameAction::JoinShootout { fun } if in_lot && host.goalie.is_none_or(|g| g.0 != *e) => {
                let fee = host.shootout.fee(fun, kicks::ENTRY);
                if (fee == 0 || charge(&mut pocket, fee)) && !host.shootout.join(me, fee) {
                    pocket.0 += fee;
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
                if !host.pending
                    && in_lot
                    && amount <= STAKE_MAX
                    && host.goalie.is_none_or(|g| g.0 != *e)
                    && !host.bets.iter().any(|b| b.0 == player.id)
                    && charge(&mut pocket, amount) =>
            {
                host.bets.push((player.id, goal, amount));
            }
            GameAction::Dive { dive } if host.goalie.is_some_and(|g| g.0 == *e) && host.dive.is_none() => {
                host.dive = Some((dive, 0));
            }
            GameAction::Kick { kick } if in_lot => {
                let started = host.shootout.started && !host.shootout.finished();
                let kicker = host.shootout.kicker() == Some(me);
                let pending = host.pending;
                let mut d = rngs.kicks.at(tick.0, &mut audit.0);
                let kicked = kick_ball(&mut commands, &mut balls, player.id, pos.0, kick, err, &mut d, |kind, at| {
                    let on_spot = (at.x - kicks::PENALTY_SPOT.0).hypot(at.z - kicks::PENALTY_SPOT.1) < 0.8;
                    Some(match kind {
                        // In a shootout only the kicker takes the penalty.
                        PropKind::SoccerBall if on_spot && !pending && (!started || kicker) => {
                            KickFor::Penalty { counts: started }
                        }
                        PropKind::SoccerBall => KickFor::Free,
                        _ => KickFor::FieldGoal { yards: kicks::tee_at(at.x, at.z).map_or(0, |t| t.0), stake: 0 },
                    })
                });
                if let Some((ball, KickFor::Penalty { .. })) = kicked {
                    host.pending = true;
                    // The house keeper reads the kick (when nobody is in goal).
                    if host.goalie.is_none()
                        && let Ok((_, _, p, v, ..)) = balls.get(ball)
                    {
                        // Where it would cross, flown straight from here.
                        let t = (kicks::GOAL.1 - p.0.z) / v.0.z.max(1.0);
                        let x = p.0.x + v.0.x * t - kicks::GOAL.0;
                        let (dive, at) = kicks::npc_dive(x, timer.calendar.week, &mut d);
                        host.npc_plan = Some((dive, tick.0 + u64::from(at)));
                    }
                }
            }
            GameAction::FieldGoal { stake, kick } if in_lot && (0..=STAKE_MAX).contains(&stake) => {
                let mut d = rngs.kicks.at(tick.0, &mut audit.0);
                // A stake rides only on a football on a tee; it is paid when the kick lands.
                let can_pay = stake == 0 || pocket.0 >= stake;
                let kicked = kick_ball(&mut commands, &mut balls, player.id, pos.0, kick, err, &mut d, |kind, at| {
                    (kind == PropKind::Football && can_pay)
                        .then(|| kicks::tee_at(at.x, at.z))
                        .flatten()
                        .map(|(yards, _)| KickFor::FieldGoal { yards, stake })
                });
                if kicked.is_some() && stake > 0 {
                    pocket.0 -= stake;
                }
            }
            _ => {}
        }
    }

    // The house keeper goes when he planned to.
    if host.goalie.is_none()
        && let Some((dive, at)) = host.npc_plan
        && tick.0 >= at
    {
        host.npc_plan = None;
        if host.dive.is_none() {
            host.dive = Some((dive, 0));
        }
    }
    if let Some((_, t)) = &mut host.dive {
        *t += 1;
        if *t > DIVE_OUT + DIVE_HOLD + DIVE_BACK {
            host.dive = None;
        }
    }
    // Move the keeper's body: the house's keeper stands in the goal; a
    // player goalie is their own body, and this one is only their dive.
    let base = match host.goalie.and_then(|(e, _)| players.get(e).ok()) {
        Some((_, _, p, ..)) => Vec3::new(p.0.x, 0.0, p.0.z),
        None => Vec3::new(kicks::GOALIE_SPOT.0, 0.0, kicks::GOALIE_SPOT.1),
    };
    let (dive, progress) = host.dive.map_or((None, 0.0), |(d, t)| (Some(d), dive_progress(t)));
    if let Ok((mut kp, mut kr, mut kv)) = keeper.single_mut() {
        let side = dive.map_or(0.0, |d| d.side());
        let target = match (host.goalie.is_some(), dive) {
            // A player goalie not diving: the body is out of the way.
            (true, None) => Vec3::new(base.x, -5.0, base.z),
            // A dive: out to the side and down, laid flat.
            (_, Some(_)) if side != 0.0 => Vec3::new(base.x + side * 1.4 * progress, 0.92 - 0.45 * progress, base.z),
            // Standing (a center dive: a crouch with arms spread).
            _ => Vec3::new(base.x, 0.92 - 0.2 * progress, base.z),
        };
        let lean = -side * std::f32::consts::FRAC_PI_2 * progress;
        let jump = (target - kp.0).length() > 2.0;
        if jump {
            kp.0 = target;
            kv.0 = Vec3::ZERO;
        } else {
            kv.0 = (target - kp.0) / shared::TICK.as_secs_f32();
        }
        kr.0 = Quat::from_rotation_z(lean);
    }

    // Judge the kicked balls.
    let dt = shared::TICK.as_secs_f32();
    let mut judged: Vec<(Entity, u64, KickFor, Option<(f32, f32)>, KickResult)> = Vec::new();
    for (ball, kind, pos, mut lin, _, held, kicked, _, _) in &mut balls {
        let Some(mut k) = kicked else { continue };
        k.ticks += 1;
        let now = pos.0;
        if held.0.is_none() && now.y > super::balls::radius(*kind) + 0.05 {
            lin.0.x += k.curve[0] * dt;
            lin.0.z += k.curve[1] * dt;
        }
        let line = if *kind == PropKind::SoccerBall { kicks::GOAL.1 } else { kicks::UPRIGHTS.1 };
        let crossed = kicks::crossing(k.prev.to_array(), now.to_array(), line);
        let stopped = k.ticks > 12 && lin.0.length() < 0.6;
        let back = k.ticks > 8 && lin.0.z < -0.5 && now.z > line - 4.0;
        let result = match (crossed, *kind) {
            (Some((x, h)), PropKind::SoccerBall) => Some(kicks::at_the_line(x, h)),
            (Some((x, h)), _) => Some(if kicks::field_goal_good(x, h) { KickResult::Goal } else { KickResult::Wide }),
            // Turned back near the goal: off the keeper (a save) or a post.
            (None, PropKind::SoccerBall) if back => {
                let post = ((now.x - kicks::GOAL.0).abs() - kicks::GOAL_HALF_WIDTH).abs() < 0.5;
                Some(if post { KickResult::Wide } else { KickResult::Saved })
            }
            _ if held.0.is_some() || stopped || k.ticks > kicks::MAX_FLIGHT_TICKS => {
                Some(if *kind == PropKind::SoccerBall && now.z > kicks::GOAL.1 - 4.0 {
                    KickResult::Saved
                } else {
                    KickResult::Wide
                })
            }
            _ => None,
        };
        k.prev = now;
        if let Some(r) = result {
            judged.push((ball, k.by, k.what, crossed, r));
        }
    }
    for (ball, by, what, crossed, result) in judged {
        commands.entity(ball).remove::<Kicked>().insert(ReturnAt(tick.0 + RETURN_TICKS));
        let (x, height) = crossed.unwrap_or((0.0, 0.0));
        match what {
            KickFor::Penalty { counts } => {
                host.pending = false;
                host.npc_plan = None;
                let goal = result == KickResult::Goal;
                host.last = Some(KickView { by, x: x - kicks::GOAL.0, height, result, dive: host.dive.map(|d| d.0) });
                host.kicks += 1;
                if counts {
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
                // A player goalie earns their keep: a save in a paid shootout is
                // worth the entry fee (practice kicks and fun shootouts pay nothing).
                if result == KickResult::Saved
                    && counts
                    && !host.shootout.fun
                    && let Some((_, id)) = host.goalie
                {
                    pay(&mut players, id, kicks::ENTRY);
                    house(&mut room, -kicks::ENTRY);
                }
            }
            KickFor::FieldGoal { yards, stake } => {
                let good = result == KickResult::Goal && yards > 0;
                let returned = kicks::field_goal_return(yards, stake, good);
                pay(&mut players, by, returned);
                house(&mut room, stake - returned);
                fg.last = Some(FieldGoalResult {
                    by,
                    yards,
                    lateral: x - kicks::UPRIGHTS.0,
                    cleared: height > kicks::CROSSBAR,
                    good: result == KickResult::Goal,
                    returned,
                });
                fg.kicks += 1;
            }
            KickFor::Free => {}
        }
    }
    // Judged balls go back to their spot after a moment.
    for (ball, _, mut pos, mut lin, mut ang, held, _, back, home) in &mut balls {
        if let Some(ReturnAt(at)) = back
            && tick.0 >= *at
        {
            commands.entity(ball).remove::<ReturnAt>();
            if held.0.is_none() {
                pos.0 = home.0;
                lin.0 = Vec3::ZERO;
                ang.0 = Vec3::ZERO;
            }
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
        in_flight: host.pending,
        last: host.last,
        kicks: host.kicks,
        bets: host.bets.clone(),
        paid: host.paid.clone(),
        keeper: KeeperView { x: base.x, dive, progress, npc: host.goalie.is_none() },
    };
    view.set_if_neq(next);
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
                    && (0..=STAKE_MAX).contains(&stake)
                    && near(&pos, (gauntlet::LANE_MID, gauntlet::START_Z), 2.0)
                    // A stake of 0 is a run for fun.
                    && (stake == 0 || charge(&mut pocket, stake)) =>
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
