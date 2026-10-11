//! Online play: netcode, the gray-box bar, players, input and camera.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use lightyear::prelude::client::*;
use lightyear::prelude::*;
use shared::bar;
use shared::client::{ClientNetPlugin, Identity, LocalInput, Session};
use shared::movement::buttons;
use shared::pipe::{PipeEnd, PipeIo};
use shared::protocol::*;

/// How to join.
#[derive(Clone, Debug)]
pub struct OnlineConfig {
    pub identity: Identity,
    /// Run the full client without a camera (nothing is drawn). Browser bot
    /// tabs use it: the netcode and game logic still run every frame.
    pub nodraw: bool,
}

/// Present when the client draws nothing.
#[derive(Resource)]
pub struct NoDraw;

/// The outside end of the client's pipe. The web bridge feeds received bytes
/// into it and drains bytes to send.
#[derive(Resource, Clone)]
pub struct NetBridge(pub PipeEnd);

/// Look direction kept on the client; sent as part of the input.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Look {
    pub yaw: f32,
    pub pitch: f32,
}

/// Input written by tests or automation (web: `window.__lcInput`). When set,
/// it replaces keyboard and mouse.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct ScriptedInput(pub Option<PlayerInput>);

/// Player settings (the page's settings panel, `web/settings.js`).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub sensitivity: f32,
    /// Low: no shadows, no bloom.
    pub low_graphics: bool,
    /// The mouse turns the view (in the browser: only while it is locked to
    /// the game, so a free cursor can use the menus).
    pub mouse_look: bool,
    /// Pull the mouse toward you to look up.
    pub invert_y: bool,
    pub keys: Keybinds,
}

impl Default for Settings {
    fn default() -> Self {
        Self { sensitivity: 1.0, low_graphics: false, mouse_look: true, invert_y: false, keys: Keybinds::default() }
    }
}

/// The rebindable keys (the settings' Controls tab). Table and side-game
/// keys are fixed and shown on their panels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keybinds {
    pub forward: KeyCode,
    pub back: KeyCode,
    pub left: KeyCode,
    pub right: KeyCode,
    pub sprint: KeyCode,
    pub jump: KeyCode,
    pub crouch: KeyCode,
    pub interact: KeyCode,
    pub throw: KeyCode,
    pub drop: KeyCode,
    pub use_item: KeyCode,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            forward: KeyCode::KeyW,
            back: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
            sprint: KeyCode::ShiftLeft,
            jump: KeyCode::Space,
            crouch: KeyCode::ControlLeft,
            interact: KeyCode::KeyE,
            throw: KeyCode::KeyF,
            drop: KeyCode::KeyQ,
            use_item: KeyCode::KeyR,
        }
    }
}

impl Keybinds {
    /// Set one action from its settings name and a browser key code
    /// (`KeyboardEvent.code`, such as `KeyE` or `ShiftLeft`).
    pub fn set(&mut self, action: &str, code: &str) {
        let Some(key) = key_code(code) else { return };
        let slot = match action {
            "forward" => &mut self.forward,
            "back" => &mut self.back,
            "left" => &mut self.left,
            "right" => &mut self.right,
            "sprint" => &mut self.sprint,
            "jump" => &mut self.jump,
            "crouch" => &mut self.crouch,
            "interact" => &mut self.interact,
            "throw" => &mut self.throw,
            "drop" => &mut self.drop,
            "use" => &mut self.use_item,
            _ => return,
        };
        *slot = key;
    }
}

/// A browser `KeyboardEvent.code` as a Bevy key code (the keys a player
/// would bind).
pub fn key_code(code: &str) -> Option<KeyCode> {
    use KeyCode as K;
    const LETTERS: [KeyCode; 26] = [
        K::KeyA,
        K::KeyB,
        K::KeyC,
        K::KeyD,
        K::KeyE,
        K::KeyF,
        K::KeyG,
        K::KeyH,
        K::KeyI,
        K::KeyJ,
        K::KeyK,
        K::KeyL,
        K::KeyM,
        K::KeyN,
        K::KeyO,
        K::KeyP,
        K::KeyQ,
        K::KeyR,
        K::KeyS,
        K::KeyT,
        K::KeyU,
        K::KeyV,
        K::KeyW,
        K::KeyX,
        K::KeyY,
        K::KeyZ,
    ];
    const DIGITS: [KeyCode; 10] =
        [K::Digit0, K::Digit1, K::Digit2, K::Digit3, K::Digit4, K::Digit5, K::Digit6, K::Digit7, K::Digit8, K::Digit9];
    if let Some(c) = code.strip_prefix("Key").filter(|c| c.len() == 1).and_then(|c| c.chars().next()) {
        return c.is_ascii_uppercase().then(|| LETTERS[(c as u8 - b'A') as usize]);
    }
    if let Some(d) = code.strip_prefix("Digit").and_then(|d| d.parse::<usize>().ok()) {
        return DIGITS.get(d).copied();
    }
    Some(match code {
        "Space" => K::Space,
        "ShiftLeft" => K::ShiftLeft,
        "ShiftRight" => K::ShiftRight,
        "ControlLeft" => K::ControlLeft,
        "ControlRight" => K::ControlRight,
        "AltLeft" => K::AltLeft,
        "AltRight" => K::AltRight,
        "Tab" => K::Tab,
        "CapsLock" => K::CapsLock,
        "Enter" => K::Enter,
        "Backquote" => K::Backquote,
        "ArrowUp" => K::ArrowUp,
        "ArrowDown" => K::ArrowDown,
        "ArrowLeft" => K::ArrowLeft,
        "ArrowRight" => K::ArrowRight,
        "Comma" => K::Comma,
        "Period" => K::Period,
        "Slash" => K::Slash,
        "Semicolon" => K::Semicolon,
        "Quote" => K::Quote,
        "BracketLeft" => K::BracketLeft,
        "BracketRight" => K::BracketRight,
        _ => return None,
    })
}

/// What the page shows and tests read.
#[derive(Resource, Default, Debug, Clone)]
pub struct NetStatus {
    pub connected: bool,
    pub player_id: Option<u64>,
    pub players_seen: usize,
    pub own_pos: Option<Vec3>,
    pub refused: Option<String>,
    pub props_seen: usize,
    pub holding: bool,
    /// Bottles and chips no longer on the counter top (thrown or knocked off).
    pub props_on_floor: usize,
    /// Every player this client draws: (id, position).
    pub players: Vec<(u64, Vec3)>,
    /// Link round-trip time and jitter, milliseconds.
    pub rtt_ms: f32,
    pub jitter_ms: f32,
    /// This client's simulation tick.
    pub tick: u32,
    /// Game state for the page and tests (`window.__lastCall.game`).
    pub game: GameStatus,
}

