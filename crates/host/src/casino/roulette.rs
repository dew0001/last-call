//! The roulette wheel on the host (plan section 5.2).
//!
//! Bets go on the layout until the croupier spins. The result is drawn the
//! moment the spin starts and sent to clients so the wheel lands on it; the
//! spin lasts 6 seconds. Winners are paid (players as a chip stack on the
//! rail), and each losing bet leaves a chip on the layout. The croupier must
//! rake those off before the next spin; customers grow impatient while the
//! wheel waits. The croupier earns 10% of the house's win on each spin.

use avian3d::prelude::*;
use bevy::prelude::*;
use lightyear::prelude::*;
use shared::casino::{self, TableAction, TableId};
use shared::customers::Mood;
use shared::minigame::{Bet, Minigame, Roulette, Spin, Who, house_take};
use shared::protocol::*;
use shared::roulette::{self, CROUPIER_COMMISSION_PERCENT, MIN_SPIN_SECS};

use super::{Audit, Players, TableQueue, TableRngs, WantsToLeave, decide, pay_chips, tier};
use crate::customers::{Activity, Npc};

const TABLE: TableId = TableId::Roulette;
/// Seconds of betting before customers put their chips down.
const CUSTOMER_BET_AFTER_TICKS: u32 = 2 * shared::TICK_HZ;
/// Losing chips left on the layout at most (one per losing bet).
const MAX_RAKE_CHIPS: usize = 12;
/// How fast one rake stroke sweeps chips toward the croupier, m/s.
const RAKE_SPEED: f32 = 1.6;

/// A losing chip on the layout, waiting for the rake.
#[derive(Component)]
pub struct RakeChip;

#[derive(Component)]
pub struct RouletteHost {
    croupier: Option<Entity>,
    bets: Vec<(Entity, Bet<roulette::Bet>)>,
    spin: Option<(Spin, u32)>,
    /// Ticks since the last spin ended (betting time).
    betting: u32,
    last: Option<u8>,
    spins: u32,
}

pub fn spawn(mut commands: Commands) {
    commands.spawn((
        Name::new("Roulette table"),
        RouletteHost { croupier: None, bets: Vec::new(), spin: None, betting: 0, last: None, spins: 0 },
        RouletteView::default(),
        Replicate::to_clients(NetworkTarget::All),
    ));
}

