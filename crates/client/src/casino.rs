//! The casino floor on the client: table meshes, cards on the felt, the
//! roulette wheel and slot reels, a table panel with key prompts, and the
//! keys that send [`TableRequest`]s.
//!
//! Gray box: cards are blank tiles and the panel spells them out. The art
//! pass (Phase 6) replaces the panel with chips and cards on the felt.

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::blackjack::{self, Action};
use shared::casino::{self, TableAction, TableId};
use shared::client::{OutgoingTable, Session};
use shared::drunk::Tier;
use shared::minigame::Who;
use shared::protocol::*;
use shared::roulette::{self, Bet as RBet};
use shared::slots::{self, Symbol};

use crate::online::{NetStatus, NoDraw};

pub fn add(app: &mut App) {
    app.init_resource::<RouletteChoice>();
    app.add_systems(Startup, (setup_tables, setup_panel));
    app.add_systems(Update, (table_keys, draw_cards, spin_wheel, spin_reels, update_panel).chain());
}

// ---------- Status for the page and tests ----------

/// The casino as this client sees it, in JSON-friendly form. Player ids are
/// 16 hex digits ("p:..."), customers "c:<id>".
#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CasinoStatus {
    /// The table this player stands at: "blackjack", "roulette", "slot0", "slot1".
    pub near: Option<String>,
    pub blackjack: Option<BlackjackStatus>,
    pub roulette: Option<RouletteStatus>,
    pub slots: Vec<SlotStatus>,
    /// Money in chip stacks on tables and the floor.
    pub chips: i64,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackjackStatus {
    pub dealer: Option<String>,
    /// "betting", "insurance", "players", "dealer".
    pub phase: &'static str,
    /// The seat to act, during "players".
    pub to_act: Option<u8>,
    pub dealer_cards: Vec<String>,
    pub dealer_should: Option<&'static str>,
    pub seats: Vec<SeatStatus>,
    /// This player's seat.
    pub my_seat: Option<usize>,
    pub rounds: u32,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatStatus {
    pub who: Option<String>,
    pub bet: i64,
    pub hands: Vec<(Vec<String>, u8, i64)>,
    pub insurance: Option<i64>,
    pub last: Option<i64>,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouletteStatus {
    pub croupier: Option<String>,
    pub bets: Vec<(String, String, i64)>,
    pub result: Option<u8>,
    pub spinning: bool,
    pub seconds_left: u8,
    pub to_rake: u8,
    pub last: Option<u8>,
    pub spins: u32,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotStatus {
    pub machine: u8,
    pub user: Option<String>,
    pub spinning: bool,
    pub line: Vec<&'static str>,
    pub last_bet: i64,
    pub last_return: i64,
    pub pulls: u32,
}

pub fn who_text(w: Who) -> String {
    match w {
        Who::Player(id) => format!("p:{id:016x}"),
        Who::Customer(id) => format!("c:{id}"),
    }
}

/// A card as text: "Ks", "10h", "??" for the hole card.
pub fn card_text(c: u8) -> String {
    if c == HIDDEN_CARD {
        return "??".into();
    }
    let r = match shared::cards::rank(c) {
        1 => "A".to_string(),
        11 => "J".to_string(),
        12 => "Q".to_string(),
        13 => "K".to_string(),
        n => n.to_string(),
    };
    format!("{r}{}", ['s', 'h', 'd', 'c'][usize::from(shared::cards::suit(c))])
}

fn action_name(a: Action) -> &'static str {
    match a {
        Action::Hit => "hit",
        Action::Stand => "stand",
        Action::Double => "double",
        Action::Split => "split",
    }
}

fn symbol_name(s: Symbol) -> &'static str {
    match s {
        Symbol::Cherry => "cherry",
        Symbol::Lemon => "lemon",
        Symbol::Bell => "bell",
        Symbol::Bar => "bar",
        Symbol::Seven => "seven",
    }
}

pub fn table_name(t: TableId) -> String {
    match t {
        TableId::Blackjack => "blackjack".into(),
        TableId::Roulette => "roulette".into(),
        TableId::Slot(i) => format!("slot{i}"),
    }
}

/// Build the casino part of the status.
pub fn status(
    me: Option<u64>,
    own: Option<Vec3>,
    bj: Option<&BlackjackView>,
    wheel: Option<&RouletteView>,
    machines: &[SlotView],
    chips: i64,
) -> CasinoStatus {
    let hex = |id: u64| format!("p:{id:016x}");
    CasinoStatus {
        near: own.and_then(|p| casino::nearest_table(p.x, p.z)).map(table_name),
        blackjack: bj.map(|v| BlackjackStatus {
            dealer: v.dealer.map(hex),
            phase: match v.phase {
                BjPhase::Betting => "betting",
                BjPhase::Insurance => "insurance",
                BjPhase::Players { .. } => "players",
                BjPhase::Dealer => "dealer",
            },
            to_act: match v.phase {
                BjPhase::Players { seat, .. } => Some(seat),
                _ => None,
            },
            dealer_cards: v.dealer_cards.iter().map(|c| card_text(*c)).collect(),
            dealer_should: v.dealer_should.map(action_name),
            seats: v
                .seats
                .iter()
                .map(|s| SeatStatus {
                    who: s.who.map(who_text),
                    bet: s.bet,
                    hands: s
                        .hands
                        .iter()
                        .map(|h| {
                            (h.cards.iter().map(|c| card_text(*c)).collect(), blackjack::hand_value(&h.cards).0, h.bet)
                        })
                        .collect(),
                    insurance: s.insurance,
                    last: s.last,
                })
                .collect(),
            my_seat: me.and_then(|id| v.seats.iter().position(|s| s.who == Some(Who::Player(id)))),
            rounds: v.rounds,
        }),
        roulette: wheel.map(|v| RouletteStatus {
            croupier: v.croupier.map(hex),
            bets: v.bets.iter().map(|(w, b, a)| (who_text(*w), b.label(), *a)).collect(),
            result: v.result,
            spinning: v.spinning,
            seconds_left: v.seconds_left,
            to_rake: v.to_rake,
            last: v.last,
            spins: v.spins,
        }),
        slots: machines
            .iter()
            .map(|m| SlotStatus {
                machine: m.machine,
                user: m.user.map(who_text),
                spinning: m.spinning,
                line: slots::line(m.stops).iter().map(|s| symbol_name(*s)).collect(),
                last_bet: m.last_bet,
                last_return: m.last_return,
                pulls: m.pulls,
            })
            .collect(),
        chips,
    }
}

// ---------- Keys ----------

/// The roulette bet this player has picked with Z and X.
#[derive(Resource, Default)]
pub struct RouletteChoice(pub usize);

/// Bets a player picks from at the roulette table, in Z/X order.
pub fn roulette_choices() -> Vec<RBet> {
    let mut out = vec![RBet::Red, RBet::Black, RBet::Odd, RBet::Even, RBet::Low, RBet::High];
    out.extend((0..3).map(RBet::Dozen));
    out.extend((0..3).map(RBet::Column));
    out.extend((0..=36).map(RBet::Straight));
    out
}

const BLACKJACK_BUTTONS: [i64; 4] = [10, 20, 50, 100];
const ROULETTE_BUTTONS: [i64; 4] = [1, 5, 25, 100];
const SLOT_BUTTONS: [i64; 3] = [1, 5, 10];
const DIGITS: [KeyCode; 4] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4];

