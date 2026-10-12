//! The side games on the host, driven by local players: fishing with bets,
//! basketball contests, penalties, field goals, the gauntlet and the fight
//! pit with lag compensation.

use bevy::prelude::*;
use host::economy::Preset;
use host::{HostConfig, HostSim};
use shared::fishing::{self, Catch, Side};
use shared::gauntlet::{self, RunEnd};
use shared::hoops::{self, Mode};
use shared::kicks::{self, Dive, Kick, KickResult};
use shared::pit::{self, Weapon};
use shared::protocol::*;
use shared::shift::Timings;

/// A quiet room (a long Setup, no chaos) where players have $1,000.
fn room(players: u8) -> (HostSim, Vec<Entity>) {
    let mut sim = HostSim::with_config(HostConfig {
        timings: Timings { setup: 10_000, ..Timings::PLAN },
        preset: Preset::Casino,
        manual_chaos: true,
        seed: [5; 32],
        ..Default::default()
    });
    let ps = (0..players).map(|i| sim.add_local_player(0x100 + u64::from(i), "p", i)).collect();
    run(&mut sim, 3);
    (sim, ps)
}

fn run(sim: &mut HostSim, n: u32) {
    for _ in 0..n {
        sim.tick();
    }
}

fn put(sim: &mut HostSim, e: Entity, (x, z): (f32, f32)) {
    sim.world_mut().get_mut::<PlayerPos>(e).unwrap().0 = Vec3::new(x, 0.0, z);
}

fn pocket(sim: &HostSim, e: Entity) -> i64 {
    sim.world().get::<Pocket>(e).unwrap().0
}

fn view<T: Component + Clone>(sim: &mut HostSim) -> T {
    let w = sim.world_mut();
    w.query::<&T>().iter(w).next().unwrap().clone()
}

fn spot(sim: &mut HostSim, n: u8) -> FishingView {
    let w = sim.world_mut();
    w.query::<&FishingView>().iter(w).find(|v| v.spot == n).unwrap().clone()
}

fn act(sim: &mut HostSim, e: Entity, a: GameAction) {
    sim.game_request(e, a);
    sim.tick();
}

#[test]
fn a_fisher_lands_a_fish_and_a_bettor_wins_on_it() {
    let (mut sim, ps) = room(2);
    let (fisher, bettor) = (ps[0], ps[1]);
    put(&mut sim, fisher, fishing::SPOTS[0]);
    put(&mut sim, bettor, (0.0, 44.0));
    let start = pocket(&sim, fisher);
    act(&mut sim, fisher, GameAction::Cast { power: 20 });
    assert_eq!(spot(&mut sim, 0).phase, FishPhase::Waiting);
    // Wait for the bite (at most 25 s), then strike.
    for _ in 0..(26 * 64) {
        if spot(&mut sim, 0).phase == FishPhase::Biting {
            break;
        }
        sim.tick();
    }
    act(&mut sim, fisher, GameAction::Hook);
    assert_eq!(spot(&mut sim, 0).phase, FishPhase::Reeling);
    act(&mut sim, bettor, GameAction::BetCatch { spot: 0, side: Side::Lands, amount: 50 });
    assert_eq!(spot(&mut sim, 0).bets.len(), 1);
    let bettor_before = pocket(&sim, bettor);
    // Reel like a steady hand: hold below the middle of the band.
    let mut held = false;
    for _ in 0..(200 * 64) {
        let v = spot(&mut sim, 0);
        if v.phase == FishPhase::Idle {
            break;
        }
        let want = v.tension < (v.band.0 + v.band.1) / 2;
        if want != held {
            held = want;
            sim.game_request(fisher, GameAction::Reel { held });
        }
        sim.tick();
    }
    let v = spot(&mut sim, 0);
    let Some((_, Catch::Landed(fish))) = v.last else { panic!("not landed: {:?}", v.last) };
    assert_eq!(pocket(&sim, fisher), start + fish.value());
    assert_eq!(pocket(&sim, bettor), bettor_before + 100, "1 to 1");
}

#[test]
fn walking_off_the_pier_loses_the_fish() {
    let (mut sim, ps) = room(1);
    put(&mut sim, ps[0], fishing::SPOTS[1]);
    act(&mut sim, ps[0], GameAction::Cast { power: 90 });
    put(&mut sim, ps[0], (0.0, 30.0));
    run(&mut sim, 2);
    let v = spot(&mut sim, 1);
    assert_eq!(v.phase, FishPhase::Idle);
    assert_eq!(v.last.map(|l| l.1), Some(Catch::Escaped));
}

