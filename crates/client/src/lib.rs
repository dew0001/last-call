//! LAST CALL client.
//!
//! Offline (no room): a spinning neon cube as the title backdrop.
//! Online: the lightyear client, the gray-box bar, every player as a capsule
//! (own player predicted, others interpolated), WASD plus mouse look.
//! On the web, `web.rs` bridges packets to `web/net.js` and mirrors status
//! into `window.__lastCall` for the page and the tests.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;

pub mod art;
pub mod casino;
pub mod chaos;
pub mod games;
pub mod look;
pub mod online;
#[cfg(target_arch = "wasm32")]
mod web;

/// Background color: dark amber, so screenshots can tell the clear color from the cube.
pub const CLEAR_COLOR: Color = Color::srgb(0.08, 0.05, 0.03);

/// Frames rendered and the backend in use. Mirrored to `window.__lastCall` on the web.
#[derive(Resource, Default, Debug, Clone)]
pub struct RenderStatus {
    pub frames: u64,
    pub backend: String,
    /// Budget proxies (plan section 9): meshes drawn this frame (an upper
    /// bound on draw calls, before batching) and their triangles.
    pub visible_meshes: u32,
    pub triangles: u32,
}

#[derive(Component)]
struct Spinner;

/// What the client shows.
pub enum Mode {
    /// Join a room.
    Online(online::OnlineConfig),
    /// The title screen: the bar, empty, with a slow camera drift, or a
    /// fixed camera (x, y, z, yaw) for screenshots of the map (`?cam=`).
    Title(Option<[f32; 4]>),
    /// A spinning cube: the renderer check (tests, `?cube`).
    Cube,
}

/// Build the client app.
pub fn build_app(mode: Mode) -> App {
    let mut app = App::new();
    app.insert_resource(ClearColor(CLEAR_COLOR))
        .init_resource::<RenderStatus>()
        .init_resource::<online::Settings>()
        .add_plugins(
            DefaultPlugins
                .set(bevy::asset::AssetPlugin {
                    // No `.meta` files ship; do not ask the server for them.
                    meta_check: bevy::asset::AssetMetaCheck::Never,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "LAST CALL".into(),
                        canvas: Some("#bevy".into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_systems(Update, track_status)
        .add_systems(
            PostUpdate,
            count_visible
                .run_if(online::every_third_frame)
                .after(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        );
    match mode {
        Mode::Online(cfg) => {
            online::add(&mut app, cfg);
        }
        Mode::Title(cam) => {
            app.insert_resource(TitleCam(cam))
                .add_systems(Startup, online::setup_bar)
                .add_systems(Update, (title_camera, online::graphics_preset));
        }
        Mode::Cube => {
            app.add_systems(Startup, setup_backdrop).add_systems(Update, spin);
        }
    }
    app
}

fn setup_backdrop(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.6, 1.6, 1.6))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.2, 0.6),
            emissive: LinearRgba::rgb(0.6, 0.05, 0.3),
            ..default()
        })),
        Transform::default(),
        Spinner,
    ));
    commands
        .spawn((PointLight { intensity: 2_000_000.0, range: 30.0, ..default() }, Transform::from_xyz(3.0, 5.0, 4.0)));
    commands.spawn((
        Camera3d::default(),
        // No lookup-table tonemapper: the LUT ships as zstd KTX2 and failed to
        // decompress in WebKit. Post-processing returns in Phase 6.
        Tonemapping::Reinhard,
        Transform::from_xyz(0.0, 1.5, 4.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

#[derive(Resource)]
struct TitleCam(Option<[f32; 4]>);

/// The title screen's camera drifts slowly along the bar.
fn title_camera(time: Res<Time>, fixed: Res<TitleCam>, mut cam: Query<&mut Transform, With<Camera3d>>) {
    let Ok(mut t) = cam.single_mut() else { return };
    if let Some([x, y, z, yaw]) = fixed.0 {
        *t = Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_y(yaw));
        return;
    }
    let s = time.elapsed_secs() * 0.06;
    let eye = Vec3::new(6.5 * s.sin(), 1.9 + 0.2 * (s * 1.7).sin(), 3.2 + 1.2 * s.cos());
    *t = Transform::from_translation(eye).looking_at(Vec3::new(0.8 * s.sin(), 1.3, -4.5), Vec3::Y);
}

fn spin(time: Res<Time>, mut q: Query<&mut Transform, With<Spinner>>) {
    for mut t in &mut q {
        t.rotate_y(time.delta_secs() * 0.8);
        t.rotate_x(time.delta_secs() * 0.3);
    }
}

/// Count what the camera draws: visible meshes and their triangles.
fn count_visible(
    mut status: ResMut<RenderStatus>,
    meshes: Res<Assets<Mesh>>,
    visible: Query<(&Mesh3d, &ViewVisibility)>,
    mut cache: Local<std::collections::HashMap<AssetId<Mesh>, u32>>,
) {
    let (mut n, mut tris) = (0, 0);
    for (m, v) in &visible {
        if !v.get() {
            continue;
        }
        n += 1;
        tris += *cache.entry(m.0.id()).or_insert_with(|| meshes.get(&m.0).map_or(0, |m| art::triangles(m) as u32));
    }
    status.visible_meshes = n;
    status.triangles = tris;
}

fn track_status(mut status: ResMut<RenderStatus>, adapter: Option<Res<RenderAdapterInfo>>) {
    status.frames += 1;
    if status.backend.is_empty()
        && let Some(info) = adapter
    {
        status.backend = format!("{:?}", info.backend).to_lowercase();
        info!("render backend: {} ({})", status.backend, info.name);
    }
}
