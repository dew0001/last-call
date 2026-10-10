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
}

impl Default for Settings {
    fn default() -> Self {
        Self { sensitivity: 1.0, low_graphics: false }
    }
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
}

impl From<ShiftClock> for ShiftStatus {
    fn from(c: ShiftClock) -> Self {
        Self {
            week: c.calendar.week,
            shift: c.calendar.shift + 1,
            phase: c.phase.label(),
            seconds_left: c.seconds_left,
            running: c.running,
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
    app.add_systems(Startup, (setup_bar, setup_hud));
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
            dress_puddles,
            follow_camera.in_set(CameraSet),
            post_effects,
            graphics_preset,
            update_status,
            update_hud,
            update_pour_gauge,
            update_drunk_text,
        )
            .chain()
            .in_set(OnlineSet),
    );
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

fn setup_bar(
    mut commands: Commands,
    assets: Option<Res<AssetServer>>,
    nodraw: Option<Res<NoDraw>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    use crate::art;
    use shared::world::{AREAS, Room};
    // Vertex colors carry the paint; one white material per surface kind.
    let paint = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    let gloss = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.3, ..default() });
    // Floors, with each room's baked lightmap (tools bake_lighting).
    let floor_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        lightmap_exposure: 9_000.0,
        ..default()
    });
    for a in AREAS.iter().filter(|a| a.room != Room::Office) {
        let (w, d) = (a.x1 - a.x0, a.z1 - a.z0);
        let mut e = commands.spawn((
            Mesh3d(meshes.add(art::floor(w, d, art::palette(a.room).0))),
            MeshMaterial3d(floor_mat.clone()),
            Transform::from_xyz(a.x0 + w / 2.0, 0.0, a.z0 + d / 2.0),
        ));
        if nodraw.is_none()
            && let Some(assets) = &assets
        {
            let name = format!("{:?}", a.room).to_lowercase();
            e.insert(bevy::pbr::Lightmap {
                image: assets.load(format!("lightmaps/{name}.ktx2")),
                uv_rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                bicubic_sampling: false,
            });
        }
    }
    // Walls: warm plaster inside, cold brick outside.
    for b in shared::world::walls() {
        let outside = shared::world::room_at(b.cx, b.cz).is_none_or(|r| r.outdoors());
        let color = if b.height < bar::WALL_HEIGHT {
            Color::srgb(0.25, 0.26, 0.3)
        } else if outside {
            Color::srgb(0.3, 0.32, 0.38)
        } else {
            Color::srgb(0.55, 0.42, 0.3)
        };
        commands.spawn((
            Mesh3d(meshes.add(art::block(b, color))),
            MeshMaterial3d(paint.clone()),
            Transform::from_xyz(b.cx, b.height / 2.0, b.cz),
        ));
    }
    // The harbor around the pier.
    let pier = shared::world::area_of(Room::Pier);
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(60.0, 0.05, pier.z1 - pier.z0 + 10.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.03, 0.1, 0.22),
            perceptual_roughness: 0.15,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.6, (pier.z0 + pier.z1) / 2.0 + 5.0),
    ));
    // The bar's furniture.
    for b in &bar::BLOCKS {
        let (mesh, mat) = match b.kind {
            bar::BlockKind::Counter => (art::counter(b), &gloss),
            bar::BlockKind::Wall => (art::block(b, Color::srgb(0.5, 0.38, 0.27)), &paint),
            bar::BlockKind::Safe => (art::block(b, Color::srgb(0.2, 0.22, 0.25)), &gloss),
            bar::BlockKind::Table => (art::table(b), &paint),
            bar::BlockKind::SlotMachine => (art::slot_machine(b), &gloss),
        };
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(b.cx, b.height / 2.0, b.cz),
        ));
    }
    // A neon strip per room in its accent color (bloom makes them glow).
    for a in AREAS.iter() {
        let accent = art::palette(a.room).1;
        let lum = accent.to_linear();
        let neon = materials.add(StandardMaterial {
            base_color: accent,
            emissive: LinearRgba::rgb(lum.red * 6.0, lum.green * 6.0, lum.blue * 6.0),
            ..default()
        });
        let len = ((a.x1 - a.x0) * 0.5).min(4.0);
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(len, 0.12, 0.06))),
            MeshMaterial3d(neon),
            Transform::from_xyz((a.x0 + a.x1) / 2.0, 2.5, a.z0 + 0.12),
        ));
    }
    // The bar's sign over the counter.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.0, 0.6, 0.1))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.2, 0.6),
            emissive: LinearRgba::rgb(4.0, 0.4, 2.0),
            ..default()
        })),
        Transform::from_xyz(0.0, 2.4, -bar::HALF_Z + 0.05),
    ));
    // Warm lamps inside (at most 8 per room); a cold moon outside, the only shadow caster.
    for (_, x, z) in shared::world::lamps() {
        commands.spawn((
            RoomLamp,
            PointLight { intensity: 300_000.0, range: 14.0, color: Color::srgb(1.0, 0.78, 0.5), ..default() },
            Transform::from_xyz(x, shared::world::LAMP_Y, z),
        ));
    }
    commands.spawn((
        DirectionalLight {
            illuminance: 1_500.0,
            color: Color::srgb(0.6, 0.7, 1.0),
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(5.0, 20.0, 30.0).looking_at(Vec3::new(0.0, 0.0, 20.0), Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        // No lookup-table tonemapper: the LUT ships as zstd KTX2 and failed to
        // decompress in WebKit.
        Tonemapping::Reinhard,
        bevy::camera::Hdr,
        bevy::post_process::bloom::Bloom::NATURAL,
        bevy::post_process::effect_stack::Vignette { intensity: 0.25, radius: 0.9, ..default() },
        bevy::post_process::effect_stack::ChromaticAberration { intensity: 0.0, ..default() },
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
    look.yaw -= motion.delta.x * sens;
    look.pitch = (look.pitch - motion.delta.y * sens).clamp(-1.4, 1.4);
    let axis =
        |pos: KeyCode, neg: KeyCode| f32::from(u8::from(keys.pressed(pos))) - f32::from(u8::from(keys.pressed(neg)));
    let mut mv = Vec2::new(axis(KeyCode::KeyD, KeyCode::KeyA), axis(KeyCode::KeyW, KeyCode::KeyS));
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
        (KeyCode::ShiftLeft, buttons::SPRINT),
        (KeyCode::Space, buttons::JUMP),
        (KeyCode::ControlLeft, buttons::CROUCH),
        (KeyCode::KeyE, buttons::INTERACT),
        (KeyCode::KeyF, buttons::THROW),
        (KeyCode::KeyQ, buttons::DROP),
        (KeyCode::KeyR, buttons::USE),
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
        let hue = (player.id % 360) as f32;
        let mut body = crate::art::figure(Color::hsl(hue, 0.65, 0.5), Color::srgb(0.95, 0.75, 0.6));
        if player.cosmetic > 0 {
            body.merge(&crate::art::hat(player.cosmetic - 1)).expect("same attributes");
        }
        let mesh = meshes.add(body);
        let material =
            materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.7, ..default() });
        commands.entity(entity).insert((Dressed, Mesh3d(mesh), MeshMaterial3d(material), Transform::default()));
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
) {
    if props.is_empty() {
        return;
    }
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
    mut cache: Local<Option<(Handle<Mesh>, Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    if customers.is_empty() {
        return;
    }
    let (body, marker, marker_mat) = cache
        .get_or_insert_with(|| {
            (
                meshes.add(crate::art::figure(Color::srgb(0.45, 0.42, 0.4), Color::srgb(0.85, 0.68, 0.55))),
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
        // Muted tints, so players stand out; eight shades share materials.
        let hue = (c.id % 8 * 45) as f32;
        let mat = materials.add(StandardMaterial { base_color: Color::hsl(hue, 0.25, 0.8), ..default() });
        commands
            .entity(entity)
            .insert((DressedCustomer, Mesh3d(body.clone()), MeshMaterial3d(mat), Transform::default()))
            .with_child((
                OrderMarker,
                Mesh3d(marker.clone()),
                MeshMaterial3d(marker_mat.clone()),
                Transform::from_xyz(0.0, 1.15, 0.0),
                Visibility::Hidden,
            ));
    }
}

fn place_customers(
    mut customers: Query<(&Customer, &NpcPose, &mut Transform, &Children), With<DressedCustomer>>,
    mut markers: Query<&mut Visibility, With<OrderMarker>>,
) {
    for (c, pose, mut t, children) in &mut customers {
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
    // Third person, behind and above the player for the gray-box phase.
    let back = Quat::from_rotation_y(look.yaw) * Vec3::new(0.0, 0.0, 3.5);
    let mut eye = pos.0 + Vec3::Y * 2.2 + back;
    // Stay inside the room: behind a player near a wall, the camera would
    // otherwise see the wall's outside.
    if let Some(room) = shared::world::room_at(pos.0.x, pos.0.z) {
        let a =
            shared::world::area_of(if room == shared::world::Room::Office { shared::world::Room::Bar } else { room });
        eye.x = eye.x.clamp(a.x0 + 0.3, a.x1 - 0.3);
        eye.z = eye.z.clamp(a.z0 + 0.3, a.z1 - 0.3);
    }
    *t = Transform::from_translation(eye).looking_at(pos.0 + Vec3::Y * 1.2, Vec3::Y);
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

/// The Low preset: no shadows, no bloom.
fn graphics_preset(
    mut commands: Commands,
    settings: Res<Settings>,
    cams: Query<(Entity, Has<bevy::post_process::bloom::Bloom>), With<Camera3d>>,
    mut suns: Query<&mut DirectionalLight>,
) {
    if !settings.is_changed() {
        return;
    }
    for (cam, has_bloom) in &cams {
        match (settings.low_graphics, has_bloom) {
            (true, true) => {
                commands.entity(cam).remove::<bevy::post_process::bloom::Bloom>();
            }
            (false, false) => {
                commands.entity(cam).insert(bevy::post_process::bloom::Bloom::NATURAL);
            }
            _ => {}
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
