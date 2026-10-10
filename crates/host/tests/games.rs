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

#[test]
fn five_of_five_wins_the_pot_and_draws_a_crowd() {
    let (mut sim, ps) = room(2);
    let (a, b) = (ps[0], ps[1]);
    put(&mut sim, a, (14.0, 2.0));
    put(&mut sim, b, (13.0, 0.0));
    act(&mut sim, a, GameAction::JoinHoops { mode: Mode::ThreeOfFive });
    act(&mut sim, b, GameAction::JoinHoops { mode: Mode::ThreeOfFive });
    let (pa, pb) = (pocket(&sim, a), pocket(&sim, b));
    assert_eq!(pa, 1000 - hoops::ENTRY);
    act(&mut sim, a, GameAction::StartHoops);
    for _ in 0..5 {
        let (yaw, pitch, power) = hoops::perfect_aim([14.0, 2.0]);
        act(&mut sim, a, GameAction::Shoot { power, yaw, pitch });
        // b throws it at the floor.
        act(&mut sim, b, GameAction::Shoot { power: 0, yaw: 0.0, pitch: -1.0 });
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
    act(&mut sim, a, GameAction::JoinHoops { mode: Mode::Horse });
    act(&mut sim, b, GameAction::JoinHoops { mode: Mode::Horse });
    act(&mut sim, a, GameAction::StartHoops);
    let mut guard = 0;
    while view::<HoopsView>(&mut sim).contest.is_some() && guard < 100 {
        let c = view::<HoopsView>(&mut sim).contest.unwrap();
        let shooter = c.shooter().unwrap();
        // a always makes it; b always misses.
        if shooter == shared::minigame::Who::Player(0x100) {
            let (yaw, pitch, power) = hoops::perfect_aim([14.0, 2.0]);
            act(&mut sim, a, GameAction::Shoot { power, yaw, pitch });
        } else {
            act(&mut sim, b, GameAction::Shoot { power: 0, yaw: 0.0, pitch: -1.0 });
        }
        guard += 1;
    }
    let v: HoopsView = view(&mut sim);
    assert_eq!(v.paid, vec![(0x100, 2 * hoops::ENTRY)], "after {guard} shots");
}

#[test]
fn a_shootout_against_the_npc_goalie_pays_the_best_kicker() {
    let (mut sim, ps) = room(2);
    let (a, b) = (ps[0], ps[1]);
    put(&mut sim, a, kicks::PENALTY_SPOT);
    put(&mut sim, b, (kicks::PENALTY_SPOT.0 + 0.5, kicks::PENALTY_SPOT.1));
    act(&mut sim, a, GameAction::JoinShootout);
    act(&mut sim, b, GameAction::JoinShootout);
    act(&mut sim, a, GameAction::StartShootout);
    for _ in 0..5 {
        for (e, aim) in [(a, 0.9f32), (b, 1.0)] {
            let kick = Kick { aim, power: 60, curve: if e == b { 1.0 } else { 0.0 } };
            act(&mut sim, e, GameAction::Kick { kick });
            run(&mut sim, kicks::flight_ticks(60) + 2);
            assert!(!view::<PenaltyView>(&mut sim).in_flight);
        }
    }
    let v: PenaltyView = view(&mut sim);
    assert_eq!(v.kicks, 10);
    assert!(!v.shootout.started, "over");
    // b's curve takes every kick wide: a wins unless the goalie saved all five.
    assert_eq!(v.paid.iter().map(|p| p.1).sum::<i64>(), 2 * kicks::ENTRY);
    assert!(v.paid.iter().all(|p| p.0 == 0x100) || v.paid.len() == 2);
}

#[test]
fn a_player_goalie_saves_with_an_early_dive_and_bettors_settle() {
    let (mut sim, ps) = room(3);
    let (kicker, goalie, punter) = (ps[0], ps[1], ps[2]);
    put(&mut sim, kicker, kicks::PENALTY_SPOT);
    put(&mut sim, goalie, kicks::GOALIE_SPOT);
    put(&mut sim, punter, (-10.0, 12.0));
    act(&mut sim, goalie, GameAction::TakeGoal);
    act(&mut sim, punter, GameAction::BetKick { goal: true, amount: 40 });
    act(&mut sim, goalie, GameAction::Dive { dive: Dive::Left });
    let gp = pocket(&sim, goalie);
    act(&mut sim, kicker, GameAction::Kick { kick: Kick { aim: -0.7, power: 50, curve: 0.0 } });
    run(&mut sim, kicks::flight_ticks(50) + 2);
    let v: PenaltyView = view(&mut sim);
    assert_eq!(v.last.unwrap().result, KickResult::Saved);
    assert_eq!(pocket(&sim, punter), 1000 - 40, "lost the bet");
    assert_eq!(pocket(&sim, goalie), gp + kicks::ENTRY, "a save pays the goalie");
}

#[test]
fn field_goals_pay_by_distance() {
    let (mut sim, ps) = room(1);
    let p = ps[0];
    put(&mut sim, p, kicks::TEE);
    act(&mut sim, p, GameAction::FieldGoal { yards: 40, stake: 50, kick: Kick { aim: 0.0, power: 76, curve: 0.0 } });
    let v: FieldGoalView = view(&mut sim);
    assert!(v.last.unwrap().good);
    assert_eq!(pocket(&sim, p), 1000 + 4 * 50);
    act(&mut sim, p, GameAction::FieldGoal { yards: 40, stake: 50, kick: Kick { aim: 0.0, power: 60, curve: 0.0 } });
    assert!(!view::<FieldGoalView>(&mut sim).last.unwrap().good, "short");
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
    act(&mut sim, a, GameAction::JoinPit { teams: false });
    act(&mut sim, b, GameAction::JoinPit { teams: false });
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
    act(&mut sim, a, GameAction::JoinPit { teams: false });
    act(&mut sim, b, GameAction::JoinPit { teams: false });
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
