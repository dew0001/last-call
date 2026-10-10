//! Surfaces and set dressing: photo-scanned CC0 textures and models from
//! Poly Haven (`scripts/fetch-assets.py`, `docs/CREDITS.md`).
//!
//! Static geometry gets texture coordinates from its world position (box
//! mapping: each face takes the two axes it spans), so a texture keeps the
//! same scale on every wall and floor however the blocks are cut. Decor
//! models have no collision; they sit against walls and in corners, clear
//! of walkways and game stations.

use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
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
    /// The Poly Haven texture id.
    fn texture(self) -> &'static str {
        match self {
            Surface::BarFloor => "wood_floor_worn",
            Surface::OfficeFloor => "herringbone_parquet",
            Surface::KitchenFloor => "floor_tiles_06",
            Surface::BackRoomFloor => "dirty_carpet",
            Surface::ConcreteFloor => "concrete_floor_worn_001",
            Surface::Roof => "brushed_concrete",
            Surface::Asphalt => "asphalt_02",
            Surface::Deck => "wood_floor_deck",
            Surface::BarWall => "brown_brick_02",
            Surface::OfficeWall => "beige_wall_001",
            Surface::KitchenWall => "long_white_tiles",
            Surface::BackRoomWall => "wood_plank_wall",
            Surface::BasementWall => "concrete_block_wall",
            Surface::OutsideWall => "dark_brick_wall",
            Surface::LowWall => "concrete_wall_003",
            Surface::Wood => "dark_planks",
            Surface::Metal => "metal_plate",
            Surface::Leather => "fabric_leather_02",
            Surface::Ceiling => "wood_plank_wall",
        }
    }

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

/// Repeat-wrapped, trilinear, anisotropic: textures tile across big
/// surfaces and stay sharp at a glancing angle.
fn repeat(srgb: bool) -> impl Fn(&mut ImageLoaderSettings) + Send + Sync + 'static {
    move |s: &mut ImageLoaderSettings| {
        s.is_srgb = srgb;
        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            anisotropy_clamp: 8,
            ..default()
        });
    }
}

