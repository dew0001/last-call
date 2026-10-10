//! Phase 4 on the client: fixtures (Zeen drawer, kitchen pass, upgrade
//! terminal, charm shelf, jukebox, breaker, service keys), the Focus meter
//! and The Spins, and chaos events (banner, NPCs, fire, darkness).
//!
//! Gray box: fixtures are colored posts, chaos NPCs are tinted capsules, and
//! a text panel lists each fixture's menu with number keys. The art pass
//! (Phase 6) replaces the panels with diegetic UI.

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::buffs::{self, FocusTier};
use shared::chaos::{ChaosKind, Ending};
use shared::client::{OutgoingFixture, OutgoingTable, Session};
use shared::fixtures::{Fixture, FixtureAction};
use shared::protocol::*;

use crate::online::{NetStatus, NoDraw, RoomLamp};

pub fn add(app: &mut App) {
    app.init_resource::<HiLo>();
    app.add_systems(Startup, (setup_fixtures, setup_panels));
    app.add_systems(
        Update,
        (
            fixture_keys,
            dress_chaos_npcs,
            tint_chaos_customers,
            place_chaos_npcs,
            dress_fires,
            lights.run_if(resource_changed::<NetStatus>),
            count_cards,
            update_panels.run_if(resource_changed::<NetStatus>),
        )
            .chain()
            .after(crate::online::OnlineSet),
    );
    app.add_systems(Update, spins_camera.after(crate::online::CameraSet));
}

// ---------- Status for the page and tests ----------

/// Focus, items, upgrades and chaos as this client sees them.
#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase4Status {
    pub focus: u8,
    /// "none", "focus" or "buzzed".
    pub focus_tier: &'static str,
    pub spinning: bool,
    pub rigged_dice: u8,
    pub marked_deck: bool,
    pub well_fed: bool,
    /// The dealer's hole card (marked deck), as text.
    pub hole_card: Option<String>,
    /// The Hi-Lo running count, shown with Focus.
    pub count: Option<i32>,
    /// Upgrade ranks, in [`shared::upgrades::ALL`] order.
    pub upgrades: Vec<u8>,
    pub track: Option<u8>,
    /// The fixture this player stands at, and its menu.
    pub near_fixture: Option<String>,
    pub menu: Vec<String>,
    /// Running events: (kind, seconds left, target, cops in).
    pub chaos: Vec<(String, Option<u16>, Option<u8>, bool)>,
    /// How the last events ended: (kind, "countered" / "consequence" / "expired").
    pub recent: Vec<(String, &'static str)>,
    pub dark: bool,
    pub kitchen_offline: bool,
    pub blackjack_broken: bool,
    /// Chaos NPCs: (kind, x, z, outlined).
    pub npcs: Vec<(String, f32, f32, bool)>,
    pub fires: usize,
    pub vomit: usize,
}

fn kind_name(k: ChaosKind) -> String {
    format!("{k:?}")
}