/// The bet buttons for this player now (Courage raises the top one; Wasted shuffles them).
fn buttons_for(amounts: &[i64], table_max: i64, tier: Tier, time: f32) -> Vec<i64> {
    let mut a = amounts.to_vec();
    let max = casino::max_bet(table_max, tier);
    if let Some(last) = a.last_mut()
        && max > *last
        && *last == table_max
    {
        *last = max;
    }
    casino::bet_buttons(&a, tier, time as u64)
}

#[allow(clippy::too_many_arguments)]
fn table_keys(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    session: Res<Session>,
    status: Res<NetStatus>,
    mut choice: ResMut<RouletteChoice>,
    mut out: ResMut<OutgoingTable>,
    bj: Query<&BlackjackView>,
    upgrades: Query<&RoomUpgrades>,
) {
    let up = upgrades.single().map(|u| u.0).unwrap_or_default();
    let Some(pos) = status.own_pos else { return };
    let Some(table) = casino::nearest_table(pos.x, pos.z) else { return };
    let tier = Tier::of(status.game.drunk.as_ref().map_or(0, |d| d.level));
    let t = time.elapsed_secs();
    let mut send = |action: TableAction| out.0.push(TableRequest { table, action });
    let at_role = casino::at_role_spot(table, pos.x, pos.z);
    if keys.just_pressed(KeyCode::KeyT) {
        let mine = match table {
            TableId::Blackjack => status.game.casino.blackjack.as_ref().and_then(|b| b.dealer.clone()),
            TableId::Roulette => status.game.casino.roulette.as_ref().and_then(|r| r.croupier.clone()),
            TableId::Slot(_) => None,
        };
        let me = session.player_id.map(|id| format!("p:{id:016x}"));
        send(if mine.is_some() && mine == me { TableAction::LeaveRole } else { TableAction::TakeRole });
    }
    match table {
        TableId::Blackjack => {
            if at_role {
                if keys.just_pressed(KeyCode::Enter) {
                    send(TableAction::Deal);
                }
                let should = bj.iter().next().and_then(|v| v.dealer_should);
                if keys.just_pressed(KeyCode::KeyH) {
                    send(TableAction::Dealer(should.filter(|a| *a == Action::Hit).unwrap_or(Action::Hit)));
                }
                if keys.just_pressed(KeyCode::KeyG) {
                    send(TableAction::Dealer(Action::Stand));
                }
                return;
            }
            let amounts = buttons_for(&BLACKJACK_BUTTONS, up.table_max(casino::BLACKJACK_MAX), tier, t);
            for (key, amount) in DIGITS.iter().zip(&amounts) {
                if keys.just_pressed(*key) {
                    send(TableAction::Bet(*amount));
                }
            }
            for (key, action) in [
                (KeyCode::Digit0, TableAction::Bet(0)),
                (KeyCode::KeyH, TableAction::Play(Action::Hit)),
                (KeyCode::KeyG, TableAction::Play(Action::Stand)),
                (KeyCode::KeyJ, TableAction::Play(Action::Double)),
                (KeyCode::KeyK, TableAction::Play(Action::Split)),
                (KeyCode::KeyY, TableAction::Insure(true)),
                (KeyCode::KeyN, TableAction::Insure(false)),
            ] {
                if keys.just_pressed(key) {
                    send(action);
                }
            }
        }
        TableId::Roulette => {
            if at_role {
                if keys.just_pressed(KeyCode::Enter) {
                    send(TableAction::Spin);
                }
                if keys.just_pressed(KeyCode::KeyK) {
                    send(TableAction::Rake);
                }
                return;
            }
            let n = roulette_choices().len();
            if keys.just_pressed(KeyCode::KeyX) {
                choice.0 = (choice.0 + 1) % n;
            }
            if keys.just_pressed(KeyCode::KeyZ) {
                choice.0 = (choice.0 + n - 1) % n;
            }
            let bet = roulette_choices()[choice.0 % n];
            let amounts = buttons_for(&ROULETTE_BUTTONS, up.table_max(casino::ROULETTE_MAX), tier, t);
            for (key, amount) in DIGITS.iter().zip(&amounts) {
                if keys.just_pressed(*key) {
                    send(TableAction::RouletteBet(bet, *amount));
                }
            }
        }
        TableId::Slot(_) => {
            let amounts = buttons_for(&SLOT_BUTTONS, up.table_max(casino::SLOT_MAX), tier, t);
            for (key, amount) in DIGITS.iter().zip(&amounts) {
                if keys.just_pressed(*key) {
                    send(TableAction::Pull(*amount));
                }
            }
        }
    }
}