/// Does this player hold a basketball?
fn holds_ball(sim: &mut HostSim, e: Entity) -> bool {
    let held = sim.world().get::<host::physics::Hands>(e).and_then(|h| h.held);
    held.is_some_and(|b| sim.world().get::<PropKind>(b) == Some(&PropKind::Basketball))
}

/// Wait (at most a second) for `e` to hold a ball, shoot, and wait for the
/// shot to end. The shot's result.
fn shoot(sim: &mut HostSim, e: Entity, power: u16, yaw: f32, pitch: f32) -> bool {
    for _ in 0..64 {
        if holds_ball(sim, e) {
            break;
        }
        run(sim, 1);
    }
    assert!(holds_ball(sim, e), "a ball in the shooter's hands");
    let before = view::<HoopsView>(sim).shots;
    act(sim, e, GameAction::Shoot { power, yaw, pitch });
    for _ in 0..(5 * shared::TICK_HZ) {
        let v: HoopsView = view(sim);
        if v.shots > before {
            return v.last.unwrap().made;
        }
        run(sim, 1);
    }
    panic!("the shot never ended");
}

fn perfect(sim: &mut HostSim, e: Entity) -> bool {
    let p = sim.world().get::<PlayerPos>(e).unwrap().0;
    let (yaw, pitch, power) = hoops::perfect_aim([p.x, p.z]);
    shoot(sim, e, power, yaw, pitch)
}

#[test]
fn a_perfect_shot_drops_through_the_real_rim_from_around_the_court() {
    let (mut sim, ps) = room(1);
    let a = ps[0];
    put(&mut sim, a, (14.0, 2.0));
    act(&mut sim, a, GameAction::JoinHoops { mode: Mode::ThreeOfFive, fun: true });
    act(&mut sim, a, GameAction::StartHoops);
    for at in [(14.0, 2.0), (12.0, -2.0), (17.0, 6.0), (19.5, 2.5), (15.0, -3.5)] {
        put(&mut sim, a, at);
        assert!(perfect(&mut sim, a), "missed from {at:?}");
    }
}

#[test]
fn a_ball_off_the_rim_bounces_and_a_ball_on_the_floor_can_be_picked_up() {
    let (mut sim, ps) = room(1);
    let a = ps[0];
    // Walk to the rack and press E: a ball in hand.
    let (rx, rz) = hoops::RACK[0];
    put(&mut sim, a, (rx - 0.6, rz));
    let e_down = PlayerInput::new(Vec2::ZERO, std::f32::consts::FRAC_PI_2, 0.0, shared::movement::buttons::INTERACT);
    sim.set_input(a, e_down);
    run(&mut sim, 2);
    sim.set_input(a, PlayerInput::default());
    run(&mut sim, 2);
    assert!(holds_ball(&mut sim, a), "picked up from the floor");
    // Free play: a short, flat shot hits the board or the rim and misses.
    put(&mut sim, a, (17.0, 2.0));
    let (yaw, _, _) = hoops::perfect_aim([17.0, 2.0]);
    assert!(!shoot(&mut sim, a, 700, yaw, 0.15));
    let v: HoopsView = view(&mut sim);
    assert_eq!(v.shots, 1);
    assert!(v.contest.is_none());
    // The ball came back off the hoop onto the roof.
    let w = sim.world_mut();
    let balls: Vec<Vec3> = w
        .query::<(&PropKind, &avian3d::prelude::Position)>()
        .iter(w)
        .filter(|(k, _)| **k == PropKind::Basketball)
        .map(|(_, p)| p.0)
        .collect();
    assert_eq!(balls.len(), hoops::RACK.len());
    assert!(balls.iter().all(|b| shared::world::room_at(b.x, b.z) == Some(shared::world::Room::Roof)));
}