/// Game state published to the page, in JSON-friendly form.
#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    pub shift: Option<ShiftStatus>,
    pub money: Option<MoneyStatus>,
    /// This player's pocket.
    pub pocket: Option<i64>,
    /// Customers this client sees: (id, mood, x, z).
    pub customers: Vec<(u32, &'static str, f32, f32)>,
    /// This player's pour at the tap, percent: (fill, foam).
    pub pour: Option<(u8, u8)>,
    /// The beer in this player's hand: (fill percent, perfect).
    pub beer: Option<(u8, bool)>,
    /// Puddles on the floor.
    pub puddles: usize,
    /// This player's drunk meter.
    pub drunk: Option<DrunkStatus>,
    /// Voice pitch per other player (hex id, factor): drunk speakers sound lower.
    pub voice_pitch: Vec<(String, f32)>,
    /// The tables.
    pub casino: crate::casino::CasinoStatus,
    /// Focus, items, upgrades and chaos.
    pub phase4: crate::chaos::Phase4Status,
    /// The side games.
    pub games: crate::games::GamesStatus,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrunkStatus {
    pub level: u8,
    pub tier: &'static str,
    pub passed_out: bool,
    /// Screen blur 0 to 1. The page applies it as a CSS blur on the canvas:
    /// Bevy turns off depth of field on WebGL2.
    pub blur: f32,
}

fn mood_name(m: shared::customers::Mood) -> &'static str {
    use shared::customers::Mood;
    match m {
        Mood::Entering => "entering",
        Mood::Waiting => "waiting",
        Mood::Drinking => "drinking",
        Mood::Leaving => "leaving",
        Mood::Gambling => "gambling",
        Mood::Trouble => "trouble",
    }
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoneyStatus {
    pub house: i64,
    pub paid: i64,
    pub debt: i64,
    /// Payment due at the end of this week.
    pub due: i64,
    pub tier: u8,
    pub missed_in_a_row: u8,
    pub ng: u8,
    /// The last collection: "paid" or "missed", and the amount.
    pub last: Option<(&'static str, i64)>,
    /// "playing", "won" or "lost".
    pub outcome: &'static str,
}

impl From<RunLedger> for MoneyStatus {
    fn from(r: RunLedger) -> Self {
        use shared::economy::{Collection, Outcome};
        Self {
            house: r.ledger.house,
            paid: r.ledger.paid,
            debt: r.ledger.debt(),
            due: r.due,
            tier: r.ledger.tier(),
            missed_in_a_row: r.ledger.missed_in_a_row,
            ng: r.ledger.ng,
            last: r.last.map(|c| match c {
                Collection::Paid { amount } => ("paid", amount),
                Collection::Missed { owed } => ("missed", owed),
            }),
            outcome: match r.outcome {
                Outcome::Playing => "playing",
                Outcome::Won => "won",
                Outcome::Lost => "lost",
            },
        }
    }
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShiftStatus {
    pub week: u8,
    /// 1-based shift within the week.
    pub shift: u8,
    pub phase: &'static str,
    pub seconds_left: u16,
    pub running: bool,
    /// Setup waits for the host to start the shift.
    pub waiting: bool,
}

impl From<ShiftClock> for ShiftStatus {
    fn from(c: ShiftClock) -> Self {
        Self {
            week: c.calendar.week,
            shift: c.calendar.shift + 1,
            phase: c.phase.label(),
            seconds_left: c.seconds_left,
            running: c.running,
            waiting: c.waiting,
        }
    }
}

/// Add online play to the app.
pub fn add(app: &mut App, cfg: OnlineConfig) {
    app.add_plugins(ClientPlugins { tick_duration: shared::TICK });
    app.add_plugins(ProtocolPlugin);
    app.add_plugins(ClientNetPlugin);
    app.insert_resource(PredictionManager::default());
    app.insert_resource(cfg.identity);
    if cfg.nodraw {
        app.insert_resource(NoDraw);
    }
    app.init_resource::<Look>()
        .init_resource::<ScriptedInput>()
        .init_resource::<NetStatus>()
        .init_resource::<Settings>();

    let (io, end) = PipeIo::new();
    app.insert_resource(NetBridge(end));
    let addr = |port| core::net::SocketAddr::new(core::net::IpAddr::V4(core::net::Ipv4Addr::LOCALHOST), port);
    app.world_mut().spawn((
        Name::new("Client"),
        Client,
        RawClient,
        io,
        LocalAddr(addr(1)),
        PeerAddr(addr(2)),
        ReplicationReceiver,
        ReplicationSender,
    ));
    app.init_resource::<LocalPour>();
    app.add_systems(FixedUpdate, predict_pour);
    crate::casino::add(app);
    crate::chaos::add(app);
    crate::games::add(app);
    app.add_systems(Startup, (setup_bar, setup_hud, crate::outside::setup_outside));
    app.add_systems(
        Update,
        (
            read_input,
            dress_players,
            place_players,
            dress_props,
            place_props,
            dress_customers,
            place_customers,
            animate_gait,
            crate::look::cull_far,
            dress_puddles,
            follow_camera.in_set(CameraSet),
            post_effects,
            graphics_preset,
            update_status.run_if(every_third_frame),
            update_hud.run_if(resource_changed::<NetStatus>),
            update_pour_gauge.run_if(resource_changed::<NetStatus>),
            update_drunk_text.run_if(resource_changed::<NetStatus>),
        )
            .chain()
            .in_set(OnlineSet),
    );
}

/// Status and screen text refresh at 20 Hz (every third frame at 60 fps):
/// building them every frame cost more than the rest of the frame.
pub fn every_third_frame(mut frame: Local<u32>) -> bool {
    *frame = frame.wrapping_add(1);
    frame.is_multiple_of(3)
}

/// The online drawing and status systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct OnlineSet;

/// The camera follows the player.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraSet;

/// The shift clock line at the top of the screen. A wall clock replaces it
/// in the art pass (plan section 7: diegetic UI).
#[derive(Component)]
struct ClockText;

/// House pool, debt and pocket. The office LED sign replaces it in the art pass.
#[derive(Component)]
struct MoneyText;

/// The drunk meter line. Two beer-glass icons replace it in the art pass.
#[derive(Component)]
struct DrunkText;

/// The pour gauge: a fill bar with the green zone marked, and the foam.
#[derive(Component)]
struct PourPanel;

#[derive(Component)]
struct FillBar;

#[derive(Component)]
struct FoamBar;

/// Gauge width in pixels for 100%.
const GAUGE_PX: f32 = 200.0;

/// The win or loss screen.
#[derive(Component)]
struct OutcomeScreen;

#[derive(Component)]
struct OutcomeText;

fn setup_hud(mut commands: Commands, nodraw: Option<Res<NoDraw>>) {
    if nodraw.is_some() {
        return;
    }
    let font = |size: f32| TextFont { font_size: bevy::text::FontSize::Px(size), ..default() };
    // In the browser the page draws the clock, money and drunk meter
    // (web/hud.js); these texts stay for the native client.
    #[cfg(not(target_arch = "wasm32"))]
    spawn_text_hud(&mut commands);
    // Pour gauge, bottom center: fill (amber) over a track with the green
    // zone, and foam (white) under it.
    let bar =
        |w: f32, h: f32, color: Color| (Node { width: px(w), height: px(h), ..default() }, BackgroundColor(color));
    commands
        .spawn((
            PourPanel,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(70),
                left: percent(50),
                margin: UiRect::left(px(-GAUGE_PX / 2.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },
        ))
        .with_children(|panel| {
            panel.spawn((Text::new("POUR: release in the green"), font(14.0), TextColor(Color::WHITE)));
            panel
                .spawn((
                    Node { width: px(GAUGE_PX * 1.05), height: px(16), ..default() },
                    BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.8)),
                ))
                .with_children(|track| {
                    let (g0, g1) = shared::beer::GREEN;
                    track.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(GAUGE_PX * g0),
                            width: px(GAUGE_PX * (g1 - g0)),
                            height: percent(100),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.1, 0.8, 0.2, 0.6)),
                    ));
                    track.spawn((FillBar, bar(0.0, 16.0, Color::srgba(0.95, 0.65, 0.15, 0.9))));
                });
            panel.spawn((FoamBar, bar(0.0, 6.0, Color::srgb(0.95, 0.95, 0.9))));
        });
    commands
        .spawn((
            OutcomeScreen,
            Visibility::Hidden,
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_child((
            OutcomeText,
            Text::new(""),
            font(40.0),
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
        ));
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_text_hud(commands: &mut Commands) {
    let font = |size: f32| TextFont { font_size: bevy::text::FontSize::Px(size), ..default() };
    commands.spawn((
        ClockText,
        Text::new(""),
        font(18.0),
        TextColor(Color::srgb(1.0, 0.85, 0.55)),
        Node { position_type: PositionType::Absolute, bottom: px(30), left: px(10), ..default() },
    ));
    commands.spawn((
        DrunkText,
        Text::new(""),
        font(16.0),
        TextColor(Color::srgb(1.0, 0.7, 0.35)),
        Node { position_type: PositionType::Absolute, bottom: px(52), left: px(10), ..default() },
    ));
    commands.spawn((
        MoneyText,
        Text::new(""),
        font(16.0),
        TextColor(Color::srgb(0.55, 1.0, 0.75)),
        Node { position_type: PositionType::Absolute, bottom: px(8), left: px(10), ..default() },
    ));
}

/// Dollars with thousands separators: 120000 -> "$120,000".
fn dollars(v: i64) -> String {
    let digits = v.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("{}${out}", if v < 0 { "-" } else { "" })
}

fn update_drunk_text(status: Res<NetStatus>, mut text: Query<&mut Text, With<DrunkText>>) {
    let Ok(mut text) = text.single_mut() else { return };
    let line = match &status.game.drunk {
        Some(d) if d.passed_out => "PASSED OUT".to_string(),
        Some(d) => format!("Drunk {}  ({})", d.level, d.tier),
        None => String::new(),
    };
    if text.0 != line {
        text.0 = line;
    }
}

fn update_pour_gauge(
    status: Res<NetStatus>,
    mut panel: Query<&mut Visibility, With<PourPanel>>,
    mut fill: Query<&mut Node, (With<FillBar>, Without<FoamBar>)>,
    mut foam: Query<&mut Node, (With<FoamBar>, Without<FillBar>)>,
) {
    let Ok(mut vis) = panel.single_mut() else { return };
    let Some((f, o)) = status.game.pour else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    vis.set_if_neq(Visibility::Inherited);
    if let Ok(mut n) = fill.single_mut() {
        n.width = px(GAUGE_PX * f32::from(f) / 100.0);
    }
    if let Ok(mut n) = foam.single_mut() {
        n.width = px(GAUGE_PX * f32::from(o) / 100.0);
    }
}

fn update_hud(
    status: Res<NetStatus>,
    mut clock: Query<&mut Text, (With<ClockText>, Without<MoneyText>, Without<OutcomeText>)>,
    mut money: Query<&mut Text, (With<MoneyText>, Without<ClockText>, Without<OutcomeText>)>,
    mut outcome: Query<&mut Text, (With<OutcomeText>, Without<ClockText>, Without<MoneyText>)>,
    mut screen: Query<&mut Visibility, With<OutcomeScreen>>,
) {
    if let (Ok(mut text), Some(m)) = (money.single_mut(), &status.game.money) {
        let mut line = format!(
            "House {}  |  Paid {} of {}  |  Due this week {}  |  Tier {}",
            dollars(m.house),
            dollars(m.paid),
            dollars(m.debt),
            dollars(m.due),
            m.tier
        );
        if let Some(p) = status.game.pocket {
            line += &format!("  |  Pocket {}", dollars(p));
        }
        if m.missed_in_a_row > 0 {
            line += &format!("  |  MISSED {}", m.missed_in_a_row);
        }

        if text.0 != line {
            text.0 = line;
        }
    }
    let (Ok(mut text), Ok(mut vis)) = (outcome.single_mut(), screen.single_mut()) else { return };
    let (shown, line) = match status.game.money.as_ref().map(|m| (m.outcome, m.ng)) {
        Some(("won", _)) => {
            (true, "THE BAR IS YOURS\nThe debt is paid.\nNext run: bigger debt, more chaos.".to_string())
        }
        Some(("lost", _)) => {
            (true, "THE BAR BURNED DOWN\nTwo payments missed.\nNext run: bigger debt, more chaos.".to_string())
        }
        _ => (false, String::new()),
    };
    vis.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    if text.0 != line {
        text.0 = line;
    }
    let Ok(mut text) = clock.single_mut() else { return };
    let line = match &status.game.shift {
        Some(s) => format!(
            "Week {}  |  Shift {}/{}  |  {} {}{}",
            s.week,
            s.shift,
            shared::shift::SHIFTS_PER_WEEK,
            s.phase,
            shared::shift::clock_text(u32::from(s.seconds_left)),
            if s.running { "" } else { "  (paused)" }
        ),
        None => String::new(),
    };
    if text.0 != line {
        text.0 = line;
    }
}

pub(crate) fn setup_bar(
    mut commands: Commands,
    assets: Option<Res<AssetServer>>,
    nodraw: Option<Res<NoDraw>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    use crate::art;
    use crate::look::{self, Surface};
    use shared::world::{AREAS, Room};
    // Vertex colors carry the paint of untextured parts; one white material
    // per surface kind.
    let paint = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    let gloss = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.3, ..default() });
    // Test tabs that do not draw skip the textures and models.
    let assets = assets.filter(|_| nodraw.is_none());
    let flat_floor = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        lightmap_exposure: 9_000.0,
        ..default()
    });
    // Floors, with each room's baked lightmap (tools bake_lighting). The
    // office sits inside the bar's rectangle: its parquet lies just above.
    for a in AREAS.iter() {
        let (w, d) = (a.x1 - a.x0, a.z1 - a.z0);
        let office = a.room == Room::Office;
        let lift = if office { 0.004 } else { 0.0 };
        let center = Transform::from_xyz(a.x0 + w / 2.0, lift, a.z0 + d / 2.0);
        let Some(assets) = &assets else {
            if !office {
                commands.spawn((
                    Mesh3d(meshes.add(art::floor(w, d, art::palette(a.room).0))),
                    MeshMaterial3d(flat_floor.clone()),
                    center,
                ));
            }
            continue;
        };
        let surface = Surface::floor(a.room);
        let mut mesh = art::floor(w, d, Color::WHITE).transformed_by(center);
        look::world_uv(&mut mesh, surface.tile());
        let mut e = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(look::material(assets, surface, !office))),
            Transform::default(),
        ));
        if !office {
            let name = format!("{:?}", a.room).to_lowercase();
            e.insert(bevy::pbr::Lightmap {
                image: assets.load(format!("lightmaps/{name}.ktx2")),
                uv_rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                bicubic_sampling: false,
            });
        }
    }
    // Ceilings over the indoor rooms (the office shares the bar's).
    if let Some(assets) = &assets {
        let mut parts = Vec::new();
        for a in AREAS.iter().filter(|a| !a.room.outdoors() && a.room != Room::Office) {
            let (w, d) = (a.x1 - a.x0, a.z1 - a.z0);
            let slab = Plane3d::new(Vec3::NEG_Y, Vec2::new(w / 2.0, d / 2.0)).mesh().build();
            let mut m =
                art::part(slab, Transform::from_xyz(a.x0 + w / 2.0, bar::WALL_HEIGHT, a.z0 + d / 2.0), Color::WHITE);
            look::world_uv(&mut m, Surface::Ceiling.tile());
            parts.push(m);
        }
        commands.spawn((
            Mesh3d(meshes.add(art::combine(parts))),
            MeshMaterial3d(materials.add(look::material(assets, Surface::Ceiling, false))),
            Transform::default(),
        ));
    }
    // Walls and the bar's furniture: static, so they merge into one mesh per
    // material (a draw call per surface instead of about a hundred).
    let mut matte: Vec<Mesh> = Vec::new();
    let mut shiny: Vec<Mesh> = Vec::new();
    let mut surfaces: std::collections::BTreeMap<Surface, Vec<Mesh>> = default();
    let place = |m: Mesh, b: &bar::Block| m.transformed_by(Transform::from_xyz(b.cx, b.height / 2.0, b.cz));
    let mut textured = |surface: Surface, mut m: Mesh| {
        look::world_uv(&mut m, surface.tile());
        surfaces.entry(surface).or_default().push(m);
    };
    for b in shared::world::walls() {
        let room = shared::world::room_at(b.cx, b.cz);
        let outside = room.is_none_or(|r| r.outdoors());
        let low = b.height < bar::WALL_HEIGHT;
        if assets.is_some() {
            let surface = if low {
                Surface::LowWall
            } else if outside {
                Surface::OutsideWall
            } else {
                Surface::wall(room)
            };
            textured(surface, place(art::block(b, Color::WHITE), b));
            continue;
        }
        let color = if low {
            Color::srgb(0.25, 0.26, 0.3)
        } else if outside {
            Color::srgb(0.3, 0.32, 0.38)
        } else {
            Color::srgb(0.55, 0.42, 0.3)
        };
        matte.push(place(art::block(b, color), b));
    }
    for b in &bar::BLOCKS {
        let has_tex = assets.is_some();
        match b.kind {
            bar::BlockKind::Counter if has_tex => textured(Surface::Wood, place(art::counter(b), b)),
            bar::BlockKind::Counter => shiny.push(place(art::counter(b), b)),
            bar::BlockKind::Wall if has_tex => textured(Surface::OfficeWall, place(art::block(b, Color::WHITE), b)),
            bar::BlockKind::Wall => matte.push(place(art::block(b, Color::srgb(0.5, 0.38, 0.27)), b)),
            bar::BlockKind::Safe if has_tex => {
                textured(Surface::Metal, place(art::block(b, Color::srgb(0.45, 0.47, 0.5)), b))
            }
            bar::BlockKind::Safe => shiny.push(place(art::block(b, Color::srgb(0.2, 0.22, 0.25)), b)),
            bar::BlockKind::Table => matte.push(place(art::table(b), b)),
            bar::BlockKind::SlotMachine => shiny.push(place(art::slot_machine(b), b)),
        }
    }
    for (parts, mat) in [(matte, &paint), (shiny, &gloss)] {
        if !parts.is_empty() {
            commands.spawn((
                Mesh3d(meshes.add(art::combine(parts))),
                MeshMaterial3d(mat.clone()),
                Transform::default(),
            ));
        }
    }
    if let Some(assets) = &assets {
        // The night sky: a panorama on the inside of a big sphere, unlit.
        let sky = Sphere::new(450.0).mesh().uv(48, 24);
        commands.spawn((
            Mesh3d(meshes.add(sky)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(assets.load("sky/night.jpg")),
                unlit: true,
                cull_mode: None,
                fog_enabled: false,
                ..default()
            })),
            // Seen from inside: mirror it so the panorama is not reversed.
            Transform::from_xyz(0.0, -20.0, 0.0).with_scale(Vec3::new(-1.0, 1.0, 1.0)),
            bevy::light::NotShadowCaster,
        ));
        for (surface, parts) in surfaces {
            commands.spawn((
                Mesh3d(meshes.add(art::combine(parts))),
                MeshMaterial3d(materials.add(look::material(assets, surface, false))),
                Transform::default(),
            ));
        }
        look::spawn_decor(&mut commands, assets);
    }
    // A neon strip per room in its accent color (bloom makes them glow).
    for a in AREAS.iter().filter(|a| a.room != Room::Bar && !a.room.outdoors()) {
        let accent = art::palette(a.room).1;
        let lum = accent.to_linear();
        let neon = materials.add(StandardMaterial {
            base_color: accent,
            emissive: LinearRgba::rgb(lum.red * 6.0, lum.green * 6.0, lum.blue * 6.0),
            ..default()
        });
        let len = ((a.x1 - a.x0) * 0.5).min(4.0);
        commands.spawn((
            Mesh3d(meshes.add(Capsule3d::new(0.025, len).mesh().longitudes(8).latitudes(4).build())),
            MeshMaterial3d(neon),
            Transform::from_xyz((a.x0 + a.x1) / 2.0, 2.7, a.z0 + 0.12)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
        ));
    }
    // The back bar behind the counter (either side of the back room door)
    // and the beer tap on the counter.
    {
        let back = art::back_bar(-bar::HALF_Z + 0.1, &[(-5.0, 0.8), (3.2, 5.8)]);
        let glass = materials.add(StandardMaterial { perceptual_roughness: 0.12, reflectance: 0.7, ..default() });
        let glow = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.8, 0.5),
            emissive: LinearRgba::rgb(6.0, 3.6, 1.4),
            ..default()
        });
        let mirror = materials.add(StandardMaterial {
            base_color: Color::srgb(0.12, 0.1, 0.09),
            perceptual_roughness: 0.05,
            metallic: 1.0,
            ..default()
        });
        for (mesh, mat) in [(back.wood, paint.clone()), (back.bottles, glass), (back.glow, glow), (back.mirror, mirror)]
        {
            commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), Transform::default()));
        }
        commands.spawn((
            Mesh3d(meshes.add(art::tap_tower())),
            MeshMaterial3d(gloss.clone()),
            Transform::from_xyz(bar::TAP.0, bar::COUNTER_HEIGHT, bar::TAP.1),
        ));
    }
    // The bar's neon sign on the back wall, over the shelves, with a pink
    // light so it glows on the bricks around it.
    commands.spawn((
        Mesh3d(meshes.add(art::neon_text("LAST CALL", 0.42, Color::WHITE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.3, 0.7),
            emissive: LinearRgba::rgb(9.0, 0.9, 4.5),
            ..default()
        })),
        Transform::from_xyz(0.0, 2.6, -bar::HALF_Z + 0.12),
    ));
    commands.spawn((
        PointLight { intensity: 40_000.0, range: 4.0, color: Color::srgb(1.0, 0.25, 0.65), ..default() },
        Transform::from_xyz(0.0, 2.6, -bar::HALF_Z + 0.6),
        crate::look::Far(25.0),
    ));
    // Outside: OPEN in blue neon over the front door, facing the lot.
    commands.spawn((
        Mesh3d(meshes.add(art::neon_text("OPEN", 0.38, Color::WHITE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.8, 1.0),
            emissive: LinearRgba::rgb(1.0, 4.0, 9.0),
            ..default()
        })),
        // The strokes read right from +Z, the lot.
        Transform::from_xyz(0.0, 2.75, bar::HALF_Z + 0.14),
    ));
    commands.spawn((
        PointLight { intensity: 60_000.0, range: 6.0, color: Color::srgb(0.3, 0.75, 1.0), ..default() },
        Transform::from_xyz(0.0, 2.6, bar::HALF_Z + 0.8),
        crate::look::Far(25.0),
    ));
    // The parking lot: painted bays along both sides, and light under the
    // street lamps (crates/client/src/look.rs places the posts).
    let lot = shared::world::area_of(Room::ParkingLot);
    let mut lines = Vec::new();
    let paint_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.95, 0.92, 0.8, 0.85),
        perceptual_roughness: 0.6,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    for i in 0..6 {
        let z = lot.z0 + 5.0 + i as f32 * 2.7;
        for x in [lot.x0 + 2.25, lot.x1 - 2.25] {
            lines.push(art::part(Cuboid::new(4.5, 0.005, 0.1), Transform::from_xyz(x, 0.006, z), Color::WHITE));
        }
    }
    // One mesh for all the lines: one draw.
    commands.spawn((Mesh3d(meshes.add(art::combine(lines))), MeshMaterial3d(paint_mat), Transform::default()));
    for (x, z) in [(-13.2, 7.6), (13.2, 7.6), (-13.2, 26.4)] {
        commands.spawn((
            PointLight { intensity: 200_000.0, range: 10.0, color: Color::srgb(1.0, 0.82, 0.55), ..default() },
            Transform::from_xyz(x, 3.6, z),
            crate::look::Far(25.0),
        ));
    }
    // Warm lamps inside (at most 8 per room); a cold moon outside, the only shadow caster.
    // Each lamp hangs from a holder that far culling hides (CPU light
    // clustering costs per light in view); the outage hides the lamp itself.
    for (_, x, z) in shared::world::lamps() {
        commands
            .spawn((Transform::from_xyz(x, shared::world::LAMP_Y, z), Visibility::default(), crate::look::Far(18.0)))
            .with_child((
                RoomLamp,
                PointLight { intensity: 300_000.0, range: 14.0, color: Color::srgb(1.0, 0.78, 0.5), ..default() },
                Transform::default(),
            ));
    }
    // Two cascades out to 40 m at 1024 px: the moon only shades the outdoors.
    commands.insert_resource(bevy::light::DirectionalLightShadowMap { size: 1024 });
    commands.spawn((
        DirectionalLight {
            illuminance: 1_500.0,
            color: Color::srgb(0.6, 0.7, 1.0),
            shadow_maps_enabled: true,
            ..default()
        },
        bevy::light::CascadeShadowConfigBuilder { num_cascades: 2, maximum_distance: 40.0, ..default() }.build(),
        Transform::from_xyz(5.0, 20.0, 30.0).looking_at(Vec3::new(0.0, 0.0, 20.0), Vec3::Y),
    ));
    // A test tab with `?nodraw` checks game state only: no camera, no rendering.
    if nodraw.is_some() {
        return;
    }
    commands.spawn((
        Camera3d::default(),
        // No lookup-table tonemapper: the LUT ships as zstd KTX2 and failed to
        // decompress in WebKit.
        Tonemapping::Reinhard,
        bevy::camera::Hdr,
        bevy::post_process::bloom::Bloom::NATURAL,
        bevy::post_process::effect_stack::Vignette { intensity: 0.25, radius: 0.9, ..default() },
        bevy::post_process::effect_stack::ChromaticAberration { intensity: 0.0, ..default() },
        // Coarse light clusters: at most eight lamps light a room, and CPU
        // clustering (WebGL2) costs per cluster.
        bevy::light::cluster::ClusterConfig::FixedZ {
            total: 256,
            z_slices: 8,
            z_config: default(),
            dynamic_resizing: true,
        },
        Transform::from_xyz(0.0, 6.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// An indoor lamp (the power outage turns these off).
#[derive(Component)]
pub struct RoomLamp;

#[allow(clippy::too_many_arguments)]
fn read_input(
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scripted: Res<ScriptedInput>,
    settings: Res<Settings>,
    time: Res<Time>,
    pads: Query<&Gamepad>,
    mut look: ResMut<Look>,
    mut input: ResMut<LocalInput>,
) {
    if let Some(s) = scripted.0 {
        input.0 = s;
        look.yaw = s.yaw();
        return;
    }
    const SENSITIVITY: f32 = 0.0025;
    let sens = SENSITIVITY * settings.sensitivity;
    if settings.mouse_look {
        let dy = if settings.invert_y { -motion.delta.y } else { motion.delta.y };
        look.yaw -= motion.delta.x * sens;
        look.pitch = (look.pitch - dy * sens).clamp(-1.4, 1.4);
    }
    let k = settings.keys;
    let axis =
        |pos: KeyCode, neg: KeyCode| f32::from(u8::from(keys.pressed(pos))) - f32::from(u8::from(keys.pressed(neg)));
    let mut mv = Vec2::new(axis(k.right, k.left), axis(k.forward, k.back));
    // A gamepad: left stick walks, right stick looks (2.5 rad/s at full tilt).
    let mut pad_buttons = 0;
    for pad in &pads {
        let l = pad.left_stick();
        if l.length() > 0.15 {
            mv = l.clamp_length_max(1.0);
        }
        let r = pad.right_stick();
        if r.length() > 0.15 {
            let turn = 2.5 * time.delta_secs() * settings.sensitivity;
            look.yaw -= r.x * turn;
            look.pitch = (look.pitch + r.y * turn).clamp(-1.4, 1.4);
        }
        for (button, bit) in [
            (GamepadButton::South, buttons::INTERACT),
            (GamepadButton::East, buttons::DROP),
            (GamepadButton::West, buttons::USE),
            (GamepadButton::RightTrigger2, buttons::THROW),
            (GamepadButton::LeftThumb, buttons::SPRINT),
            (GamepadButton::North, buttons::JUMP),
            (GamepadButton::RightThumb, buttons::CROUCH),
        ] {
            if pad.pressed(button) {
                pad_buttons |= bit;
            }
        }
    }
    let mut b = pad_buttons;
    for (key, bit) in [
        (k.sprint, buttons::SPRINT),
        (k.jump, buttons::JUMP),
        (k.crouch, buttons::CROUCH),
        (k.interact, buttons::INTERACT),
        (k.throw, buttons::THROW),
        (k.drop, buttons::DROP),
        (k.use_item, buttons::USE),
    ] {
        if keys.pressed(key) {
            b |= bit;
        }
    }
    input.0 = PlayerInput::new(mv, look.yaw, look.pitch, b);
}

/// This player's pour, predicted on the client's own timeline. The host's
/// gauge arrives about a round trip late; inputs are stamped with the
/// client's tick, so a release lands on the host at the tick this predicts.
#[derive(Resource, Default)]
pub struct LocalPour(pub Option<shared::beer::Pour>);

/// Mirror the host's pour rules (crates/host/src/beer.rs) with this player's
/// own inputs, once per tick. When the predicted pour starts or ends, stamp
/// the tick for the host ([`TapEvent`]), so a late input packet does not
/// stretch or shorten the pour there.
#[allow(clippy::too_many_arguments)]
fn predict_pour(
    session: Res<Session>,
    input: Res<LocalInput>,
    timeline: Option<Res<LocalTimeline>>,
    own: Query<&PlayerPos, (With<Predicted>, With<Player>)>,
    drunks: Query<(&Player, &Drunk)>,
    held: Query<&HeldBy>,
    mut pour: ResMut<LocalPour>,
    mut taps: ResMut<shared::client::OutgoingTaps>,
    upgrades: Query<&RoomUpgrades>,
) {
    let Some(id) = session.player_id else { return };
    let Ok(pos) = own.single() else { return };
    let tick = timeline.map_or(0, |t| t.tick().0);
    let input = input.0;
    let at_tap = shared::movement::distance_to_tap(pos.0.to_array()) < bar::TAP_REACH;
    let holding = held.iter().any(|h| h.0 == Some(id));
    let out = drunks.iter().any(|(p, d)| p.id == id && d.passed_out);
    if input.buttons & buttons::INTERACT == 0 || !at_tap || holding || out {
        // A pour that had not overflowed ends here: the glass is filled to this tick.
        if pour.0.take().is_some_and(|p| p.fill <= shared::beer::OVERFLOW) {
            taps.0.push(TapEvent { tick, down: false });
        }
        return;
    }
    if pour.0.is_none() {
        taps.0.push(TapEvent { tick, down: true });
    }
    let p = pour.0.get_or_insert_default();
    if p.fill <= shared::beer::OVERFLOW {
        let speed = upgrades.single().map_or(1.0, |u| u.0.pour_speed());
        p.step(input.pitch(), shared::TICK.as_secs_f32() * speed);
    }
}

#[derive(Component)]
struct Dressed;

/// Give every replicated player a capsule mesh once.
fn dress_players(
    mut commands: Commands,
    players: Query<(Entity, &Player, Has<Predicted>, Has<Interpolated>), Without<Dressed>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, player, predicted, interpolated) in &players {
        // Only draw the predicted or interpolated copy, not the raw confirmed one.
        if !predicted && !interpolated {
            continue;
        }
        // A player's shirt is a bright colour from their id, so friends tell
        // each other apart; the rest of the outfit varies too.
        let mut outfit = crate::art::Outfit::from_seed(player.id);
        outfit.shirt = Color::hsl((player.id % 360) as f32, 0.7, 0.5);
        let mut parts = crate::art::figure_parts(outfit);
        if player.cosmetic > 0 {
            parts.body.merge(&crate::art::hat(player.cosmetic - 1)).expect("same attributes");
        }
        let material =
            materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.7, ..default() });
        rig(&mut commands, entity, parts, &mut meshes, &material);
        let mut e = commands.entity(entity);
        e.insert((Dressed, Transform::default()));
        // First person: your own body would fill the view.
        if predicted {
            e.insert(Visibility::Hidden);
        }
    }
}

