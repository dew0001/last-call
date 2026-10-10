//! The blackjack table on the host (plan section 5.1).
//!
//! A player runs the table from the dealer spot: they press Deal once bets
//! are down, and Hit or Stand for the house by the rules (a wrong press is
//! refused). The dealer earns 10% of the house's win on each round. A
//! Wasted player cannot deal.
//!
//! Players sit by betting near the table; customers walk to a free seat.
//! Customers play the strategy table with a 15% mistake rate, bet about an
//! eighth of what they came with, and leave when they hit a walk-away
//! threshold, run out of money, or wait too long for a dealer. Players who
//! stall get stood after 20 seconds.
//!
//! Money: stakes leave the bettor when they go on the table. At the end of a
//! round, customers get their winnings in cash and players get a chip stack
//! on the felt in front of their seat. The house books the net.

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::blackjack::{self, Action, CUSTOMER_MISTAKE_PERCENT, DEALER_COMMISSION_PERCENT, Phase, Round, SEATS};
use shared::casino::{self, TableAction, TableId};
use shared::customers::Mood;
use shared::minigame::{Bet, Blackjack, BlackjackInput, BlackjackTable, Minigame, Refused, Who, house_take};
use shared::protocol::*;

use super::{Audit, PlayerSpots, Players, TableQueue, TableRngs, WantsToLeave, decide, pay_chips, tier};
use crate::customers::{Activity, Npc};

const TABLE: TableId = TableId::Blackjack;
/// Ticks a customer thinks before each decision.
const THINK_TICKS: u32 = 48;

/// Host-only state of the blackjack table.
#[derive(Component)]
pub struct BjHost {
    game: BlackjackTable,
    dealer: Option<Entity>,
    /// Players sitting at each seat, with the bet for the next round.
    players: [Option<(Entity, i64)>; SEATS],
    /// For the round in play, per round seat: the table seat, who, the
    /// entity to pay, and the stake.
    round: Vec<(usize, Who, Entity)>,
    bets: Vec<Bet<()>>,
    /// Ticks since the current decision came up.
    waiting: u32,
    last: [Option<i64>; SEATS],
    rounds: u32,
}

pub fn spawn(mut commands: Commands, mut rngs: ResMut<TableRngs>, mut audit: ResMut<Audit>) {
    let game = decide(
        &mut rngs,
        &mut audit,
        TABLE,
        0,
        |d| Blackjack::start(d, &()),
        |t| Some(shared::audit::Derived::Shuffle { cards: t.shoe.cards.clone() }),
    );
    commands.spawn((
        Name::new("Blackjack table"),
        BjHost {
            game,
            dealer: None,
            players: [None; SEATS],
            round: Vec::new(),
            bets: Vec::new(),
            waiting: 0,
            last: [None; SEATS],
            rounds: 0,
        },
        BlackjackView { seats: vec![SeatView::default(); SEATS], ..default() },
        Replicate::to_clients(NetworkTarget::All),
    ));
}

/// Customers sitting at each seat (arrived, not leaving).
fn customers_at(npcs: &Query<(Entity, &Customer, &mut Npc), Without<WantsToLeave>>) -> [Option<Entity>; SEATS] {
    let mut out = [None; SEATS];
    for (e, c, n) in npcs.iter() {
        if c.mood == Mood::Gambling
            && let Activity::Table(TABLE, s) = n.activity
        {
            out[usize::from(s) % SEATS] = Some(e);
        }
    }
    out
}

