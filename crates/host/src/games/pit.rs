//! The fight pit (plan section 5.9) on the host, with lag compensation.
//!
//! The host keeps every player's position for the last half second, by
//! lightyear tick. A shot names the tick the shooter saw the others at (its
//! interpolated view); the host rewinds the other fighters to that tick (at
//! most 200 ms back) and casts the ray against them there.

use std::collections::{BTreeMap, VecDeque};

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::minigame::Who;
use shared::pit::{self, Round, Weapon};
use shared::protocol::*;
use shared::sports::{aim_error, signed_unit};
use shared::world::{Room, room_at};

use super::{GamePlayers, GameQueue, GameRngs, charge, pay, player_id, who};
use crate::casino::Audit;

/// Ticks of position history kept per player.
const HISTORY_TICKS: usize = 32;
/// Tracers kept for clients to draw.
const TRACERS: usize = 16;
/// How far the bat knocks a fighter back.
const KNOCKBACK: f32 = 1.0;

/// Every player's recent positions: id -> (lightyear tick, position).
#[derive(Resource, Default)]
pub struct History(pub BTreeMap<u64, VecDeque<(u32, Vec3)>>);

impl History {
    /// Where `id` stood at `tick` (the closest recorded tick at or before it).
    pub fn at(&self, id: u64, tick: u32) -> Option<Vec3> {
        let h = self.0.get(&id)?;
        h.iter().rev().find(|(t, _)| *t <= tick).or(h.front()).map(|(_, p)| *p)
    }
}

pub fn record_history(
    timeline: Res<LocalTimeline>,
    mut history: ResMut<History>,
    players: Query<(&Player, &PlayerPos)>,
) {
    let now = timeline.tick().0;
    for (p, pos) in &players {
        let h = history.0.entry(p.id).or_default();
        h.push_back((now, pos.0));
        while h.len() > HISTORY_TICKS {
            h.pop_front();
        }
    }
    history.0.retain(|id, _| players.iter().any(|(p, _)| p.id == *id));
}