#[test]
fn five_of_five_wins_the_pot_and_draws_a_crowd() {
    let (mut sim, ps) = room(2);
    let (a, b) = (ps[0], ps[1]);
    put(&mut sim, a, (14.0, 2.0));
    put(&mut sim, b, (13.0, 0.0));
    act(&mut sim, a, GameAction::JoinHoops { mode: Mode::ThreeOfFive, fun: false });
    act(&mut sim, b, GameAction::JoinHoops { mode: Mode::ThreeOfFive, fun: false });
    let (pa, pb) = (pocket(&sim, a), pocket(&sim, b));
    assert_eq!(pa, 1000 - hoops::ENTRY);
    act(&mut sim, a, GameAction::StartHoops);
    for _ in 0..5 {
        assert!(perfect(&mut sim, a));
        // b throws it at the floor.
        assert!(!shoot(&mut sim, b, 0, 0.0, -1.0));
    }
    let v: HoopsView = view(&mut sim);
    assert!(v.contest.is_none(), "over");
    assert_eq!(v.paid, vec![(0x100, 2 * hoops::ENTRY)]);
    assert_eq!(pocket(&sim, a), pa + 2 * hoops::ENTRY);
    assert_eq!(pocket(&sim, b), pb);
    assert!(v.crowd_secs > 100, "a crowd for two minutes");
    assert_eq!(v.shots, 10);
}

#[test]
fn horse_takes_two() {
    let (mut sim, ps) = room(2);
    let (a, b) = (ps[0], ps[1]);
    put(&mut sim, a, (14.0, 2.0));
    put(&mut sim, b, (14.0, 2.5));
    act(&mut sim, a, GameAction::JoinHoops { mode: Mode::Horse, fun: false });
    act(&mut sim, b, GameAction::JoinHoops { mode: Mode::Horse, fun: false });
    act(&mut sim, a, GameAction::StartHoops);
    let mut guard = 0;
    while view::<HoopsView>(&mut sim).contest.is_some() && guard < 100 {
        let c = view::<HoopsView>(&mut sim).contest.unwrap();
        let shooter = c.shooter().unwrap();
        // a always makes it; b always misses.
        if shooter == shared::minigame::Who::Player(0x100) {
            perfect(&mut sim, a);
        } else {
            shoot(&mut sim, b, 0, 0.0, -1.0);
        }
        guard += 1;
    }
    let v: HoopsView = view(&mut sim);
    assert_eq!(v.paid, vec![(0x100, 2 * hoops::ENTRY)], "after {guard} shots");
}

/// Where a lot ball of this kind is.
fn ball_at(sim: &mut HostSim, kind: PropKind) -> Vec3 {
    let w = sim.world_mut();
    w.query::<(&PropKind, &avian3d::prelude::Position)>().iter(w).find(|(k, _)| **k == kind).unwrap().1.0
}

/// Stand behind the ball on its spot (it may still be on its way back),
/// and kick it at `target` (x, height, z) with `power` and `curve`.
fn kick_at(
    sim: &mut HostSim,
    e: Entity,
    kind: PropKind,
    spot: (f32, f32),
    target: [f32; 3],
    power: u8,
    curve: f32,
    stake: Option<i64>,
) {
    for _ in 0..(3 * shared::TICK_HZ) {
        let b = ball_at(sim, kind);
        if (b.x - spot.0).hypot(b.z - spot.1) < 0.05 && b.y < 0.2 {
            break;
        }
        run(sim, 1);
    }
    // (A put moves the player's body over one tick: one put right next to
    // the ball would sweep through it. Come in from behind and settle.)
    let b = ball_at(sim, kind);
    put(sim, e, (b.x, b.z - 3.0));
    run(sim, 2);
    put(sim, e, (b.x, b.z - 0.7));
    run(sim, 4);
    let b = ball_at(sim, kind);
    let (yaw, pitch) = kicks::aim_at(b.to_array(), target, power).expect("in range");
    let kick = Kick { power, yaw, pitch, curve };
    match stake {
        Some(stake) => act(sim, e, GameAction::FieldGoal { stake, kick }),
        None => act(sim, e, GameAction::Kick { kick }),
    }
}

/// Run until the penalty view counts another kick (at most 4 s).
fn next_kick(sim: &mut HostSim, before: u32) -> KickView {
    for _ in 0..(4 * shared::TICK_HZ) {
        let v: PenaltyView = view(sim);
        if v.kicks > before {
            return v.last.unwrap();
        }
        run(sim, 1);
    }
    panic!("the kick was never judged");
}