/// Seats a customer heading to the table has claimed (walking there included).
pub fn claimed_by_customers(npcs: impl Iterator<Item = (Mood, Activity)>) -> Vec<u8> {
    npcs.filter(|(m, _)| *m != Mood::Leaving)
        .filter_map(|(_, a)| match a {
            Activity::Table(TABLE, s) => Some(s),
            _ => None,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    mut commands: Commands,
    queue: Res<TableQueue>,
    tick: Res<crate::TickCount>,
    mut rngs: ResMut<TableRngs>,
    mut audit: ResMut<Audit>,
    mut spots: ResMut<PlayerSpots>,
    mut tables: Query<(&mut BjHost, &mut BlackjackView)>,
    mut players: Players,
    mut npcs: Query<(Entity, &Customer, &mut Npc), Without<WantsToLeave>>,
    mut room: Query<&mut RunLedger, With<RoomState>>,
    owned: Res<crate::fixtures::Owned>,
    effects: Res<crate::chaos::TableEffects>,
    mut inventories: Query<&mut Inventory>,
) {
    if effects.paused || effects.blackjack_broken {
        return;
    }
    let Ok((mut host, mut view)) = tables.single_mut() else { return };
    let table_max = owned.0.table_max(casino::BLACKJACK_MAX);
    let patience = casino::TABLE_PATIENCE_SECS * shared::TICK_HZ * owned.0.patience_percent() / 100;
    let host = &mut *host;
    let tick = tick.0;
    let seated_customers = customers_at(&npcs);

    // Players who left: off their seats, out of the dealer's spot.
    for seat in &mut host.players {
        if let Some((e, _)) = *seat
            && players.get(e).is_err()
        {
            *seat = None;
        }
    }
    if let Some(d) = host.dealer {
        let keep = players.get(d).is_ok_and(|(_, _, pos, _, drunk)| {
            shared::drunk::can_deal(tier(drunk))
                && casino::role_spot(TABLE)
                    .is_some_and(|(x, z)| (pos.0.x - x).hypot(pos.0.z - z) < casino::ROLE_REACH * 2.0)
        });
        if !keep {
            host.dealer = None;
        }
    }

    let mut finished: Option<Round> = None;
    for (who_e, action) in queue.for_table(TABLE) {
        let Ok((_, player, pos, pocket, drunk)) = players.get(who_e) else { continue };
        let (id, pos, money, tier) = (player.id, pos.0, pocket.0, tier(drunk));
        let me = Who::Player(id);
        match action {
            TableAction::TakeRole => {
                if host.dealer.is_none()
                    && shared::drunk::can_deal(tier)
                    && casino::at_role_spot(TABLE, pos.x, pos.z)
                    && !host.players.iter().flatten().any(|(e, _)| *e == who_e)
                {
                    host.dealer = Some(who_e);
                }
            }
            TableAction::LeaveRole => {
                if host.dealer == Some(who_e) {
                    host.dealer = None;
                }
            }
            TableAction::Bet(0) => {
                for seat in &mut host.players {
                    if seat.is_some_and(|(e, _)| e == who_e) {
                        *seat = None;
                    }
                }
            }
            TableAction::Bet(amount) => {
                if !casino::blackjack_bet_ok(amount, tier, table_max)
                    || !casino::can_reach(TABLE, pos.x, pos.z)
                    || money < amount
                    || host.dealer == Some(who_e)
                {
                    continue;
                }
                let mine = host.players.iter().position(|s| s.is_some_and(|(e, _)| e == who_e));
                let claimed = claimed_by_customers(npcs.iter().map(|(_, c, n)| (c.mood, n.activity)));
                let seat = mine.or_else(|| {
                    casino::bettor_spots(TABLE)
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| host.players[*i].is_none() && !claimed.contains(&(*i as u8)))
                        .min_by(|a, b| {
                            let d = |p: &(f32, f32)| (p.0 - pos.x).hypot(p.1 - pos.z);
                            d(a.1).total_cmp(&d(b.1))
                        })
                        .map(|(i, _)| i)
                });
                if let Some(s) = seat {
                    host.players[s] = Some((who_e, amount));
                }
            }
            TableAction::Deal => {
                if host.dealer != Some(who_e) || host.game.round.is_some() {
                    continue;
                }
                let mut round = Vec::new();
                let mut bets = Vec::new();
                for (s, customer) in seated_customers.iter().enumerate() {
                    if let Some((pe, bet)) = host.players[s] {
                        if let Ok((_, p, _, mut pocket, _)) = players.get_mut(pe)
                            && pocket.0 >= bet
                        {
                            pocket.0 -= bet;
                            round.push((s, Who::Player(p.id), pe));
                            bets.push(Bet { who: Who::Player(p.id), amount: bet, selection: () });
                        }
                    } else if let Some(ce) = *customer
                        && let Ok((_, c, mut npc)) = npcs.get_mut(ce)
                    {
                        match casino::customer_blackjack_bet(npc.start_cash, npc.cash) {
                            Some(bet) => {
                                npc.cash -= bet;
                                npc.ticks = patience;
                                round.push((s, Who::Customer(c.id), ce));
                                bets.push(Bet { who: Who::Customer(c.id), amount: bet, selection: () });
                            }
                            None => {
                                commands.entity(ce).insert(WantsToLeave);
                            }
                        }
                    }
                }
                if bets.is_empty() {
                    continue;
                }
                host.round = round;
                host.bets = bets.clone();
                host.last = [None; SEATS];
                host.waiting = 0;
                finished = apply(host, &mut rngs, &mut audit, tick, me, BlackjackInput::Deal(bets)).ok().flatten();
            }
            TableAction::Insure(_) | TableAction::Play(_) if !host.round.iter().any(|(_, w, _)| *w == me) => {}
            TableAction::Insure(take) => {
                let cost = match (&host.game.round, take) {
                    (Some(r), true) => {
                        let i = host.round.iter().position(|(_, w, _)| *w == me).unwrap_or(0);
                        r.seats.get(i).map_or(0, |s| s.hands[0].bet / 2)
                    }
                    _ => 0,
                };
                if money < cost {
                    continue;
                }
                if let Ok(out) = apply(host, &mut rngs, &mut audit, tick, me, BlackjackInput::Insure(take)) {
                    charge(&mut players, who_e, cost);
                    finished = finished.or(out);
                }
            }
            TableAction::Play(a) => {
                let cost = host.game.round.as_ref().map_or(0, |r| r.cost(a));
                if money < cost {
                    continue;
                }
                if let Ok(out) = apply(host, &mut rngs, &mut audit, tick, me, BlackjackInput::Play(a)) {
                    charge(&mut players, who_e, cost);
                    finished = finished.or(out);
                }
            }
            TableAction::Dealer(a) => {
                if host.dealer == Some(who_e)
                    && let Ok(out) = apply(host, &mut rngs, &mut audit, tick, me, BlackjackInput::Dealer(a))
                {
                    finished = finished.or(out);
                }
            }
            _ => {}
        }
    }

    // Decisions nobody made: customers think, then play; idle players stand.
    if finished.is_none()
        && let Some(round) = host.game.round.clone()
    {
        host.waiting += 1;
        let pending = match round.phase {
            Phase::Insurance => round.seats.iter().position(|s| s.insurance.is_none()),
            Phase::Players { seat, .. } => Some(usize::from(seat)),
            _ => None,
        };
        if let Some(i) = pending {
            let (_, who, entity) = host.round[i];
            let gone_customer =
                matches!(who, Who::Customer(_)) && !npcs.get(entity).is_ok_and(|(_, c, _)| c.mood == Mood::Gambling);
            let input = match who {
                Who::Customer(_) if gone_customer => Some(default_input(&round)),
                Who::Customer(_) if host.waiting >= THINK_TICKS => {
                    let cash = npcs.get(entity).map_or(0, |(_, _, n)| n.cash);
                    let mut d = rngs.draw(TABLE, tick, &mut audit);
                    Some(if round.phase == Phase::Insurance {
                        let take = blackjack::customer_insures(CUSTOMER_MISTAKE_PERCENT, &mut d);
                        BlackjackInput::Insure(take && cash >= round.seats[i].hands[0].bet / 2)
                    } else {
                        let a = blackjack::customer_choice(&round, CUSTOMER_MISTAKE_PERCENT, &mut d);
                        BlackjackInput::Play(if round.cost(a) > cash { Action::Hit } else { a })
                    })
                }
                Who::Player(_) if host.waiting >= casino::PLAYER_TURN_SECS * shared::TICK_HZ => {
                    Some(default_input(&round))
                }
                _ => None,
            };
            if let Some(input) = input {
                let cost = match (&input, &round.phase) {
                    (BlackjackInput::Insure(true), _) => round.seats[i].hands[0].bet / 2,
                    (BlackjackInput::Play(a), _) => round.cost(*a),
                    _ => 0,
                };
                if let Ok(out) = apply(host, &mut rngs, &mut audit, tick, who, input) {
                    if let Ok((_, _, mut npc)) = npcs.get_mut(entity) {
                        npc.cash -= cost;
                    }
                    finished = out;
                }
            }
        }
    }

    if let Some(round) = finished {
        settle(&mut commands, host, &round, &mut players, &mut npcs, &mut room, &effects);
    }

    // Customers waiting on a dealer lose patience; broke ones leave.
    let waiting_on_dealer = host.game.round.as_ref().is_none_or(|r| r.phase == Phase::Dealer);
    for e in seated_customers.into_iter().flatten() {
        let Ok((_, _, mut npc)) = npcs.get_mut(e) else { continue };
        if waiting_on_dealer {
            npc.ticks = npc.ticks.saturating_sub(1);
        }
        let in_round = host.round.iter().any(|(_, _, re)| *re == e);
        if !in_round
            && (npc.ticks == 0
                || casino::walks_away(npc.cash, npc.start_cash)
                || casino::customer_blackjack_bet(npc.start_cash, npc.cash).is_none())
        {
            commands.entity(e).insert(WantsToLeave);
        }
    }

    spots.0.retain(|(t, _)| *t != TABLE);
    for (s, seat) in host.players.iter().enumerate() {
        if seat.is_some() {
            spots.0.insert((TABLE, s as u8));
        }
    }

    let next = build_view(host, &seated_customers, &players, &npcs);
    view.set_if_neq(next);
    // A marked deck shows its holder the dealer's hole card.
    let hole = host
        .game
        .round
        .as_ref()
        .filter(|r| matches!(r.phase, Phase::Insurance | Phase::Players { .. }))
        .and_then(|r| r.dealer.get(1).copied());
    for mut inv in &mut inventories {
        let want = if inv.marked_deck { hole } else { None };
        if inv.hole_card != want {
            inv.hole_card = want;
        }
    }
}

/// What happens to a seat that cannot or will not decide: decline insurance, stand.
fn default_input(round: &Round) -> BlackjackInput {
    if round.phase == Phase::Insurance { BlackjackInput::Insure(false) } else { BlackjackInput::Play(Action::Stand) }
}

fn charge(players: &mut Players, e: Entity, cost: i64) {
    if let Ok((_, _, _, mut pocket, _)) = players.get_mut(e) {
        pocket.0 -= cost;
    }
}

/// Apply an input with the table's RNG; log a reshuffle.
fn apply(
    host: &mut BjHost,
    rngs: &mut TableRngs,
    audit: &mut Audit,
    tick: u64,
    who: Who,
    input: BlackjackInput,
) -> Result<Option<Round>, Refused> {
    let shuffles = host.game.shuffles;
    let first = audit.0.mark(TABLE.stream());
    let out = Blackjack::apply(&mut host.game, who, input, &mut rngs.draw(TABLE, tick, audit));
    if host.game.shuffles != shuffles {
        let cards = host.game.shoe.cards.clone();
        audit.0.outcome(TABLE.stream(), tick, first, shared::audit::Derived::Shuffle { cards });
    }
    if out.is_ok() {
        host.waiting = 0;
    }
    out
}

fn settle(
    commands: &mut Commands,
    host: &mut BjHost,
    round: &Round,
    players: &mut Players,
    npcs: &mut Query<(Entity, &Customer, &mut Npc), Without<WantsToLeave>>,
    room: &mut Query<&mut RunLedger, With<RoomState>>,
    effects: &crate::chaos::TableEffects,
) {
    let mut payouts = Blackjack::payout(round, &host.bets);
    let spots = casino::bettor_spots(TABLE);
    let mut shark_won = false;
    for (p, (seat, who, entity)) in payouts.iter_mut().zip(&host.round) {
        // The card counter wins every hand.
        if effects.counter == Some(*entity) {
            p.returned = p.returned.max(p.staked * 2);
        }
        shark_won |= effects.shark == Some(*entity) && p.returned > p.staked;
        host.last[*seat] = Some(p.returned - p.staked);
        match who {
            Who::Customer(_) => match npcs.get_mut(*entity) {
                Ok((_, _, mut npc)) => npc.cash += p.returned,
                // Walked out mid-round: the house keeps it.
                Err(_) => p.returned = 0,
            },
            Who::Player(_) => {
                if p.returned > 0 {
                    let (x, _) = spots[*seat];
                    let z = casino::BLACKJACK.1 + casino::BLACKJACK_HALF.1 - 0.15;
                    pay_chips(commands, Vec3::new(x, casino::FELT_HEIGHT + 0.01, z), p.returned);
                }
            }
        }
    }
    let (net, mut commission) = house_take(&payouts, DEALER_COMMISSION_PERCENT);
    match host.dealer.and_then(|d| players.get_mut(d).ok()) {
        Some((_, _, _, mut pocket, _)) => {
            pocket.0 += commission;
            // The loan shark tips the dealer when he wins.
            if shark_won {
                pocket.0 += shared::chaos::SHARK_TIP;
            }
        }
        None => commission = 0,
    }
    super::book(room, net, commission);
    host.round.clear();
    host.bets.clear();
    host.rounds += 1;
}

fn build_view(
    host: &BjHost,
    customers: &[Option<Entity>; SEATS],
    players: &Players,
    npcs: &Query<(Entity, &Customer, &mut Npc), Without<WantsToLeave>>,
) -> BlackjackView {
    let player_id = |e: Entity| players.get(e).ok().map(|(_, p, ..)| p.id);
    let mut seats = vec![SeatView::default(); SEATS];
    for (s, seat) in seats.iter_mut().enumerate() {
        if let Some((e, bet)) = host.players[s] {
            seat.who = player_id(e).map(Who::Player);
            seat.bet = bet;
        } else if let Some(e) = customers[s]
            && let Ok((_, c, n)) = npcs.get(e)
        {
            seat.who = Some(Who::Customer(c.id));
            seat.bet = casino::customer_blackjack_bet(n.start_cash, n.cash).unwrap_or(0);
        }
        seat.last = host.last[s];
    }
    let mut view = BlackjackView {
        dealer: host.dealer.and_then(player_id),
        shoe_left: (host.game.shoe.cards.len() - host.game.shoe.next) as u16,
        rounds: host.rounds,
        ..default()
    };
    if let Some(round) = &host.game.round {
        for (i, (s, who, _)) in host.round.iter().enumerate() {
            let st = &round.seats[i];
            seats[*s].who = Some(*who);
            seats[*s].hands =
                st.hands.iter().map(|h| HandView { cards: h.cards.clone(), bet: h.bet, done: h.done }).collect();
            seats[*s].insurance = st.insurance;
        }
        view.phase = match round.phase {
            Phase::Insurance => BjPhase::Insurance,
            Phase::Players { seat, hand } => BjPhase::Players { seat: host.round[usize::from(seat)].0 as u8, hand },
            Phase::Dealer | Phase::Done => BjPhase::Dealer,
        };
        view.dealer_cards = round.dealer.clone();
        if matches!(round.phase, Phase::Insurance | Phase::Players { .. }) {
            view.dealer_cards[1] = HIDDEN_CARD;
        }
        view.dealer_should = round.dealer_should();
    }
    view.seats = seats;
    view
}
