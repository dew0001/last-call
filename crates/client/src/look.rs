//! Surfaces and set dressing in one cartoony style: stylized textures made
//! in code (planks, bricks, tiles), KayKit CC0 furniture placed by
//! `shared::layout`, and small props built in code (`art.rs`).
//!
//! Static geometry gets texture coordinates from its world position (box
//! mapping: each face takes the two axes it spans), so a texture keeps the
//! same scale on every wall and floor however the blocks are cut.

use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use shared::world::Room;

/// A textured surface kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Surface {
    BarFloor,
    OfficeFloor,
    KitchenFloor,
    BackRoomFloor,
    ConcreteFloor,
    Roof,
    Asphalt,
    Deck,
    BarWall,
    OfficeWall,
    KitchenWall,
    BackRoomWall,
    BasementWall,
    OutsideWall,
    LowWall,
    Wood,
    Metal,
    Leather,
    Ceiling,
}

impl Surface {
    /// Metres one texture repeat covers.
    pub fn tile(self) -> f32 {
        match self {
            Surface::KitchenFloor | Surface::KitchenWall => 1.5,
            Surface::Wood | Surface::Metal | Surface::Leather => 1.0,
            Surface::Asphalt | Surface::Roof => 4.0,
            _ => 2.5,
        }
    }

    /// The floor of a room.
    pub fn floor(room: Room) -> Surface {
        match room {
            Room::Bar => Surface::BarFloor,
            Room::Office => Surface::OfficeFloor,
            Room::Kitchen => Surface::KitchenFloor,
            Room::BackRoom => Surface::BackRoomFloor,
            Room::Stairwell | Room::Basement => Surface::ConcreteFloor,
            Room::Roof => Surface::Roof,
            Room::ParkingLot => Surface::Asphalt,
            Room::Pier => Surface::Deck,
        }
    }

    /// The walls of a room (None: outdoors).
    pub fn wall(room: Option<Room>) -> Surface {
        match room {
            Some(Room::Bar) => Surface::BarWall,
            Some(Room::Office) => Surface::OfficeWall,
            Some(Room::Kitchen) => Surface::KitchenWall,
            Some(Room::BackRoom) => Surface::BackRoomWall,
            Some(Room::Stairwell | Room::Basement) => Surface::BasementWall,
            _ => Surface::OutsideWall,
        }
    }
}

/// A small integer hash: the same noise every run.
fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ seed.wrapping_mul(2_147_483_647);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

type Rgb = [f32; 3];

fn shade(c: Rgb, k: f32) -> Rgb {
    [c[0] * k, c[1] * k, c[2] * k]
}

/// Texture size (square, pixels).
const TEX: u32 = 128;

/// Boards running along u: `n` boards per repeat, each its own shade, with
/// dark seams and a little grain.
fn boards(u: u32, v: u32, n: u32, base: Rgb, spread: f32, seam: Rgb, seed: u32) -> Rgb {
    let h = TEX / n;
    let row = v / h;
    // Boards end at a staggered joint along their length.
    let joint = (hash(row, 0, seed) * TEX as f32) as u32;
    let piece = u32::from((u + joint) % TEX < TEX / 2);
    if v.is_multiple_of(h) || (u + joint).is_multiple_of(TEX / 2) {
        return seam;
    }
    let k = 1.0 + (hash(row, piece, seed) - 0.5) * spread;
    let grain = 1.0 + (hash(u / 6, v, seed + 1) - 0.5) * 0.06;
    shade(base, k * grain)
}

/// Bricks or blocks: `rows` courses, `cols` per course, every other course
/// offset by half, with mortar lines.
fn bricks(u: u32, v: u32, rows: u32, cols: u32, base: Rgb, spread: f32, mortar: Rgb, seed: u32) -> Rgb {
    let (h, w) = (TEX / rows, TEX / cols);
    let row = v / h;
    let uu = (u + if row % 2 == 1 { w / 2 } else { 0 }) % TEX;
    if v % h < 2 || uu % w < 2 {
        return mortar;
    }
    let k = 1.0 + (hash(uu / w, row, seed) - 0.5) * spread;
    let speck = 1.0 + (hash(u, v, seed + 3) - 0.5) * 0.08;
    shade(base, k * speck)
}