#[derive(Component, Default)]
pub struct PitHost {
    round: Round,
    tracers: Vec<Tracer>,
    shots: u32,
    paid: Vec<(u64, i64)>,
    rounds: u32,
    /// The next corner to respawn at.
    corner: usize,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    queue: Res<GameQueue>,
    tick: Res<crate::TickCount>,
    timeline: Res<LocalTimeline>,
    history: Res<History>,
    mut rngs: ResMut<GameRngs>,
    mut audit: ResMut<Audit>,
    mut arena: Query<(&mut PitHost, &mut PitView)>,
    mut players: GamePlayers,
) {
    let Ok((mut host, mut view)) = arena.single_mut() else { return };
    let host = &mut *host;
    let now = timeline.tick().0;
    for (e, action) in &queue.0 {
        let Ok((_, player, pos, _, mut pocket, drunk, focus)) = players.get_mut(*e) else { continue };
        let me = who(player);
        let in_basement = room_at(pos.0.x, pos.0.z) == Some(Room::Basement);
        match *action {
            GameAction::JoinPit { teams } if in_basement && !host.round.started => {
                if charge(&mut pocket, pit::ENTRY) && !host.round.join(me, pit::ENTRY, teams) {
                    pocket.0 += pit::ENTRY;
                }
            }
            GameAction::StartPit if host.round.fighter(me).is_some() => {
                host.round.start();
            }
            GameAction::Pick { weapon } => {
                let at_rack = pit::RACKS
                    .iter()
                    .any(|(w, x, z)| *w == weapon && (pos.0.x - x).hypot(pos.0.z - z) <= pit::RACK_REACH);
                if at_rack {
                    host.round.pick(me, weapon);
                }
            }
            GameAction::Fire { yaw, pitch, view_tick } => {
                if !pit::in_pit(pos.0.x, pos.0.z) {
                    continue;
                }
                let Some(weapon) = host.round.fire(me) else { continue };
                let origin = [pos.0.x, pos.0.y + pit::EYE_HEIGHT, pos.0.z];
                let err = aim_error(drunk.map_or(0, |d| d.level), focus.map_or(0, |f| f.level));
                // Rewind the others to what the shooter saw, at most 200 ms back.
                let seen = view_tick.clamp(now.saturating_sub(pit::MAX_REWIND_TICKS), now);
                let targets: Vec<(Who, u64, Vec3)> = host
                    .round
                    .fighters
                    .iter()
                    .filter(|f| f.who != me && f.down == 0)
                    .filter_map(|f| {
                        let id = player_id(f.who);
                        history.at(id, seen).map(|p| (f.who, id, p))
                    })
                    .collect();
                let mut d = rngs.pit.at(tick.0, &mut audit.0);
                let (y0, p0) = (yaw + signed_unit(&mut d) * err, pitch + signed_unit(&mut d) * err);
                let mut hits: Vec<(Who, i32, [f32; 3])> = Vec::new();
                for _ in 0..weapon.pellets() {
                    let (y, p) = if weapon.spread() > 0.0 {
                        (y0 + signed_unit(&mut d) * weapon.spread(), p0 + signed_unit(&mut d) * weapon.spread())
                    } else {
                        (y0, p0)
                    };
                    let dir = pit::look_dir(y, p);
                    let nearest = targets
                        .iter()
                        .filter_map(|(w, _, at)| {
                            pit::hit_player(origin, dir, at.to_array()).map(|(t, part)| (*w, t, part))
                        })
                        .filter(|(_, t, _)| *t <= weapon.range())
                        .min_by(|a, b| a.1.total_cmp(&b.1));
                    let reach = nearest.map_or(weapon.range(), |n| n.1);
                    let to = [origin[0] + dir[0] * reach, origin[1] + dir[1] * reach, origin[2] + dir[2] * reach];
                    match nearest {
                        Some((w, t, part)) => {
                            hits.push((w, pit::damage(weapon, part, t), dir));
                            host.tracers.push(Tracer { by: player.id, from: origin, to, hit: Some(player_id(w)) });
                        }
                        None => host.tracers.push(Tracer { by: player.id, from: origin, to, hit: None }),
                    }
                }
                host.shots += 1;
                for (w, dmg, dir) in hits {
                    host.round.hit(me, w, dmg);
                    if weapon == Weapon::Bat
                        && let Some((.., mut tp, _, _, _, _)) = players.iter_mut().find(|(_, p, ..)| who(p) == w)
                    {
                        tp.0.x += dir[0] * KNOCKBACK;
                        tp.0.z += dir[2] * KNOCKBACK;
                    }
                }
                let keep = host.tracers.len().saturating_sub(TRACERS);
                host.tracers.drain(..keep);
            }
            _ => {}
        }
    }

    // Down fighters get up at a corner; the clock runs.
    let (up, over) = host.round.step();
    for w in up {
        let (x, z) = pit::CORNERS[host.corner % pit::CORNERS.len()];
        host.corner += 1;
        if let Some((.., mut p, _, _, _, _)) = players.iter_mut().find(|(_, pl, ..)| who(pl) == w) {
            p.0 = Vec3::new(x, 0.0, z);
        }
    }
    if over {
        let round = std::mem::take(&mut host.round);
        host.paid = round.payouts().into_iter().map(|(w, a)| (player_id(w), a)).collect();
        for (id, amount) in &host.paid {
            pay(&mut players, *id, *amount);
        }
        host.rounds += 1;
        host.tracers.clear();
    }
    // A round everyone left is dropped (their fees are gone with them).
    if !host.round.fighters.is_empty()
        && host.round.fighters.iter().all(|f| !players.iter().any(|(_, p, ..)| who(p) == f.who))
    {
        host.round = Round::new();
    }
    let next = PitView {
        round: host.round.clone(),
        tracers: host.tracers.clone(),
        shots: host.shots,
        paid: host.paid.clone(),
        rounds: host.rounds,
    };
    view.set_if_neq(next);
}