fn place_players(mut q: Query<(&PlayerPos, &PlayerYaw, Option<&Drunk>, &mut Transform), With<Dressed>>) {
    for (pos, yaw, drunk, mut t) in &mut q {
        if drunk.is_some_and(|d| d.passed_out) {
            // Lying on the floor.
            t.translation = pos.0 + Vec3::Y * bar::PLAYER_RADIUS;
            t.rotation = Quat::from_rotation_y(yaw.0) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        } else {
            t.translation = pos.0 + Vec3::Y * (bar::PLAYER_HEIGHT / 2.0);
            t.rotation = Quat::from_rotation_y(yaw.0);
        }
    }
}

#[derive(Component)]
struct DressedProp;

/// Give every replicated prop a mesh once. Shared meshes and materials per
/// kind let Bevy batch them.
fn dress_props(
    mut commands: Commands,
    props: Query<(Entity, &PropKind), (With<Interpolated>, Without<DressedProp>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Option<[(Handle<Mesh>, Handle<StandardMaterial>); 5]>>,
    assets: Option<Res<AssetServer>>,
    nodraw: Option<Res<NoDraw>>,
) {
    if props.is_empty() {
        return;
    }
    // Stools draw as the scanned bar stool (its origin is on the floor; the
    // physics body's centre is 0.38 m up).
    let stool_model = assets
        .filter(|_| nodraw.is_none())
        .map(|a| a.load(GltfAssetLabel::Scene(0).from_asset("models/bar_chair_round_01/bar_chair_round_01.gltf")));
    let looks = cache.get_or_insert_with(|| {
        let white =
            materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.4, ..default() });
        let glow = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            emissive: LinearRgba::rgb(0.3, 0.18, 0.02),
            perceptual_roughness: 0.2,
            ..default()
        });
        [
            (meshes.add(crate::art::bottle()), white.clone()),
            (meshes.add(crate::art::chip()), white.clone()),
            (meshes.add(crate::art::stool()), white.clone()),
            (meshes.add(crate::art::glass()), glow),
            (meshes.add(crate::art::mop()), white),
        ]
    });
    for (entity, kind) in &props {
        if let (PropKind::Stool, Some(model)) = (kind, &stool_model) {
            commands.entity(entity).insert((DressedProp, Transform::default(), Visibility::default())).with_child((
                bevy::world_serialization::WorldAssetRoot(model.clone()),
                Transform::from_xyz(0.0, -0.38, 0.0),
            ));
            continue;
        }
        let (mesh, mat) = &looks[match kind {
            PropKind::Bottle => 0,
            PropKind::Chip => 1,
            PropKind::Stool => 2,
            PropKind::Glass => 3,
            PropKind::Mop => 4,
        }];
        commands.entity(entity).insert((
            DressedProp,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::default(),
        ));
        // Bottles and glasses are a few pixels across the room, chips (3.6 cm,
        // a hundred of them) at a few metres.
        match kind {
            PropKind::Bottle | PropKind::Glass => {
                commands.entity(entity).insert(crate::look::Far(14.0));
            }
            PropKind::Chip => {
                commands.entity(entity).insert(crate::look::Far(6.0));
            }
            _ => {}
        }
    }
}