fn ending_name(e: Ending) -> &'static str {
    match e {
        Ending::Countered => "countered",
        Ending::Consequence => "consequence",
        Ending::Expired => "expired",
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct Phase4Queries<'w, 's> {
    room: Query<
        'w,
        's,
        (Option<&'static RoomUpgrades>, Option<&'static JukeboxState>, Option<&'static ChaosState>),
        With<RoomState>,
    >,
    me: Query<'w, 's, (&'static Player, Option<&'static Focus>, Option<&'static Inventory>)>,
    npcs: Query<'w, 's, (&'static ChaosNpc, &'static NpcPose)>,
    fires: Query<'w, 's, (), With<Fire>>,
    vomit: Query<'w, 's, (), With<Vomit>>,
    hilo: Res<'w, HiLo>,
}

impl Phase4Queries<'_, '_> {
    pub fn status(&self, me: Option<u64>, own: Option<Vec3>) -> Phase4Status {
        let (upgrades, jukebox, chaos) = self.room.iter().next().unwrap_or((None, None, None));
        let up = upgrades.map(|u| u.0).unwrap_or_default();
        let mine = self.me.iter().find(|(p, ..)| Some(p.id) == me);
        let focus = mine.and_then(|(_, f, _)| f.copied()).unwrap_or_default();
        let inv = mine.and_then(|(_, _, i)| i.copied()).unwrap_or_default();
        let near = own.and_then(|p| Fixture::nearest(p.x, p.z));
        let chaos = chaos.cloned().unwrap_or_default();
        let mut npcs: Vec<(String, f32, f32, bool)> =
            self.npcs.iter().map(|(c, p)| (kind_name(c.kind), p.pos.x, p.pos.z, c.outlined)).collect();
        npcs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        Phase4Status {
            focus: focus.level,
            focus_tier: FocusTier::of(focus.level).label(),
            spinning: focus.spinning,
            rigged_dice: inv.rigged_dice,
            marked_deck: inv.marked_deck,
            well_fed: inv.well_fed,
            hole_card: inv.hole_card.map(shared::cards::name),
            count: buffs::shows_count(focus.level).then_some(self.hilo.count),
            upgrades: shared::upgrades::ALL.iter().map(|u| up.rank(*u)).collect(),
            track: jukebox.and_then(|j| j.track),
            near_fixture: near.map(|f| f.label().to_string()),
            menu: near.map(|f| f.menu().iter().map(|a| a.label(&up)).collect()).unwrap_or_default(),
            chaos: chaos.active.iter().map(|a| (kind_name(a.kind), a.seconds_left, a.target, a.cops_in)).collect(),
            recent: chaos.recent.iter().map(|(k, e)| (kind_name(*k), ending_name(*e))).collect(),
            dark: chaos.dark,
            kitchen_offline: chaos.kitchen_offline,
            blackjack_broken: chaos.blackjack_broken,
            npcs,
            fires: self.fires.iter().count(),
            vomit: self.vomit.iter().count(),
        }
    }
}

// ---------- Fixtures ----------

/// Gray-box posts where the fixtures stand.
fn setup_fixtures(
    mut commands: Commands,
    nodraw: Option<Res<NoDraw>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if nodraw.is_some() {
        return;
    }
    let post = meshes.add(Cuboid::new(0.5, 1.2, 0.5));
    for f in Fixture::all() {
        let color = match f {
            Fixture::ZeenDrawer => Color::srgb(0.2, 0.8, 0.9),
            Fixture::KitchenPass => Color::srgb(0.9, 0.6, 0.2),
            Fixture::Shop => Color::srgb(0.3, 0.9, 0.4),
            Fixture::CharmShelf => Color::srgb(0.9, 0.85, 0.2),
            Fixture::Jukebox => Color::srgb(0.8, 0.3, 0.9),
            Fixture::Breaker => Color::srgb(0.9, 0.2, 0.2),
            Fixture::ServiceKey(_) => Color::srgb(0.7, 0.7, 0.75),
        };
        let mat = materials.add(StandardMaterial { base_color: color, emissive: color.to_linear() * 0.6, ..default() });
        let (x, z) = f.position();
        let h = if matches!(f, Fixture::ServiceKey(_)) { 0.3 } else { 1.0 };
        commands.spawn((
            Mesh3d(post.clone()),
            MeshMaterial3d(mat),
            Transform::from_xyz(x, h / 2.0, z).with_scale(Vec3::new(1.0, h / 1.2, 1.0)),
        ));
    }
}

/// The digit keys of a fixture's menu.
const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
];

/// Number keys at a fixture pick from its menu (U at a breaker or service
/// key, whose number keys belong to the slot machine next to it). V at
/// roulette uses a rigged die on the chosen dozen (the first if none).
fn fixture_keys(
    keys: Res<ButtonInput<KeyCode>>,
    status: Res<NetStatus>,
    choice: Res<crate::casino::RouletteChoice>,
    mut out: ResMut<OutgoingFixture>,
    mut tables: ResMut<OutgoingTable>,
) {
    let Some(pos) = status.own_pos else { return };
    let near_table = shared::casino::nearest_table(pos.x, pos.z);
    if keys.just_pressed(KeyCode::KeyV) && near_table == Some(shared::casino::TableId::Roulette) {
        let choices = crate::casino::roulette_choices();
        let dozen = match choices.get(choice.0 % choices.len()) {
            Some(shared::roulette::Bet::Dozen(d)) => *d,
            _ => 0,
        };
        tables.0.push(TableRequest {
            table: shared::casino::TableId::Roulette,
            action: shared::casino::TableAction::RiggedDie(dozen),
        });
    }
    let Some(fixture) = Fixture::nearest(pos.x, pos.z) else { return };
    let menu = fixture.menu();
    if menu == [FixtureAction::Use] {
        if keys.just_pressed(KeyCode::KeyU) {
            out.0.push(FixtureRequest { fixture, action: FixtureAction::Use });
        }
        return;
    }
    if near_table.is_some() {
        return;
    }
    for (key, action) in DIGITS.iter().zip(menu) {
        if keys.just_pressed(*key) {
            out.0.push(FixtureRequest { fixture, action });
        }
    }
}

// ---------- Panels ----------

#[derive(Component)]
struct FixturePanel;

#[derive(Component)]
struct ChaosBanner;

#[derive(Component)]
struct FocusText;

fn setup_panels(mut commands: Commands, nodraw: Option<Res<NoDraw>>) {
    if nodraw.is_some() {
        return;
    }
    let font = |size: f32| TextFont { font_size: bevy::text::FontSize::Px(size), ..default() };
    commands.spawn((
        FixturePanel,
        Text::new(""),
        font(15.0),
        TextColor(Color::srgb(0.85, 1.0, 0.9)),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            top: px(40),
            right: px(10),
            padding: UiRect::all(px(6)),
            ..default()
        },
        Visibility::Hidden,
    ));
    commands.spawn((
        ChaosBanner,
        Text::new(""),
        font(20.0),
        TextColor(Color::srgb(1.0, 0.35, 0.3)),
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, top: px(10), left: percent(30), width: percent(40), ..default() },
    ));
    commands.spawn((
        FocusText,
        Text::new(""),
        font(16.0),
        TextColor(Color::srgb(0.5, 0.9, 1.0)),
        Node { position_type: PositionType::Absolute, bottom: px(74), left: px(10), ..default() },
    ));
}

