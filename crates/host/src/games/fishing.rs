//! Fishing spots on the pier (plan section 5.5).

use bevy::prelude::*;
use shared::fishing::{self, CastParams, Catch, FishInput, Fishing, LinePhase, Side};
use shared::minigame::{Bet, Minigame, Who};
use shared::protocol::*;

use super::{GamePlayers, GameQueue, GameRngs, charge, pay};
use crate::casino::Audit;

/// How far a fisher may wander from the spot before the line is lost.
const LEAVE_DISTANCE: f32 = 3.0;

#[derive(Component)]
pub struct FishingHost {
    spot: u8,
    fisher: Option<(Entity, u64)>,
    line: Option<fishing::Line>,
    held: bool,
    bets: Vec<(u64, Side, i64)>,
    last: Option<(u64, Catch)>,
    casts: u32,
}

impl FishingHost {
    pub fn new(spot: u8) -> Self {
        Self { spot, fisher: None, line: None, held: false, bets: Vec::new(), last: None, casts: 0 }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    queue: Res<GameQueue>,
    tick: Res<crate::TickCount>,
    mut rngs: ResMut<GameRngs>,
    mut audit: ResMut<Audit>,
    mut spots: Query<(&mut FishingHost, &mut FishingView)>,
    mut players: GamePlayers,
    mut room: Query<&mut RunLedger, With<RoomState>>,
    mut inventories: Query<(&Player, &mut Inventory)>,
) {
    let mut sorted: Vec<_> = spots.iter_mut().collect();
    sorted.sort_by_key(|(h, _)| h.spot);
    for (mut host, mut view) in sorted {
        let host = &mut *host;
        let (sx, sz) = fishing::SPOTS[usize::from(host.spot)];
        for (e, action) in &queue.0 {
            let Ok((_, player, pos, _, mut pocket, _, focus)) = players.get_mut(*e) else { continue };
            let here = fishing::spot_at(pos.0.x, pos.0.z) == Some(host.spot);
            let active = host.line.is_some_and(|l| !matches!(l.phase, LinePhase::Done(_)));
            let mine = host.fisher.is_some_and(|(f, _)| f == *e);
            match *action {
                GameAction::Cast { power } if here && !active => {
                    let focus = focus.is_some_and(|f| shared::buffs::shows_count(f.level));
                    let mut d = rngs.fishing.at(tick.0, &mut audit.0);
                    host.line = Some(Fishing::start(&mut d, &CastParams { power: power.min(100), focus }));
                    host.fisher = Some((*e, player.id));
                    host.held = false;
                    host.bets.clear();
                    host.casts += 1;
                }
                GameAction::Hook if mine && active => {
                    if let Some(line) = &mut host.line {
                        let mut d = rngs.fishing.at(tick.0, &mut audit.0);
                        if let Ok(Some(c)) = Fishing::apply(line, Who::Player(player.id), FishInput::Hook, &mut d) {
                            host.last = Some((player.id, c));
                        }
                    }
                }
                GameAction::Reel { held } if mine => host.held = held,
                GameAction::BetCatch { spot, side, amount }
                    if spot == host.spot
                        && !mine
                        && host.line.is_some_and(|l| l.phase == LinePhase::Reeling)
                        && amount <= fishing::BET_MAX
                        && !host.bets.iter().any(|b| b.0 == player.id)
                        && charge(&mut pocket, amount) =>
                {
                    host.bets.push((player.id, side, amount));
                }
                _ => {}
            }
        }

        // A fisher who left the spot (or the room) loses the fish.
        if let Some((e, _)) = host.fisher
            && host.line.is_some_and(|l| !matches!(l.phase, LinePhase::Done(_)))
            && !players.get(e).is_ok_and(|(_, _, p, ..)| (p.0.x - sx).hypot(p.0.z - sz) < LEAVE_DISTANCE)
            && let Some(line) = &mut host.line
        {
            line.phase = LinePhase::Done(Catch::Escaped);
            finish(host, Catch::Escaped, &mut players, &mut room, &mut inventories);
        }

        let mut ended = None;
        if let (Some(line), Some((_, id))) = (&mut host.line, host.fisher)
            && !matches!(line.phase, LinePhase::Done(_))
        {
            let mut d = rngs.fishing.at(tick.0, &mut audit.0);
            if let Ok(Some(c)) = Fishing::apply(line, Who::Player(id), FishInput::Tick { held: host.held }, &mut d) {
                ended = Some(c);
            }
        }
        if let Some(c) = ended {
            finish(host, c, &mut players, &mut room, &mut inventories);
        }

        let line = host.line.filter(|l| !matches!(l.phase, LinePhase::Done(_)));
        let next = FishingView {
            spot: host.spot,
            fisher: line.and(host.fisher.map(|f| f.1)),
            phase: match line.map(|l| l.phase) {
                Some(LinePhase::Waiting { .. }) => FishPhase::Waiting,
                Some(LinePhase::Biting { .. }) => FishPhase::Biting,
                Some(LinePhase::Reeling) => FishPhase::Reeling,
                _ => FishPhase::Idle,
            },
            zone: line.map_or(0, |l| l.zone),
            tension: line.map_or(0, |l| l.tension as u16),
            band: line.map_or((0, 0), |l| (l.band.0 as u16, l.band.1 as u16)),
            progress: line.map_or(0, |l| l.progress.min(u32::from(u16::MAX)) as u16),
            need: line.map_or(0, |l| l.need().min(u32::from(u16::MAX)) as u16),
            last: host.last,
            bets: host.bets.clone(),
            casts: host.casts,
        };
        view.set_if_neq(next);
    }
}

/// The fight is over: sell a landed fish and settle the bets.
fn finish(
    host: &mut FishingHost,
    c: Catch,
    players: &mut GamePlayers,
    room: &mut Query<&mut RunLedger, With<RoomState>>,
    inventories: &mut Query<(&Player, &mut Inventory)>,
) {
    let Some((_, id)) = host.fisher else { return };
    host.last = Some((id, c));
    if let Catch::Landed(fish) = c {
        pay(players, id, fish.value());
        // A real fish can go to the kitchen for a plate.
        if fish != shared::fishing::Fish::Boot
            && let Some((_, mut inv)) = inventories.iter_mut().find(|(p, _)| p.id == id)
        {
            inv.fish = inv.fish.saturating_add(1);
        }
    }
    let bets: Vec<Bet<Side>> = host
        .bets
        .iter()
        .map(|(p, side, amount)| Bet { who: Who::Player(*p), amount: *amount, selection: *side })
        .collect();
    let payouts = Fishing::payout(&c, &bets);
    let mut house = 0;
    for p in payouts {
        let paid = if pay(players, super::player_id(p.who), p.returned) { p.returned } else { 0 };
        house += p.staked - paid;
    }
    if let Ok(mut run) = room.single_mut() {
        run.ledger.house += house;
    }
    host.bets.clear();
}