fn corner(x: f32, h: f32) -> [f32; 3] {
    [kicks::GOAL.0 + x, h, kicks::GOAL.1]
}

#[test]
fn a_shootout_against_a_goalie_who_stands_still_pays_the_best_kicker() {
    let (mut sim, ps) = room(3);
    let (a, b, keeper) = (ps[0], ps[1], ps[2]);
    put(&mut sim, keeper, kicks::GOALIE_SPOT);
    act(&mut sim, keeper, GameAction::TakeGoal);
    put(&mut sim, a, (kicks::PENALTY_SPOT.0 - 1.5, kicks::PENALTY_SPOT.1 - 2.0));
    put(&mut sim, b, (kicks::PENALTY_SPOT.0 + 1.5, kicks::PENALTY_SPOT.1 - 2.0));
    act(&mut sim, a, GameAction::JoinShootout { fun: false });
    act(&mut sim, b, GameAction::JoinShootout { fun: false });
    act(&mut sim, a, GameAction::StartShootout);
    for _ in 0..5 {
        for (e, target, curve) in [(a, corner(2.6, 0.6), 0.0), (b, corner(3.4, 0.6), -1.0)] {
            let before = view::<PenaltyView>(&mut sim).kicks;
            put(&mut sim, keeper, kicks::GOALIE_SPOT);
            kick_at(&mut sim, e, PropKind::SoccerBall, kicks::PENALTY_SPOT, target, 70, curve, None);
            let k = next_kick(&mut sim, before);
            // a slots it in the corner past the keeper; b's curl goes wide.
            let want = if e == a { KickResult::Goal } else { KickResult::Wide };
            assert_eq!(k.result, want, "{k:?}");
        }
    }
    let v: PenaltyView = view(&mut sim);
    assert_eq!(v.kicks, 10);
    assert!(!v.shootout.started, "over");
    assert_eq!(v.paid, vec![(0x100, 2 * kicks::ENTRY)]);
}

#[test]
fn a_dive_the_right_way_saves_and_bettors_settle() {
    let (mut sim, ps) = room(3);
    let (kicker, goalie, punter) = (ps[0], ps[1], ps[2]);
    put(&mut sim, goalie, kicks::GOALIE_SPOT);
    put(&mut sim, punter, (-10.0, 12.0));
    act(&mut sim, goalie, GameAction::TakeGoal);
    act(&mut sim, punter, GameAction::BetKick { goal: true, amount: 40 });
    let gp = pocket(&sim, goalie);
    // A low kick to the keeper's right (the kicker's left, +x), and he goes that way.
    kick_at(&mut sim, kicker, PropKind::SoccerBall, kicks::PENALTY_SPOT, corner(1.6, 0.4), 55, 0.0, None);
    act(&mut sim, goalie, GameAction::Dive { dive: Dive::Left });
    let k = next_kick(&mut sim, 0);
    assert_eq!(k.result, KickResult::Saved, "{k:?}");
    assert_eq!(pocket(&sim, punter), 1000 - 40, "lost the bet");
    // A practice kick (no shootout): the save earns the goalie nothing.
    assert_eq!(pocket(&sim, goalie), gp, "a practice save pays nothing");
    // The ball goes back to the spot.
    run(&mut sim, 3 * shared::TICK_HZ);
    let b = ball_at(&mut sim, PropKind::SoccerBall);
    assert!((b.x - kicks::PENALTY_SPOT.0).hypot(b.z - kicks::PENALTY_SPOT.1) < 0.05, "back on the spot: {b}");
}

