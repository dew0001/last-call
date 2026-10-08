//! LAST CALL client. Phase 0: one lit, spinning cube on a dark bar-amber
//! background, plus a status bridge that tells the page which GPU backend runs.

use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;

#[cfg(target_arch = "wasm32")]
mod web;

/// Background color: dark amber, so screenshots can tell the clear color from the cube.
pub const CLEAR_COLOR: Color = Color::srgb(0.08, 0.05, 0.03);

/// Frames rendered and the backend in use. Mirrored to `window.__lastCall` on the web.
#[derive(Resource, Default, Debug, Clone)]
pub struct RenderStatus {
    pub frames: u64,
    pub backend: String,
}

#[derive(Component)]
struct Spinner;

/// Build the client app.
pub fn build_app() -> App {
    let mut app = App::new();
    app.insert_resource(ClearColor(CLEAR_COLOR))
        .init_resource::<RenderStatus>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "LAST CALL".into(),
                canvas: Some("#bevy".into()),
                fit_canvas_to_parent: true,
                prevent_default_event_handling: true,
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup_scene)
        .add_systems(Update, (spin, track_status));
    app
}

fn setup_scene(
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
    commands.spawn((Camera3d::default(), Transform::from_xyz(0.0, 1.5, 4.5).looking_at(Vec3::ZERO, Vec3::Y)));
}

fn spin(time: Res<Time>, mut q: Query<&mut Transform, With<Spinner>>) {
    for mut t in &mut q {
        t.rotate_y(time.delta_secs() * 0.8);
        t.rotate_x(time.delta_secs() * 0.3);
    }
}

fn track_status(mut status: ResMut<RenderStatus>, adapter: Option<Res<RenderAdapterInfo>>) {
    status.frames += 1;
    if status.backend.is_empty()
        && let Some(info) = adapter
    {
        status.backend = format!("{:?}", info.backend).to_lowercase();
        info!("render backend: {} ({})", status.backend, info.name);
    }
    #[cfg(target_arch = "wasm32")]
    web::publish_status(&status);
}
