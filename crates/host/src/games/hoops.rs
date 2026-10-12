//! Basketball on the roof (plan section 5.6): real balls (see
//! [`shared::hoops`]). A shot throws the ball the shooter holds; [`follow`]
//! watches it until it drops through the rim or lands, and a contest
//! counts it. In a contest the host hands the shooter a ball.

use avian3d::prelude::*;
use bevy::prelude::*;
use shared::hoops::{self, Contest, Mode, Shot};
use shared::minigame::Who;
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
    /// Shots that ended since the last run: (shot, from, made, in a contest).
    ended: Vec<(ShotView, [f32; 2], bool)>,
    /// Contest shots in the air: the next shooter gets a ball after they land.
    contest_balls: u32,
}

/// A ball in the air from a shot, until it goes in or lands.
#[derive(Component, Debug)]
pub struct InFlight {
    view: ShotView,
    from: [f32; 2],
    contest: bool,
    ticks: u32,
    prev: Vec3,
}

/// The basketballs.
type Balls<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PropKind,
        &'static mut Position,
        &'static mut LinearVelocity,
        &'static mut AngularVelocity,
        &'static mut HeldBy,
        Option<&'static mut InFlight>,
        Option<&'static crate::physics::BallHome>,
    ),
    Without<Player>,
>;

/// Follow each shot ball: through the rim is a make; landing, being caught
/// or a long flight is a miss. The roof's wind pushes balls in the air. A
/// ball that leaves the roof goes back to its spot by the door.
pub fn follow(mut commands: Commands, timer: Res<ShiftTimer>, mut court: Query<&mut HoopsHost>, mut balls: Balls) {
    let Ok(mut host) = court.single_mut() else { return };
    let wind = hoops::wind(timer.calendar.week);
    let dt = shared::TICK.as_secs_f32();
    for (e, kind, mut pos, mut lin, mut ang, held, flight, home) in &mut balls {
        if *kind != PropKind::Basketball {
            continue;
        }
        if let Some(mut f) = flight {
            f.ticks += 1;
            let now = pos.0;
            let made = hoops::through_rim(f.prev.to_array(), now.to_array());
            let over = made
                || held.0.is_some()
                || (now.y < hoops::MISS_BELOW && f.ticks > 4)
                || f.ticks as usize > hoops::MAX_FLIGHT_TICKS;
            if over {
                let mut view = f.view;
                view.made = made;
                host.ended.push((view, f.from, f.contest));
                commands.entity(e).remove::<InFlight>();
            } else {
                f.prev = now;
                if held.0.is_none() {
                    lin.0.x += wind * dt;
                }
            }
        }
        // Rolling on the floor: the physics has no rolling resistance, so a
        // ball would roll on forever. Slow it down (about half its speed a
        // second) so it comes to rest and sleeps.
        // (Only a moving ball: writing the velocity wakes a sleeping one.)
        let rolling = held.0.is_none() && pos.0.y < hoops::BALL_RADIUS + 0.03;
        let moving = lin.0.length_squared() > 1e-4 || ang.0.length_squared() > 1e-3;
        if rolling && moving && !bouncing(&lin) {
            lin.0.x *= ROLL_DRAG;
            lin.0.z *= ROLL_DRAG;
            ang.0 *= ROLL_DRAG;
        }
        let (x0, x1, z0, z1) = ROOF;
        let off = pos.0.y < -1.0 || !(x0..=x1).contains(&pos.0.x) || !(z0..=z1).contains(&pos.0.z);
        if off
            && held.0.is_none()
            && let Some(home) = home
        {
            pos.0 = home.0;
            lin.0 = Vec3::ZERO;
            ang.0 = Vec3::ZERO;
        }
    }
}

/// A rolling ball keeps this much of its speed each tick.
const ROLL_DRAG: f32 = 0.99;

/// Bouncing off the floor (moving up or down fast) is not rolling.
fn bouncing(lin: &LinearVelocity) -> bool {
    lin.0.y.abs() > 0.5
}