/// Draw props at their interpolated pose, except the one this player holds:
/// that one follows the predicted hand, so carrying feels instant.
fn place_props(
    session: Res<Session>,
    look: Res<Look>,
    own: Query<&PlayerPos, (With<Predicted>, With<Player>)>,
    mut props: Query<(&PropPose, Option<&HeldBy>, &mut Transform), With<DressedProp>>,
) {
    let hand = own.single().ok().map(|p| Vec3::from_array(shared::movement::hand_point(p.0.to_array(), look.yaw)));
    for (pose, held, mut t) in &mut props {
        let mine = held.and_then(|h| h.0).is_some_and(|id| Some(id) == session.player_id);
        t.translation = match (mine, hand) {
            (true, Some(h)) => h,
            _ => pose.pos,
        };
        t.rotation = pose.rot;
    }
}

#[derive(Component)]
struct DressedPuddle;

/// Spilled beer: a flat amber disc on the floor.
fn dress_puddles(
    mut commands: Commands,
    puddles: Query<(Entity, &Puddle), Without<DressedPuddle>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    if puddles.is_empty() {
        return;
    }
    let (mesh, mat) = cache
        .get_or_insert_with(|| {
            (
                meshes.add(Cylinder::new(0.5, 0.01)),
                materials.add(StandardMaterial {
                    base_color: Color::srgba(0.75, 0.5, 0.1, 0.8),
                    perceptual_roughness: 0.05,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                }),
            )
        })
        .clone();
    for (entity, puddle) in &puddles {
        commands.entity(entity).insert((
            DressedPuddle,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(puddle.pos),
        ));
    }
}

