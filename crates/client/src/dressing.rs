//! Set dressing built in code for the basement: the keg rack, boiler, pipes,
//! workbench and pallets (the other rooms use KayKit furniture, see
//! `shared::layout`, which also holds these pieces' footprints).
//! Each room is one vertex-painted mesh (plus one for its metal), placed at
//! the room's center so far culling can hide it. None of it collides: it
//! stands against the walls, out of the walkways.

use bevy::prelude::*;
use shared::world::{Room, area_of};

use crate::art::{combine, part};
use crate::look::Far;
use crate::online::NoDraw;

fn at(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

fn boxy(x: f32, y: f32, z: f32) -> Mesh {
    Cuboid::new(x, y, z).into()
}

fn tube(r: f32, h: f32) -> Mesh {
    Cylinder::new(r, h).mesh().resolution(12).build()
}

/// A tube lying along X.
fn tube_x(r: f32, len: f32, x: f32, y: f32, z: f32) -> (Mesh, Transform) {
    (tube(r, len), Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)))
}

/// One room's dressing: painted parts and metal parts, in world coordinates.
#[derive(Default)]
pub struct RoomDressing {
    pub paint: Vec<Mesh>,
    pub metal: Vec<Mesh>,
}

impl RoomDressing {
    fn paint(&mut self, m: Mesh, t: Transform, c: Color) {
        self.paint.push(part(m, t, c));
    }
    fn metal(&mut self, m: Mesh, t: Transform, c: Color) {
        self.metal.push(part(m, t, c));
    }
}

const STEEL: Color = Color::srgb(0.75, 0.76, 0.78);
const DARK_STEEL: Color = Color::srgb(0.35, 0.36, 0.38);
const WOOD: Color = Color::srgb(0.4, 0.25, 0.13);

pub fn basement() -> RoomDressing {
    let mut d = RoomDressing::default();
    // A keg rack along the north wall: a steel frame, two rows of kegs.
    let (x0, x1, z) = (-30.0, -20.0, -7.5);
    let xc = (x0 + x1) / 2.0;
    for y in [0.15, 0.85] {
        d.metal(boxy(x1 - x0, 0.05, 0.7), at(xc, y, z), DARK_STEEL);
    }
    let mut x = x0 + 0.1;
    while x <= x1 + 0.01 {
        d.metal(boxy(0.05, 1.5, 0.05), at(x, 0.75, z - 0.32), DARK_STEEL);
        d.metal(boxy(0.05, 1.5, 0.05), at(x, 0.75, z + 0.32), DARK_STEEL);
        x += 2.5;
    }
    for row in [0.45f32, 1.15] {
        let mut x = x0 + 0.5;
        while x < x1 - 0.3 {
            // Lying front to back on the rack.
            d.metal(
                tube(0.25, 0.55),
                Transform::from_xyz(x, row, z).with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                STEEL,
            );
            x += 0.7;
        }
    }
    // The boiler in the corner, with pipes up to the ceiling.
    d.metal(tube(0.6, 2.2), at(-32.6, 1.1, 6.6), Color::srgb(0.45, 0.12, 0.08));
    d.metal(tube(0.62, 0.1), at(-32.6, 2.2, 6.6), DARK_STEEL);
    d.metal(tube(0.08, 1.0), at(-32.3, 2.7, 6.6), DARK_STEEL);
    d.metal(tube(0.08, 1.0), at(-32.9, 2.7, 6.4), DARK_STEEL);
    d.metal(boxy(0.3, 0.3, 0.05), at(-32.6, 1.2, 5.98), Color::srgb(0.85, 0.8, 0.6));
    // Pipes along the ceiling, the length of the room.
    for (z, r) in [(-6.5f32, 0.09), (-6.1, 0.06), (6.0, 0.1)] {
        let (m, t) = tube_x(r, 20.0, -24.0, 2.95, z);
        d.metal(m, t, DARK_STEEL);
    }
    // A workbench on the south wall with a vise and a toolbox.
    d.paint(boxy(2.4, 0.08, 0.7), at(-18.5, 0.9, 7.55), WOOD);
    for x in [-19.6, -17.4] {
        d.paint(boxy(0.08, 0.86, 0.6), at(x, 0.43, 7.55), WOOD);
    }
    d.metal(boxy(0.2, 0.15, 0.15), at(-19.3, 1.02, 7.4), Color::srgb(0.2, 0.3, 0.5));
    d.paint(boxy(0.5, 0.2, 0.25), at(-18.0, 1.04, 7.6), Color::srgb(0.75, 0.1, 0.08));
    d.paint(boxy(2.4, 1.0, 0.03), at(-18.5, 1.7, 7.9), Color::srgb(0.6, 0.5, 0.35));
    // Pallets of boxes.
    for (x, z) in [(-27.0f32, 7.2f32), (-25.6, 7.2)] {
        d.paint(boxy(1.2, 0.14, 1.0), at(x, 0.07, z), Color::srgb(0.6, 0.48, 0.3));
        d.paint(boxy(1.0, 0.5, 0.8), at(x, 0.4, z), Color::srgb(0.7, 0.58, 0.4));
        d.paint(boxy(0.6, 0.35, 0.5), at(x + 0.1, 0.83, z), Color::srgb(0.75, 0.62, 0.42));
    }
    d
}

pub(crate) fn setup_dressing(
    mut commands: Commands,
    nodraw: Option<Res<NoDraw>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if nodraw.is_some() {
        return;
    }
    let paint = materials.add(StandardMaterial { perceptual_roughness: 0.8, ..default() });
    let metal = materials.add(StandardMaterial { perceptual_roughness: 0.35, metallic: 0.6, ..default() });
    for (room, d) in [(Room::Basement, basement())] {
        let a = area_of(room);
        let center = Vec3::new((a.x0 + a.x1) / 2.0, 0.0, (a.z0 + a.z1) / 2.0);
        // Far enough to cover the room from any corner of it.
        let reach = (a.x1 - a.x0).hypot(a.z1 - a.z0) / 2.0 + 10.0;
        for (parts, mat) in [(d.paint, &paint), (d.metal, &metal)] {
            if parts.is_empty() {
                continue;
            }
            let mesh = combine(parts).translated_by(-center);
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(center),
                Far(reach),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::primitives::MeshAabb;

    fn inside(room: Room, d: &RoomDressing) {
        let a = area_of(room);
        for m in d.paint.iter().chain(&d.metal) {
            let aabb = m.compute_aabb().expect("a mesh");
            let (lo, hi) = (aabb.min(), aabb.max());
            assert!(lo.x >= a.x0 - 0.01 && hi.x <= a.x1 + 0.01, "{room:?}: x {} to {}", lo.x, hi.x);
            assert!(lo.z >= a.z0 - 0.01 && hi.z <= a.z1 + 0.01, "{room:?}: z {} to {}", lo.z, hi.z);
            assert!(lo.y >= -0.01 && hi.y <= shared::bar::WALL_HEIGHT + 0.01, "{room:?}: y {} to {}", lo.y, hi.y);
        }
    }

    #[test]
    fn dressing_stays_inside_its_room() {
        inside(Room::Basement, &basement());
    }

    #[test]
    fn dressing_is_cheap() {
        let d = basement();
        let t: usize = d.paint.iter().chain(&d.metal).map(crate::art::triangles).sum();
        assert!(t < 12_000, "{t}");
    }
}