/// Flat color with speckle.
fn speckle(u: u32, v: u32, base: Rgb, amount: f32, seed: u32) -> Rgb {
    let blotch = 1.0 + (hash(u / 16, v / 16, seed) - 0.5) * amount * 0.6;
    shade(base, blotch * (1.0 + (hash(u, v, seed + 7) - 0.5) * amount))
}

fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    [f32::from(r), f32::from(g), f32::from(b)]
}

/// The stylized texture for a surface: flat colors and simple patterns, to
/// match the cartoony KayKit furniture.
fn texture_image(surface: Surface) -> Image {
    let mut data = Vec::with_capacity((TEX * TEX * 4) as usize);
    for v in 0..TEX {
        for u in 0..TEX {
            let c = match surface {
                Surface::BarFloor => boards(u, v, 6, rgb(160, 98, 52), 0.25, rgb(70, 40, 22), 1),
                Surface::OfficeFloor => boards(u, v, 8, rgb(186, 132, 80), 0.2, rgb(110, 70, 40), 2),
                Surface::Deck => boards(u, v, 5, rgb(132, 118, 102), 0.22, rgb(60, 52, 46), 3),
                Surface::Wood => boards(u, v, 4, rgb(102, 58, 32), 0.2, rgb(50, 28, 16), 4),
                Surface::Ceiling => boards(u, v, 6, rgb(92, 62, 42), 0.2, rgb(45, 30, 20), 5),
                Surface::BackRoomWall => boards(v, u, 6, rgb(112, 74, 46), 0.25, rgb(55, 36, 22), 6),
                Surface::KitchenFloor => {
                    if ((u / (TEX / 4)) + (v / (TEX / 4))).is_multiple_of(2) {
                        speckle(u, v, rgb(232, 224, 206), 0.05, 7)
                    } else {
                        speckle(u, v, rgb(52, 54, 60), 0.08, 8)
                    }
                }
                Surface::KitchenWall => bricks(u, v, 8, 4, rgb(236, 236, 230), 0.04, rgb(185, 186, 182), 9),
                Surface::BarWall => bricks(u, v, 8, 4, rgb(158, 74, 52), 0.25, rgb(208, 192, 170), 10),
                Surface::OutsideWall => bricks(u, v, 8, 4, rgb(92, 58, 52), 0.25, rgb(64, 60, 60), 11),
                Surface::BasementWall => bricks(u, v, 4, 2, rgb(132, 132, 128), 0.12, rgb(88, 88, 86), 12),
                Surface::OfficeWall => speckle(u, v, rgb(150, 178, 158), 0.04, 13),
                Surface::BackRoomFloor => speckle(u, v, rgb(122, 32, 42), 0.12, 14),
                Surface::ConcreteFloor => speckle(u, v, rgb(142, 142, 140), 0.1, 15),
                Surface::Roof => speckle(u, v, rgb(112, 112, 118), 0.18, 16),
                Surface::Asphalt => speckle(u, v, rgb(58, 60, 66), 0.16, 17),
                Surface::LowWall => speckle(u, v, rgb(168, 162, 152), 0.08, 18),
                Surface::Metal => shade(rgb(152, 157, 162), 1.0 + (hash(0, v, 19) - 0.5) * 0.12),
                Surface::Leather => speckle(u, v, rgb(96, 28, 26), 0.05, 20),
            };
            data.extend(c.iter().map(|x| x.clamp(0.0, 255.0) as u8));
            data.push(255);
        }
    }
    let mut img = Image::new(
        bevy::render::render_resource::Extent3d { width: TEX, height: TEX, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 1,
        ..default()
    });
    img
}