#[derive(Component)]
struct DressedCustomer;

/// The yellow marker over a customer who is waiting for a beer.
#[derive(Component)]
struct OrderMarker;

/// Give every customer a capsule (muted colors, so players stand out) and an
/// order marker above the head.
fn dress_customers(
    mut commands: Commands,
    customers: Query<(Entity, &Customer), (With<Interpolated>, Without<DressedCustomer>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Option<(Handle<StandardMaterial>, Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    if customers.is_empty() {
        return;
    }
    let (cloth, marker, marker_mat) = cache
        .get_or_insert_with(|| {
            (
                materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.75, ..default() }),
                meshes.add(Sphere::new(0.12)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.85, 0.2),
                    emissive: LinearRgba::rgb(1.5, 1.2, 0.2),
                    ..default()
                }),
            )
        })
        .clone();
    for (entity, c) in &customers {
        // Every customer dresses differently; their shirts are a little
        // duller than the players'.
        let mut outfit = crate::art::Outfit::from_seed(u64::from(c.id) + 7);
        outfit.shirt = outfit.shirt.mix(&Color::srgb(0.4, 0.38, 0.36), 0.35);
        rig(&mut commands, entity, crate::art::figure_parts(outfit), &mut meshes, &cloth);
        commands.entity(entity).insert((DressedCustomer, Transform::default())).with_child((
            OrderMarker,
            Mesh3d(marker.clone()),
            MeshMaterial3d(marker_mat.clone()),
            Transform::from_xyz(0.0, 1.15, 0.0),
            Visibility::Hidden,
        ));
    }
}

