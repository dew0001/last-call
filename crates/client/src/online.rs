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
    app.init_resource::<Look>().init_resource::<ScriptedInput>().init_resource::<NetStatus>();

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
            follow_camera,
            update_status,
            update_hud,
            update_pour_gauge,
            update_drunk_text,
        )
            .chain(),
    );
}

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
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let gray = |v: f32| Color::srgb(v, v * 0.92, v * 0.85);
    let mut solid = |commands: &mut Commands, size: Vec3, at: Vec3, color: Color| {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.9,
                ..default()
            })),
            Transform::from_translation(at),
        ));
    };
    use shared::world::{AREAS, Room};
    // A floor per room, walls along their edges (door gaps open).
    for a in AREAS.iter().filter(|a| a.room != Room::Office) {
        let color = match a.room {
            Room::ParkingLot => Color::srgb(0.12, 0.12, 0.14),
            Room::Pier => Color::srgb(0.32, 0.22, 0.13),
            Room::Roof => Color::srgb(0.18, 0.17, 0.17),
            Room::Basement | Room::Stairwell => Color::srgb(0.25, 0.24, 0.22),
            Room::Kitchen => Color::srgb(0.55, 0.55, 0.5),
            _ => gray(0.35),
        };
        let (w, d) = (a.x1 - a.x0, a.z1 - a.z0);
        solid(&mut commands, Vec3::new(w, 0.1, d), Vec3::new(a.x0 + w / 2.0, -0.05, a.z0 + d / 2.0), color);
    }
    for b in shared::world::walls() {
        let color = if b.height < bar::WALL_HEIGHT { gray(0.3) } else { gray(0.45) };
        solid(&mut commands, Vec3::new(b.hx * 2.0, b.height, b.hz * 2.0), Vec3::new(b.cx, b.height / 2.0, b.cz), color);
    }
    // The harbor around the pier.
    let pier = shared::world::area_of(Room::Pier);
    solid(
        &mut commands,
        Vec3::new(60.0, 0.05, pier.z1 - pier.z0 + 10.0),
        Vec3::new(0.0, -0.6, (pier.z0 + pier.z1) / 2.0 + 5.0),
        Color::srgb(0.05, 0.15, 0.3),
    );
    let hz = bar::HALF_Z;
    // Counter, office walls, safe.
    for b in &bar::BLOCKS {
        let color = match b.kind {
            bar::BlockKind::Counter => Color::srgb(0.45, 0.28, 0.15),
            bar::BlockKind::Wall => gray(0.42),
            bar::BlockKind::Safe => Color::srgb(0.2, 0.22, 0.25),
            bar::BlockKind::Table => Color::srgb(0.3, 0.18, 0.1),
            bar::BlockKind::SlotMachine => Color::srgb(0.55, 0.1, 0.45),
        };
        solid(&mut commands, Vec3::new(b.hx * 2.0, b.height, b.hz * 2.0), Vec3::new(b.cx, b.height / 2.0, b.cz), color);
    }
    // Neon sign over the counter (pink accent).
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.0, 0.6, 0.1))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.2, 0.6),
            emissive: LinearRgba::rgb(2.0, 0.2, 1.0),
            ..default()
        })),
        Transform::from_xyz(0.0, 2.4, -hz + 0.05),
    ));
    // Warm lights in the bar, one in each other indoor room; a cold moon outside.
    let mut lamps: Vec<(f32, f32)> = vec![(-6.0, 0.0), (0.0, 0.0), (6.0, 0.0)];
    for a in AREAS.iter().filter(|a| !a.room.outdoors() && a.room != Room::Bar) {
        lamps.push(((a.x0 + a.x1) / 2.0, (a.z0 + a.z1) / 2.0));
    }
    for (x, z) in lamps {
        commands.spawn((
            RoomLamp,
            PointLight { intensity: 400_000.0, range: 14.0, color: Color::srgb(1.0, 0.8, 0.55), ..default() },
            Transform::from_xyz(x, 2.9, z),
        ));
    }
    commands.spawn((
        DirectionalLight { illuminance: 1_500.0, color: Color::srgb(0.6, 0.7, 1.0), ..default() },
        Transform::from_xyz(5.0, 20.0, 30.0).looking_at(Vec3::new(0.0, 0.0, 20.0), Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        // No lookup-table tonemapper: the LUT ships as zstd KTX2 and failed to
        // decompress in WebKit. Post-processing returns in Phase 6.
        Tonemapping::Reinhard,
        Transform::from_xyz(0.0, 6.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// An indoor lamp (the power outage turns these off).
#[derive(Component)]
pub struct RoomLamp;

fn read_input(
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scripted: Res<ScriptedInput>,
    mut look: ResMut<Look>,
    mut input: ResMut<LocalInput>,
) {
    if let Some(s) = scripted.0 {
        input.0 = s;
        look.yaw = s.yaw();
        return;
    }
    const SENSITIVITY: f32 = 0.0025;
    look.yaw -= motion.delta.x * SENSITIVITY;
    look.pitch = (look.pitch - motion.delta.y * SENSITIVITY).clamp(-1.4, 1.4);
    let axis =
        |pos: KeyCode, neg: KeyCode| f32::from(u8::from(keys.pressed(pos))) - f32::from(u8::from(keys.pressed(neg)));
    let mv = Vec2::new(axis(KeyCode::KeyD, KeyCode::KeyA), axis(KeyCode::KeyW, KeyCode::KeyS));
    let mut b = 0;
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
        let mesh = meshes.add(Capsule3d::new(bar::PLAYER_RADIUS, bar::PLAYER_HEIGHT - 2.0 * bar::PLAYER_RADIUS));
        let material = materials.add(StandardMaterial { base_color: Color::hsl(hue, 0.6, 0.55), ..default() });
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
        let mut mat =
            |c: Color| materials.add(StandardMaterial { base_color: c, perceptual_roughness: 0.6, ..default() });
        let mop = (meshes.add(Cuboid::new(0.06, 1.3, 0.06)), mat(Color::srgb(0.55, 0.5, 0.42)));
        [
            (meshes.add(Cylinder::new(0.04, 0.28)), mat(Color::srgb(0.2, 0.55, 0.25))),
            (meshes.add(Cylinder::new(0.02, 0.012)), mat(Color::srgb(0.85, 0.15, 0.15))),
            (meshes.add(Cuboid::new(0.4, 0.75, 0.4)), mat(Color::srgb(0.4, 0.25, 0.12))),
            (
                meshes.add(Cylinder::new(0.045, 0.15)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.95, 0.65, 0.15),
                    emissive: LinearRgba::rgb(0.3, 0.18, 0.02),
                    perceptual_roughness: 0.2,
                    ..default()
                }),
            ),
            mop,
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
                meshes.add(Capsule3d::new(0.3, bar::PLAYER_HEIGHT - 0.6)),
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
        let hue = (c.id * 47 % 360) as f32;
        let mat = materials.add(StandardMaterial { base_color: Color::hsl(hue, 0.25, 0.45), ..default() });
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