/// A material for a surface, its texture made once and shared. With
/// `lightmapped`, the floor's baked lightmap lights it (`tools bake_lighting`).
pub fn material(assets: &AssetServer, surface: Surface, lightmapped: bool) -> StandardMaterial {
    static CACHE: std::sync::Mutex<Vec<(Surface, Handle<Image>)>> = std::sync::Mutex::new(Vec::new());
    let texture = {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        match cache.iter().find(|(s, _)| *s == surface) {
            Some((_, h)) => h.clone(),
            None => {
                let h = assets.add(texture_image(surface));
                cache.push((surface, h.clone()));
                h
            }
        }
    };
    let (roughness, metallic) = match surface {
        Surface::KitchenFloor | Surface::KitchenWall => (0.35, 0.0),
        Surface::Metal => (0.4, 0.7),
        Surface::Leather => (0.5, 0.0),
        Surface::BarFloor | Surface::OfficeFloor | Surface::Wood => (0.55, 0.0),
        _ => (0.9, 0.0),
    };
    StandardMaterial {
        base_color_texture: Some(texture),
        perceptual_roughness: roughness,
        metallic,
        // The bake is in the plain gray-box units, scaled for these textures.
        lightmap_exposure: if lightmapped { 2_500.0 } else { 1.0 },
        ..default()
    }
}