fn place_customers(
    mut customers: Query<(&Customer, &NpcPose, &mut Transform, &Children, Option<&mut Gait>), With<DressedCustomer>>,
    mut markers: Query<&mut Visibility, With<OrderMarker>>,
) {
    use shared::customers::Mood;
    for (c, pose, mut t, children, gait) in &mut customers {
        // Waiting or drinking at the counter: sitting on a stool.
        if let Some(mut gait) = gait {
            let at_counter = (pose.pos.z - (bar::COUNTER.1 + bar::COUNTER.3 + 0.8)).abs() < 0.6;
            gait.seated = at_counter && matches!(c.mood, Mood::Waiting | Mood::Drinking);
        }
        t.translation = pose.pos + Vec3::Y * (bar::PLAYER_HEIGHT / 2.0);
        t.rotation = Quat::from_rotation_y(pose.yaw);
        let shown = if c.mood == shared::customers::Mood::Waiting { Visibility::Inherited } else { Visibility::Hidden };
        for child in children.iter() {
            if let Ok(mut v) = markers.get_mut(child) {
                v.set_if_neq(shown);
            }
        }
    }
}

/// A walking figure's stride (players and customers).
#[derive(Component, Default)]
struct Gait {
    phase: f32,
    swing: f32,
    last: Option<Vec3>,
    /// On a stool at the counter: legs forward, forearms on the bar.
    seated: bool,
}

