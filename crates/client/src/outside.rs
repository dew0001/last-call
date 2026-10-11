//! The world past the fences: open water to the horizon, a harbor town's
//! lit skyline across it, the bar's own building rising over the lot with
//! windows and a big sign, side streets with parked cars, and pilings under
//! the pier. Nothing here collides or changes the game: it is all one-draw
//! merged meshes, so it costs a handful of draw calls.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use shared::bar;
use shared::world::{AREAS, Room, area_of};

use crate::art;
use crate::look::{self, Surface};
use crate::online::NoDraw;

/// Water level, under the floors (which sit at 0).
const SEA: f32 = -0.6;
/// Top of the bar's building, over the 3.2 m rooms.
const ROOF_TOP: f32 = 6.4;

/// A tiny deterministic random sequence (the same skyline every time).
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1 << 24) as f32
    }
}

/// Windows for the skyline: dark walls, a grid of windows, about a third lit
/// warm or cold. One texture tile is 16 m wide and 16 m tall.
fn window_texture() -> Image {
    const W: u32 = 32;
    const H: u32 = 32;
    let mut rng = Lcg(7);
    let mut data = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            data[i..i + 4].copy_from_slice(&[10, 12, 18, 255]);
        }
    }
    // Windows: 2 px wide and 2 px tall, every 4 px across and 4 px down.
    for wy in 0..H / 4 {
        for wx in 0..W / 4 {
            let r = rng.next();
            let c: [u8; 4] = if r < 0.22 {
                [255, 196, 110, 255]
            } else if r < 0.32 {
                [170, 205, 255, 255]
            } else {
                [22, 26, 36, 255]
            };
            for dy in 0..2 {
                for dx in 0..2 {
                    let (x, y) = (wx * 4 + 1 + dx, wy * 4 + 1 + dy);
                    let i = ((y * W + x) * 4) as usize;
                    data[i..i + 4].copy_from_slice(&c);
                }
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    img
}

pub(crate) fn setup_outside(
    mut commands: Commands,
    assets: Option<Res<AssetServer>>,
    nodraw: Option<Res<NoDraw>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(assets) = assets.filter(|_| nodraw.is_none()) else { return };
    let mut rng = Lcg(42);

    // The sea, out to the horizon.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(450.0)).mesh().build())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.004, 0.012, 0.03),
            perceptual_roughness: 0.3,
            reflectance: 0.15,
            ..default()
        })),
        Transform::from_xyz(0.0, SEA, 0.0),
        bevy::light::NotShadowCaster,
    ));

    // The land the bar stands on: a concrete quay under every area but the
    // pier, plus the side streets, down to the water.
    let pier = area_of(Room::Pier);
    let (mut x0, mut x1, mut z0, mut z1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for a in AREAS.iter().filter(|a| a.room != Room::Pier) {
        (x0, x1, z0, z1) = (x0.min(a.x0), x1.max(a.x1), z0.min(a.z0), z1.max(a.z1));
    }
    let (x0, x1) = (x0.min(-STREET_OUT) - 1.0, x1.max(STREET_OUT) + 1.0);
    let quay = art::part(
        Cuboid::new(x1 - x0, 1.2, z1 - z0 + 2.0),
        Transform::from_xyz((x0 + x1) / 2.0, -0.62, (z0 + z1) / 2.0 - 1.0),
        Color::WHITE,
    );
    let mut quay = quay;
    look::world_uv(&mut quay, Surface::LowWall.tile());
    commands.spawn((
        Mesh3d(meshes.add(quay)),
        MeshMaterial3d(materials.add(look::material(&assets, Surface::LowWall, false))),
        Transform::default(),
    ));

    // Pilings under the pier's edges.
    let mut piles = Vec::new();
    let mut z = pier.z0 + 1.0;
    while z < pier.z1 {
        for x in [pier.x0 + 0.2, pier.x1 - 0.2] {
            piles.push(art::part(
                Cylinder::new(0.18, 2.0).mesh().resolution(8).build(),
                Transform::from_xyz(x, -0.95, z),
                Color::srgb(0.22, 0.16, 0.11),
            ));
        }
        z += 3.0;
    }
    piles.push(art::part(
        Cuboid::new(pier.x1 - pier.x0, 0.3, pier.z1 - pier.z0),
        Transform::from_xyz((pier.x0 + pier.x1) / 2.0, -0.16, (pier.z0 + pier.z1) / 2.0),
        Color::srgb(0.25, 0.18, 0.12),
    ));
    let painted = materials.add(StandardMaterial { perceptual_roughness: 0.8, ..default() });
    commands.spawn((Mesh3d(meshes.add(art::combine(piles))), MeshMaterial3d(painted.clone()), Transform::default()));

    // The skyline: a ring of towers across the water, lit windows, unlit
    // (they glow at night and cost no lighting).
    let mut towers = Vec::new();
    let n = 64;
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU + rng.next() * 0.05;
        let r = 170.0 + rng.next() * 90.0;
        let (w, d, h) = (14.0 + rng.next() * 22.0, 14.0 + rng.next() * 18.0, 12.0 + rng.next().powi(2) * 70.0);
        // Snap heights to whole floors of the texture (4 m) so windows line up.
        let h = (h / 4.0).round() * 4.0;
        let m = art::part(
            Cuboid::new(w, h, d),
            Transform::from_xyz(a.sin() * r, SEA + h / 2.0, a.cos() * r).with_rotation(Quat::from_rotation_y(a)),
            Color::WHITE,
        );
        towers.push(m);
    }
    let mut towers = art::combine(towers);
    look::world_uv(&mut towers, 16.0);
    let windows = images.add(window_texture());
    commands.spawn((
        Mesh3d(meshes.add(towers)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(windows),
            unlit: true,
            fog_enabled: false,
            ..default()
        })),
        Transform::default(),
        bevy::light::NotShadowCaster,
    ));

    // The bar's building above its rooms: brick up to the roof line on every
    // outer wall, a concrete parapet, and lit windows over the lot.
    let (hx, top) = (bar::HALF_X, ROOF_TOP);
    let rise = top - bar::WALL_HEIGHT;
    let mid = bar::WALL_HEIGHT + rise / 2.0;
    let front = bar::HALF_Z;
    let back = area_of(Room::Kitchen).z0;
    let shell: [(f32, f32, f32, f32); 6] = [
        // (x0, x1, z0, z1) of each wall run.
        (-hx - 0.2, hx + 0.2, front - 0.2, front + 0.2),
        (-hx - 0.2, -hx + 0.2, back, front),
        (hx - 0.2, hx + 0.2, -bar::HALF_Z, front),
        (-hx - 0.2, area_of(Room::BackRoom).x1 + 0.2, back - 0.2, back + 0.2),
        (area_of(Room::BackRoom).x1 - 0.2, area_of(Room::BackRoom).x1 + 0.2, back, -bar::HALF_Z),
        (area_of(Room::BackRoom).x1, hx + 0.2, -bar::HALF_Z - 0.2, -bar::HALF_Z + 0.2),
    ];
    let mut brick = Vec::new();
    let mut cap = Vec::new();
    for (a, b, c, d) in shell {
        let (cx, cz, w, dd) = ((a + b) / 2.0, (c + d) / 2.0, b - a, d - c);
        brick.push(art::part(Cuboid::new(w, rise, dd), Transform::from_xyz(cx, mid, cz), Color::WHITE));
        cap.push(art::part(Cuboid::new(w + 0.2, 0.2, dd + 0.2), Transform::from_xyz(cx, top + 0.1, cz), Color::WHITE));
    }
    // A flat roof over it all (seen from the bar's own roof deck next door).
    cap.push(art::part(
        Cuboid::new(hx * 2.0, 0.1, front - back),
        Transform::from_xyz(0.0, top - 0.3, (front + back) / 2.0),
        Color::WHITE,
    ));
    for (parts, surface) in [(brick, Surface::OutsideWall), (cap, Surface::LowWall)] {
        let mut m = art::combine(parts);
        look::world_uv(&mut m, surface.tile());
        commands.spawn((
            Mesh3d(meshes.add(m)),
            MeshMaterial3d(materials.add(look::material(&assets, surface, false))),
            Transform::default(),
        ));
    }
    // Upstairs windows over the lot: frames, warm glass (two left dark).
    let mut frames = Vec::new();
    let (mut lit, mut dark) = (Vec::new(), Vec::new());
    for (i, x) in [-8.0, -5.0, 5.0, 8.0].into_iter().enumerate() {
        frames.push(art::part(
            Cuboid::new(1.3, 1.5, 0.08),
            Transform::from_xyz(x, 4.4, front + 0.22),
            Color::srgb(0.12, 0.1, 0.09),
        ));
        let pane = art::part(Cuboid::new(1.1, 1.3, 0.04), Transform::from_xyz(x, 4.4, front + 0.25), Color::WHITE);
        if i == 1 { dark.push(pane) } else { lit.push(pane) }
    }
    commands.spawn((Mesh3d(meshes.add(art::combine(frames))), MeshMaterial3d(painted.clone()), Transform::default()));
    for (panes, base, glow) in [
        (lit, Color::srgb(1.0, 0.55, 0.2), LinearRgba::rgb(4.0, 1.9, 0.6)),
        (dark, Color::srgb(0.03, 0.035, 0.05), LinearRgba::BLACK),
    ] {
        commands.spawn((
            Mesh3d(meshes.add(art::combine(panes))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: base,
                emissive: glow,
                perceptual_roughness: 0.1,
                ..default()
            })),
            Transform::default(),
        ));
    }
    // The big sign over the door: LAST CALL in pink neon across the facade.
    commands.spawn((
        Mesh3d(meshes.add(art::neon_text("LAST CALL", 0.9, Color::WHITE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.3, 0.7),
            emissive: LinearRgba::rgb(9.0, 0.9, 4.5),
            ..default()
        })),
        Transform::from_xyz(0.0, 5.3, front + 0.3),
    ));

    // Side streets past the lot's fences, with cars parked along them.
    let lot = area_of(Room::ParkingLot);
    let (sz0, sz1) = (lot.z0 + 2.5, lot.z1);
    let mut road = Vec::new();
    for x in [-(STREET_IN + STREET_OUT) / 2.0, (STREET_IN + STREET_OUT) / 2.0] {
        road.push(art::part(
            Plane3d::new(Vec3::Y, Vec2::new((STREET_OUT - STREET_IN) / 2.0, (sz1 - sz0) / 2.0)).mesh().build(),
            Transform::from_xyz(x, 0.002, (sz0 + sz1) / 2.0),
            Color::WHITE,
        ));
    }
    let mut road = art::combine(road);
    look::world_uv(&mut road, Surface::Asphalt.tile());
    commands.spawn((
        Mesh3d(meshes.add(road)),
        MeshMaterial3d(materials.add(look::material(&assets, Surface::Asphalt, false))),
        Transform::default(),
    ));
    // KayKit cars, parked along the kerbs (toy-sized models: 4.8x is a
    // 4.5 m car).
    let models = ["car_sedan", "car_hatchback", "car_taxi", "car_stationwagon", "car_sedan", "car_police"];
    let mut k = 0;
    for side in [-1.0f32, 1.0] {
        let mut z = sz0 + 2.5;
        while z < sz1 - 2.5 {
            if rng.next() < 0.75 {
                let flip = if rng.next() < 0.5 { 0.0 } else { std::f32::consts::PI };
                let name = models[k % models.len()];
                let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("kk/city/{name}.gltf")));
                commands.spawn((
                    crate::look::Far(45.0),
                    Visibility::default(),
                    bevy::world_serialization::WorldAssetRoot(scene),
                    Transform::from_xyz(side * (STREET_IN + 1.4), 0.3, z)
                        .with_rotation(Quat::from_rotation_y(flip + (rng.next() - 0.5) * 0.06))
                        .with_scale(Vec3::splat(4.8)),
                ));
                k += 1;
            }
            z += 5.2;
        }
    }
}

/// The side streets run from just past the lot's fences out to here (|x|, m).
const STREET_IN: f32 = 14.3;
const STREET_OUT: f32 = 22.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skyline_texture_has_lit_and_dark_windows() {
        let img = window_texture();
        let px = img.data.as_ref().expect("pixels");
        let warm = px.chunks(4).filter(|p| p[0] == 255).count();
        let dark = px.chunks(4).filter(|p| p[0] == 22).count();
        assert!(warm > 0 && dark > warm, "{warm} lit, {dark} dark");
    }
}
