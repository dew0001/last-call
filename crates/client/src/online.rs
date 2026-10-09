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
    app.add_systems(Startup, (setup_bar, setup_hud));
    app.add_systems(
        Update,
        (read_input, dress_players, place_players, dress_props, place_props, follow_camera, update_status, update_hud)
            .chain(),
    );
}

/// The shift clock line at the top of the screen. A wall clock replaces it
/// in the art pass (plan section 7: diegetic UI).
#[derive(Component)]
struct ClockText;

fn setup_hud(mut commands: Commands, nodraw: Option<Res<NoDraw>>) {
    if nodraw.is_some() {
        return;
    }
    commands.spawn((
        ClockText,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(18.0), ..default() },
        TextColor(Color::srgb(1.0, 0.85, 0.55)),
        Node { position_type: PositionType::Absolute, top: px(8), left: px(10), ..default() },
    ));
}

fn update_hud(status: Res<NetStatus>, mut text: Query<&mut Text, With<ClockText>>) {
    let Ok(mut text) = text.single_mut() else { return };
    let line = match &status.game.shift {
        Some(s) => format!(
            "Week {}  ·  Shift {}/{}  ·  {} {}{}",
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
    let (hx, hz, h) = (bar::HALF_X, bar::HALF_Z, bar::WALL_HEIGHT);
    // Floor and walls.
    solid(&mut commands, Vec3::new(hx * 2.0, 0.1, hz * 2.0), Vec3::new(0.0, -0.05, 0.0), gray(0.35));
    solid(&mut commands, Vec3::new(hx * 2.0, h, 0.2), Vec3::new(0.0, h / 2.0, -hz - 0.1), gray(0.5));
    solid(&mut commands, Vec3::new(hx * 2.0, h, 0.2), Vec3::new(0.0, h / 2.0, hz + 0.1), gray(0.5));
    solid(&mut commands, Vec3::new(0.2, h, hz * 2.0), Vec3::new(-hx - 0.1, h / 2.0, 0.0), gray(0.45));
    solid(&mut commands, Vec3::new(0.2, h, hz * 2.0), Vec3::new(hx + 0.1, h / 2.0, 0.0), gray(0.45));
    // Counter.
    let (cx, cz, chx, chz) = bar::COUNTER;
    solid(
        &mut commands,
        Vec3::new(chx * 2.0, bar::COUNTER_HEIGHT, chz * 2.0),
        Vec3::new(cx, bar::COUNTER_HEIGHT / 2.0, cz),
        Color::srgb(0.45, 0.28, 0.15),
    );
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
    // Warm lights.
    for x in [-6.0, 0.0, 6.0] {
        commands.spawn((
            PointLight { intensity: 400_000.0, range: 14.0, color: Color::srgb(1.0, 0.8, 0.55), ..default() },
            Transform::from_xyz(x, 2.9, 0.0),
        ));
    }
    commands.spawn((
        Camera3d::default(),
        // No lookup-table tonemapper: the LUT ships as zstd KTX2 and failed to
        // decompress in WebKit. Post-processing returns in Phase 6.
        Tonemapping::Reinhard,
        Transform::from_xyz(0.0, 6.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

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
    ] {
        if keys.pressed(key) {
            b |= bit;
        }
    }
    input.0 = PlayerInput::new(mv, look.yaw, look.pitch, b);
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

fn place_players(mut q: Query<(&PlayerPos, &PlayerYaw, &mut Transform), With<Dressed>>) {
    for (pos, yaw, mut t) in &mut q {
        t.translation = pos.0 + Vec3::Y * (bar::PLAYER_HEIGHT / 2.0);
        t.rotation = Quat::from_rotation_y(yaw.0);
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
    mut cache: Local<Option<[(Handle<Mesh>, Handle<StandardMaterial>); 3]>>,
) {
    if props.is_empty() {
        return;
    }
    let looks = cache.get_or_insert_with(|| {
        let mut mat =
            |c: Color| materials.add(StandardMaterial { base_color: c, perceptual_roughness: 0.6, ..default() });
        [
            (meshes.add(Cylinder::new(0.04, 0.28)), mat(Color::srgb(0.2, 0.55, 0.25))),
            (meshes.add(Cylinder::new(0.02, 0.012)), mat(Color::srgb(0.85, 0.15, 0.15))),
            (meshes.add(Cuboid::new(0.4, 0.75, 0.4)), mat(Color::srgb(0.4, 0.25, 0.12))),
        ]
    });
    for (entity, kind) in &props {
        let (mesh, mat) = &looks[match kind {
            PropKind::Bottle => 0,
            PropKind::Chip => 1,
            PropKind::Stool => 2,
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

fn follow_camera(
    look: Res<Look>,
    own: Query<&PlayerPos, (With<Predicted>, With<Player>)>,
    mut cam: Query<&mut Transform, (With<Camera3d>, Without<Player>)>,
) {
    let (Ok(pos), Ok(mut t)) = (own.single(), cam.single_mut()) else { return };
    // Third person, behind and above the player for the gray-box phase.
    let back = Quat::from_rotation_y(look.yaw) * Vec3::new(0.0, 0.0, 3.5);
    let eye = pos.0 + Vec3::Y * 2.2 + back;
    *t = Transform::from_translation(eye).looking_at(pos.0 + Vec3::Y * 1.2, Vec3::Y);
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
    room: Query<&ShiftClock, With<RoomState>>,
    mut status: ResMut<NetStatus>,
) {
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
        game: GameStatus { shift: room.iter().next().map(|c| (*c).into()) },
    };
}