/// An arm or a leg: 0 left arm, 1 right arm, 2 left leg, 3 right leg.
#[derive(Component)]
struct Limb(u8);

/// Put an articulated figure on `entity`: its body as the entity's mesh,
/// the limbs as children turning at their joints.
fn rig(
    commands: &mut Commands,
    entity: Entity,
    parts: crate::art::FigureParts,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
) {
    let arm = meshes.add(parts.arm);
    let leg = meshes.add(parts.leg);
    let mut e = commands.entity(entity);
    e.insert((Mesh3d(meshes.add(parts.body)), MeshMaterial3d(material.clone()), Gait::default()));
    for (i, joint) in parts.joints.into_iter().enumerate() {
        let mesh = if i < 2 { arm.clone() } else { leg.clone() };
        e.with_child((
            Limb(i as u8),
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(joint),
        ));
    }
}

/// Swing arms and legs with the speed a figure moves at: a stride that grows
/// from standing to a run, arms opposite to legs.
fn animate_gait(
    time: Res<Time>,
    mut walkers: Query<(&Transform, &mut Gait, &Children)>,
    mut limbs: Query<(&Limb, &mut Transform), Without<Gait>>,
) {
    let dt = time.delta_secs().max(1e-4);
    for (t, mut gait, children) in &mut walkers {
        let pos = t.translation;
        let speed = gait.last.map_or(0.0, |l| Vec2::new(pos.x - l.x, pos.z - l.z).length() / dt).min(8.0);
        gait.last = Some(pos);
        let target = (speed / 3.0).min(1.0);
        gait.swing += (target - gait.swing) * (dt * 8.0).min(1.0);
        gait.phase += dt * (3.0 + speed * 2.4);
        let a = gait.phase.sin() * 0.7 * gait.swing;
        for child in children.iter() {
            if let Ok((limb, mut lt)) = limbs.get_mut(child) {
                let angle = if gait.seated {
                    // Thighs forward off the stool, arms resting forward.
                    if limb.0 < 2 { 0.9 + 0.05 * (gait.phase * 0.3 + f32::from(limb.0)).sin() } else { 1.35 }
                } else {
                    match limb.0 {
                        0 | 3 => a,
                        _ => -a,
                    }
                };
                lt.rotation = Quat::from_rotation_x(if limb.0 < 2 && !gait.seated { angle * 0.8 } else { angle });
            }
        }
    }
}

/// Eye height above the feet: a little under the 1.6 m player.
const EYE_HEIGHT: f32 = 1.5;

fn follow_camera(
    look: Res<Look>,
    time: Res<Time>,
    status: Res<NetStatus>,
    own: Query<&PlayerPos, (With<Predicted>, With<Player>)>,
    mut cam: Query<&mut Transform, (With<Camera3d>, Without<Player>)>,
) {
    let (Ok(pos), Ok(mut t)) = (own.single(), cam.single_mut()) else { return };
    let drunk = status.game.drunk.as_ref();
    if drunk.is_some_and(|d| d.passed_out) {
        // On the floor: a low view, looking up and across the room.
        let eye = pos.0 + Vec3::Y * 0.3;
        *t = Transform::from_translation(eye).looking_at(eye + Vec3::new(0.3, 1.2, -2.0), Vec3::Y);
        return;
    }
    // First person: at eye height, turned by the mouse.
    let eye = pos.0 + Vec3::Y * EYE_HEIGHT;
    *t = Transform::from_translation(eye).with_rotation(Quat::from_euler(EulerRot::YXZ, look.yaw, look.pitch, 0.0));
    // Courage and up: the view sways.
    let sway = shared::drunk::camera_sway(drunk.map_or(0, |d| d.level));
    if sway > 0.0 {
        let s = time.elapsed_secs();
        t.rotate_local_z((s * 0.9).sin() * sway);
        t.rotate_local_y((s * 0.6).cos() * sway * 0.5);
    }
}

