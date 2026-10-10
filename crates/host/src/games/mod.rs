//! The side games on the host (plan sections 5.5 to 5.9): fishing on the
//! pier, basketball on the roof, penalties, field goals and the gauntlet in
//! the parking lot, and the fight pit in the basement.
//!
//! Players act with [`GameRequest`]s; the host finds the station by where
//! the player stands. Each game draws from its own RNG stream (logged).
//! Entry fees go into a pot the winners split; bets against the house and
//! fish sales move house money.

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::minigame::Who;
use shared::protocol::*;
use shared::rng::{StreamId, TableRng};

pub mod field;
pub mod fishing;
pub mod hoops;
pub mod pit;

/// RNG streams: fishing, hoops, kicks, gauntlet, pit.
pub const FISHING_STREAM: StreamId = StreamId(60);
pub const HOOPS_STREAM: StreamId = StreamId(61);
pub const KICKS_STREAM: StreamId = StreamId(62);
pub const GAUNTLET_STREAM: StreamId = StreamId(63);
pub const PIT_STREAM: StreamId = StreamId(64);

/// This tick's game requests: (player entity, action).
#[derive(Resource, Default)]
pub struct GameQueue(pub Vec<(Entity, GameAction)>);

/// One RNG per game.
#[derive(Resource)]
pub struct GameRngs {
    pub fishing: TableRng,
    pub hoops: TableRng,
    pub kicks: TableRng,
    pub gauntlet: TableRng,
    pub pit: TableRng,
}

/// A crowd after a 5 of 5 on the roof: ticks left of 20% better drink sales.
#[derive(Resource, Default, Debug)]
pub struct Crowd(pub u32);

impl Crowd {
    /// What a beer sells for with the crowd in.
    pub fn price(&self, paid: i64) -> i64 {
        if self.0 > 0 { paid * 120 / 100 } else { paid }
    }
}

/// The side-game systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GamesSet;

pub struct GamesPlugin;

impl Plugin for GamesPlugin {
    fn build(&self, app: &mut App) {
        let seed = app.world().resource::<crate::RoomSeed>().0;
        app.insert_resource(GameRngs {
            fishing: TableRng::new(seed, FISHING_STREAM),
            hoops: TableRng::new(seed, HOOPS_STREAM),
            kicks: TableRng::new(seed, KICKS_STREAM),
            gauntlet: TableRng::new(seed, GAUNTLET_STREAM),
            pit: TableRng::new(seed, PIT_STREAM),
        });
        app.init_resource::<GameQueue>().init_resource::<Crowd>().init_resource::<pit::History>();
        app.add_systems(Startup, spawn.after(crate::casino::slots::spawn));
        app.add_systems(
            FixedUpdate,
            (
                collect,
                pit::record_history,
                fishing::run,
                hoops::run,
                field::penalties,
                field::field_goals,
                field::gauntlet,
                pit::run,
                beer_hits,
                clear,
            )
                .chain()
                .in_set(GamesSet)
                .after(crate::shift::RunClock)
                .after(crate::physics::HandsSet)
                .after(crate::casino::CasinoSet),
        );
    }
}

fn spawn(mut commands: Commands) {
    for spot in 0..shared::fishing::SPOTS.len() as u8 {
        commands.spawn((
            Name::new("Fishing spot"),
            fishing::FishingHost::new(spot),
            FishingView { spot, ..default() },
            Replicate::to_clients(NetworkTarget::All),
        ));
    }
    commands.spawn((
        Name::new("Hoop"),
        hoops::HoopsHost::default(),
        HoopsView::default(),
        Replicate::to_clients(NetworkTarget::All),
    ));
    commands.spawn((
        Name::new("Penalty goal"),
        field::PenaltyHost::default(),
        PenaltyView::default(),
        Replicate::to_clients(NetworkTarget::All),
    ));
    commands.spawn((Name::new("Field goal"), FieldGoalView::default(), Replicate::to_clients(NetworkTarget::All)));
    commands.spawn((
        Name::new("Gauntlet"),
        field::GauntletHost::default(),
        GauntletView::default(),
        Replicate::to_clients(NetworkTarget::All),
    ));
    commands.spawn((
        Name::new("Fight pit"),
        pit::PitHost::default(),
        PitView::default(),
        Replicate::to_clients(NetworkTarget::All),
    ));
}

fn collect(
    mut queue: ResMut<GameQueue>,
    mut links: Query<(Entity, &mut MessageReceiver<GameRequest>)>,
    players: Query<(Entity, &ControlledBy), With<Player>>,
) {
    for (link, mut receiver) in &mut links {
        let player = players.iter().find(|(_, c)| c.owner == link).map(|(e, _)| e);
        for request in receiver.receive() {
            if let Some(p) = player {
                queue.0.push((p, request.0));
            }
        }
    }
}

fn clear(mut queue: ResMut<GameQueue>, mut crowd: ResMut<Crowd>) {
    queue.0.clear();
    crowd.0 = crowd.0.saturating_sub(1);
}

/// The players every game uses.
pub type GamePlayers<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Player,
        &'static mut PlayerPos,
        &'static PlayerYaw,
        &'static mut Pocket,
        Option<&'static Drunk>,
        Option<&'static Focus>,
    ),
    Without<crate::drunk::PassedOut>,
>;

/// A player's id as a contest entrant.
pub fn who(player: &Player) -> Who {
    Who::Player(player.id)
}

pub fn player_id(who: Who) -> u64 {
    match who {
        Who::Player(id) => id,
        Who::Customer(id) => u64::from(id),
    }
}

/// Pay `amount` into the pocket of the player with this id, if they are here.
pub fn pay(players: &mut GamePlayers, id: u64, amount: i64) -> bool {
    match players.iter_mut().find(|(_, p, ..)| p.id == id) {
        Some((.., mut pocket, _, _)) => {
            pocket.0 += amount;
            true
        }
        None => false,
    }
}

/// Take a stake or fee from a pocket: false if it cannot pay.
pub fn charge(pocket: &mut Pocket, amount: i64) -> bool {
    if amount <= 0 || pocket.0 < amount {
        return false;
    }
    pocket.0 -= amount;
    true
}

/// A spectator's thrown beer that hits a player gives them +20 drunk
/// (plan section 5.9, anywhere in the bar).
fn beer_hits(
    mut commands: Commands,
    glasses: Query<(Entity, &avian3d::prelude::Position, &avian3d::prelude::LinearVelocity, &HeldBy, &Beer)>,
    mut players: Query<(&Player, &PlayerPos, &mut Drunk)>,
) {
    for (e, pos, vel, held, beer) in &glasses {
        if held.0.is_some() || vel.0.length() < 2.0 {
            continue;
        }
        let hit = players.iter_mut().find(|(p, at, _)| {
            p.id != beer.poured_by
                && Vec2::new(at.0.x - pos.0.x, at.0.z - pos.0.z).length() < 0.5
                && (0.2..2.0).contains(&(pos.0.y - at.0.y))
        });
        if let Some((_, _, mut drunk)) = hit {
            drunk.level = drunk.level.saturating_add(shared::pit::BEER_HIT_DRUNK).min(100);
            commands.entity(e).despawn();
        }
    }
}