/// A textured material for a surface. With `lightmapped`, the floor's baked
/// lightmap brightens it (`tools bake_lighting`).
pub fn material(assets: &AssetServer, surface: Surface, lightmapped: bool) -> StandardMaterial {
    let id = surface.texture();
    StandardMaterial {
        base_color_texture: Some(assets.load_builder().with_settings(repeat(true)).load(format!("tex/{id}_d.jpg"))),
        normal_map_texture: Some(assets.load_builder().with_settings(repeat(false)).load(format!("tex/{id}_n.jpg"))),
        metallic_roughness_texture: Some(
            assets.load_builder().with_settings(repeat(false)).load(format!("tex/{id}_a.jpg")),
        ),
        occlusion_texture: Some(assets.load_builder().with_settings(repeat(false)).load(format!("tex/{id}_a.jpg"))),
        perceptual_roughness: 1.0,
        metallic: if surface == Surface::Metal { 1.0 } else { 0.0 },
        // The bake is in the plain gray-box units: scaled down for the
        // darker photo textures it would otherwise wash out.
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

/// One piece of set dressing: a model, where it stands and which way it
/// faces (yaw, radians), and its scale.
pub struct Decor {
    pub model: &'static str,
    pub at: [f32; 3],
    pub yaw: f32,
    pub scale: f32,
}

const fn d(model: &'static str, x: f32, y: f32, z: f32, yaw: f32) -> Decor {
    Decor { model, at: [x, y, z], yaw, scale: 1.0 }
}

const fn ds(model: &'static str, x: f32, y: f32, z: f32, yaw: f32, scale: f32) -> Decor {
    Decor { model, at: [x, y, z], yaw, scale }
}

use std::f32::consts::{FRAC_PI_2 as QUARTER, PI as HALF};

/// Where every model stands. Positions are against walls or in corners
/// (see the room bounds in `shared::world::AREAS`).
pub const DECOR: &[Decor] = &[
    // Main bar: shelves of bottles behind the counter, the till, lamps over
    // the counter, a dartboard and pictures, plants by the door.
    ds("steel_frame_shelves_01", -2.6, 0.0, -6.7, 0.0, 0.1),
    ds("steel_frame_shelves_01", 2.6, 0.0, -6.7, 0.0, 0.1),
    d("wine_bottles_01", -3.0, 1.12, -6.7, 0.0),
    d("wine_bottles_01", -2.3, 1.12, -6.7, 0.4),
    d("wine_bottles_01", 2.2, 1.12, -6.7, 0.0),
    d("wine_bottles_01", 3.0, 1.12, -6.7, 1.0),
    d("wine_bottles_01", -2.7, 1.62, -6.7, 2.0),
    d("wine_bottles_01", 2.7, 1.62, -6.7, 0.7),
    d("CashRegister_01", -4.2, 1.1, -4.0, HALF),
    d("hanging_industrial_lamp", -3.0, 3.2, -4.0, 0.0),
    d("hanging_industrial_lamp", 0.0, 3.2, -4.0, 0.0),
    d("hanging_industrial_lamp", 3.0, 3.2, -4.0, 0.0),
    d("dartboard", 9.95, 1.73, 3.0, -QUARTER),
    d("hanging_picture_frame_01", -9.95, 2.0, -3.0, QUARTER),
    d("hanging_picture_frame_01", 9.95, 2.0, 5.5, -QUARTER),
    d("wall_clock", 0.0, 2.65, -6.95, 0.0),
    d("potted_plant_01", -9.4, 0.0, 6.4, 0.0),
    d("potted_plant_01", 9.4, 0.0, 6.4, 1.0),
    d("WetFloorSign_01", -6.5, 0.0, -6.4, 0.6),
    // Office: the desk against the side wall.
    ds("metal_office_desk", 8.0, 0.0, -2.6, HALF, 0.8),
    // Kitchen: stoves on the back wall, shelves at the end.
    d("electric_stove", -8.5, 0.0, -14.6, 0.0),
    d("electric_stove", -7.5, 0.0, -14.6, 0.0),
    ds("steel_frame_shelves_01", -3.0, 0.0, -14.6, 0.0, 0.1),
    d("cardboard_box_01", -2.9, 1.3, -14.6, 0.3),
    // Back room: a sofa facing a TV on a crate.
    d("Sofa_01", 2.0, 0.0, -14.4, 0.0),
    d("wooden_crate_01", 2.0, 0.0, -8.0, HALF),
    d("Television_01", 2.0, 0.34, -8.0, HALF),
    // Basement: barrels, crates and boxes along the walls.
    d("Barrel_01", -33.3, 0.0, -7.3, 0.0),
    d("Barrel_01", -32.6, 0.0, -7.4, 1.0),
    d("Barrel_01", -33.4, 0.0, -6.6, 2.0),
    d("wooden_crate_01", -15.0, 0.0, -7.5, 0.2),
    d("wooden_crate_01", -15.0, 0.34, -7.5, 0.5),
    d("cardboard_box_01", -16.0, 0.18, -7.5, 0.0),
    d("cardboard_box_01", -33.3, 0.18, 7.3, 0.7),
    ds("steel_frame_shelves_01", -24.0, 0.0, 7.6, HALF, 0.1),
    // Roof: air conditioners on the parapet side.
    d("exterior_aircon_unit", 23.4, 0.32, -4.4, -QUARTER),
    d("exterior_aircon_unit", 23.4, 0.32, -3.2, -QUARTER),
    // Parking lot: a car under a cover, street lamps, bins, a hydrant.
    d("covered_car", 12.0, 0.3, 24.0, 0.1),
    d("street_lamp_01", -13.5, 0.0, 7.6, 0.0),
    d("street_lamp_01", 13.5, 0.0, 7.6, 0.0),
    d("street_lamp_01", -13.5, 0.0, 26.4, 0.0),
    d("metal_trash_can", -2.2, 0.0, 7.6, 0.0),
    d("metal_trash_can", 2.2, 0.0, 7.6, 1.5),
    d("fire_hydrant", 13.4, 0.0, 15.0, 0.0),
    // Pier: a picnic table, life rings on the railing.
    d("wooden_picnic_table", -2.6, 0.0, 30.0, 0.0),
    d("lifebuoy", -3.9, 0.9, 38.0, QUARTER),
    d("lifebuoy", 3.9, 0.9, 41.0, -QUARTER),
];

/// Spawn the set dressing.
pub fn spawn_decor(commands: &mut Commands, assets: &AssetServer) {
    for item in DECOR {
        let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("models/{0}/{0}.gltf", item.model)));
        commands.spawn((
            bevy::world_serialization::WorldAssetRoot(scene),
            Transform::from_translation(Vec3::from(item.at))
                .with_rotation(Quat::from_rotation_y(item.yaw))
                .with_scale(Vec3::splat(item.scale)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decor_stands_inside_the_map() {
        for item in DECOR {
            let [x, _, z] = item.at;
            let inside = shared::world::AREAS
                .iter()
                .any(|a| x >= a.x0 - 0.1 && x <= a.x1 + 0.1 && z >= a.z0 - 0.1 && z <= a.z1 + 0.1);
            assert!(inside, "{} at {x},{z} is off the map", item.model);
        }
    }

    #[test]
    fn every_decor_model_was_fetched() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/assets/models");
        for item in DECOR {
            let path = format!("{root}/{0}/{0}.gltf", item.model);
            assert!(std::path::Path::new(&path).exists(), "{path} is missing: run scripts/fetch-assets.py");
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