// ---------- Panel ----------

#[derive(Component)]
struct TablePanel;

fn setup_panel(mut commands: Commands, nodraw: Option<Res<NoDraw>>) {
    if nodraw.is_some() {
        return;
    }
    commands.spawn((
        TablePanel,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(15.0), ..default() },
        TextColor(Color::srgb(0.85, 1.0, 0.85)),
        BackgroundColor(Color::srgba(0.0, 0.08, 0.02, 0.7)),
        Node {
            position_type: PositionType::Absolute,
            // Below the page's room banner.
            top: px(56),
            right: px(10),
            max_width: px(420),
            padding: UiRect::all(px(8)),
            ..default()
        },
    ));
}

fn update_panel(
    time: Res<Time>,
    status: Res<NetStatus>,
    choice: Res<RouletteChoice>,
    mut panel: Query<(&mut Text, &mut Visibility), With<TablePanel>>,
    upgrades: Query<&RoomUpgrades>,
) {
    let up = upgrades.single().map(|u| u.0).unwrap_or_default();
    let Ok((mut text, mut vis)) = panel.single_mut() else { return };
    let c = &status.game.casino;
    let me = status.player_id.map(|id| format!("p:{id:016x}"));
    let pos = status.own_pos.unwrap_or_default();
    let tier = Tier::of(status.game.drunk.as_ref().map_or(0, |d| d.level));
    let t = time.elapsed_secs();
    let line = match c.near.as_deref() {
        Some("blackjack") => {
            let Some(b) = &c.blackjack else { return };
            let mut s = format!("BLACKJACK  (round {})\n", b.rounds);
            s += &format!(
                "Dealer: {}  {}\n",
                b.dealer.as_ref().map_or("nobody (stand behind the table, T)".into(), |d| {
                    if Some(d) == me.as_ref() { "YOU".into() } else { d.clone() }
                }),
                b.dealer_cards.join(" ")
            );
            for (i, seat) in b.seats.iter().enumerate() {
                let Some(who) = &seat.who else { continue };
                let name = if Some(who) == me.as_ref() { "YOU".to_string() } else { who.clone() };
                let hands: Vec<String> =
                    seat.hands.iter().map(|(cards, v, bet)| format!("{} = {v} (${bet})", cards.join(" "))).collect();
                let turn = if b.to_act == Some(i as u8) { " <-" } else { "" };
                let last = seat.last.map_or(String::new(), |l| format!("  last {l:+}"));
                s += &format!("Seat {}: {name} bet ${}  {}{turn}{last}\n", i + 1, seat.bet, hands.join(" | "));
            }
            let p4 = &status.game.phase4;
            if let Some(count) = p4.count {
                s += &format!("Hi-Lo count {count:+}\n");
            }
            if let Some(card) = &p4.hole_card {
                s += &format!("Marked deck: the hole card is {card}\n");
            }
            if casino::at_role_spot(TableId::Blackjack, pos.x, pos.z) {
                s += "\nT take/leave the deal   Enter deal   H hit   G stand";
                if let Some(a) = b.dealer_should {
                    s += &format!("\nThe house must {}", a.to_uppercase());
                }
            } else {
                let amounts = buttons_for(&BLACKJACK_BUTTONS, up.table_max(casino::BLACKJACK_MAX), tier, t);
                let keys: Vec<String> = amounts.iter().enumerate().map(|(i, a)| format!("{} ${a}", i + 1)).collect();
                s += &format!(
                    "\nBet: {}   0 stand up\nH hit   G stand   J double   K split   Y/N insurance",
                    keys.join("  ")
                );
            }
            s
        }
        Some("roulette") => {
            let Some(r) = &c.roulette else { return };
            let mut s = format!(
                "ROULETTE  (spin {})  last: {}\nCroupier: {}\n",
                r.spins,
                r.last.map_or("-".into(), |n| n.to_string()),
                r.croupier.as_ref().map_or(
                    "nobody (stand behind the table, T)".into(),
                    |d| if Some(d) == me.as_ref() { "YOU".into() } else { d.clone() }
                )
            );
            if r.spinning {
                s += &format!("Spinning... {} s\n", r.seconds_left);
            }
            if r.to_rake > 0 {
                s += &format!("{} losing chips to rake\n", r.to_rake);
            }
            for (who, label, amount) in r.bets.iter().take(10) {
                let name = if Some(who) == me.as_ref() { "YOU" } else { who.as_str() };
                s += &format!("  {name}: ${amount} on {label}\n");
            }
            if casino::at_role_spot(TableId::Roulette, pos.x, pos.z) {
                s += "\nT take/leave the wheel   Enter spin   K rake";
            } else {
                let choices = roulette_choices();
                let bet = choices[choice.0 % choices.len()];
                let amounts = buttons_for(&ROULETTE_BUTTONS, up.table_max(casino::ROULETTE_MAX), tier, t);
                let keys: Vec<String> = amounts.iter().enumerate().map(|(i, a)| format!("{} ${a}", i + 1)).collect();
                s += &format!("\nZ/X pick: {} (pays {} to 1)\n{}", bet.label(), bet.odds(), keys.join("  "));
            }
            s
        }
        Some(name) if name.starts_with("slot") => {
            let i: usize = name[4..].parse().unwrap_or(0);
            let Some(m) = c.slots.iter().find(|m| usize::from(m.machine) == i) else { return };
            let amounts = buttons_for(&SLOT_BUTTONS, up.table_max(casino::SLOT_MAX), tier, t);
            let keys: Vec<String> = amounts.iter().enumerate().map(|(i, a)| format!("{} ${a}", i + 1)).collect();
            format!(
                "SLOTS  (pull {})\n{}\nLast: ${} back on ${}\nPull: {}\n7 7 7 pays 150, BAR x3 50, bells 10, lemons 8,\ncherries 5, two cherries 2, one cherry 1",
                m.pulls,
                if m.spinning { "spinning...".into() } else { m.line.join(" | ") },
                m.last_return,
                m.last_bet,
                keys.join("  ")
            )
        }
        _ => String::new(),
    };
    vis.set_if_neq(if line.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
    if text.0 != line {
        text.0 = line;
    }
}

// ---------- Meshes ----------

#[derive(Component)]
struct Wheel;

#[derive(Component)]
struct Ball;

#[derive(Component)]
struct Reel(u8, usize);

#[derive(Component)]
struct CardTile;

#[derive(Resource)]
struct CardLooks {
    mesh: Handle<Mesh>,
    face: Handle<StandardMaterial>,
    back: Handle<StandardMaterial>,
}

#[derive(Resource)]
struct SymbolLooks([Handle<StandardMaterial>; 5]);

fn setup_tables(
    mut commands: Commands,
    nodraw: Option<Res<NoDraw>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if nodraw.is_some() {
        return;
    }
    let mut mat = |c: Color, e: f32| {
        materials.add(StandardMaterial {
            base_color: c,
            emissive: LinearRgba::from(c) * e,
            perceptual_roughness: 0.8,
            ..default()
        })
    };
    let felt = mat(Color::srgb(0.05, 0.4, 0.18), 0.0);
    let h = casino::FELT_HEIGHT;
    for ((cx, cz), (hx, hz)) in [(casino::BLACKJACK, casino::BLACKJACK_HALF), (casino::ROULETTE, casino::ROULETTE_HALF)]
    {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(hx * 2.0 - 0.1, 0.01, hz * 2.0 - 0.1))),
            MeshMaterial3d(felt.clone()),
            Transform::from_xyz(cx, h + 0.005, cz),
        ));
    }
    // The wheel: a dark disc at the roulette table's west end, with a ball.
    let (rx, rz) = casino::ROULETTE;
    let wheel_at = Vec3::new(rx - casino::ROULETTE_HALF.0 + 0.45, h + 0.03, rz);
    commands
        .spawn((
            Wheel,
            Mesh3d(meshes.add(Cylinder::new(0.38, 0.05))),
            MeshMaterial3d(mat(Color::srgb(0.25, 0.1, 0.05), 0.0)),
            Transform::from_translation(wheel_at),
        ))
        .with_children(|w| {
            // Red and black pocket marks around the rim.
            let red = mat(Color::srgb(0.7, 0.05, 0.05), 0.2);
            let black = mat(Color::srgb(0.05, 0.05, 0.05), 0.0);
            let green = mat(Color::srgb(0.0, 0.6, 0.2), 0.2);
            let mark = meshes.add(Cuboid::new(0.05, 0.02, 0.05));
            for (i, n) in roulette::WHEEL_ORDER.iter().enumerate() {
                let a = i as f32 / 37.0 * std::f32::consts::TAU;
                let m = if *n == 0 {
                    green.clone()
                } else if roulette::is_red(*n) {
                    red.clone()
                } else {
                    black.clone()
                };
                w.spawn((
                    Mesh3d(mark.clone()),
                    MeshMaterial3d(m),
                    Transform::from_xyz(a.sin() * 0.32, 0.03, a.cos() * 0.32),
                ));
            }
        });
    commands.spawn((
        Ball,
        Mesh3d(meshes.add(Sphere::new(0.025))),
        MeshMaterial3d(mat(Color::WHITE, 0.5)),
        Transform::from_translation(wheel_at + Vec3::Y * 0.06),
    ));
    // Slot reels: three tiles on each machine's front face.
    let symbols = [
        mat(Color::srgb(0.85, 0.05, 0.1), 0.6),
        mat(Color::srgb(0.95, 0.9, 0.1), 0.6),
        mat(Color::srgb(0.95, 0.6, 0.1), 0.6),
        mat(Color::srgb(0.2, 0.2, 0.9), 0.6),
        mat(Color::srgb(1.0, 1.0, 1.0), 0.8),
    ];
    let tile = meshes.add(Cuboid::new(0.02, 0.18, 0.14));
    for (m, (sx, sz)) in casino::SLOTS.iter().enumerate() {
        for r in 0..3 {
            commands.spawn((
                Reel(m as u8, r),
                Mesh3d(tile.clone()),
                MeshMaterial3d(symbols[0].clone()),
                Transform::from_xyz(sx + casino::SLOT_HALF.0 + 0.011, 1.2, sz - 0.17 + r as f32 * 0.17),
            ));
        }
    }
    commands.insert_resource(SymbolLooks(symbols));
    commands.insert_resource(CardLooks {
        mesh: meshes.add(Cuboid::new(0.09, 0.004, 0.13)),
        face: mat(Color::srgb(0.95, 0.95, 0.9), 0.1),
        back: mat(Color::srgb(0.6, 0.05, 0.1), 0.0),
    });
}