#[test]
fn field_goals_pay_by_distance() {
    let (mut sim, ps) = room(1);
    let p = ps[0];
    let wait = |sim: &mut HostSim, before: u32| {
        for _ in 0..(5 * shared::TICK_HZ) {
            let v: FieldGoalView = view(sim);
            if v.kicks > before {
                return v.last.unwrap();
            }
            run(sim, 1);
        }
        panic!("the kick was never judged");
    };
    // The football sits on the 15-yard tee: carry it to the 20.
    let (_, tx, tz, _) = kicks::TEES[2];
    let b = ball_at(&mut sim, PropKind::Football);
    put(&mut sim, p, (b.x, b.z - 3.0));
    run(&mut sim, 2);
    put(&mut sim, p, (b.x, b.z - 0.7));
    run(&mut sim, 4);
    sim.set_input(p, PlayerInput::new(Vec2::ZERO, std::f32::consts::PI, 0.0, shared::movement::buttons::INTERACT));
    run(&mut sim, 2);
    // Facing the uprights (+z) the whole way.
    let facing = PlayerInput::new(Vec2::ZERO, std::f32::consts::PI, 0.0, 0);
    sim.set_input(p, facing);
    run(&mut sim, 2);
    assert!(sim.world().get::<host::physics::Hands>(p).unwrap().held.is_some(), "picked up the football");
    // Walk it over (in steps: a long put would fling it out of the hands).
    for i in 1..=20 {
        let f = i as f32 / 20.0;
        put(&mut sim, p, (b.x + (tx - b.x) * f, (b.z - 0.7) + (tz - b.z) * f));
        run(&mut sim, 2);
    }
    sim.set_input(p, PlayerInput::new(Vec2::ZERO, std::f32::consts::PI, 0.0, shared::movement::buttons::DROP));
    run(&mut sim, 2);
    sim.set_input(p, facing);
    run(&mut sim, 64);
    let b = ball_at(&mut sim, PropKind::Football);
    assert_eq!(kicks::tee_at(b.x, b.z).map(|t| t.0), Some(20), "dropped on the 20: {b}");
    let over = [kicks::UPRIGHTS.0, kicks::CROSSBAR + 1.5, kicks::UPRIGHTS.1];
    kick_at(&mut sim, p, PropKind::Football, (b.x, b.z), over, 85, 0.0, Some(50));
    let r = wait(&mut sim, 0);
    assert!(r.good && r.yards == 20, "{r:?}");
    assert_eq!(pocket(&sim, p), 1000 + 4 * 50);
    // Under the bar: no good. (The ball comes back to the 20.)
    let low = [kicks::UPRIGHTS.0, 1.0, kicks::UPRIGHTS.1];
    kick_at(&mut sim, p, PropKind::Football, (b.x, b.z), low, 85, 0.0, Some(50));
    let r = wait(&mut sim, 1);
    assert!(!r.good && !r.cleared, "under the bar: {r:?}");
    assert_eq!(pocket(&sim, p), 1000 + 3 * 50);
}

#[test]
fn the_gauntlet_tackles_a_runner_who_stands_still_and_pays_one_who_scores() {
    let (mut sim, ps) = room(1);
    let p = ps[0];
    put(&mut sim, p, (gauntlet::LANE_MID, gauntlet::START_Z));
    act(&mut sim, p, GameAction::Run { stake: 30 });
    assert_eq!(pocket(&sim, p), 970);
    run(&mut sim, gauntlet::SECONDS * 64);
    let v: GauntletView = view(&mut sim);
    assert_eq!(v.last.map(|l| l.1), Some(RunEnd::Tackled));

    // Run the lane like the scripted runner, moving the player directly.
    let mut scored = false;
    for _ in 0..10 {
        put(&mut sim, p, (gauntlet::LANE_MID, gauntlet::START_Z));
        act(&mut sim, p, GameAction::Run { stake: 30 });
        loop {
            let v: GauntletView = view(&mut sim);
            let Some(run) = v.run else { break };
            let at = sim.world().get::<PlayerPos>(p).unwrap().0;
            let at2 = [at.x, at.z];
            let near = run
                .tacklers
                .iter()
                .any(|t| t.stunned == 0 && ((t.pos[0] - at.x).powi(2) + (t.pos[1] - at.z).powi(2)).sqrt() < 1.2);
            if near {
                sim.game_request(p, GameAction::StiffArm);
                sim.game_request(p, GameAction::Dodge { right: at.x < gauntlet::LANE_MID });
            }
            let dir = gauntlet::bot_run_dir(&run, at2);
            put(&mut sim, p, (at.x + dir[0] * 6.0 / 64.0, at.z + dir[1] * 6.0 / 64.0));
            sim.tick();
        }
        if view::<GauntletView>(&mut sim).last.map(|l| l.1) == Some(RunEnd::Scored) {
            scored = true;
            break;
        }
    }
    assert!(scored, "the scripted runner scores within ten runs");
}