/// Is a point on the layout (the felt, not the floor or the croupier's tray)?
pub fn on_layout(p: Vec3) -> bool {
    let (cx, cz) = casino::ROULETTE;
    let (hx, hz) = casino::ROULETTE_HALF;
    (p.x - cx).abs() < hx && (p.z - cz).abs() < hz && p.y > casino::FELT_HEIGHT - 0.05
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    mut commands: Commands,
    queue: Res<TableQueue>,
    tick: Res<crate::TickCount>,
    mut rngs: ResMut<TableRngs>,
    mut audit: ResMut<Audit>,
    mut tables: Query<(&mut RouletteHost, &mut RouletteView)>,
    mut players: Players,
    mut npcs: Query<(Entity, &Customer, &mut Npc), Without<WantsToLeave>>,
    mut chips: Query<(Entity, &Position, &mut LinearVelocity), With<RakeChip>>,
    mut room: Query<&mut RunLedger, With<RoomState>>,
) {
    let Ok((mut host, mut view)) = tables.single_mut() else { return };
    let host = &mut *host;
    let tick = tick.0;

    // Chips swept (or knocked) off the layout are gone.
    let mut to_rake = 0;
    for (e, pos, _) in &chips {
        if on_layout(pos.0) {
            to_rake += 1;
        } else {
            commands.entity(e).despawn();
        }
    }

    if let Some(c) = host.croupier {
        let keep = players.get(c).is_ok_and(|(_, _, pos, _, drunk)| {
            shared::drunk::can_deal(tier(drunk))
                && casino::role_spot(TABLE)
                    .is_some_and(|(x, z)| (pos.0.x - x).hypot(pos.0.z - z) < casino::ROLE_REACH * 2.0)
        });
        if !keep {
            host.croupier = None;
        }
    }

    for (who_e, action) in queue.for_table(TABLE) {
        let Ok((_, player, pos, mut pocket, drunk)) = players.get_mut(who_e) else { continue };
        let tier = tier(drunk);
        let is_croupier = host.croupier == Some(who_e);
        match action {
            TableAction::TakeRole => {
                if host.croupier.is_none()
                    && shared::drunk::can_deal(tier)
                    && casino::at_role_spot(TABLE, pos.0.x, pos.0.z)
                {
                    host.croupier = Some(who_e);
                }
            }
            TableAction::LeaveRole if is_croupier => host.croupier = None,
            TableAction::RouletteBet(bet, amount) => {
                let mine = host.bets.iter().filter(|(e, _)| *e == who_e).count();
                if host.spin.is_none()
                    && bet.is_valid()
                    && (casino::ROULETTE_MIN..=casino::max_bet(casino::ROULETTE_MAX, tier)).contains(&amount)
                    && casino::can_reach(TABLE, pos.0.x, pos.0.z)
                    && mine < casino::ROULETTE_BETS_PER_PLAYER
                    && pocket.0 >= amount
                    && !is_croupier
                {
                    pocket.0 -= amount;
                    host.bets.push((who_e, Bet { who: Who::Player(player.id), amount, selection: bet }));
                }
            }
            TableAction::Spin => {
                if is_croupier && host.spin.is_none() && !host.bets.is_empty() && to_rake == 0 {
                    let spin = decide(
                        &mut rngs,
                        &mut audit,
                        TABLE,
                        tick,
                        |d| Roulette::start(d, &()),
                        |s| Some(shared::audit::Derived::Spin { result: s.result }),
                    );
                    host.spin = Some((spin, MIN_SPIN_SECS * shared::TICK_HZ));
                }
            }
            TableAction::Rake if is_croupier => {
                for (e, pos, mut vel) in &mut chips {
                    if on_layout(pos.0) {
                        vel.0 = Vec3::new(0.0, 0.5, -RAKE_SPEED);
                        commands.entity(e).remove::<Sleeping>();
                    }
                }
            }
            _ => {}
        }
    }

    // Customers at the rail.
    let mut at_table: Vec<Entity> = npcs
        .iter()
        .filter(|(_, c, n)| c.mood == Mood::Gambling && matches!(n.activity, Activity::Table(TABLE, _)))
        .map(|(e, ..)| e)
        .collect();
    at_table.sort_by_key(|e| npcs.get(*e).map_or(0, |(_, c, _)| c.id));

    match &mut host.spin {
        None => {
            host.betting += 1;
            if host.betting >= CUSTOMER_BET_AFTER_TICKS {
                for &e in &at_table {
                    let Ok((_, c, mut npc)) = npcs.get_mut(e) else { continue };
                    if host.bets.iter().any(|(be, _)| *be == e) {
                        // Waiting on the croupier.
                        npc.ticks = npc.ticks.saturating_sub(1);
                        if npc.ticks == 0 {
                            commands.entity(e).insert(WantsToLeave);
                        }
                        continue;
                    }
                    match casino::customer_roulette_bet(npc.start_cash, npc.cash) {
                        Some(amount) => {
                            let bet = roulette::customer_bet(&mut rngs.draw(TABLE, tick, &mut audit));
                            npc.cash -= amount;
                            host.bets.push((e, Bet { who: Who::Customer(c.id), amount, selection: bet }));
                        }
                        None => {
                            commands.entity(e).insert(WantsToLeave);
                        }
                    }
                }
            }
        }
        Some((spin, ticks_left)) => {
            *ticks_left = ticks_left.saturating_sub(1);
            if *ticks_left == 0 {
                let mut rng = rngs.draw(TABLE, tick, &mut audit);
                let outcome = Roulette::apply(spin, Who::Customer(0), (), &mut rng).ok().flatten();
                if let Some(outcome) = outcome {
                    settle(&mut commands, host, outcome.result, &mut players, &mut npcs, &mut room);
                }
                host.spin = None;
                host.betting = 0;
            }
        }
    }

    // Customers who are done.
    for &e in &at_table {
        let Ok((_, _, npc)) = npcs.get(e) else { continue };
        let betting = host.bets.iter().any(|(be, _)| *be == e);
        if !betting && (casino::walks_away(npc.cash, npc.start_cash) || npc.cash < casino::ROULETTE_MIN) {
            commands.entity(e).insert(WantsToLeave);
        }
    }

    let next = RouletteView {
        croupier: host.croupier.and_then(|e| players.get(e).ok().map(|(_, p, ..)| p.id)),
        bets: host.bets.iter().map(|(_, b)| (b.who, b.selection, b.amount)).collect(),
        result: host.spin.as_ref().map(|(s, _)| s.result),
        spinning: host.spin.is_some(),
        seconds_left: host.spin.as_ref().map_or(0, |(_, t)| t.div_ceil(shared::TICK_HZ) as u8),
        to_rake: to_rake as u8,
        last: host.last,
        spins: host.spins,
    };
    view.set_if_neq(next);
}