/// Lay the cards out on the felt whenever the table changes: the dealer's
/// along the dealer side, each seat's in front of the seat.
fn draw_cards(
    mut commands: Commands,
    looks: Option<Res<CardLooks>>,
    views: Query<&BlackjackView, Changed<BlackjackView>>,
    tiles: Query<Entity, With<CardTile>>,
) {
    let (Some(looks), Ok(v)) = (looks, views.single()) else { return };
    for e in &tiles {
        commands.entity(e).despawn();
    }
    let (cx, cz) = casino::BLACKJACK;
    let y = casino::FELT_HEIGHT + 0.012;
    let mut tile = |x: f32, z: f32, hidden: bool| {
        commands.spawn((
            CardTile,
            Mesh3d(looks.mesh.clone()),
            MeshMaterial3d(if hidden { looks.back.clone() } else { looks.face.clone() }),
            Transform::from_xyz(x, y, z),
        ));
    };
    for (i, c) in v.dealer_cards.iter().enumerate() {
        tile(cx - 0.15 + i as f32 * 0.1, cz - 0.3, *c == HIDDEN_CARD);
    }
    let spots = casino::bettor_spots(TableId::Blackjack);
    for (s, seat) in v.seats.iter().enumerate() {
        for (h, hand) in seat.hands.iter().enumerate() {
            for (i, _) in hand.cards.iter().enumerate() {
                tile(spots[s].0 - 0.08 + i as f32 * 0.04 + h as f32 * 0.2, cz + 0.25 - i as f32 * 0.03, false);
            }
        }
    }
}

