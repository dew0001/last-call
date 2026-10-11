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

use crate::art;

use crate::online::{NetStatus, NoDraw};

pub fn add(app: &mut App) {
    app.init_resource::<RouletteChoice>();
    app.init_resource::<TablePanelText>();
    app.init_resource::<LastPull>();
    app.add_systems(Startup, (setup_tables, setup_panel));
    app.add_systems(
        Update,
        (table_keys, draw_cards, spin_wheel, spin_reels, update_panel.run_if(resource_changed::<NetStatus>)).chain(),
    );
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

/// The amount the left mouse button pulls a slot machine for (the last one
/// picked with a number key).
#[derive(Resource, Default)]
pub struct LastPull(pub usize);

/// Keys at a table, one scheme with the side games: E takes or leaves the
/// dealer's or croupier's spot; the left mouse button is the main action
/// (hit, deal, spin, pull), the right one the second (stand, rake); 1 to 4
/// bet; the mouse wheel (or Z and X) picks a roulette bet.
#[allow(clippy::too_many_arguments)]
fn table_keys(
    keys: Res<ButtonInput<KeyCode>>,
    clicks: Res<crate::online::Clicks>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    settings: Res<crate::online::Settings>,
    mut last_pull: ResMut<LastPull>,
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
    let (lmb, rmb) = (clicks.left, clicks.right);
    if at_role && keys.just_pressed(settings.keys.interact) {
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
                // The left button does the dealer's next job: deal a round,
                // or hit or stand as the house rules say.
                let should = bj.iter().next().and_then(|v| v.dealer_should);
                if lmb {
                    send(match should {
                        Some(a) => TableAction::Dealer(a),
                        None => TableAction::Deal,
                    });
                }
                if rmb {
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
            if lmb {
                send(TableAction::Play(Action::Hit));
            }
            if rmb {
                send(TableAction::Play(Action::Stand));
            }
            for (key, action) in [
                (KeyCode::Digit0, TableAction::Bet(0)),
                (KeyCode::KeyX, TableAction::Play(Action::Double)),
                (KeyCode::KeyC, TableAction::Play(Action::Split)),
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
                if lmb {
                    send(TableAction::Spin);
                }
                if rmb {
                    send(TableAction::Rake);
                }
                return;
            }
            let n = roulette_choices().len();
            if keys.just_pressed(KeyCode::KeyX) || scroll.delta.y < 0.0 {
                choice.0 = (choice.0 + 1) % n;
            }
            if keys.just_pressed(KeyCode::KeyZ) || scroll.delta.y > 0.0 {
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
            for (i, (key, amount)) in DIGITS.iter().zip(&amounts).enumerate() {
                if keys.just_pressed(*key) {
                    last_pull.0 = i;
                    send(TableAction::Pull(*amount));
                }
            }
            // The left button pulls again for the last amount.
            if lmb && let Some(amount) = amounts.get(last_pull.0).or(amounts.first()) {
                send(TableAction::Pull(*amount));
            }
        }
    }
}

// ---------- Panel ----------

#[derive(Component)]
struct TablePanel;

/// The table panel's text: native draws it with `bevy_ui`; the browser page
/// shows it as a styled card (web/hud.js reads `tablePanel`).
#[derive(Resource, Default)]
pub struct TablePanelText(pub String);

fn setup_panel(mut commands: Commands, nodraw: Option<Res<NoDraw>>) {
    if nodraw.is_some() || cfg!(target_arch = "wasm32") {
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
    mut shown: ResMut<TablePanelText>,
    upgrades: Query<&RoomUpgrades>,
) {
    let up = upgrades.single().map(|u| u.0).unwrap_or_default();
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
                b.dealer.as_ref().map_or("nobody (stand behind the table, press E)".into(), |d| {
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
                s += "\nE take/leave the deal   LMB deal / the house's move   RMB stand";
                if let Some(a) = b.dealer_should {
                    s += &format!("\nThe house must {}", a.to_uppercase());
                }
            } else {
                let amounts = buttons_for(&BLACKJACK_BUTTONS, up.table_max(casino::BLACKJACK_MAX), tier, t);
                let keys: Vec<String> = amounts.iter().enumerate().map(|(i, a)| format!("{} ${a}", i + 1)).collect();
                s += &format!(
                    "\nBet: {}   0 stand up\nLMB hit   RMB stand   X double   C split   Y/N insurance",
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
                r.croupier.as_ref().map_or("nobody (stand behind the table, press E)".into(), |d| {
                    if Some(d) == me.as_ref() { "YOU".into() } else { d.clone() }
                })
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
                s += "\nE take/leave the wheel   LMB spin   RMB rake";
            } else {
                let choices = roulette_choices();
                let bet = choices[choice.0 % choices.len()];
                let amounts = buttons_for(&ROULETTE_BUTTONS, up.table_max(casino::ROULETTE_MAX), tier, t);
                let keys: Vec<String> = amounts.iter().enumerate().map(|(i, a)| format!("{} ${a}", i + 1)).collect();
                s += &format!("\nWheel or Z/X: {} (pays {} to 1)\n{}", bet.label(), bet.odds(), keys.join("  "));
            }
            s
        }
        Some(name) if name.starts_with("slot") => {
            let i: usize = name[4..].parse().unwrap_or(0);
            let Some(m) = c.slots.iter().find(|m| usize::from(m.machine) == i) else { return };
            let amounts = buttons_for(&SLOT_BUTTONS, up.table_max(casino::SLOT_MAX), tier, t);
            let keys: Vec<String> = amounts.iter().enumerate().map(|(i, a)| format!("{} ${a}", i + 1)).collect();
            format!(
                "SLOTS  (pull {})\n{}\nLast: ${} back on ${}\n\nPull: {}   LMB pull again\n7 7 7 pays 150, BAR x3 50, bells 10, lemons 8,\ncherries 5, two cherries 2, one cherry 1",
                m.pulls,
                if m.spinning { "spinning...".into() } else { m.line.join(" | ") },
                m.last_return,
                m.last_bet,
                keys.join("  ")
            )
        }
        _ => String::new(),
    };
    if shown.0 != line {
        shown.0.clone_from(&line);
    }
    let Ok((mut text, mut vis)) = panel.single_mut() else { return };
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
    back_mesh: Handle<Mesh>,
    back: Handle<StandardMaterial>,
    face: Handle<StandardMaterial>,
    /// Face meshes by card, made on first deal.
    faces: std::collections::HashMap<u8, Handle<Mesh>>,
}

#[derive(Resource)]
struct SymbolLooks([Handle<Mesh>; 5]);

pub(crate) fn setup_tables(
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
    // Vertex-painted pieces (art.rs) share one material.
    let painted = mat(Color::WHITE, 0.0);
    let h = casino::FELT_HEIGHT;
    // The felt printing: seat rings and the chip rack at blackjack, the
    // number layout at roulette.
    let (bx, bz) = casino::BLACKJACK;
    let seats: Vec<(f32, f32)> =
        casino::bettor_spots(TableId::Blackjack).iter().map(|(x, z)| (x - bx, z - bz)).collect();
    commands.spawn((
        Mesh3d(meshes.add(art::blackjack_layout(&seats, casino::BLACKJACK_HALF.1))),
        MeshMaterial3d(painted.clone()),
        Transform::from_xyz(bx, h + 0.002, bz),
    ));
    let (rx, rz) = casino::ROULETTE;
    commands.spawn((
        Mesh3d(meshes.add(art::roulette_layout(-casino::ROULETTE_HALF.0 + 1.0, roulette::is_red))),
        MeshMaterial3d(painted.clone()),
        Transform::from_xyz(rx, h + 0.002, rz + 0.05),
    ));
    // The wheel turns in a bowl at the roulette table's west end; the ball
    // rides on top.
    let wheel_at = Vec3::new(rx - casino::ROULETTE_HALF.0 + 0.55, h + 0.03, rz);
    commands.spawn((
        Mesh3d(meshes.add(art::wheel_bowl())),
        MeshMaterial3d(painted.clone()),
        Transform::from_translation(wheel_at - Vec3::Y * 0.02),
    ));
    commands.spawn((
        Wheel,
        Mesh3d(meshes.add(art::wheel(&roulette::WHEEL_ORDER, roulette::is_red))),
        MeshMaterial3d(painted.clone()),
        Transform::from_translation(wheel_at + Vec3::Y * 0.02),
    ));
    commands.spawn((
        Ball,
        Mesh3d(meshes.add(Sphere::new(0.022))),
        MeshMaterial3d(mat(Color::WHITE, 0.5)),
        Transform::from_translation(wheel_at + Vec3::Y * 0.08),
    ));
    // Slot reels: three white drums in each machine's window, each showing
    // its symbol; a lit topper over each machine.
    let symbols = art::reel_symbols().map(|m| meshes.add(m));
    let drum = meshes.add(Cuboid::new(0.02, 0.2, 0.16));
    let drum_mat = mat(Color::srgb(0.97, 0.96, 0.92), 0.15);
    let symbol_mat = painted.clone();
    let topper = meshes.add(Cuboid::new(0.04, 0.12, casino::SLOT_HALF.1 * 2.0 - 0.06));
    let topper_mat = mat(Color::srgb(1.0, 0.8, 0.35), 1.2);
    for (m, (sx, sz)) in casino::SLOTS.iter().enumerate() {
        for r in 0..3 {
            commands
                .spawn((
                    Mesh3d(drum.clone()),
                    MeshMaterial3d(drum_mat.clone()),
                    Transform::from_xyz(sx + casino::SLOT_HALF.0 + 0.011, 1.2, sz - 0.18 + r as f32 * 0.18),
                ))
                .with_child((
                    Reel(m as u8, r),
                    Mesh3d(symbols[0].clone()),
                    MeshMaterial3d(symbol_mat.clone()),
                    Transform::from_xyz(0.012, 0.0, 0.0),
                ));
        }
        // The topper: a lit panel with three sevens.
        commands
            .spawn((
                Mesh3d(topper.clone()),
                MeshMaterial3d(topper_mat.clone()),
                Transform::from_xyz(sx + casino::SLOT_HALF.0 - 0.01, casino::SLOT_HEIGHT - 0.1, *sz),
            ))
            .with_children(|t| {
                for i in -1..=1 {
                    t.spawn((
                        Mesh3d(symbols[4].clone()),
                        MeshMaterial3d(symbol_mat.clone()),
                        Transform::from_xyz(0.022, 0.0, i as f32 * 0.17).with_scale(Vec3::splat(0.85)),
                    ));
                }
            });
    }
    commands.insert_resource(SymbolLooks(symbols));
    commands.insert_resource(CardLooks {
        back_mesh: meshes.add(Cuboid::new(0.09, 0.004, 0.13)),
        back: mat(Color::srgb(0.6, 0.05, 0.1), 0.0),
        face: painted,
        faces: default(),
    });
}

/// Lay the cards out on the felt whenever the table changes: the dealer's
/// along the dealer side, each seat's in front of the seat.
fn draw_cards(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    looks: Option<ResMut<CardLooks>>,
    views: Query<&BlackjackView, Changed<BlackjackView>>,
    tiles: Query<Entity, With<CardTile>>,
) {
    let (Some(mut looks), Ok(v)) = (looks, views.single()) else { return };
    for e in &tiles {
        commands.entity(e).despawn();
    }
    let (cx, cz) = casino::BLACKJACK;
    let y = casino::FELT_HEIGHT + 0.012;
    let mut tile = |x: f32, z: f32, card: u8| {
        let (mesh, mat) = if card == HIDDEN_CARD {
            (looks.back_mesh.clone(), looks.back.clone())
        } else {
            let face = looks
                .faces
                .entry(card)
                .or_insert_with(|| {
                    let suit = shared::cards::suit(card);
                    meshes.add(art::card_face(shared::cards::rank(card), suit == 1 || suit == 2))
                })
                .clone();
            (face, looks.face.clone())
        };
        commands.spawn((CardTile, Mesh3d(mesh), MeshMaterial3d(mat), Transform::from_xyz(x, y, z)));
    };
    for (i, c) in v.dealer_cards.iter().enumerate() {
        tile(cx - 0.15 + i as f32 * 0.1, cz - 0.3, *c);
    }
    let spots = casino::bettor_spots(TableId::Blackjack);
    for (s, seat) in v.seats.iter().enumerate() {
        for (h, hand) in seat.hands.iter().enumerate() {
            for (i, c) in hand.cards.iter().enumerate() {
                tile(spots[s].0 - 0.08 + i as f32 * 0.04 + h as f32 * 0.2, cz + 0.25 - i as f32 * 0.03, *c);
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
    mut reels: Query<(&Reel, &mut Mesh3d)>,
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
