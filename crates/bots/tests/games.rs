//! Eight bots walk to the side games and play them for a minute: two
//! fishers, a shooter on the roof, a penalty kicker, a field goal kicker, a
//! gauntlet runner and two fighters in the pit. Every game makes progress,
//! and every client's replicated state keeps matching the host's.

use std::collections::VecDeque;

use bots::games::SideGame;
use bots::{LocalRoom, Script};
use host::HostConfig;
use host::economy::Preset;
use shared::pit::Weapon;
use shared::protocol::*;
use shared::state::replicated_hash;

fn view<T: bevy::prelude::Component + Clone>(room: &mut LocalRoom) -> T {
    let w = room.host.world_mut();
    w.query::<&T>().iter(w).next().unwrap().clone()
}

#[test]
fn eight_bots_play_every_side_game_and_agree_with_the_host() {
    // The plan's two-minute Setup: no customers, no chaos, money to play with.
    let config = HostConfig { preset: Preset::Casino, manual_chaos: true, ..Default::default() };
    let mut room = LocalRoom::with_config(8, config, |i| {
        Script::Game(match i {
            0 => SideGame::Fish(0),
            1 => SideGame::Fish(1),
            2 => SideGame::Hoops,
            3 => SideGame::Kicks,
            4 => SideGame::FieldGoal,
            5 => SideGame::Gauntlet,
            6 => SideGame::Pit(Weapon::Smg),
            _ => SideGame::Pit(Weapon::Pistol),
        })
    });
    let mut history: VecDeque<u64> = VecDeque::new();
    let mut checks = 0;
    let start = std::time::Instant::now();
    for tick in 1..=(70 * 64u32) {
        room.step();
        if let Some(wait) = (shared::TICK * tick).checked_sub(start.elapsed()) {
            std::thread::sleep(wait);
        }
        history.push_back(replicated_hash(room.host.world_mut()));
        if history.len() > 128 {
            history.pop_front();
        }
        if tick % 640 == 0 {
            for bot in 0..room.bots.len() {
                let mut agreed = false;
                for _ in 0..64 {
                    if history.contains(&replicated_hash(room.bots[bot].world_mut())) {
                        agreed = true;
                        break;
                    }
                    room.step();
                    std::thread::sleep(shared::TICK);
                    history.push_back(replicated_hash(room.host.world_mut()));
                    if history.len() > 128 {
                        history.pop_front();
                    }
                }
                assert!(agreed, "tick {tick}: bot {bot} never matched the host within a second");
                checks += 1;
            }
        }
    }
    assert_eq!(checks, 56);
    let w = room.host.world_mut();
    let casts: u32 = w.query::<&FishingView>().iter(w).map(|v| v.casts).sum();
    let fished: Vec<_> = w.query::<&FishingView>().iter(w).filter_map(|v| v.last).collect();
    let hoops: HoopsView = view(&mut room);
    let pens: PenaltyView = view(&mut room);
    let fg: FieldGoalView = view(&mut room);
    let lane: GauntletView = view(&mut room);
    let pit: PitView = view(&mut room);
    println!(
        "casts {casts} ({fished:?}), shots {} paid {:?}, kicks {} paid {:?}, field goals {}, runs {} last {:?}, pit shots {} rounds {} fighters {:?}",
        hoops.shots,
        hoops.paid,
        pens.kicks,
        pens.paid,
        fg.kicks,
        lane.runs,
        lane.last,
        pit.shots,
        pit.rounds,
        pit.round.fighters.iter().map(|f| (f.kills, f.weapon)).collect::<Vec<_>>()
    );
    assert!(casts >= 2 && !fished.is_empty(), "the fishers cast and something ended");
    assert!(hoops.shots >= 5 && !hoops.paid.is_empty(), "3 of 5 played to the end");
    assert!(pens.kicks >= 5 && !pens.paid.is_empty(), "a shootout played to the end");
    assert!(fg.kicks >= 5);
    assert!(lane.runs >= 1 && lane.last.is_some());
    assert!(pit.shots >= 10 && (pit.rounds >= 1 || pit.round.fighters.iter().any(|f| f.kills > 0)), "the pit fought");
}