/// Spin the wheel while the host's spin runs, and land the ball on the result.
fn spin_wheel(
    time: Res<Time>,
    views: Query<&RouletteView>,
    mut wheel: Query<&mut Transform, (With<Wheel>, Without<Ball>)>,
    mut ball: Query<&mut Transform, (With<Ball>, Without<Wheel>)>,
) {
    let (Ok(v), Ok(mut w), Ok(mut b)) = (views.single(), wheel.single_mut(), ball.single_mut()) else { return };
    if v.spinning {
        w.rotate_y(time.delta_secs() * (1.0 + f32::from(v.seconds_left)) * 1.5);
    }
    let pocket = v.result.or(v.last).unwrap_or(0);
    let i = roulette::WHEEL_ORDER.iter().position(|n| *n == pocket).unwrap_or(0);
    let a = i as f32 / 37.0 * std::f32::consts::TAU;
    let local = if v.spinning {
        // Racing round the rim until the spin ends.
        let s = time.elapsed_secs() * 7.0;
        Vec3::new(s.sin() * 0.36, 0.06, s.cos() * 0.36)
    } else {
        Vec3::new(a.sin() * 0.32, 0.06, a.cos() * 0.32)
    };
    b.translation = w.translation + w.rotation * local;
}

/// Reels flicker while spinning and show the stops after.
fn spin_reels(
    time: Res<Time>,
    looks: Option<Res<SymbolLooks>>,
    machines: Query<&SlotView>,
    mut reels: Query<(&Reel, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(looks) = looks else { return };
    for v in &machines {
        let line = slots::line(v.stops);
        for (reel, mut m) in &mut reels {
            if reel.0 != v.machine {
                continue;
            }
            let i = if v.spinning {
                ((time.elapsed_secs() * 12.0) as usize + reel.1 * 2) % 5
            } else {
                line[reel.1] as usize
            };
            if m.0 != looks.0[i] {
                m.0 = looks.0[i].clone();
            }
        }
    }
}

/// Every table view this client has, for the status.
#[derive(bevy::ecs::system::SystemParam)]
pub struct CasinoQueries<'w, 's> {
    pub bj: Query<'w, 's, &'static BlackjackView>,
    pub wheel: Query<'w, 's, &'static RouletteView>,
    pub slots: Query<'w, 's, &'static SlotView>,
    pub chips: Query<'w, 's, &'static ChipValue, With<Interpolated>>,
}

impl CasinoQueries<'_, '_> {
    pub fn status(&self, me: Option<u64>, own: Option<Vec3>) -> CasinoStatus {
        let mut machines: Vec<SlotView> = self.slots.iter().cloned().collect();
        machines.sort_by_key(|m| m.machine);
        machines.dedup_by_key(|m| m.machine);
        status(
            me,
            own,
            self.bj.iter().next(),
            self.wheel.iter().next(),
            &machines,
            self.chips.iter().map(|c| c.0).sum(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_texts() {
        assert_eq!(card_text(0), "As");
        assert_eq!(card_text(13 + 9), "10h");
        assert_eq!(card_text(51), "Kc");
        assert_eq!(card_text(HIDDEN_CARD), "??");
    }

    #[test]
    fn courage_raises_the_top_button() {
        assert_eq!(buttons_for(&BLACKJACK_BUTTONS, 100, Tier::Sober, 0.0), vec![10, 20, 50, 100]);
        assert_eq!(buttons_for(&BLACKJACK_BUTTONS, 100, Tier::Courage, 0.0), vec![10, 20, 50, 150]);
    }

    #[test]
    fn every_roulette_choice_is_a_real_bet() {
        assert!(roulette_choices().iter().all(RBet::is_valid));
    }
}
