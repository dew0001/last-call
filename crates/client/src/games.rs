//! The side games on the client (plan sections 5.5 to 5.9): a panel for
//! the station the player stands at, the keys that send [`GameRequest`]s,
//! and gray-box visuals (the hoop and the ball's flight, the goal, the
//! uprights, the tacklers, the pit's racks and tracers, the bobbers).
//!
//! Keys at a station: see [`game_keys`]. One scheme for every game: the
//! left mouse button charges and acts, the right one is the second action,
//! E starts or takes, digits join or pick, 0 plays for fun.

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use lightyear::prelude::*;
use shared::client::OutgoingGame;
use shared::fishing::{self, Catch, Side};
use shared::gauntlet;
use shared::hoops::{self, Mode};
use shared::kicks::{self, Dive, Kick};
use shared::minigame::Who;
use shared::pit;
use shared::protocol::*;
use shared::world::{Room, room_at};

use crate::online::{Look, NetStatus, NoDraw};

pub fn add(app: &mut App) {
    app.init_resource::<Charge>().init_resource::<ForFun>();
    app.add_systems(Startup, setup_stations);
    app.add_systems(
        Update,
        (
            game_keys,
            fill_view_ticks,
            draw_tacklers,
            draw_tracers,
            draw_bobbers,
            draw_keeper,
            update_panel.run_if(resource_changed::<NetStatus>),
        )
            .chain()
            .after(crate::online::OnlineSet),
    );
}

/// Which station a player at (x, z) is at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Station {
    Fishing(u8),
    Court,
    Penalty,
    Goal,
    Tee,
    Lane,
    Pit,
}

pub fn station(x: f32, z: f32) -> Option<Station> {
    let near = |(sx, sz): (f32, f32), r: f32| (x - sx).hypot(z - sz) <= r;
    if let Some(s) = fishing::spot_at(x, z) {
        return Some(Station::Fishing(s));
    }
    match room_at(x, z)? {
        Room::Roof => Some(Station::Court),
        Room::Basement => Some(Station::Pit),
        Room::ParkingLot if near(kicks::GOALIE_SPOT, kicks::SPOT_REACH) => Some(Station::Goal),
        Room::ParkingLot if kicks::TEES.iter().any(|(_, tx, tz, _)| near((*tx, *tz), kicks::SPOT_REACH)) => {
            Some(Station::Tee)
        }
        Room::ParkingLot if x >= gauntlet::LANE_X.0 - 1.0 => Some(Station::Lane),
        Room::ParkingLot => Some(Station::Penalty),
        _ => None,
    }
}

fn hex(id: u64) -> String {
    format!("{id:016x}")
}

fn who_hex(w: Who) -> String {
    match w {
        Who::Player(id) => hex(id),
        Who::Customer(id) => format!("c:{id}"),
    }
}