/// The post stack follows the meters (plan section 7): chromatic aberration
/// grows with drink, the vignette closes in when Buzzed or spinning.
fn post_effects(
    status: Res<NetStatus>,
    mut cam: Query<
        (&mut bevy::post_process::effect_stack::ChromaticAberration, &mut bevy::post_process::effect_stack::Vignette),
        With<Camera3d>,
    >,
) {
    let Ok((mut ca, mut vig)) = cam.single_mut() else { return };
    let drunk = status.game.drunk.as_ref().map_or(0, |d| d.level);
    let p4 = &status.game.phase4;
    let aberration = if drunk > 20 { f32::from(drunk - 20) / 80.0 * 0.04 } else { 0.0 };
    let vignette = if p4.spinning {
        0.9
    } else if p4.focus > 60 {
        0.5
    } else {
        0.25
    };
    if (ca.intensity - aberration).abs() > 1e-4 {
        ca.intensity = aberration;
    }
    if (vig.intensity - vignette).abs() > 1e-4 {
        vig.intensity = vignette;
    }
}

/// The Low preset: no shadows, no bloom, and (with no bloom to feed) no HDR
/// target or post passes; the screen blur and grain are CSS either way.
pub(crate) fn graphics_preset(
    mut commands: Commands,
    settings: Res<Settings>,
    cams: Query<Entity, With<Camera3d>>,
    mut suns: Query<&mut DirectionalLight>,
) {
    use bevy::post_process::bloom::Bloom;
    use bevy::post_process::effect_stack::{ChromaticAberration, Vignette};
    if !settings.is_changed() {
        return;
    }
    for cam in &cams {
        if settings.low_graphics {
            // Low: no HDR, no post passes and no multisampling (its resolve
            // is a pass every frame).
            commands
                .entity(cam)
                .remove::<(Bloom, bevy::camera::Hdr, Vignette, ChromaticAberration)>()
                .insert(bevy::render::view::Msaa::Off);
        } else {
            commands.entity(cam).insert((
                bevy::render::view::Msaa::Sample4,
                bevy::camera::Hdr,
                Bloom::NATURAL,
                Vignette { intensity: 0.25, radius: 0.9, ..default() },
                ChromaticAberration { intensity: 0.0, ..default() },
            ));
        }
    }
    for mut sun in &mut suns {
        sun.shadow_maps_enabled = !settings.low_graphics;
    }
}

/// Everything [`update_status`] reads about the game (one system parameter,
/// so the system stays under Bevy's parameter limit).
#[derive(bevy::ecs::system::SystemParam)]
struct GameQueries<'w, 's> {
    room: Query<'w, 's, (&'static ShiftClock, Option<&'static RunLedger>), With<RoomState>>,
    pockets: Query<'w, 's, (&'static Player, &'static Pocket)>,
    customers: Query<'w, 's, (&'static Customer, &'static NpcPose), With<Interpolated>>,
    gauges: Query<'w, 's, (&'static Player, &'static PourGauge)>,
    beers: Query<'w, 's, (&'static Beer, &'static HeldBy)>,
    puddles: Query<'w, 's, (), With<Puddle>>,
    drunks: Query<'w, 's, (&'static Player, &'static Drunk)>,
    local_pour: Res<'w, LocalPour>,
}

fn update_status(
    session: Res<Session>,
    connected: Query<(), (With<Client>, With<Connected>)>,
    players: Query<&Player>,
    drawn: Query<(&Player, &PlayerPos), Or<(With<Predicted>, With<Interpolated>)>>,
    own: Query<&PlayerPos, (With<Predicted>, With<Player>)>,
    props: Query<(&PropKind, &PropPose, &HeldBy), With<Interpolated>>,
    link: Query<&Link, With<Client>>,
    timeline: Option<Res<LocalTimeline>>,
    game: GameQueries,
    casino: crate::casino::CasinoQueries,
    phase4: crate::chaos::Phase4Queries,
    games: crate::games::GamesQueries,
    mut status: ResMut<NetStatus>,
) {
    let GameQueries { room, pockets, customers, gauges, beers, puddles, drunks, local_pour } = game;
    let (rtt_ms, jitter_ms) = link
        .single()
        .map(|l| (l.stats.rtt.as_secs_f32() * 1000.0, l.stats.jitter.as_secs_f32() * 1000.0))
        .unwrap_or_default();
    let on_floor =
        props.iter().filter(|(k, p, _)| **k != PropKind::Stool && p.pos.y < bar::COUNTER_HEIGHT - 0.3).count();
    let holding = props.iter().any(|(_, _, h)| h.0.is_some() && h.0 == session.player_id);
    let mut ids: Vec<u64> = players.iter().map(|p| p.id).collect();
    ids.sort_unstable();
    ids.dedup();
    *status = NetStatus {
        connected: !connected.is_empty(),
        player_id: session.player_id,
        players_seen: ids.len(),
        own_pos: own.single().ok().map(|p| p.0),
        refused: session.refused.clone(),
        props_seen: props.iter().count(),
        holding,
        props_on_floor: on_floor,
        players: drawn.iter().map(|(p, pos)| (p.id, pos.0)).collect(),
        rtt_ms,
        jitter_ms,
        tick: timeline.map(|t| t.tick().0).unwrap_or(0),
        game: GameStatus {
            shift: room.iter().next().map(|(c, _)| (*c).into()),
            money: room.iter().next().and_then(|(_, l)| l.copied()).map(Into::into),
            pocket: pockets.iter().find(|(p, _)| Some(p.id) == session.player_id).map(|(_, m)| m.0),
            customers: {
                let mut v: Vec<_> =
                    customers.iter().map(|(c, p)| (c.id, mood_name(c.mood), p.pos.x, p.pos.z)).collect();
                v.sort_by_key(|c| c.0);
                v
            },
            // The predicted pour while E is held; the host's gauge otherwise.
            pour: local_pour
                .0
                .filter(|p| p.fill <= shared::beer::OVERFLOW)
                .map(|p| ((p.fill * 100.0).round() as u8, (p.foam * 100.0).round() as u8))
                .or_else(|| {
                    gauges.iter().find(|(p, _)| Some(p.id) == session.player_id).map(|(_, g)| (g.fill, g.foam))
                }),
            beer: beers
                .iter()
                .find(|(_, h)| h.0.is_some() && h.0 == session.player_id)
                .map(|(b, _)| (b.fill, b.perfect)),
            puddles: puddles.iter().count(),
            drunk: drunks.iter().find(|(p, _)| Some(p.id) == session.player_id).map(|(_, d)| DrunkStatus {
                level: d.level,
                tier: shared::drunk::Tier::of(d.level).label(),
                passed_out: d.passed_out,
                blur: shared::drunk::blur(d.level),
            }),
            voice_pitch: {
                let mut v: Vec<(String, f32)> = drunks
                    .iter()
                    .filter(|(p, _)| Some(p.id) != session.player_id)
                    .map(|(p, d)| (format!("{:016x}", p.id), shared::drunk::voice_pitch(d.level)))
                    .collect();
                v.sort_by(|a, b| a.0.cmp(&b.0));
                v.dedup_by(|a, b| a.0 == b.0);
                v
            },
            casino: casino.status(session.player_id, own.single().ok().map(|p| p.0)),
            phase4: phase4.status(session.player_id, own.single().ok().map(|p| p.0)),
            games: games.status(own.single().ok().map(|p| p.0)),
        },
    };
}

#[cfg(test)]
mod tests {
    use super::dollars;

    #[test]
    fn dollars_have_separators() {
        assert_eq!(dollars(0), "$0");
        assert_eq!(dollars(999), "$999");
        assert_eq!(dollars(1_000), "$1,000");
        assert_eq!(dollars(120_000), "$120,000");
        assert_eq!(dollars(-1_234_567), "-$1,234,567");
    }
}