#[test]
fn the_pit_rewinds_targets_to_what_the_shooter_saw() {
    let (mut sim, ps) = room(2);
    let (a, b) = (ps[0], ps[1]);
    put(&mut sim, a, (-25.0, 0.0));
    put(&mut sim, b, (-25.0, -4.0));
    act(&mut sim, a, GameAction::JoinPit { teams: false, fun: false });
    act(&mut sim, b, GameAction::JoinPit { teams: false, fun: false });
    act(&mut sim, a, GameAction::StartPit);
    put(&mut sim, a, (-25.0, -5.0));
    act(&mut sim, a, GameAction::Pick { weapon: Weapon::Pistol });
    put(&mut sim, a, (-25.0, 0.0));
    run(&mut sim, 2);
    let seen = sim.net_tick();
    // b steps aside after a saw him; a's shot at what a saw 4 ticks ago hits.
    put(&mut sim, b, (-23.0, -4.0));
    run(&mut sim, 4);
    act(&mut sim, a, GameAction::Fire { yaw: 0.0, pitch: -0.05, view_tick: seen });
    let v: PitView = view(&mut sim);
    assert_eq!(v.tracers.last().unwrap().hit, Some(0x101));
    // A shot at where b is now (seen now) misses the empty spot.
    run(&mut sim, 20);
    let now = sim.net_tick();
    act(&mut sim, a, GameAction::Fire { yaw: 0.0, pitch: -0.05, view_tick: now });
    let v: PitView = view(&mut sim);
    assert_eq!(v.tracers.last().unwrap().hit, None);
    // b steps back in line and takes the second hit.
    put(&mut sim, b, (-25.0, -4.0));
    run(&mut sim, 20);
    let now = sim.net_tick();
    act(&mut sim, a, GameAction::Fire { yaw: 0.0, pitch: -0.05, view_tick: now });
    let v: PitView = view(&mut sim);
    assert_eq!(v.round.fighters[0].kills, 1, "two pistol hits");
    assert!(v.round.fighters[1].down > 0);
    // Down for 3 s, then up at a corner.
    run(&mut sim, pit::DOWN_TICKS + 2);
    let at = sim.world().get::<PlayerPos>(b).unwrap().0;
    assert!(pit::CORNERS.iter().any(|(x, z)| (at.x - x).abs() < 0.01 && (at.z - z).abs() < 0.01), "{at}");
    // The round ends and a takes the pot.
    let before = pocket(&sim, a);
    run(&mut sim, pit::ROUND_SECS * 64);
    let v: PitView = view(&mut sim);
    assert_eq!(v.rounds, 1);
    assert_eq!(pocket(&sim, a), before + 2 * pit::ENTRY);
}

#[test]
fn rewinding_stops_at_200_ms() {
    let (mut sim, ps) = room(2);
    let (a, b) = (ps[0], ps[1]);
    put(&mut sim, a, (-25.0, 0.0));
    put(&mut sim, b, (-25.0, -4.0));
    act(&mut sim, a, GameAction::JoinPit { teams: false, fun: false });
    act(&mut sim, b, GameAction::JoinPit { teams: false, fun: false });
    act(&mut sim, a, GameAction::StartPit);
    put(&mut sim, a, (-25.0, 5.0));
    act(&mut sim, a, GameAction::Pick { weapon: Weapon::Smg });
    put(&mut sim, a, (-25.0, 0.0));
    run(&mut sim, 2);
    let seen = sim.net_tick();
    put(&mut sim, b, (-22.0, -4.0));
    run(&mut sim, 30);
    act(&mut sim, a, GameAction::Fire { yaw: 0.0, pitch: -0.05, view_tick: seen });
    let v: PitView = view(&mut sim);
    assert_eq!(v.tracers.last().unwrap().hit, None, "half a second is too old to rewind to");
}

#[test]
fn a_thrown_beer_that_hits_a_player_adds_twenty_drunk() {
    let (mut sim, ps) = room(1);
    let p = ps[0];
    put(&mut sim, p, (0.0, 3.0));
    run(&mut sim, 1);
    let mut glass = sim.world_mut().spawn(host::beer::glass(Vec3::new(0.0, 1.0, 3.6), 0.9, false, 0x999, None));
    glass.insert(avian3d::prelude::LinearVelocity(Vec3::new(0.0, 0.0, -6.0)));
    run(&mut sim, 6);
    assert_eq!(sim.world().get::<Drunk>(p).unwrap().level, 20);
}