fn settle(
    commands: &mut Commands,
    host: &mut RouletteHost,
    result: u8,
    players: &mut Players,
    npcs: &mut Query<(Entity, &Customer, &mut Npc), Without<WantsToLeave>>,
    room: &mut Query<&mut RunLedger, With<RoomState>>,
) {
    let bets: Vec<Bet<roulette::Bet>> = host.bets.iter().map(|(_, b)| *b).collect();
    let mut payouts = Roulette::payout(&shared::minigame::RouletteOutcome { result }, &bets);
    let mut losers = 0;
    for (p, (entity, bet)) in payouts.iter_mut().zip(&host.bets) {
        // A spin came: customers' patience starts over.
        if let Ok((_, _, mut npc)) = npcs.get_mut(*entity) {
            npc.ticks = casino::TABLE_PATIENCE_SECS * shared::TICK_HZ;
        }
        if p.returned == 0 {
            if losers < MAX_RAKE_CHIPS {
                // Spread the losing chips over the layout.
                let (cx, cz) = casino::ROULETTE;
                let x = cx - 0.2 + (losers % 6) as f32 * 0.22;
                let z = cz - 0.3 + (losers / 6) as f32 * 0.3;
                let at = Vec3::new(x, casino::FELT_HEIGHT + 0.01, z);
                let (collider, mass) = crate::physics::prop_body(PropKind::Chip);
                commands.spawn((
                    Name::new("Losing chip"),
                    RakeChip,
                    PropKind::Chip,
                    PropPose { pos: at, rot: Quat::IDENTITY },
                    HeldBy(None),
                    RigidBody::Dynamic,
                    collider,
                    Mass(mass),
                    Transform::from_translation(at),
                    LinearDamping(0.8),
                    AngularDamping(1.5),
                    Replicate::to_clients(NetworkTarget::All),
                    InterpolationTarget::to_clients(NetworkTarget::All),
                ));
            }
            losers += 1;
            continue;
        }
        match bet.who {
            Who::Customer(_) => match npcs.get_mut(*entity) {
                Ok((_, _, mut npc)) => npc.cash += p.returned,
                Err(_) => p.returned = 0,
            },
            Who::Player(_) => {
                // On the rail in front of the player.
                let x = players.get(*entity).map_or(casino::ROULETTE.0, |(_, _, pos, ..)| pos.0.x);
                let (cx, cz) = casino::ROULETTE;
                let (hx, hz) = casino::ROULETTE_HALF;
                let at = Vec3::new(x.clamp(cx - hx + 0.1, cx + hx - 0.1), casino::FELT_HEIGHT + 0.01, cz + hz - 0.12);
                pay_chips(commands, at, p.returned);
            }
        }
    }
    let (net, mut commission) = house_take(&payouts, CROUPIER_COMMISSION_PERCENT);
    match host.croupier.and_then(|c| players.get_mut(c).ok()) {
        Some((_, _, _, mut pocket, _)) => pocket.0 += commission,
        None => commission = 0,
    }
    super::book(room, net, commission);
    host.bets.clear();
    host.last = Some(result);
    host.spins += 1;
}