/// Give a mesh in world space box-mapped texture coordinates (one repeat
/// per `tile` metres) and tangents for its normal map.
pub fn world_uv(mesh: &mut Mesh, tile: f32) {
    let Some(pos) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a| a.as_float3()) else { return };
    let Some(nor) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL).and_then(|a| a.as_float3()) else { return };
    let uv: Vec<[f32; 2]> = pos
        .iter()
        .zip(nor)
        .map(|(p, n)| {
            let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
            let (u, v) = if ay >= ax && ay >= az {
                (p[0], p[2])
            } else if ax >= az {
                (p[2] * n[0].signum(), -p[1])
            } else {
                (-p[0] * n[2].signum(), -p[1])
            };
            [u / tile, v / tile]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    let _ = mesh.generate_tangents();
}

use std::f32::consts::{FRAC_PI_2 as QUARTER, PI as HALF};

/// Small props built in code (art.rs), in the same flat style as the KayKit
/// furniture: what they are, where (x, y, z) and which way they face.
#[derive(Clone, Copy, Debug)]
pub enum Prop {
    Register,
    Pendant,
    Dartboard,
    Clock,
    WetFloor,
    Tv,
    Barrel,
    Aircon,
    Lifebuoy,
}

pub const PROPS: &[(Prop, [f32; 3], f32)] = &[
    (Prop::Register, [-4.2, 1.1, -4.0], HALF),
    (Prop::Pendant, [-3.4, 3.2, -4.0], 0.0),
    (Prop::Pendant, [3.4, 3.2, -4.0], 0.0),
    (Prop::Pendant, [-5.0, 3.2, 1.0], 0.0),
    (Prop::Pendant, [4.5, 3.2, 1.0], 0.0),
    (Prop::Dartboard, [9.9, 1.73, 3.0], -QUARTER),
    (Prop::Clock, [-4.0, 2.65, -6.9], 0.0),
    (Prop::WetFloor, [-6.5, 0.0, -6.4], 0.6),
    (Prop::Tv, [3.9, 0.0, -7.6], HALF),
    (Prop::Barrel, [-33.2, 0.0, -7.2], 0.0),
    (Prop::Barrel, [-32.5, 0.0, -7.4], 1.0),
    (Prop::Barrel, [-33.3, 0.0, -6.5], 2.0),
    (Prop::Aircon, [23.3, 0.0, 6.0], -QUARTER),
    (Prop::Aircon, [23.3, 0.0, 7.2], -QUARTER),
    (Prop::Lifebuoy, [-3.85, 0.9, 38.0], QUARTER),
    (Prop::Lifebuoy, [3.85, 0.9, 41.0], -QUARTER),
];

/// Set dressing hides beyond this many metres from the camera: every drawn
/// mesh costs CPU time each frame, and across the map nobody sees a till.
pub const FAR: f32 = 22.0;

/// A model that hides when the camera is farther from it than this (metres).
#[derive(Component)]
pub struct Far(pub f32);

/// Hide far set dressing (five times a second is plenty).
pub fn cull_far(
    mut tick: Local<u32>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    mut items: Query<(&Transform, &Far, &mut Visibility)>,
) {
    *tick = tick.wrapping_add(1);
    if !tick.is_multiple_of(12) {
        return;
    }
    let Ok(cam) = cam.single() else { return };
    let eye = cam.translation();
    for (t, far, mut v) in &mut items {
        let want = if t.translation.distance(eye) > far.0 { Visibility::Hidden } else { Visibility::Inherited };
        v.set_if_neq(want);
    }
}

/// Spawn the furniture (shared::layout, KayKit models) and the code-built
/// props.
pub fn spawn_decor(
    commands: &mut Commands,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    for piece in shared::layout::PIECES.iter().filter(|p| !p.model.is_empty()) {
        let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("{}.gltf", piece.model)));
        commands.spawn((
            Far(FAR),
            Visibility::default(),
            bevy::world_serialization::WorldAssetRoot(scene),
            Transform::from_translation(Vec3::from(piece.at))
                .with_rotation(Quat::from_rotation_y(piece.yaw))
                .with_scale(Vec3::from(piece.scale)),
        ));
    }
    let paint = materials.add(StandardMaterial { perceptual_roughness: 0.6, ..default() });
    let glow = |c: LinearRgba, m: &mut Assets<StandardMaterial>| {
        m.add(StandardMaterial { base_color: Color::WHITE, emissive: c, ..default() })
    };
    let bulb = glow(LinearRgba::rgb(6.0, 4.0, 1.8), materials);
    let screen = glow(LinearRgba::rgb(0.6, 1.2, 2.0), materials);
    let mut cache: std::collections::HashMap<&'static str, (Handle<Mesh>, Option<Handle<Mesh>>)> = default();
    for (prop, at, yaw) in PROPS {
        let (name, make): (&str, fn() -> (Mesh, Option<Mesh>)) = match prop {
            Prop::Register => ("register", || (crate::art::register(), None)),
            Prop::Pendant => ("pendant", || {
                let (a, b) = crate::art::pendant_lamp();
                (a, Some(b))
            }),
            Prop::Dartboard => ("dartboard", || (crate::art::dartboard(), None)),
            Prop::Clock => ("clock", || (crate::art::wall_clock(), None)),
            Prop::WetFloor => ("wet", || (crate::art::wet_floor_sign(), None)),
            Prop::Tv => ("tv", || {
                let (a, b) = crate::art::tv();
                (a, Some(b))
            }),
            Prop::Barrel => ("barrel", || (crate::art::barrel(), None)),
            Prop::Aircon => ("aircon", || (crate::art::aircon(), None)),
            Prop::Lifebuoy => ("lifebuoy", || (crate::art::lifebuoy(), None)),
        };
        let (body, lit) = cache
            .entry(name)
            .or_insert_with(|| {
                let (a, b) = make();
                (meshes.add(a), b.map(|b| meshes.add(b)))
            })
            .clone();
        let glow_mat = if matches!(prop, Prop::Tv) { screen.clone() } else { bulb.clone() };
        let mut e = commands.spawn((
            Far(FAR),
            Mesh3d(body),
            MeshMaterial3d(paint.clone()),
            Transform::from_translation(Vec3::from(*at)).with_rotation(Quat::from_rotation_y(*yaw)),
        ));
        if let Some(lit) = lit {
            e.with_child((Mesh3d(lit), MeshMaterial3d(glow_mat), Transform::default()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_furniture_model_is_in_the_assets() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/assets");
        for piece in shared::layout::PIECES.iter().filter(|p| !p.model.is_empty()) {
            let path = format!("{root}/{}.gltf", piece.model);
            assert!(std::path::Path::new(&path).exists(), "{path} is missing");
        }
    }

    #[test]
    fn a_wall_gets_texture_coordinates_from_its_size() {
        let mut m: Mesh = Cuboid::new(5.0, 2.5, 0.2).into();
        world_uv(&mut m, 2.5);
        let uv = m.attribute(Mesh::ATTRIBUTE_UV_0).unwrap();
        let bevy::mesh::VertexAttributeValues::Float32x2(uv) = uv else { panic!("uv format") };
        let span = uv.iter().map(|u| u[0]).fold(f32::MIN, f32::max) - uv.iter().map(|u| u[0]).fold(f32::MAX, f32::min);
        assert!((span - 2.0).abs() < 1e-4, "a 5 m wall repeats a 2.5 m texture twice, got {span}");
        assert!(m.attribute(Mesh::ATTRIBUTE_TANGENT).is_some());
    }
}