// ---------- Status for the page and tests ----------

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamesStatus {
    pub station: Option<String>,
    pub fishing: Vec<FishStatus>,
    pub hoops: HoopsStatus,
    pub penalties: PenaltyStatus,
    pub field_goal: FieldGoalStatus,
    pub gauntlet: GauntletStatus,
    pub pit: PitStatus,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FishStatus {
    pub spot: u8,
    pub fisher: Option<String>,
    pub phase: &'static str,
    pub tension: u16,
    pub band: (u16, u16),
    pub progress: u16,
    pub need: u16,
    /// The last cast: (fisher, "minnow" / "snapped" / "escaped" / "missed").
    pub last: Option<(String, String)>,
    pub bets: usize,
    pub casts: u32,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoopsStatus {
    pub mode: Option<&'static str>,
    pub started: bool,
    /// (player, shots, makes or letters, out).
    pub entrants: Vec<(String, u8, u8, bool)>,
    pub shooter: Option<String>,
    pub pot: i64,
    pub last: Option<(String, bool)>,
    pub shots: u32,
    pub paid: Vec<(String, i64)>,
    pub crowd_secs: u16,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PenaltyStatus {
    pub started: bool,
    pub entrants: Vec<(String, u8, u8)>,
    pub kicker: Option<String>,
    pub goalie: Option<String>,
    pub in_flight: bool,
    pub last: Option<(String, &'static str)>,
    pub kicks: u32,
    pub bets: usize,
    pub paid: Vec<(String, i64)>,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldGoalStatus {
    /// (kicker, yards, good, returned).
    pub last: Option<(String, u8, bool, i64)>,
    pub kicks: u32,
    pub yards: u8,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GauntletStatus {
    pub runner: Option<String>,
    pub tacklers: Vec<(f32, f32)>,
    pub seconds_left: u32,
    pub last: Option<(String, &'static str)>,
    pub runs: u32,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PitStatus {
    pub started: bool,
    pub teams: bool,
    /// (player, team, takedowns, health, weapon, ammo, down).
    pub fighters: Vec<(String, u8, u8, i32, Option<&'static str>, u8, bool)>,
    pub seconds_left: u32,
    pub pot: i64,
    pub shots: u32,
    pub last_hit: Option<String>,
    pub paid: Vec<(String, i64)>,
    pub rounds: u32,
}

fn catch_name(c: Catch) -> String {
    match c {
        Catch::Landed(f) => f.label().to_string(),
        Catch::Snapped => "snapped".into(),
        Catch::Escaped => "escaped".into(),
        Catch::Missed => "missed".into(),
    }
}

fn result_name(r: kicks::KickResult) -> &'static str {
    match r {
        kicks::KickResult::Goal => "goal",
        kicks::KickResult::Saved => "saved",
        kicks::KickResult::Wide => "wide",
        kicks::KickResult::OverTheBar => "over",
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct GamesQueries<'w, 's> {
    fishing: Query<'w, 's, &'static FishingView>,
    hoops: Query<'w, 's, &'static HoopsView>,
    penalties: Query<'w, 's, &'static PenaltyView>,
    field: Query<'w, 's, &'static FieldGoalView>,
    gauntlet: Query<'w, 's, &'static GauntletView>,
    pit: Query<'w, 's, &'static PitView>,
}

impl GamesQueries<'_, '_> {
    pub fn status(&self, own: Option<Vec3>) -> GamesStatus {
        let mut fishing: Vec<FishStatus> = self
            .fishing
            .iter()
            .map(|v| FishStatus {
                spot: v.spot,
                fisher: v.fisher.map(hex),
                phase: match v.phase {
                    FishPhase::Idle => "idle",
                    FishPhase::Waiting => "waiting",
                    FishPhase::Biting => "biting",
                    FishPhase::Reeling => "reeling",
                },
                tension: v.tension,
                band: v.band,
                progress: v.progress,
                need: v.need,
                last: v.last.map(|(id, c)| (hex(id), catch_name(c))),
                bets: v.bets.len(),
                casts: v.casts,
            })
            .collect();
        fishing.sort_by_key(|f| f.spot);
        fishing.dedup_by_key(|f| f.spot);
        let hoops = self
            .hoops
            .iter()
            .next()
            .map(|v| {
                let c = v.contest.as_ref();
                HoopsStatus {
                    mode: c.map(|c| if c.mode == Mode::Horse { "horse" } else { "3of5" }),
                    started: c.is_some_and(|c| c.started),
                    entrants: c
                        .map(|c| c.entrants.iter().map(|e| (who_hex(e.who), e.tries, e.score, e.out)).collect())
                        .unwrap_or_default(),
                    shooter: c.and_then(|c| c.shooter()).map(who_hex),
                    pot: c.map_or(0, |c| c.pot),
                    last: v.last.map(|s| (hex(s.by), s.made)),
                    shots: v.shots,
                    paid: v.paid.iter().map(|(id, a)| (hex(*id), *a)).collect(),
                    crowd_secs: v.crowd_secs,
                }
            })
            .unwrap_or_default();
        let penalties = self
            .penalties
            .iter()
            .next()
            .map(|v| PenaltyStatus {
                started: v.shootout.started,
                entrants: v.shootout.entrants.iter().map(|e| (who_hex(e.who), e.tries, e.score)).collect(),
                kicker: v.shootout.kicker().map(who_hex),
                goalie: v.shootout.goalie.map(who_hex),
                in_flight: v.in_flight,
                last: v.last.map(|k| (hex(k.by), result_name(k.result))),
                kicks: v.kicks,
                bets: v.bets.len(),
                paid: v.paid.iter().map(|(id, a)| (hex(*id), *a)).collect(),
            })
            .unwrap_or_default();
        let field_goal = self
            .field
            .iter()
            .next()
            .map(|v| FieldGoalStatus {
                last: v.last.map(|r| (hex(r.by), r.yards, r.good, r.returned)),
                kicks: v.kicks,
                // The tee this player stands at.
                yards: own
                    .and_then(|o| {
                        kicks::TEES.iter().find(|(_, tx, tz, _)| (o.x - tx).hypot(o.z - tz) <= kicks::SPOT_REACH + 0.8)
                    })
                    .map_or(0, |t| t.0),
            })
            .unwrap_or_default();
        let gauntlet = self
            .gauntlet
            .iter()
            .next()
            .map(|v| GauntletStatus {
                runner: v.runner.map(hex),
                tacklers: v
                    .run
                    .as_ref()
                    .map(|r| r.tacklers.iter().map(|t| (t.pos[0], t.pos[1])).collect())
                    .unwrap_or_default(),
                seconds_left: v.run.as_ref().map_or(0, |r| r.ticks_left.div_ceil(shared::TICK_HZ)),
                last: v.last.map(|(id, e)| {
                    (
                        hex(id),
                        match e {
                            gauntlet::RunEnd::Scored => "scored",
                            gauntlet::RunEnd::Tackled => "tackled",
                            gauntlet::RunEnd::TimeUp => "time",
                        },
                    )
                }),
                runs: v.runs,
            })
            .unwrap_or_default();
        let pit = self
            .pit
            .iter()
            .next()
            .map(|v| PitStatus {
                started: v.round.started,
                teams: v.round.teams == pit::Teams::TwoTeams,
                fighters: v
                    .round
                    .fighters
                    .iter()
                    .map(|f| {
                        (who_hex(f.who), f.team, f.kills, f.health, f.weapon.map(|w| w.label()), f.ammo, f.down > 0)
                    })
                    .collect(),
                seconds_left: v.round.ticks_left.div_ceil(shared::TICK_HZ),
                pot: v.round.pot,
                shots: v.shots,
                last_hit: v.tracers.iter().rev().find_map(|t| t.hit).map(hex),
                paid: v.paid.iter().map(|(id, a)| (hex(*id), *a)).collect(),
                rounds: v.rounds,
            })
            .unwrap_or_default();
        GamesStatus {
            station: own.and_then(|p| station(p.x, p.z)).map(|s| format!("{s:?}")),
            fishing,
            hoops,
            penalties,
            field_goal,
            gauntlet,
            pit,
        }
    }
}

// ---------- Keys ----------

/// Holding C charges a cast, shot or kick: seconds held.
#[derive(Resource, Default)]
struct Charge {
    held: Option<f32>,
    reeling: bool,
    /// The last time A and D went down, for double taps.
    last_a: f32,
    last_d: f32,
}

/// Seconds of holding for a full charge.
const FULL_CHARGE: f32 = 1.5;
/// Stake for a field goal or a run, and a spectator's bet, from the keys.
const KEY_STAKE: i64 = 20;
const KEY_BET: i64 = 10;

/// Play the station's game for fun (no stake, no winnings): 0 toggles it.
#[derive(Resource, Default)]
pub struct ForFun(pub bool);

/// The keys at a station, one scheme for every game:
/// - hold the left mouse button to charge, let go to cast, shoot or kick
///   (in the pit it fires; with a line out it reels in);
/// - the right mouse button strikes at a bite, or stiff-arms in the lane;
/// - E (the interact key) starts a game, takes the goal, takes a weapon;
/// - 1 to 3 join or pick (a contest, a distance, a dive), 7 and 8 bet as a
///   spectator, 0 toggles playing for fun.
#[allow(clippy::too_many_arguments)]
fn game_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    clicks: Res<crate::online::Clicks>,
    settings: Res<crate::online::Settings>,
    time: Res<Time>,
    status: Res<NetStatus>,
    look: Res<Look>,
    mut charge: ResMut<Charge>,
    mut fun: ResMut<ForFun>,
    mut out: ResMut<OutgoingGame>,
    session: Res<shared::client::Session>,
    props: Query<(&PropKind, &HeldBy, &PropPose)>,
) {
    let Some(pos) = status.own_pos else { return };
    // A ball in hand: the left button shoots (the roof) or throws it.
    let held = session
        .player_id
        .and_then(|id| props.iter().find(|(k, h, _)| k.is_ball() && h.0 == Some(id)).map(|(k, ..)| *k));
    let holding_ball = held == Some(PropKind::Basketball);
    let holding_lot_ball = matches!(held, Some(PropKind::SoccerBall | PropKind::Football));
    // A lot ball at this player's feet, in front of them: the left button kicks it.
    let fwd = Vec2::new(-look.yaw.sin(), -look.yaw.cos());
    let at_feet = props
        .iter()
        .filter(|(k, h, p)| matches!(k, PropKind::SoccerBall | PropKind::Football) && h.0.is_none() && p.pos.y < 0.6)
        .map(|(k, _, p)| (*k, p.pos, Vec2::new(p.pos.x - pos.x, p.pos.z - pos.z)))
        .filter(|(_, _, off)| off.length() <= kicks::KICK_REACH && off.dot(fwd) > -0.2)
        .min_by(|a, b| a.2.length().total_cmp(&b.2.length()))
        .map(|(k, p, _)| (k, p));
    let st = match station(pos.x, pos.z) {
        Some(st) => st,
        None if holding_ball && room_at(pos.x, pos.z) == Some(Room::Roof) => Station::Court,
        None if (holding_lot_ball || at_feet.is_some()) && room_at(pos.x, pos.z) == Some(Room::ParkingLot) => {
            Station::Penalty
        }
        None => {
            charge.held = None;
            return;
        }
    };
    let t = time.elapsed_secs();
    let pressed = |k: KeyCode| keys.just_pressed(k);
    let lmb = mouse.as_ref().is_some_and(|m| m.pressed(MouseButton::Left));
    let rmb = clicks.right;
    let use_key = pressed(settings.keys.interact);
    let mut send = |a: GameAction| out.0.push(a);
    if pressed(KeyCode::Digit0) {
        fun.0 = !fun.0;
    }
    let stake = if fun.0 { 0 } else { KEY_STAKE };
    let me = status.player_id.map(|id| format!("{id:016x}"));

    // Fishing with a line out: hold the button to reel instead of charging.
    let line_out = match st {
        Station::Fishing(spot) => status
            .game
            .games
            .fishing
            .get(usize::from(spot))
            .is_some_and(|f| f.fisher.is_some() && f.fisher == me && f.phase != "idle"),
        _ => false,
    };
    if let Station::Fishing(spot) = st {
        if line_out {
            if lmb != charge.reeling {
                charge.reeling = lmb;
                send(GameAction::Reel { held: lmb });
            }
            if rmb {
                send(GameAction::Hook);
            }
        }
        if pressed(KeyCode::Digit7) {
            send(GameAction::BetCatch { spot, side: Side::Lands, amount: KEY_BET });
        }
        if pressed(KeyCode::Digit8) {
            send(GameAction::BetCatch { spot, side: Side::Snaps, amount: KEY_BET });
        }
    }

    // Charge with the left button; let go to act. (The pit fires instead.)
    let can_charge = match st {
        Station::Fishing(_) => !line_out,
        Station::Court => holding_ball,
        Station::Pit => false,
        _ => holding_lot_ball || at_feet.is_some(),
    };
    if can_charge && lmb {
        *charge.held.get_or_insert(0.0) += time.delta_secs();
    } else if let Some(held) = charge.held.take() {
        let frac = (held / FULL_CHARGE).min(1.0);
        let power = (frac * 100.0) as u8;
        // Curve from A/D held at release.
        let curve = f32::from(u8::from(keys.pressed(KeyCode::KeyD))) - f32::from(u8::from(keys.pressed(KeyCode::KeyA)));
        match st {
            Station::Fishing(_) => send(GameAction::Cast { power }),
            Station::Court => {
                send(GameAction::Shoot { power: (frac * 1000.0) as u16, yaw: look.yaw, pitch: look.pitch })
            }
            Station::Pit => {}
            // The lot: throw the ball in hand, or kick the one at the feet
            // along the look (a football on a tee is a field goal).
            _ if holding_lot_ball => {
                send(GameAction::Shoot { power: (frac * 1000.0) as u16, yaw: look.yaw, pitch: look.pitch })
            }
            _ => {
                let kick = Kick { power, yaw: look.yaw, pitch: look.pitch, curve };
                match at_feet {
                    Some((PropKind::Football, b)) if kicks::tee_at(b.x, b.z).is_some() => {
                        send(GameAction::FieldGoal { stake, kick })
                    }
                    Some(_) => send(GameAction::Kick { kick }),
                    None => {}
                }
            }
        }
    }
    match st {
        Station::Fishing(_) => {}
        Station::Court => {
            if pressed(KeyCode::Digit1) {
                send(GameAction::JoinHoops { mode: Mode::ThreeOfFive, fun: fun.0 });
            }
            if pressed(KeyCode::Digit2) {
                send(GameAction::JoinHoops { mode: Mode::Horse, fun: fun.0 });
            }
            // E picks up a ball here (the host's hands), so 3 starts.
            if pressed(KeyCode::Digit3) {
                send(GameAction::StartHoops);
            }
        }
        Station::Penalty => {
            if pressed(KeyCode::Digit1) {
                send(GameAction::JoinShootout { fun: fun.0 });
            }
            if use_key {
                send(GameAction::StartShootout);
            }
            if pressed(KeyCode::Digit7) {
                send(GameAction::BetKick { goal: true, amount: KEY_BET });
            }
            if pressed(KeyCode::Digit8) {
                send(GameAction::BetKick { goal: false, amount: KEY_BET });
            }
        }
        Station::Tee => {}
        Station::Goal => {
            if use_key {
                send(GameAction::TakeGoal);
            }
            for (k, dive) in
                [(KeyCode::Digit1, Dive::Left), (KeyCode::Digit2, Dive::Center), (KeyCode::Digit3, Dive::Right)]
            {
                if pressed(k) {
                    send(GameAction::Dive { dive });
                }
            }
            // RMB dives toward the side held. The goalie faces the kicker,
            // and dives are named from the kicker's side: A (the goalie's
            // left) is the kicker's right.
            if rmb {
                let dive = match (keys.pressed(KeyCode::KeyA), keys.pressed(KeyCode::KeyD)) {
                    (true, false) => Dive::Right,
                    (false, true) => Dive::Left,
                    _ => Dive::Center,
                };
                send(GameAction::Dive { dive });
            }
        }
        Station::Lane => {
            if use_key {
                send(GameAction::Run { stake });
            }
            if rmb {
                send(GameAction::StiffArm);
            }
            if pressed(KeyCode::KeyA) {
                if t - charge.last_a < 0.3 {
                    send(GameAction::Dodge { right: false });
                }
                charge.last_a = t;
            }
            if pressed(KeyCode::KeyD) {
                if t - charge.last_d < 0.3 {
                    send(GameAction::Dodge { right: true });
                }
                charge.last_d = t;
            }
        }
        Station::Pit => {
            if pressed(KeyCode::Digit1) {
                send(GameAction::JoinPit { teams: false, fun: fun.0 });
            }
            if pressed(KeyCode::Digit2) {
                send(GameAction::JoinPit { teams: true, fun: fun.0 });
            }
            if use_key {
                match pit::RACKS.iter().find(|(_, x, z)| (pos.x - x).hypot(pos.z - z) <= pit::RACK_REACH) {
                    Some((w, ..)) => send(GameAction::Pick { weapon: *w }),
                    None => send(GameAction::StartPit),
                }
            }
            // Hold to fire (the host keeps each weapon's rate).
            if lmb {
                send(GameAction::Fire { yaw: look.yaw, pitch: look.pitch, view_tick: 0 });
            }
        }
    }
}

/// Shots name the tick this client sees the others at (its interpolation
/// timeline), so the host can rewind to it.
fn fill_view_ticks(mut out: ResMut<OutgoingGame>, timeline: Option<Res<InterpolationTimeline>>) {
    let tick = timeline.map_or(0, |t| t.tick().0);
    for a in &mut out.0 {
        if let GameAction::Fire { view_tick, .. } = a
            && *view_tick == 0
        {
            *view_tick = tick;
        }
    }
}

// ---------- Panel ----------

#[derive(Component)]
struct GamePanel;

/// The fee line on a game's card: what it costs, or that it is for fun.
fn fee_hint(fun: bool, fee: i64) -> String {
    if fun { "0 for fun: ON (no stake, no winnings)".into() } else { format!("${fee} to play   0 play for fun") }
}

fn update_panel(
    fun: Res<ForFun>,
    status: Res<NetStatus>,
    nodraw: Option<Res<NoDraw>>,
    mut commands: Commands,
    mut panel: Query<(&mut Text, &mut Visibility), With<GamePanel>>,
) {
    if nodraw.is_some() {
        return;
    }
    let Ok((mut text, mut vis)) = panel.single_mut() else {
        commands.spawn((
            GamePanel,
            crate::online::PagePanel("game"),
            Text::new(""),
            TextFont { font_size: bevy::text::FontSize::Px(15.0), ..default() },
            TextColor(Color::srgb(0.9, 0.95, 1.0)),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            Node {
                position_type: PositionType::Absolute,
                top: px(40),
                left: px(10),
                padding: UiRect::all(px(6)),
                ..default()
            },
            Visibility::Hidden,
        ));
        return;
    };
    let g = &status.game.games;
    let me = status.player_id.map(hex);
    let name = |id: &Option<String>| match id {
        Some(i) if Some(i) == me.as_ref() => "YOU".to_string(),
        Some(i) => i[..6].to_string(),
        None => "nobody".to_string(),
    };
    let s = match g.station.as_deref() {
        Some(st) if st.starts_with("Fishing") => {
            let spot = st.trim_start_matches("Fishing(").trim_end_matches(')').parse::<usize>().unwrap_or(0);
            let f = g.fishing.get(spot).cloned().unwrap_or_default();
            let mut s =
                format!("FISHING (spot {})\nFisher: {}  {}\n", spot + 1, name(&f.fisher), f.phase.to_uppercase());
            if f.phase == "reeling" {
                s += &format!(
                    "Tension {} (keep {}-{})  landed {}%\n",
                    f.tension,
                    f.band.0,
                    f.band.1,
                    u32::from(f.progress) * 100 / u32::from(f.need.max(1))
                );
            }
            if let Some((who, c)) = &f.last {
                s += &format!("Last: {} {}\n", name(&Some(who.clone())), c);
            }
            s + "\nLMB hold, let go: cast   RMB strike at a bite   LMB hold: reel\n7 bet $10 it lands   8 bet $10 it snaps"
        }
        Some("Court") => {
            let h = &g.hoops;
            let mut s =
                format!("BASKETBALL  {}\n", h.mode.map_or("practice".to_string(), |m| format!("{m} pot ${}", h.pot)));
            for (who, shots, score, out) in &h.entrants {
                let turn = if h.shooter.as_ref() == Some(who) { " <-" } else { "" };
                let score = if h.mode == Some("horse") {
                    hoops::HORSE[..usize::from(*score)].to_string()
                } else {
                    format!("{score}/{shots}")
                };
                s += &format!("{} {score}{}{turn}\n", name(&Some(who.clone())), if *out { " OUT" } else { "" });
            }
            if let Some((who, made)) = &h.last {
                s += &format!("Last shot: {} {}\n", name(&Some(who.clone())), if *made { "MADE" } else { "missed" });
            }
            if h.crowd_secs > 0 {
                s += &format!("A crowd! Drinks sell 20% better for {} s\n", h.crowd_secs);
            }
            s + &format!(
                "\nE pick up a ball   LMB hold, let go: shoot (aim with the look)\n1 join five shots   2 join HORSE   3 start   {}",
                fee_hint(fun.0, hoops::ENTRY)
            )
        }
        Some("Penalty") | Some("Goal") => {
            let p = &g.penalties;
            let mut s = format!(
                "PENALTIES  goalie: {}\n",
                p.goalie.as_ref().map_or("NPC".to_string(), |g| name(&Some(g.clone())))
            );
            for (who, kicks, goals) in &p.entrants {
                let turn = if p.kicker.as_ref() == Some(who) { " <-" } else { "" };
                s += &format!("{} {goals}/{kicks}{turn}\n", name(&Some(who.clone())));
            }
            if let Some((who, r)) = &p.last {
                s += &format!("Last kick: {} {}\n", name(&Some(who.clone())), r.to_uppercase());
            }
            if g.station.as_deref() == Some("Goal") {
                s + "\nE stand in goal\nRMB dive (hold A or D for a side)"
            } else {
                s + &format!(
                    "\nLMB hold, let go: kick the ball at your feet (aim with the look, A or D curls it)\n1 join   E start   {}   7 bet $10 goal   8 bet $10 miss",
                    fee_hint(fun.0, kicks::ENTRY)
                )
            }
        }
        Some("Tee") => {
            let f = &g.field_goal;
            let odds = kicks::TEES.iter().find(|(y, ..)| *y == f.yards).map_or(1, |t| t.3);
            let mut s = format!("FIELD GOAL  {} yard tee (pays {odds} to 1)\n", f.yards);
            if let Some((who, y, good, ret)) = &f.last {
                s += &format!(
                    "Last: {} from {y}: {} (${ret})\n",
                    name(&Some(who.clone())),
                    if *good { "GOOD" } else { "no good" }
                );
            }
            s + &format!(
                "\nLMB hold, let go: kick it over the bar   {}\nE pick up the ball, Q put it down: carry it to the 10, 15 or 20 yard tee",
                fee_hint(fun.0, KEY_STAKE)
            )
        }
        Some("Lane") => {
            let r = &g.gauntlet;
            let mut s = format!("THE GAUNTLET  runner: {}  {} s\n", name(&r.runner), r.seconds_left);
            if let Some((who, e)) = &r.last {
                s += &format!("Last run: {} {}\n", name(&Some(who.clone())), e.to_uppercase());
            }
            s + &format!("\nE run   {}\nDouble-tap A or D: dodge   RMB stiff arm", fee_hint(fun.0, KEY_STAKE))
        }
        Some("Pit") => {
            let p = &g.pit;
            let mut s = format!(
                "FIGHT PIT  {}  pot ${}  {}\n",
                if p.teams { "teams" } else { "free for all" },
                p.pot,
                if p.started { format!("{} s", p.seconds_left) } else { "waiting".into() }
            );
            for (who, team, kills, health, weapon, ammo, down) in &p.fighters {
                s += &format!(
                    "{} {}takedowns {kills}  hp {health}  {} {}{}\n",
                    name(&Some(who.clone())),
                    if p.teams { format!("team {}  ", team + 1) } else { String::new() },
                    weapon.unwrap_or("unarmed"),
                    if weapon.is_some_and(|w| w != "foam bat") { ammo.to_string() } else { String::new() },
                    if *down { "  DOWN" } else { "" }
                );
            }
            s + &format!(
                "\n1 join   2 join, vote teams   E start (at a rack: take its weapon)   {}\nLMB fire",
                fee_hint(fun.0, pit::ENTRY)
            )
        }
        _ => String::new(),
    };
    if s.is_empty() {
        vis.set_if_neq(Visibility::Hidden);
    } else {
        vis.set_if_neq(Visibility::Inherited);
        if text.0 != s {
            text.0 = s;
        }
    }
}

// ---------- Visuals ----------

fn setup_stations(
    mut commands: Commands,
    nodraw: Option<Res<NoDraw>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if nodraw.is_some() {
        return;
    }
    // The hoop: rim, net, backboard, pole (the host's colliders match).
    commands.spawn((
        Mesh3d(meshes.add(crate::art::hoop())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.6,
            ..default()
        })),
        Transform::from_xyz(hoops::HOOP.0, hoops::RIM_Y, hoops::HOOP.1),
    ));
    let mut mat = |c: Color| materials.add(StandardMaterial { base_color: c, ..default() });
    let white = mat(Color::srgb(0.9, 0.9, 0.9));
    let orange = mat(Color::srgb(1.0, 0.45, 0.1));
    let wood = mat(Color::srgb(0.35, 0.22, 0.12));
    let goal = meshes.add(crate::art::soccer_goal());
    commands.spawn((
        Mesh3d(goal),
        MeshMaterial3d(white.clone()),
        Transform::from_xyz(kicks::GOAL.0, 0.0, kicks::GOAL.1),
    ));
    let mut post = |commands: &mut Commands, size: Vec3, at: Vec3, m: &Handle<StandardMaterial>| {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(m.clone()),
            Transform::from_translation(at),
        ));
    };
    // The goal: posts, bar and net (the host's colliders match), the spot.
    post(
        &mut commands,
        Vec3::new(0.3, 0.01, 0.3),
        Vec3::new(kicks::PENALTY_SPOT.0, 0.005, kicks::PENALTY_SPOT.1),
        &white,
    );
    // The uprights, at the lot's north end straight up from the tees: a
    // post behind the bar's plane, the crossbar, two uprights.
    let (ux, uz) = kicks::UPRIGHTS;
    post(
        &mut commands,
        Vec3::new(0.15, kicks::CROSSBAR, 0.15),
        Vec3::new(ux, kicks::CROSSBAR / 2.0, uz + 0.6),
        &orange,
    );
    post(&mut commands, Vec3::new(0.1, 0.1, 0.6), Vec3::new(ux, kicks::CROSSBAR, uz + 0.3), &orange);
    post(&mut commands, Vec3::new(kicks::UPRIGHT_HALF * 2.0, 0.1, 0.1), Vec3::new(ux, kicks::CROSSBAR, uz), &orange);
    for side in [-1.0, 1.0] {
        post(
            &mut commands,
            Vec3::new(0.1, kicks::UPRIGHT_HEIGHT, 0.1),
            Vec3::new(ux + side * kicks::UPRIGHT_HALF, kicks::CROSSBAR + kicks::UPRIGHT_HEIGHT / 2.0, uz),
            &orange,
        );
    }
    // The tees: a yard line and an orange tee on each.
    for (_, tx, tz, _) in kicks::TEES {
        post(&mut commands, Vec3::new(3.0, 0.01, 0.1), Vec3::new(tx, 0.005, tz), &white);
        post(&mut commands, Vec3::new(0.12, 0.04, 0.12), Vec3::new(tx, 0.02, tz), &orange);
    }
    // Gauntlet lane end zone, fishing spots, pit racks.
    post(
        &mut commands,
        Vec3::new(gauntlet::LANE_X.1 - gauntlet::LANE_X.0, 0.02, 1.0),
        Vec3::new(gauntlet::LANE_MID, 0.01, gauntlet::END_Z - 0.5),
        &white,
    );
    for (x, z) in fishing::SPOTS {
        post(&mut commands, Vec3::new(0.6, 0.4, 0.6), Vec3::new(x, 0.2, z), &wood);
    }
    for (_, x, z) in pit::RACKS {
        post(&mut commands, Vec3::new(0.8, 1.6, 0.3), Vec3::new(x, 0.8, z), &wood);
    }
}

/// The house keeper in the penalty goal (when no player is in goal): a
/// person who leans out and drops into the dive the host is making.
#[derive(Component)]
struct KeeperFigure;

fn draw_keeper(
    mut commands: Commands,
    people: Option<Res<crate::people::People>>,
    views: Query<&PenaltyView>,
    mut keeper: Query<(&mut Transform, &mut Visibility), With<KeeperFigure>>,
) {
    let Some(v) = views.iter().next() else { return };
    let Ok((mut t, mut vis)) = keeper.single_mut() else {
        if let Some(people) = people {
            let body = commands
                .spawn((KeeperFigure, Transform::default(), Visibility::Hidden, crate::people::Motion::default()))
                .id();
            crate::people::dress(&mut commands, &people, body, 6);
        }
        return;
    };
    let k = v.keeper;
    vis.set_if_neq(if k.npc { Visibility::Inherited } else { Visibility::Hidden });
    // The same pose the host gives the keeper's body.
    let side = k.dive.map_or(0.0, |d| d.side());
    let half = shared::bar::PLAYER_HEIGHT / 2.0;
    let (x, y) = if side != 0.0 {
        (k.x + side * 1.4 * k.progress, half - 0.45 * k.progress)
    } else {
        (k.x, half - 0.2 * k.progress)
    };
    t.translation = Vec3::new(x, y, kicks::GOALIE_SPOT.1);
    t.rotation = Quat::from_rotation_z(-side * std::f32::consts::FRAC_PI_2 * k.progress);
}

#[derive(Component)]
struct TacklerMesh(usize);

fn draw_tacklers(
    mut commands: Commands,
    nodraw: Option<Res<NoDraw>>,
    views: Query<&GauntletView>,
    mut tacklers: Query<(&TacklerMesh, &mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if nodraw.is_some() {
        return;
    }
    let Some(v) = views.iter().next() else { return };
    if tacklers.is_empty() {
        let mesh = meshes.add(Capsule3d::new(0.35, 1.0));
        let mat = materials.add(StandardMaterial { base_color: Color::srgb(0.2, 0.5, 0.2), ..default() });
        for i in 0..6 {
            commands.spawn((
                TacklerMesh(i),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::default(),
                Visibility::Hidden,
            ));
        }
        return;
    }
    for (m, mut t, mut vis) in &mut tacklers {
        match v.run.as_ref().and_then(|r| r.tacklers.get(m.0)) {
            Some(k) => {
                t.translation = Vec3::new(k.pos[0], 0.85, k.pos[1]);
                t.rotation = if k.stunned > 0 { Quat::from_rotation_x(1.2) } else { Quat::IDENTITY };
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

#[derive(Component)]
struct TracerMesh {
    born: f32,
}

fn draw_tracers(
    mut commands: Commands,
    time: Res<Time>,
    nodraw: Option<Res<NoDraw>>,
    views: Query<&PitView>,
    old: Query<(Entity, &TracerMesh)>,
    mut seen: Local<u32>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if nodraw.is_some() {
        return;
    }
    let now = time.elapsed_secs();
    for (e, t) in &old {
        if now - t.born > 0.15 {
            commands.entity(e).despawn();
        }
    }
    let Some(v) = views.iter().next() else { return };
    if v.shots == *seen {
        return;
    }
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.9, 0.4),
        emissive: LinearRgba::rgb(3.0, 2.5, 0.8),
        ..default()
    });
    for tr in v.tracers.iter().rev().take((v.shots - *seen).min(8) as usize) {
        let (a, b) = (Vec3::from_array(tr.from), Vec3::from_array(tr.to));
        let len = a.distance(b);
        if len < 0.01 {
            continue;
        }
        commands.spawn((
            TracerMesh { born: now },
            Mesh3d(meshes.add(Cuboid::new(0.02, 0.02, len))),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation((a + b) / 2.0).looking_at(b, Vec3::Y),
        ));
    }
    *seen = v.shots;
}

#[derive(Component)]
struct Bobber(u8);

fn draw_bobbers(
    mut commands: Commands,
    time: Res<Time>,
    nodraw: Option<Res<NoDraw>>,
    views: Query<&FishingView>,
    mut bobbers: Query<(&Bobber, &mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if nodraw.is_some() {
        return;
    }
    if bobbers.is_empty() {
        let mesh = meshes.add(Sphere::new(0.08));
        let mat = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.1, 0.1), ..default() });
        for i in 0..fishing::SPOTS.len() as u8 {
            commands.spawn((
                Bobber(i),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::default(),
                Visibility::Hidden,
            ));
        }
        return;
    }
    let t = time.elapsed_secs();
    for (b, mut tr, mut vis) in &mut bobbers {
        let Some(v) = views.iter().find(|v| v.spot == b.0) else { continue };
        if v.phase == FishPhase::Idle {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let (x, z) = fishing::SPOTS[usize::from(b.0)];
        let out = 3.0 + 4.0 * f32::from(v.zone);
        let dip = match v.phase {
            FishPhase::Biting => -0.15,
            FishPhase::Reeling => -0.1 + (t * 9.0).sin() * 0.05,
            _ => (t * 2.0).sin() * 0.02,
        };
        tr.translation = Vec3::new(x, -0.55 + dip, z + out);
        vis.set_if_neq(Visibility::Inherited);
    }
}
