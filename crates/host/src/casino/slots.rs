//! Slot machines on the host (plan section 5.3).
//!
//! A pull draws the reel stops at once; the reels spin for 2 seconds on
//! clients, then the machine pays coins straight into the player's pocket
//! (or the customer's cash). No player job: slots are the quiet income.

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::casino::{self, SLOT_MACHINES, TableAction, TableId};
use shared::customers::Mood;
use shared::minigame::{Minigame, Slots, Who};
use shared::protocol::*;

use super::{Audit, PlayerSpots, Players, TableQueue, TableRngs, WantsToLeave, decide, tier};
use crate::customers::{Activity, Npc};

/// How long the reels spin.
pub const SPIN_TICKS: u32 = 2 * shared::TICK_HZ;
/// A customer's pause between pulls.
const CUSTOMER_PAUSE_TICKS: u32 = 3 * shared::TICK_HZ / 2;

#[derive(Component)]
pub struct SlotHost {
    machine: u8,
    user: Option<(Entity, Who)>,
    bet: i64,
    stops: [u8; 3],
    /// Ticks left on the spin.
    spinning: u32,
    idle: u32,
    last_return: i64,
    pulls: u32,
}

pub fn spawn(mut commands: Commands) {
    for machine in 0..SLOT_MACHINES {
        commands.spawn((
            Name::new("Slot machine"),
            SlotHost { machine, user: None, bet: 0, stops: [0; 3], spinning: 0, idle: 0, last_return: 0, pulls: 0 },
            SlotView { machine, ..default() },
            Replicate::to_clients(NetworkTarget::All),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    mut commands: Commands,
    queue: Res<TableQueue>,
    tick: Res<crate::TickCount>,
    mut rngs: ResMut<TableRngs>,
    mut audit: ResMut<Audit>,
    mut spots: ResMut<PlayerSpots>,
    mut machines: Query<(&mut SlotHost, &mut SlotView)>,
    mut players: Players,
    mut npcs: Query<(Entity, &Customer, &mut Npc), Without<WantsToLeave>>,
    mut room: Query<&mut RunLedger, With<RoomState>>,
    owned: Res<crate::fixtures::Owned>,
) {
    let tick = tick.0;
    let slot_max = owned.0.table_max(casino::SLOT_MAX);
    let mut sorted: Vec<_> = machines.iter_mut().collect();
    sorted.sort_by_key(|(h, _)| h.machine);
    for (mut host, mut view) in sorted {
        let host = &mut *host;
        let table = TableId::Slot(host.machine);
        let customer = npcs
            .iter()
            .find(|(_, c, n)| c.mood == Mood::Gambling && n.activity == Activity::Table(table, 0))
            .map(|(e, c, _)| (e, Who::Customer(c.id)));

        // A player who walked away frees the machine once the reels stop.
        if host.spinning == 0
            && let Some((e, Who::Player(_))) = host.user
            && !players.get(e).is_ok_and(|(_, _, pos, ..)| casino::can_reach(table, pos.0.x, pos.0.z))
        {
            host.user = None;
        }
        if host.spinning == 0 && host.user.is_none() {
            host.user = customer;
        }

        let mut pull: Option<(Entity, Who, i64)> = None;
        for (who_e, action) in queue.for_table(table) {
            let TableAction::Pull(bet) = action else { continue };
            let Ok((_, player, pos, mut pocket, drunk)) = players.get_mut(who_e) else { continue };
            let free = host.user.is_none_or(|(e, _)| e == who_e);
            if host.spinning == 0
                && free
                && pull.is_none()
                && casino::can_reach(table, pos.0.x, pos.0.z)
                && (casino::SLOT_MIN..=casino::max_bet(slot_max, tier(drunk))).contains(&bet)
                && pocket.0 >= bet
            {
                pocket.0 -= bet;
                pull = Some((who_e, Who::Player(player.id), bet));
            }
        }

        if host.spinning == 0
            && pull.is_none()
            && let Some((e, who)) = customer
            && host.user.is_some_and(|(u, _)| u == e)
        {
            host.idle += 1;
            if host.idle >= CUSTOMER_PAUSE_TICKS
                && let Ok((_, _, mut npc)) = npcs.get_mut(e)
            {
                match casino::customer_slot_bet(npc.start_cash, npc.cash) {
                    Some(bet) if !casino::walks_away(npc.cash, npc.start_cash) => {
                        npc.cash -= bet;
                        pull = Some((e, who, bet));
                    }
                    _ => {
                        commands.entity(e).insert(WantsToLeave);
                        host.user = None;
                    }
                }
            }
        }

        if let Some((e, who, bet)) = pull {
            host.stops = decide(
                &mut rngs,
                &mut audit,
                table,
                tick,
                |d| Slots::start(d, &()),
                |stops| Some(shared::audit::Derived::Reels { stops: *stops }),
            );
            host.user = Some((e, who));
            host.bet = bet;
            host.spinning = SPIN_TICKS;
            host.idle = 0;
            host.pulls += 1;
        } else if host.spinning > 0 {
            host.spinning -= 1;
            if host.spinning == 0 {
                let returned = shared::slots::payout(host.bet, host.stops);
                let mut paid = returned;
                match host.user {
                    Some((e, Who::Player(_))) => match players.get_mut(e) {
                        Ok((_, _, _, mut pocket, _)) => pocket.0 += returned,
                        Err(_) => paid = 0,
                    },
                    Some((e, Who::Customer(_))) => match npcs.get_mut(e) {
                        Ok((_, _, mut npc)) => npc.cash += returned,
                        Err(_) => paid = 0,
                    },
                    None => paid = 0,
                }
                host.last_return = returned;
                super::book(&mut room, host.bet - paid, 0);
            }
        }

        spots.0.remove(&(table, 0));
        if matches!(host.user, Some((_, Who::Player(_)))) {
            spots.0.insert((table, 0));
        }
        let next = SlotView {
            machine: host.machine,
            user: host.user.map(|(_, w)| w),
            spinning: host.spinning > 0,
            stops: host.stops,
            last_bet: host.bet,
            last_return: host.last_return,
            pulls: host.pulls,
        };
        view.set_if_neq(next);
    }
}
