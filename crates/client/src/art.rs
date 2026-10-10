//! The art pass (plan section 7): low-poly, chunky meshes built in code.
//!
//! Every model is a few primitives painted with vertex colors and merged
//! into one mesh, so it costs one draw call, and models that share a mesh
//! and material batch together (chips, bottles, glasses, stools). Toy
//! figures stand 1.5 heads tall with big hands. Palette: warm amber inside,
//! cold blue outside, one neon accent per room.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use shared::bar;
use shared::world::Room;

/// Paint every vertex of `mesh` one color, then move it into place.
pub fn part(mesh: impl Into<Mesh>, at: Transform, color: Color) -> Mesh {
    let mut mesh: Mesh = mesh.into();
    let n = mesh.count_vertices();
    let c = color.to_linear().to_f32_array();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    // Merging needs the same attributes everywhere: drop tangents if any.
    mesh.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
    mesh.transformed_by(at)
}

/// Merge parts into one mesh.
pub fn combine(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut out = it.next().unwrap_or_else(|| Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
    for p in it {
        out.merge(&p).expect("parts share attributes");
    }
    out
}

/// Triangles in a mesh.
pub fn triangles(mesh: &Mesh) -> usize {
    match mesh.indices() {
        Some(Indices::U16(i)) => i.len() / 3,
        Some(Indices::U32(i)) => i.len() / 3,
        None => mesh.count_vertices() / 3,
    }
}

fn at(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

fn ball(r: f32) -> Mesh {
    Sphere::new(r).mesh().uv(8, 6)
}

fn tube(r: f32, h: f32, res: u32) -> Mesh {
    Cylinder::new(r, h).mesh().resolution(res).build()
}

fn boxy(x: f32, y: f32, z: f32) -> Mesh {
    Cuboid::new(x, y, z).into()
}

/// A toy figure, centered on its middle (it stands [`bar::PLAYER_HEIGHT`]
/// tall): a stubby body, a big head, big hands, boots. `body` is the shirt,
/// `skin` the head and hands.
pub fn figure(body: Color, skin: Color) -> Mesh {
    let h = bar::PLAYER_HEIGHT;
    let base = -h / 2.0;
    let dark = Color::srgb(0.12, 0.1, 0.1);
    combine(vec![
        // Body: a short capsule.
        part(Capsule3d::new(0.3, 0.25).mesh().longitudes(10).latitudes(6).build(), at(0.0, base + 0.62, 0.0), body),
        // Head: 0.64 of the 1.6 m (about 1.5 heads tall with the body under it).
        part(ball(0.34), at(0.0, base + 1.26, 0.0), skin),
        // Eyes, facing -Z.
        part(boxy(0.07, 0.09, 0.02), at(-0.11, base + 1.3, -0.33), dark),
        part(boxy(0.07, 0.09, 0.02), at(0.11, base + 1.3, -0.33), dark),
        // Big hands.
        part(ball(0.13), at(-0.42, base + 0.55, -0.05), skin),
        part(ball(0.13), at(0.42, base + 0.55, -0.05), skin),
        // Boots.
        part(boxy(0.18, 0.12, 0.28), at(-0.13, base + 0.06, -0.04), dark),
        part(boxy(0.18, 0.12, 0.28), at(0.13, base + 0.06, -0.04), dark),
    ])
}

/// A hat for a figure (cosmetic id picks the style), on top of the head.
pub fn hat(style: u16) -> Mesh {
    let top = bar::PLAYER_HEIGHT / 2.0 - 0.06;
    let (a, b) = match style % 3 {
        0 => (Color::srgb(0.15, 0.15, 0.2), Color::srgb(0.8, 0.1, 0.3)),
        1 => (Color::srgb(0.6, 0.45, 0.2), Color::srgb(0.3, 0.2, 0.1)),
        _ => (Color::srgb(0.1, 0.5, 0.6), Color::srgb(0.9, 0.9, 0.9)),
    };
    combine(vec![
        part(tube(0.36, 0.03, 10), at(0.0, top, 0.0), a),
        part(tube(0.22, 0.22, 10), at(0.0, top + 0.12, 0.0), b),
    ])
}

// ---------- Props (each under 300 triangles) ----------

pub fn bottle() -> Mesh {
    let green = Color::srgb(0.15, 0.45, 0.2);
    combine(vec![
        part(tube(0.04, 0.2, 8), at(0.0, -0.04, 0.0), green),
        part(tube(0.015, 0.08, 6), at(0.0, 0.1, 0.0), green),
        part(tube(0.041, 0.06, 8), at(0.0, -0.03, 0.0), Color::srgb(0.9, 0.85, 0.7)),
    ])
}

pub fn glass() -> Mesh {
    combine(vec![
        part(tube(0.045, 0.12, 10), at(0.0, -0.015, 0.0), Color::srgb(0.95, 0.65, 0.15)),
        part(tube(0.046, 0.03, 10), at(0.0, 0.06, 0.0), Color::srgb(0.98, 0.96, 0.9)),
    ])
}

pub fn chip() -> Mesh {
    combine(vec![
        part(tube(0.02, 0.012, 10), Transform::IDENTITY, Color::srgb(0.85, 0.15, 0.15)),
        part(tube(0.012, 0.013, 8), Transform::IDENTITY, Color::srgb(0.95, 0.95, 0.95)),
    ])
}

/// A bar stool, centered on its middle (0.75 m tall).
pub fn stool() -> Mesh {
    let wood = Color::srgb(0.42, 0.25, 0.12);
    let red = Color::srgb(0.6, 0.12, 0.1);
    let mut parts =
        vec![part(tube(0.2, 0.08, 10), at(0.0, 0.33, 0.0), red), part(tube(0.17, 0.03, 10), at(0.0, 0.0, 0.0), wood)];
    for i in 0..3 {
        let a = i as f32 * std::f32::consts::TAU / 3.0;
        parts.push(part(boxy(0.04, 0.72, 0.04), at(a.cos() * 0.13, 0.0, a.sin() * 0.13), wood));
    }
    combine(parts)
}

pub fn mop() -> Mesh {
    combine(vec![
        part(tube(0.02, 1.2, 6), at(0.0, 0.05, 0.0), Color::srgb(0.55, 0.45, 0.3)),
        part(boxy(0.25, 0.1, 0.12), at(0.0, -0.6, 0.0), Color::srgb(0.85, 0.85, 0.75)),
    ])
}

// ---------- The rooms ----------

/// Each room's floor color and neon accent (plan section 7).
pub fn palette(room: Room) -> (Color, Color) {
    match room {
        Room::Bar => (Color::srgb(0.32, 0.2, 0.11), Color::srgb(1.0, 0.2, 0.6)),
        Room::Office => (Color::srgb(0.25, 0.2, 0.15), Color::srgb(0.2, 1.0, 0.5)),
        Room::Kitchen => (Color::srgb(0.6, 0.6, 0.55), Color::srgb(1.0, 0.6, 0.1)),
        Room::BackRoom => (Color::srgb(0.18, 0.25, 0.18), Color::srgb(0.9, 0.9, 0.2)),
        Room::Stairwell => (Color::srgb(0.25, 0.24, 0.22), Color::srgb(1.0, 0.15, 0.1)),
        Room::Basement => (Color::srgb(0.22, 0.22, 0.24), Color::srgb(1.0, 0.1, 0.1)),
        Room::Roof => (Color::srgb(0.2, 0.2, 0.22), Color::srgb(0.6, 0.3, 1.0)),
        Room::ParkingLot => (Color::srgb(0.1, 0.11, 0.13), Color::srgb(0.1, 0.9, 1.0)),
        Room::Pier => (Color::srgb(0.32, 0.22, 0.13), Color::srgb(0.1, 0.6, 1.0)),
    }
}

/// The counter: a dark wooden body with a lighter top and a brass foot rail.
pub fn counter(b: &bar::Block) -> Mesh {
    let (w, d, h) = (b.hx * 2.0, b.hz * 2.0, b.height);
    combine(vec![
        part(boxy(w, h - 0.06, d), at(0.0, -0.03, 0.0), Color::srgb(0.3, 0.17, 0.08)),
        part(boxy(w + 0.1, 0.06, d + 0.12), at(0.0, h / 2.0 - 0.03, 0.0), Color::srgb(0.55, 0.36, 0.18)),
        part(boxy(w, 0.05, 0.05), at(0.0, -h / 2.0 + 0.2, d / 2.0 + 0.12), Color::srgb(0.85, 0.65, 0.25)),
    ])
}

/// A casino table: a wooden base and a green felt top.
pub fn table(b: &bar::Block) -> Mesh {
    let (w, d, h) = (b.hx * 2.0, b.hz * 2.0, b.height);
    combine(vec![
        part(boxy(w * 0.7, h - 0.08, d * 0.6), at(0.0, -0.04, 0.0), Color::srgb(0.25, 0.14, 0.07)),
        part(boxy(w, 0.08, d), at(0.0, h / 2.0 - 0.04, 0.0), Color::srgb(0.08, 0.38, 0.18)),
        part(boxy(w + 0.06, 0.06, 0.06), at(0.0, h / 2.0 - 0.02, d / 2.0), Color::srgb(0.35, 0.2, 0.1)),
    ])
}

/// A slot machine: a cabinet with a glowing screen facing +X (the room).
pub fn slot_machine(b: &bar::Block) -> Mesh {
    let (w, d, h) = (b.hx * 2.0, b.hz * 2.0, b.height);
    combine(vec![
        part(boxy(w, h, d), Transform::IDENTITY, Color::srgb(0.5, 0.08, 0.4)),
        part(boxy(0.02, h * 0.3, d * 0.7), at(w / 2.0 + 0.01, h * 0.15, 0.0), Color::srgb(1.0, 0.95, 0.6)),
        part(tube(0.03, 0.4, 6), at(w / 2.0 - 0.05, h * 0.15, d / 2.0 + 0.05), Color::srgb(0.8, 0.8, 0.8)),
    ])
}

/// A plain block (walls, the safe) with a darker skirting.
pub fn block(b: &bar::Block, color: Color) -> Mesh {
    let (w, d, h) = (b.hx * 2.0, b.hz * 2.0, b.height);
    let skirt = color.mix(&Color::BLACK, 0.5);
    combine(vec![
        part(boxy(w, h, d), Transform::IDENTITY, color),
        part(boxy(w + 0.02, 0.15, d + 0.02), at(0.0, -h / 2.0 + 0.075, 0.0), skirt),
    ])
}

/// A floor slab for an area, with UVs over the whole slab in both UV sets
/// (the second carries the baked lightmap).
pub fn floor(w: f32, d: f32, color: Color) -> Mesh {
    let mut m = part(Plane3d::new(Vec3::Y, Vec2::new(w / 2.0, d / 2.0)).mesh().build(), Transform::IDENTITY, color);
    if let Some(uv) = m.attribute(Mesh::ATTRIBUTE_UV_0).cloned() {
        m.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn props_stay_under_300_triangles() {
        for (name, m) in
            [("bottle", bottle()), ("glass", glass()), ("chip", chip()), ("stool", stool()), ("mop", mop())]
        {
            let t = triangles(&m);
            assert!(t > 0 && t < 300, "{name}: {t} triangles");
        }
    }

    #[test]
    fn a_figure_is_one_cheap_mesh() {
        let m = figure(Color::WHITE, Color::WHITE);
        assert!(triangles(&m) < 1_000, "{}", triangles(&m));
        assert!(m.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
        assert!(triangles(&hat(0)) < 100);
        // It stands on the floor: its lowest point is half its height down.
        let ys: Vec<f32> = match m.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) => v.iter().map(|p| p[1]).collect(),
            _ => panic!(),
        };
        let low = ys.iter().copied().fold(f32::MAX, f32::min);
        assert!((low + bar::PLAYER_HEIGHT / 2.0).abs() < 0.01, "{low}");
    }

    #[test]
    fn a_floor_has_both_uv_sets() {
        let f = floor(4.0, 2.0, Color::WHITE);
        assert!(f.attribute(Mesh::ATTRIBUTE_UV_1).is_some());
        assert_eq!(triangles(&f), 2);
    }
}
