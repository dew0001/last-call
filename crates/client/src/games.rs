//! The side games on the client (plan sections 5.5 to 5.9): a panel for
//! the station the player stands at, the keys that send [`GameRequest`]s,
//! and gray-box visuals (the hoop and the ball's flight, the goal, the
//! uprights, the tacklers, the pit's racks and tracers, the bobbers).
//!
//! Keys at a station: hold C to charge and let go to cast, shoot or kick;
//! B strikes at a bite; hold N to reel. 1 and 2 enter a contest, Enter
//! starts it. At the goal: G stands in goal; 4, 5, 6 dive left, center,
//! right; 7 and 8 bet $10 on goal or miss. At the tee, 1 to 3 pick the
//! distance. In the lane: 1 starts a run, double-tap A or D to dodge, G to
//! stiff-arm. In the pit: 1 or 2 enter (free for all or teams), 3 takes the
//! rack's weapon, the left mouse button fires.

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
    app.init_resource::<Charge>().init_resource::<FieldGoalPick>();
    app.add_systems(Startup, setup_stations);
    app.add_systems(
        Update,
        (
            game_keys,
            fill_view_ticks,
            draw_ball,
            draw_tacklers,
            draw_tracers,
            draw_bobbers,
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
        Room::ParkingLot if near(kicks::TEE, kicks::SPOT_REACH) => Some(Station::Tee),
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
    pick: Res<'w, FieldGoalPick>,
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
                yards: self.pick.0,
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

/// The field goal distance picked at the tee.
#[derive(Resource)]
pub struct FieldGoalPick(pub u8);

impl Default for FieldGoalPick {
    fn default() -> Self {
        Self(20)
    }
}

/// Seconds of holding for a full charge.
const FULL_CHARGE: f32 = 1.5;
/// Stake for a field goal, a run or a bet from the keys.
const KEY_STAKE: i64 = 20;
const KEY_BET: i64 = 10;

#[allow(clippy::too_many_arguments)]
fn game_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    time: Res<Time>,
    status: Res<NetStatus>,
    look: Res<Look>,
    mut charge: ResMut<Charge>,
    mut pick: ResMut<FieldGoalPick>,
    mut out: ResMut<OutgoingGame>,
) {
    let Some(pos) = status.own_pos else { return };
    let Some(st) = station(pos.x, pos.z) else {
        charge.held = None;
        return;
    };
    let t = time.elapsed_secs();
    let pressed = |k: KeyCode| keys.just_pressed(k);
    let mut send = |a: GameAction| out.0.push(a);

    // Charge with C; let go to act.
    if keys.pressed(KeyCode::KeyC) {
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
            Station::Penalty => {
                // Aim: the look's angle off the line to the goal.
                let to_goal = shared::math::atan2(-(kicks::GOAL.0 - pos.x), -(kicks::GOAL.1 - pos.z));
                let aim = ((to_goal - look.yaw) / 0.35).clamp(-1.0, 1.0);
                send(GameAction::Kick { kick: Kick { aim, power, curve } });
            }
            Station::Tee => {
                let aim = (-look.yaw.sin() / 0.35).clamp(-1.0, 1.0);
                send(GameAction::FieldGoal { yards: pick.0, stake: KEY_STAKE, kick: Kick { aim, power, curve } });
            }
            _ => {}
        }
    }
    match st {
        Station::Fishing(spot) => {
            if pressed(KeyCode::KeyB) {
                send(GameAction::Hook);
            }
            let reel = keys.pressed(KeyCode::KeyN);
            if reel != charge.reeling {
                charge.reeling = reel;
                send(GameAction::Reel { held: reel });
            }
            if pressed(KeyCode::Digit7) {
                send(GameAction::BetCatch { spot, side: Side::Lands, amount: KEY_BET });
            }
            if pressed(KeyCode::Digit8) {
                send(GameAction::BetCatch { spot, side: Side::Snaps, amount: KEY_BET });
            }
        }
        Station::Court => {
            if pressed(KeyCode::Digit1) {
                send(GameAction::JoinHoops { mode: Mode::ThreeOfFive });
            }
            if pressed(KeyCode::Digit2) {
                send(GameAction::JoinHoops { mode: Mode::Horse });
            }
            if pressed(KeyCode::Enter) {
                send(GameAction::StartHoops);
            }
        }
        Station::Penalty | Station::Goal => {
            if pressed(KeyCode::Digit1) {
                send(GameAction::JoinShootout);
            }
            if pressed(KeyCode::Enter) {
                send(GameAction::StartShootout);
            }
            if pressed(KeyCode::KeyG) {
                send(GameAction::TakeGoal);
            }
            for (k, dive) in
                [(KeyCode::Digit4, Dive::Left), (KeyCode::Digit5, Dive::Center), (KeyCode::Digit6, Dive::Right)]
            {
                if pressed(k) {
                    send(GameAction::Dive { dive });
                }
            }
            if pressed(KeyCode::Digit7) {
                send(GameAction::BetKick { goal: true, amount: KEY_BET });
            }
            if pressed(KeyCode::Digit8) {
                send(GameAction::BetKick { goal: false, amount: KEY_BET });
            }
        }
        Station::Tee => {
            for (k, (y, _)) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3].into_iter().zip(kicks::FIELD_GOALS) {
                if pressed(k) {
                    pick.0 = y;
                }
            }
        }
        Station::Lane => {
            if pressed(KeyCode::Digit1) {
                send(GameAction::Run { stake: KEY_STAKE });
            }
            if pressed(KeyCode::KeyG) {
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
                send(GameAction::JoinPit { teams: false });
            }
            if pressed(KeyCode::Digit2) {
                send(GameAction::JoinPit { teams: true });
            }
            if pressed(KeyCode::Enter) {
                send(GameAction::StartPit);
            }
            if pressed(KeyCode::Digit3)
                && let Some((w, ..)) =
                    pit::RACKS.iter().find(|(_, x, z)| (pos.x - x).hypot(pos.z - z) <= pit::RACK_REACH)
            {
                send(GameAction::Pick { weapon: *w });
            }
            // Hold to fire (the host keeps each weapon's rate).
            if mouse.as_ref().is_some_and(|m| m.pressed(MouseButton::Left)) {
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

fn update_panel(
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
            s + "\nHold C, let go: cast   B strike   hold N: reel\n7 bet $10 lands it   8 bet $10 snaps"
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
            s + "\n1 join 3 of 5 ($20)   2 join HORSE ($20)   Enter start\nHold C, let go: shoot (aim with the look)"
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
            s + "\n1 join ($20)  Enter start  G stand in goal  4/5/6 dive L/C/R\nHold C, let go: kick (A/D at release curves)  7/8 bet $10 goal/miss"
        }
        Some("Tee") => {
            let f = &g.field_goal;
            let mut s = format!(
                "FIELD GOAL  {} yards (pays {} to 1), stake $20\n",
                f.yards,
                kicks::FIELD_GOALS.iter().find(|(y, _)| *y == f.yards).map_or(1, |x| x.1)
            );
            if let Some((who, y, good, ret)) = &f.last {
                s += &format!(
                    "Last: {} from {y}: {} (${ret})\n",
                    name(&Some(who.clone())),
                    if *good { "GOOD" } else { "no good" }
                );
            }
            s + "\n1/2/3: 20/30/40 yards   Hold C, let go: kick"
        }
        Some("Lane") => {
            let r = &g.gauntlet;
            let mut s = format!("THE GAUNTLET  runner: {}  {} s\n", name(&r.runner), r.seconds_left);
            if let Some((who, e)) = &r.last {
                s += &format!("Last run: {} {}\n", name(&Some(who.clone())), e.to_uppercase());
            }
            s + "\n1 run ($20, double your money)  double-tap A/D dodge  G stiff arm"
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
            s + "\n1 join ($25)  2 join, vote teams  Enter start  3 take the rack's weapon\nLeft mouse: fire"
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
    let mut mat = |c: Color| materials.add(StandardMaterial { base_color: c, ..default() });
    let white = mat(Color::srgb(0.9, 0.9, 0.9));
    let orange = mat(Color::srgb(1.0, 0.45, 0.1));
    let wood = mat(Color::srgb(0.35, 0.22, 0.12));
    let mut post = |commands: &mut Commands, size: Vec3, at: Vec3, m: &Handle<StandardMaterial>| {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(m.clone()),
            Transform::from_translation(at),
        ));
    };
    // The hoop: pole, backboard, rim.
    post(
        &mut commands,
        Vec3::new(0.15, hoops::RIM_Y, 0.15),
        Vec3::new(hoops::HOOP.0 + 0.9, hoops::RIM_Y / 2.0, hoops::HOOP.1),
        &white,
    );
    post(
        &mut commands,
        Vec3::new(0.05, 1.05, 1.8),
        Vec3::new(hoops::HOOP.0 + 0.4, hoops::RIM_Y + 0.4, hoops::HOOP.1),
        &white,
    );
    post(&mut commands, Vec3::new(0.46, 0.03, 0.46), Vec3::new(hoops::HOOP.0, hoops::RIM_Y, hoops::HOOP.1), &orange);
    // The goal frame.
    let (gx, gz) = kicks::GOAL;
    for side in [-1.0, 1.0] {
        post(
            &mut commands,
            Vec3::new(0.12, kicks::BAR, 0.12),
            Vec3::new(gx + side * kicks::GOAL_HALF_WIDTH, kicks::BAR / 2.0, gz),
            &white,
        );
    }
    post(&mut commands, Vec3::new(kicks::GOAL_HALF_WIDTH * 2.0, 0.12, 0.12), Vec3::new(gx, kicks::BAR, gz), &white);
    // The uprights, at the lot's north end opposite the tee.
    let (tx, _) = kicks::TEE;
    for side in [-1.0, 1.0] {
        post(&mut commands, Vec3::new(0.1, 6.0, 0.1), Vec3::new(tx + 2.0 + side * 1.5, 3.0, 26.5), &orange);
    }
    post(&mut commands, Vec3::new(3.0, 0.1, 0.1), Vec3::new(tx + 2.0, kicks::CROSSBAR, 26.5), &orange);
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

/// The basketball, flying the last shot's path (the shared flight).
#[derive(Component)]
struct Ball {
    shots: u32,
    path: Vec<[f32; 3]>,
    started: f32,
}

#[allow(clippy::type_complexity)]
fn draw_ball(
    mut commands: Commands,
    time: Res<Time>,
    nodraw: Option<Res<NoDraw>>,
    views: Query<&HoopsView>,
    mut ball: Query<(&mut Ball, &mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if nodraw.is_some() {
        return;
    }
    let Some(v) = views.iter().next() else { return };
    let Ok((mut b, mut t, mut vis)) = ball.single_mut() else {
        commands.spawn((
            Ball { shots: v.shots, path: Vec::new(), started: 0.0 },
            Mesh3d(meshes.add(Sphere::new(0.12))),
            MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.9, 0.4, 0.1), ..default() })),
            Transform::default(),
            Visibility::Hidden,
        ));
        return;
    };
    if b.shots != v.shots
        && let Some(s) = v.last
    {
        b.shots = v.shots;
        b.path = hoops::fly(&s.shot).points;
        b.started = time.elapsed_secs();
    }
    let i = ((time.elapsed_secs() - b.started) * shared::TICK_HZ as f32) as usize;
    match b.path.get(i) {
        Some(p) => {
            t.translation = Vec3::from_array(*p);
            vis.set_if_neq(Visibility::Inherited);
        }
        None => {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
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