#[allow(clippy::type_complexity)]
fn update_panels(
    time: Res<Time>,
    status: Res<NetStatus>,
    mut panel: Query<(&mut Text, &mut Visibility), (With<FixturePanel>, Without<ChaosBanner>, Without<FocusText>)>,
    mut banner: Query<(&mut Text, &mut Node), (With<ChaosBanner>, Without<FixturePanel>, Without<FocusText>)>,
    mut focus: Query<&mut Text, (With<FocusText>, Without<FixturePanel>, Without<ChaosBanner>)>,
) {
    let p = &status.game.phase4;
    if let Ok((mut text, mut vis)) = panel.single_mut() {
        match &p.near_fixture {
            Some(name) => {
                let one = p.menu.len() == 1 && p.menu[0] == "Use";
                let mut s = format!("{}\n", name.to_uppercase());
                if one {
                    s += "U  use";
                } else {
                    for (i, line) in p.menu.iter().enumerate() {
                        s += &format!("{}  {line}\n", (i + 1) % 10);
                    }
                }
                if name == "Kitchen pass" && p.kitchen_offline {
                    s += "\nKITCHEN CLOSED";
                }
                if name == "Upgrade terminal" {
                    s += "\nUpgrades are bought from the house pool, during Setup.";
                }
                if text.0 != s {
                    text.0 = s;
                }
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
    if let Ok((mut text, mut node)) = banner.single_mut() {
        let mut lines: Vec<String> = Vec::new();
        for (kind, secs, _, cops) in &p.chaos {
            let k = chaos_kind(kind);
            let left = secs.map_or(String::new(), |s| format!(" ({s} s)"));
            let cops = if *cops { "  COPS IN" } else { "" };
            lines.push(format!(
                "{}{left}{cops}\n{}",
                k.map_or(kind.as_str(), |k| k.label()).to_uppercase(),
                k.map_or("", |k| k.hint())
            ));
        }
        if p.blackjack_broken {
            lines.push("The blackjack table is broken for the shift".into());
        }
        let s = lines.join("\n");
        if text.0 != s {
            text.0 = s;
        }
        // Buzzed: the UI shakes.
        let jitter = buffs::ui_jitter(p.focus);
        let t = time.elapsed_secs();
        node.margin = UiRect::left(px((t * 37.0).sin() * jitter));
    }
    if let Ok(mut text) = focus.single_mut() {
        let mut s = if p.spinning {
            "THE SPINS".to_string()
        } else if p.focus > 0 {
            format!("Focus {}  ({})", p.focus, p.focus_tier)
        } else {
            String::new()
        };
        let mut items = Vec::new();
        if p.rigged_dice > 0 {
            items.push(format!("{} rigged dice (V at roulette)", p.rigged_dice));
        }
        if p.marked_deck {
            items.push("marked deck".to_string());
        }
        if p.well_fed {
            items.push("well fed".to_string());
        }
        if !items.is_empty() {
            s += &format!("   Items: {}", items.join(", "));
        }
        if text.0 != s {
            text.0 = s;
        }
    }
}

fn chaos_kind(name: &str) -> Option<ChaosKind> {
    shared::chaos::ALL.into_iter().find(|k| kind_name(*k) == name)
}

// ---------- Hi-Lo count ----------

/// The running Hi-Lo count of the cards this client has seen since the last
/// shuffle (Focus shows it).
#[derive(Resource, Default, Debug)]
pub struct HiLo {
    pub count: i32,
    round: u32,
    shoe_left: u16,
    /// Cards of the current round already counted.
    counted: Vec<u8>,
}

fn count_cards(mut hilo: ResMut<HiLo>, views: Query<&BlackjackView>) {
    let Some(v) = views.iter().next() else { return };
    if v.shoe_left > hilo.shoe_left {
        // A fresh shoe.
        hilo.count = 0;
        hilo.counted.clear();
    }
    hilo.shoe_left = v.shoe_left;
    if v.rounds != hilo.round {
        hilo.round = v.rounds;
        hilo.counted.clear();
    }
    let mut seen: Vec<u8> = v
        .seats
        .iter()
        .flat_map(|s| s.hands.iter().flat_map(|h| h.cards.iter().copied()))
        .chain(v.dealer_cards.iter().copied())
        .filter(|c| *c != HIDDEN_CARD)
        .collect();
    // Count each card not counted yet (multiset difference).
    let mut left = hilo.counted.clone();
    seen.retain(|c| match left.iter().position(|x| x == c) {
        Some(i) => {
            left.swap_remove(i);
            false
        }
        None => true,
    });
    for c in seen {
        hilo.count += shared::cards::hi_lo(c);
        hilo.counted.push(c);
    }
}

// ---------- Chaos visuals ----------

#[derive(Component)]
struct DressedChaos;

fn chaos_color(kind: ChaosKind) -> Color {
    match kind {
        ChaosKind::Raid => Color::srgb(0.1, 0.2, 0.8),
        ChaosKind::Brawl => Color::srgb(0.85, 0.15, 0.1),
        ChaosKind::Inspector => Color::srgb(0.9, 0.9, 0.85),
        ChaosKind::LoanShark => Color::srgb(0.1, 0.5, 0.25),
        ChaosKind::CardCounter => Color::srgb(0.45, 0.4, 0.5),
        _ => Color::srgb(0.5, 0.5, 0.5),
    }
}

/// Cops and the inspector (not customers): capsules in their colors.
fn dress_chaos_npcs(
    mut commands: Commands,
    npcs: Query<(Entity, &ChaosNpc), (Without<Customer>, Without<DressedChaos>, With<Interpolated>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, c) in &npcs {
        commands.entity(e).insert((
            DressedChaos,
            Mesh3d(meshes.add(Capsule3d::new(0.32, shared::bar::PLAYER_HEIGHT - 0.6))),
            MeshMaterial3d(materials.add(StandardMaterial { base_color: chaos_color(c.kind), ..default() })),
            Transform::default(),
        ));
    }
}

/// Brawlers, the loan shark and the card counter are customers: recolor
/// them, and light up the counter when the Security Camera outlines him.
fn tint_chaos_customers(
    mut commands: Commands,
    npcs: Query<(Entity, &ChaosNpc, &MeshMaterial3d<StandardMaterial>), (With<Customer>, Without<DressedChaos>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, c, mat) in &npcs {
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.base_color = chaos_color(c.kind);
            if c.outlined {
                m.emissive = LinearRgba::rgb(2.0, 0.1, 0.1);
            }
        }
        commands.entity(e).insert(DressedChaos);
    }
}

fn place_chaos_npcs(mut npcs: Query<(&NpcPose, &mut Transform), (With<DressedChaos>, Without<Customer>)>) {
    for (pose, mut t) in &mut npcs {
        t.translation = pose.pos + Vec3::Y * (shared::bar::PLAYER_HEIGHT / 2.0);
        t.rotation = Quat::from_rotation_y(pose.yaw);
    }
}

#[derive(Component)]
struct DressedFire;

fn dress_fires(
    mut commands: Commands,
    fires: Query<(Entity, &Fire), Without<DressedFire>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, f) in &fires {
        commands.entity(e).insert((
            DressedFire,
            Mesh3d(meshes.add(Cone { radius: 0.6, height: 1.4 })),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.4, 0.05),
                emissive: LinearRgba::rgb(6.0, 1.5, 0.1),
                ..default()
            })),
            Transform::from_translation(f.pos + Vec3::Y * 0.7),
        ));
    }
}

/// The outage turns the indoor lamps off.
fn lights(status: Res<NetStatus>, mut lamps: Query<&mut Visibility, With<RoomLamp>>) {
    let want = if status.game.phase4.dark { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut lamps {
        v.set_if_neq(want);
    }
}

/// The Spins: the view rolls.
fn spins_camera(
    time: Res<Time>,
    status: Res<NetStatus>,
    session: Res<Session>,
    mut cam: Query<&mut Transform, With<Camera3d>>,
) {
    if session.player_id.is_none() || !status.game.phase4.spinning {
        return;
    }
    let Ok(mut t) = cam.single_mut() else { return };
    let s = time.elapsed_secs();
    t.rotate_local_z(s * 2.5);
}