/// The roof's floor, with a margin: a ball outside it has fallen off.
const ROOF: (f32, f32, f32, f32) = (9.0, 25.0, -6.0, 10.0);

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
    mut hands: Query<&mut crate::physics::Hands>,
    mut balls: Balls,
    mut commands: Commands,
) {
    let Ok((mut host, mut view)) = court.single_mut() else { return };
    let host = &mut *host;
    for (e, action) in &queue.0 {
        let Ok((_, player, pos, _, mut pocket, drunk, focus)) = players.get_mut(*e) else { continue };
        let me = who(player);
        let on_roof = room_at(pos.0.x, pos.0.z) == Some(Room::Roof);
        match *action {
            GameAction::JoinHoops { mode, fun } if on_roof => {
                let contest = host.contest.get_or_insert_with(|| Contest::new(mode));
                let fee = contest.fee(fun, hoops::ENTRY);
                if contest.mode == mode
                    && !contest.started
                    && (fee == 0 || charge(&mut pocket, fee))
                    && !contest.join(me, fee)
                {
                    pocket.0 += fee;
                }
            }
            GameAction::StartHoops => {
                if let Some(c) = &mut host.contest
                    && c.entrants.iter().any(|x| x.who == me)
                {
                    c.start();
                }
            }
            GameAction::Shoot { power, yaw, pitch } if on_roof => {
                // The ball in this player's hands.
                let Ok(mut h) = hands.get_mut(*e) else { continue };
                let Some(ball) = h.held else { continue };
                let Ok((_, kind, mut bpos, mut lin, mut ang, mut held, ..)) = balls.get_mut(ball) else { continue };
                if *kind != PropKind::Basketball {
                    continue;
                }
                let from = [pos.0.x, pos.0.z];
                let shooter = host.contest.as_ref().and_then(|c| c.shooter());
                let in_contest = shooter == Some(me);
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
                // Released over the head, along the aim.
                let at = Vec3::new(from[0], hoops::RELEASE_Y, from[1]);
                bpos.0 = at;
                lin.0 = Vec3::from_array(shot.velocity());
                // Backspin.
                ang.0 = Quat::from_rotation_y(shot.yaw) * Vec3::new(-8.0, 0.0, 0.0);
                held.0 = None;
                h.held = None;
                host.contest_balls += u32::from(in_contest);
                commands.entity(ball).insert((
                    RigidBody::Dynamic,
                    InFlight {
                        view: ShotView { by: player.id, shot, made: false },
                        from,
                        contest: in_contest,
                        ticks: 0,
                        prev: at,
                    },
                ));
                commands.entity(ball).remove::<Sleeping>();
            }
            _ => {}
        }
    }

    // Shots that ended: the last one shows, a contest counts its shooter's.
    for (view, from, contest) in std::mem::take(&mut host.ended) {
        host.last = Some(view);
        host.shots += 1;
        host.contest_balls = host.contest_balls.saturating_sub(u32::from(contest));
        if contest
            && let Some(c) = &mut host.contest
            && c.shooter() == Some(Who::Player(view.by))
        {
            c.record(from, view.made);
        }
    }

    // The shooter gets a ball once the last shot is down: a free one if
    // there is one, else whoever holds one hands it over.
    if let Some(c) = &host.contest
        && let Some(shooter) = c.shooter()
        && host.contest_balls == 0
        && let Some((pe, id)) = players.iter().find(|(_, p, ..)| who(p) == shooter).map(|(pe, p, ..)| (pe, p.id))
        && hands.get(pe).is_ok_and(|h| h.held.is_none())
    {
        let free = balls
            .iter()
            .filter(|(_, k, _, _, _, held, f, _)| **k == PropKind::Basketball && held.0.is_none() && f.is_none())
            .map(|(b, ..)| b)
            .next();
        let ball = free.or_else(|| {
            balls
                .iter()
                .find(|(_, k, _, _, _, held, f, _)| **k == PropKind::Basketball && held.0.is_some() && f.is_none())
                .map(|(b, ..)| b)
        });
        if let Some(ball) = ball
            && let Ok((_, _, _, mut lin, mut ang, mut held, ..)) = balls.get_mut(ball)
        {
            if let Some(other) = held.0
                && let Some((oe, ..)) = players.iter().find(|(_, p, ..)| p.id == other)
                && let Ok(mut oh) = hands.get_mut(oe)
            {
                oh.held = None;
            }
            held.0 = Some(id);
            lin.0 = Vec3::ZERO;
            ang.0 = Vec3::ZERO;
            if let Ok(mut h) = hands.get_mut(pe) {
                h.held = Some(ball);
                h.charge = 0;
            }
            commands.entity(ball).insert(RigidBody::Kinematic).remove::<Sleeping>();
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
