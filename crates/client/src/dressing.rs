//! Set dressing built in code for the side rooms: the kitchen's hood, prep
//! counter, fridge and pans; the back room's rug, coffee table, crates and
//! posters; the basement's keg rack, boiler, pipes, workbench and pallets.
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

pub fn kitchen() -> RoomDressing {
    let mut d = RoomDressing::default();
    // The hood over the stoves, with its duct to the ceiling.
    d.metal(boxy(2.3, 0.45, 0.85), at(-8.0, 2.3, -14.5), STEEL);
    d.metal(boxy(0.45, 0.7, 0.45), at(-8.0, 2.85, -14.6), STEEL);
    // The prep counter along the west wall: a steel top, a sink, a board,
    // pots and a shelf under it.
    let (x, z0, z1) = (-9.62, -13.8, -8.4);
    let (zc, len) = ((z0 + z1) / 2.0, z1 - z0);
    d.metal(boxy(0.7, 0.05, len), at(x, 0.9, zc), STEEL);
    d.metal(boxy(0.66, 0.04, len - 0.04), at(x, 0.25, zc), DARK_STEEL);
    for z in [z0 + 0.03, zc, z1 - 0.03] {
        for dx in [-0.3, 0.3] {
            d.metal(boxy(0.04, 0.88, 0.04), at(x + dx, 0.44, z), DARK_STEEL);
        }
    }
    d.metal(boxy(0.5, 0.02, 0.55), at(x, 0.93, -11.6), Color::srgb(0.15, 0.16, 0.17));
    d.metal(tube(0.015, 0.3), at(x - 0.25, 1.08, -11.6), STEEL);
    d.paint(boxy(0.35, 0.03, 0.5), at(x, 0.94, -10.4), Color::srgb(0.7, 0.5, 0.3));
    d.paint(boxy(0.06, 0.03, 0.18), at(x + 0.05, 0.965, -10.3), Color::srgb(0.9, 0.2, 0.15));
    d.metal(tube(0.16, 0.22), at(x, 1.03, -9.3), STEEL);
    d.metal(tube(0.12, 0.16), at(x - 0.1, 1.0, -8.85), DARK_STEEL);
    d.paint(boxy(0.3, 0.25, 0.4), at(x, 0.4, -12.8), Color::srgb(0.75, 0.62, 0.42));
    d.paint(boxy(0.3, 0.25, 0.4), at(x, 0.4, -9.2), Color::srgb(0.75, 0.62, 0.42));
    // The fridge in the corner by the back room wall.
    d.metal(boxy(0.85, 2.0, 0.75), at(-2.5, 1.0, -8.0), Color::srgb(0.82, 0.83, 0.85));
    d.metal(boxy(0.03, 0.6, 0.04), at(-2.95, 1.3, -7.75), DARK_STEEL);
    d.metal(boxy(0.86, 0.02, 0.76), at(-2.5, 1.35, -8.0), DARK_STEEL);
    // A pan rail on the east wall, four pans hanging.
    let along_z = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    d.metal(tube(0.015, 3.0), Transform::from_xyz(-2.1, 1.85, -11.5).with_rotation(along_z), DARK_STEEL);
    for (i, r) in [0.14f32, 0.11, 0.16, 0.1].into_iter().enumerate() {
        let z = -12.7 + i as f32 * 0.75;
        d.metal(
            tube(r, 0.03),
            Transform::from_xyz(-2.12, 1.65 - r, z).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            Color::srgb(0.2, 0.2, 0.22),
        );
        d.metal(boxy(0.02, 0.2, 0.02), at(-2.12, 1.75, z), DARK_STEEL);
    }
    d
}

pub fn back_room() -> RoomDressing {
    let mut d = RoomDressing::default();
    // A worn rug in front of the sofa, with a border.
    d.paint(boxy(3.2, 0.01, 2.2), at(2.0, 0.006, -12.9), Color::srgb(0.45, 0.08, 0.08));
    d.paint(boxy(2.9, 0.012, 1.9), at(2.0, 0.008, -12.9), Color::srgb(0.6, 0.18, 0.12));
    // A low coffee table with bottles and a pizza box.
    d.paint(boxy(1.1, 0.06, 0.55), at(2.0, 0.42, -13.1), WOOD);
    for (x, z) in [(-0.5, -0.24), (0.5, -0.24), (-0.5, 0.24), (0.5, 0.24)] {
        d.paint(boxy(0.05, 0.4, 0.05), at(2.0 + x, 0.2, -13.1 + z), WOOD);
    }
    d.paint(boxy(0.4, 0.04, 0.4), at(1.75, 0.47, -13.1), Color::srgb(0.85, 0.75, 0.55));
    for (i, c) in [Color::srgb(0.2, 0.45, 0.15), Color::srgb(0.5, 0.3, 0.08)].into_iter().enumerate() {
        d.paint(tube(0.035, 0.22), at(2.3 + i as f32 * 0.12, 0.56, -13.05), c);
    }
    // Beer crates stacked in the corner.
    let crate_colors = [Color::srgb(0.75, 0.55, 0.1), Color::srgb(0.15, 0.4, 0.2), Color::srgb(0.7, 0.12, 0.1)];
    for (i, c) in crate_colors.into_iter().enumerate() {
        let y = 0.16 + i as f32 * 0.32;
        d.paint(
            boxy(0.6, 0.3, 0.42),
            Transform::from_xyz(5.4, y, -14.5).with_rotation(Quat::from_rotation_y(i as f32 * 0.08)),
            c,
        );
    }
    d.paint(boxy(0.6, 0.3, 0.42), at(4.7, 0.16, -14.5), crate_colors[1]);
    // A floor lamp by the sofa.
    d.metal(tube(0.18, 0.03), at(-1.4, 0.015, -14.4), DARK_STEEL);
    d.metal(tube(0.02, 1.6), at(-1.4, 0.8, -14.4), DARK_STEEL);
    d.paint(Cone::new(0.25, 0.3).mesh().resolution(12).build(), at(-1.4, 1.65, -14.4), Color::srgb(0.95, 0.8, 0.55));
    // Gig posters on the west wall.
    for (i, (c, z)) in
        [(Color::srgb(0.9, 0.3, 0.5), -12.5), (Color::srgb(0.2, 0.6, 0.85), -10.5)].into_iter().enumerate()
    {
        d.paint(boxy(0.02, 0.9, 0.65), at(-1.93, 1.7, z), Color::srgb(0.08, 0.06, 0.05));
        d.paint(boxy(0.025, 0.8, 0.55), at(-1.925, 1.7, z), c);
        d.paint(boxy(0.03, 0.12, 0.45), at(-1.92, 1.85 - i as f32 * 0.2, z), Color::srgb(0.98, 0.95, 0.85));
    }
    d
}

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
    for (room, d) in [(Room::Kitchen, kitchen()), (Room::BackRoom, back_room()), (Room::Basement, basement())] {
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
        inside(Room::Kitchen, &kitchen());
        inside(Room::BackRoom, &back_room());
        inside(Room::Basement, &basement());
    }

    #[test]
    fn dressing_is_cheap() {
        for d in [kitchen(), back_room(), basement()] {
            let t: usize = d.paint.iter().chain(&d.metal).map(crate::art::triangles).sum();
            assert!(t < 12_000, "{t}");
        }
    }
}
