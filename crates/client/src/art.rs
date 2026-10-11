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
use std::f32::consts::FRAC_PI_2;

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

/// Clothes and colouring for an articulated figure.
#[derive(Clone, Copy)]
pub struct Outfit {
    pub shirt: Color,
    pub pants: Color,
    pub skin: Color,
    pub hair: Color,
}

impl Outfit {
    /// A varied, believable outfit from a seed (customers, players).
    pub fn from_seed(seed: u64) -> Outfit {
        const SKIN: [(f32, f32, f32); 6] = [
            (0.98, 0.8, 0.66),
            (0.92, 0.7, 0.55),
            (0.8, 0.58, 0.42),
            (0.62, 0.42, 0.3),
            (0.45, 0.3, 0.2),
            (0.32, 0.2, 0.14),
        ];
        const HAIR: [(f32, f32, f32); 6] = [
            (0.08, 0.06, 0.05),
            (0.25, 0.15, 0.08),
            (0.45, 0.3, 0.15),
            (0.75, 0.6, 0.35),
            (0.55, 0.15, 0.08),
            (0.6, 0.6, 0.62),
        ];
        const PANTS: [(f32, f32, f32); 5] =
            [(0.12, 0.16, 0.3), (0.1, 0.1, 0.12), (0.35, 0.3, 0.22), (0.2, 0.25, 0.2), (0.28, 0.28, 0.32)];
        let mix = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let pick = |n: u64, shift: u32| ((mix >> shift) % n) as usize;
        let rgb = |c: (f32, f32, f32)| Color::srgb(c.0, c.1, c.2);
        let hue = ((mix >> 40) % 360) as f32;
        Outfit {
            shirt: Color::hsl(hue, 0.45, 0.42),
            pants: rgb(PANTS[pick(5, 3)]),
            skin: rgb(SKIN[pick(6, 11)]),
            hair: rgb(HAIR[pick(6, 19)]),
        }
    }
}

/// An articulated figure: the body (torso, head, hair, face) and the limbs.
/// Each limb's origin is its joint (shoulder or hip), so turning it about X
/// swings it. Positions are from the figure's centre (0.8 m above the feet).
pub struct FigureParts {
    pub body: Mesh,
    pub arm: Mesh,
    pub leg: Mesh,
    /// Joints from the centre: left shoulder, right shoulder, left hip, right hip.
    pub joints: [Vec3; 4],
}

pub fn figure_parts(o: Outfit) -> FigureParts {
    let base = -bar::PLAYER_HEIGHT / 2.0;
    let dark = Color::srgb(0.1, 0.08, 0.07);
    let white = Color::srgb(0.95, 0.95, 0.92);
    let body = combine(vec![
        // Torso and a belt.
        part(Capsule3d::new(0.2, 0.32).mesh().longitudes(12).latitudes(8).build(), at(0.0, base + 1.0, 0.0), o.shirt),
        part(tube(0.205, 0.08, 12), at(0.0, base + 0.78, 0.0), o.pants),
        // Neck and head.
        part(tube(0.07, 0.12, 8), at(0.0, base + 1.3, 0.0), o.skin),
        part(Sphere::new(0.17).mesh().uv(12, 9), at(0.0, base + 1.45, 0.0), o.skin),
        // Hair: a cap over the back and top of the head.
        part(Sphere::new(0.178).mesh().uv(12, 9), at(0.0, base + 1.48, 0.025), o.hair),
        // Eyes and nose, facing -Z.
        part(boxy(0.05, 0.05, 0.02), at(-0.065, base + 1.47, -0.165), white),
        part(boxy(0.05, 0.05, 0.02), at(0.065, base + 1.47, -0.165), white),
        part(boxy(0.025, 0.03, 0.022), at(-0.065, base + 1.465, -0.175), dark),
        part(boxy(0.025, 0.03, 0.022), at(0.065, base + 1.465, -0.175), dark),
        part(boxy(0.04, 0.06, 0.05), at(0.0, base + 1.42, -0.18), o.skin),
    ]);
    // An arm hangs from the shoulder: sleeve, forearm, hand.
    let arm = combine(vec![
        part(Capsule3d::new(0.065, 0.22).mesh().longitudes(8).latitudes(4).build(), at(0.0, -0.17, 0.0), o.shirt),
        part(Capsule3d::new(0.055, 0.2).mesh().longitudes(8).latitudes(4).build(), at(0.0, -0.42, 0.0), o.skin),
        part(Sphere::new(0.075).mesh().uv(8, 6), at(0.0, -0.58, -0.01), o.skin),
    ]);
    // A leg hangs from the hip: trouser leg and a shoe.
    let leg = combine(vec![
        part(Capsule3d::new(0.085, 0.55).mesh().longitudes(8).latitudes(4).build(), at(0.0, -0.36, 0.0), o.pants),
        part(boxy(0.13, 0.1, 0.26), at(0.0, -0.73, -0.05), dark),
    ]);
    FigureParts {
        body,
        arm,
        leg,
        joints: [
            Vec3::new(-0.27, base + 1.2, 0.0),
            Vec3::new(0.27, base + 1.2, 0.0),
            Vec3::new(-0.1, base + 0.78, 0.0),
            Vec3::new(0.1, base + 0.78, 0.0),
        ],
    }
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

// ---------- Fixtures ----------

/// A jukebox: a wooden cabinet with a rounded top, a glass dome and glowing
/// bands (the bands are a separate mesh: they take the light material).
pub fn jukebox() -> (Mesh, Mesh) {
    let wood = Color::srgb(0.35, 0.16, 0.08);
    let chrome = Color::srgb(0.85, 0.85, 0.9);
    let body = combine(vec![
        part(boxy(0.9, 1.1, 0.55), at(0.0, 0.55, 0.0), wood),
        part(
            tube(0.45, 0.55, 16).rotated_by(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            at(0.0, 1.1, 0.0),
            wood,
        ),
        part(boxy(0.6, 0.35, 0.02), at(0.0, 0.85, -0.28), Color::srgb(0.05, 0.05, 0.06)),
        part(boxy(0.94, 0.05, 0.58), at(0.0, 0.03, 0.0), chrome),
        part(boxy(0.5, 0.18, 0.04), at(0.0, 0.35, -0.28), chrome),
    ]);
    let bands = combine(vec![
        part(boxy(0.06, 1.0, 0.04), at(-0.38, 0.6, -0.28), Color::WHITE),
        part(boxy(0.06, 1.0, 0.04), at(0.38, 0.6, -0.28), Color::WHITE),
        part(
            Torus::new(0.33, 0.37)
                .mesh()
                .minor_resolution(6)
                .major_resolution(24)
                .build()
                .rotated_by(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            at(0.0, 1.1, -0.28),
            Color::WHITE,
        ),
    ]);
    (body, bands)
}

/// A brass key on a ring, lying flat.
pub fn key() -> Mesh {
    let brass = Color::srgb(0.85, 0.65, 0.25);
    combine(vec![
        part(
            Torus::new(0.025, 0.045).mesh().minor_resolution(6).major_resolution(12).build(),
            at(0.0, 0.0, 0.0),
            brass,
        ),
        part(boxy(0.13, 0.015, 0.02), at(0.11, 0.0, 0.0), brass),
        part(boxy(0.02, 0.015, 0.04), at(0.15, 0.0, 0.025), brass),
        part(boxy(0.02, 0.015, 0.03), at(0.12, 0.0, 0.02), brass),
    ])
}

/// The kitchen pass: a steel counter in the hatch with a heat lamp over it
/// (the lamp bar is a separate mesh for the glow material).
pub fn kitchen_pass() -> (Mesh, Mesh) {
    let steel = Color::srgb(0.72, 0.74, 0.78);
    let body = combine(vec![
        part(boxy(1.3, 0.9, 0.55), at(0.0, 0.45, 0.0), steel),
        part(boxy(1.36, 0.04, 0.6), at(0.0, 0.92, 0.0), steel),
        part(tube(0.015, 0.7, 6), at(-0.55, 1.3, 0.0), steel),
        part(tube(0.015, 0.7, 6), at(0.55, 1.3, 0.0), steel),
        part(boxy(1.2, 0.08, 0.2), at(0.0, 1.66, 0.0), steel),
    ]);
    let lamp = part(boxy(1.1, 0.03, 0.12), at(0.0, 1.61, 0.0), Color::WHITE);
    (body, lamp)
}

// ---------- Neon ----------

/// Strokes of the neon letters, in a 0.6 by 1 box (x right, y up): each
/// stroke is a polyline of points.
fn glyph(c: char) -> &'static [&'static [(f32, f32)]] {
    match c {
        'L' => &[&[(0.0, 1.0), (0.0, 0.0), (0.6, 0.0)]],
        'A' => &[&[(0.0, 0.0), (0.3, 1.0), (0.6, 0.0)], &[(0.12, 0.4), (0.48, 0.4)]],
        'S' => &[&[
            (0.6, 0.85),
            (0.48, 1.0),
            (0.12, 1.0),
            (0.0, 0.85),
            (0.0, 0.62),
            (0.12, 0.5),
            (0.48, 0.5),
            (0.6, 0.38),
            (0.6, 0.15),
            (0.48, 0.0),
            (0.12, 0.0),
            (0.0, 0.15),
        ]],
        'T' => &[&[(0.0, 1.0), (0.6, 1.0)], &[(0.3, 1.0), (0.3, 0.0)]],
        'C' => {
            &[&[(0.6, 0.85), (0.45, 1.0), (0.15, 1.0), (0.0, 0.85), (0.0, 0.15), (0.15, 0.0), (0.45, 0.0), (0.6, 0.15)]]
        }
        'O' => &[&[
            (0.15, 0.0),
            (0.45, 0.0),
            (0.6, 0.15),
            (0.6, 0.85),
            (0.45, 1.0),
            (0.15, 1.0),
            (0.0, 0.85),
            (0.0, 0.15),
            (0.15, 0.0),
        ]],
        'P' => &[&[(0.0, 0.0), (0.0, 1.0), (0.45, 1.0), (0.6, 0.85), (0.6, 0.62), (0.45, 0.48), (0.0, 0.48)]],
        'E' => &[&[(0.6, 1.0), (0.0, 1.0), (0.0, 0.0), (0.6, 0.0)], &[(0.0, 0.5), (0.45, 0.5)]],
        'N' => &[&[(0.0, 0.0), (0.0, 1.0), (0.6, 0.0), (0.6, 1.0)]],
        _ => &[],
    }
}

/// Neon tube lettering, centred on the origin in the XY plane, `height`
/// metres tall. Only the letters the bar's sign needs are drawn.
pub fn neon_text(text: &str, height: f32, color: Color) -> Mesh {
    let advance = 0.85 * height;
    let width = advance * text.chars().count() as f32 - 0.25 * height;
    let r = height * 0.045;
    let mut parts = Vec::new();
    for (i, c) in text.chars().enumerate() {
        let x0 = -width / 2.0 + i as f32 * advance;
        for stroke in glyph(c) {
            let pts: Vec<Vec3> =
                stroke.iter().map(|&(x, y)| Vec3::new(x0 + x * height, (y - 0.5) * height, 0.0)).collect();
            for w in pts.windows(2) {
                let (a, b) = (w[0], w[1]);
                let len = a.distance(b);
                let mid = (a + b) / 2.0;
                let rot = Quat::from_rotation_arc(Vec3::Y, (b - a) / len);
                let tube = Capsule3d::new(r, len).mesh().longitudes(8).latitudes(4).build();
                parts.push(part(tube, Transform::from_translation(mid).with_rotation(rot), color));
            }
        }
    }
    combine(parts)
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

/// A casino table: a pedestal on a wide foot, a wooden apron, a green felt
/// top and a padded leather rail round the edge.
pub fn table(b: &bar::Block) -> Mesh {
    let (w, d, h) = (b.hx * 2.0, b.hz * 2.0, b.height);
    let (wood, dark, leather) =
        (Color::srgb(0.32, 0.17, 0.08), Color::srgb(0.12, 0.07, 0.04), Color::srgb(0.16, 0.06, 0.04));
    let top = h / 2.0;
    let mut parts = vec![
        // Foot and pedestal.
        part(boxy(w * 0.55, 0.06, d * 0.55), at(0.0, -top + 0.03, 0.0), dark),
        part(boxy(w * 0.35, h - 0.2, d * 0.3), at(0.0, -0.07, 0.0), wood),
        // Apron under the top, with a brass trim along its edge.
        part(boxy(w - 0.08, 0.12, d - 0.08), at(0.0, top - 0.11, 0.0), wood),
        part(boxy(w - 0.06, 0.02, d - 0.06), at(0.0, top - 0.06, 0.0), Color::srgb(0.85, 0.65, 0.25)),
        // The felt.
        part(boxy(w, 0.04, d), at(0.0, top - 0.02, 0.0), Color::srgb(0.06, 0.36, 0.17)),
    ];
    // The rail: four padded bars, a little proud of the felt.
    for (x, z, sx, sz) in [
        (0.0, d / 2.0 - 0.05, w + 0.04, 0.12),
        (0.0, -d / 2.0 + 0.05, w + 0.04, 0.12),
        (w / 2.0 - 0.05, 0.0, 0.12, d),
        (-w / 2.0 + 0.05, 0.0, 0.12, d),
    ] {
        parts.push(part(boxy(sx, 0.07, sz), at(x, top + 0.02, z), leather));
    }
    combine(parts)
}

/// A slot machine facing +X (the room): a plinth, a cabinet with gold trim,
/// a framed reel window, a button deck, a coin tray and a lever. The reels
/// and the lit topper are separate (they change or glow).
pub fn slot_machine(b: &bar::Block) -> Mesh {
    let (w, d, h) = (b.hx * 2.0, b.hz * 2.0, b.height);
    let y = |world: f32| world - h / 2.0;
    let (body, plinth, gold, black) = (
        Color::srgb(0.45, 0.04, 0.12),
        Color::srgb(0.1, 0.05, 0.07),
        Color::srgb(0.95, 0.72, 0.25),
        Color::srgb(0.02, 0.02, 0.03),
    );
    let front = w / 2.0;
    let mut parts = vec![
        part(boxy(w + 0.04, 0.5, d + 0.04), at(0.0, y(0.25), 0.0), plinth),
        part(boxy(w, 1.1, d), at(0.0, y(1.05), 0.0), body),
        // The reel window: a black recess behind the reels, a gold frame round it.
        part(boxy(0.01, 0.26, 0.56), at(front + 0.004, y(1.2), 0.0), black),
        part(boxy(0.05, 0.03, 0.62), at(front + 0.02, y(1.345), 0.0), gold),
        part(boxy(0.05, 0.03, 0.62), at(front + 0.02, y(1.055), 0.0), gold),
        part(boxy(0.05, 0.32, 0.03), at(front + 0.02, y(1.2), 0.295), gold),
        part(boxy(0.05, 0.32, 0.03), at(front + 0.02, y(1.2), -0.295), gold),
        // The button deck, sloping toward the player, with three buttons.
        part(
            boxy(0.16, 0.04, d - 0.04),
            Transform::from_xyz(front + 0.06, y(0.93), 0.0).with_rotation(Quat::from_rotation_z(-0.3)),
            plinth,
        ),
        // The coin tray.
        part(boxy(0.12, 0.06, 0.3), at(front + 0.04, y(0.58), 0.0), Color::srgb(0.7, 0.7, 0.72)),
        part(boxy(0.1, 0.02, 0.26), at(front + 0.05, y(0.6), 0.0), black),
        // Gold corner trim.
        part(boxy(0.03, 1.1, 0.03), at(front, y(1.05), d / 2.0), gold),
        part(boxy(0.03, 1.1, 0.03), at(front, y(1.05), -d / 2.0), gold),
        // The topper's housing.
        part(boxy(w + 0.02, 0.06, d + 0.02), at(0.0, y(1.63), 0.0), gold),
        // The lever on the right side, with a red knob.
        part(
            tube(0.025, 0.08, 8),
            Transform::from_xyz(0.05, y(1.1), d / 2.0 + 0.04)
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            Color::srgb(0.75, 0.75, 0.78),
        ),
        part(tube(0.012, 0.4, 6), at(0.05, y(1.3), d / 2.0 + 0.08), Color::srgb(0.75, 0.75, 0.78)),
        part(ball(0.045), at(0.05, y(1.52), d / 2.0 + 0.08), Color::srgb(0.85, 0.05, 0.08)),
    ];
    for (i, c) in
        [Color::srgb(0.9, 0.1, 0.1), Color::srgb(0.95, 0.8, 0.1), Color::srgb(0.1, 0.7, 0.2)].into_iter().enumerate()
    {
        parts.push(part(
            boxy(0.04, 0.03, 0.06),
            Transform::from_xyz(front + 0.07, y(0.955), -0.15 + i as f32 * 0.15)
                .with_rotation(Quat::from_rotation_z(-0.3)),
            c,
        ));
    }
    combine(parts)
}

/// The five reel symbols in [`shared::slots::Symbol`] order (cherry, lemon,
/// bell, bar, seven), each about 0.12 m, drawn on the YZ plane facing +X.
pub fn reel_symbols() -> [Mesh; 5] {
    let red = Color::srgb(0.85, 0.05, 0.1);
    let stem = Color::srgb(0.15, 0.55, 0.15);
    let gold = Color::srgb(0.95, 0.7, 0.15);
    let rot = |a: f32| Quat::from_rotation_x(a);
    [
        combine(vec![
            part(ball(0.03), at(0.0, -0.025, -0.03), red),
            part(ball(0.03), at(0.0, -0.035, 0.03), red),
            part(boxy(0.01, 0.07, 0.008), Transform::from_xyz(0.0, 0.015, -0.015).with_rotation(rot(0.4)), stem),
            part(boxy(0.01, 0.07, 0.008), Transform::from_xyz(0.0, 0.01, 0.017).with_rotation(rot(-0.35)), stem),
            part(boxy(0.01, 0.02, 0.04), at(0.0, 0.05, 0.0), stem),
        ]),
        part(ball(0.05), Transform::from_scale(Vec3::new(0.4, 0.8, 1.15)), Color::srgb(0.98, 0.88, 0.1)),
        combine(vec![
            part(Cone::new(0.05, 0.08).mesh().resolution(10).build(), at(0.0, 0.0, 0.0), gold),
            part(ball(0.015), at(0.0, 0.05, 0.0), gold),
            part(ball(0.014), at(0.0, -0.045, 0.0), Color::srgb(0.5, 0.35, 0.1)),
        ]),
        combine(vec![
            part(boxy(0.012, 0.05, 0.12), Transform::IDENTITY, Color::srgb(0.05, 0.05, 0.08)),
            part(boxy(0.014, 0.008, 0.1), at(0.0, 0.012, 0.0), Color::WHITE),
            part(boxy(0.014, 0.008, 0.1), at(0.0, -0.012, 0.0), Color::WHITE),
        ]),
        combine(vec![
            part(boxy(0.012, 0.022, 0.08), at(0.0, 0.045, 0.0), red),
            part(boxy(0.012, 0.11, 0.024), Transform::from_xyz(0.0, -0.005, 0.01).with_rotation(rot(-0.38)), red),
        ]),
    ]
}

/// A face-up card (0.09 by 0.13 m, lying flat): white, with pips in the
/// suit's color (one per point up to ten), a big pip on a gold frame for
/// a face card, and one big pip for an ace.
pub fn card_face(rank: u8, red_suit: bool) -> Mesh {
    let ink = if red_suit { Color::srgb(0.8, 0.05, 0.08) } else { Color::srgb(0.05, 0.05, 0.08) };
    let mut parts = vec![part(boxy(0.09, 0.004, 0.13), Transform::IDENTITY, Color::srgb(0.96, 0.95, 0.9))];
    let pip = |x: f32, z: f32, s: f32| part(boxy(s, 0.002, s), at(x, 0.003, z), ink);
    match rank {
        1 => parts.push(pip(0.0, 0.0, 0.03)),
        11..=13 => {
            parts.push(part(boxy(0.06, 0.001, 0.09), at(0.0, 0.0025, 0.0), Color::srgb(0.9, 0.7, 0.2)));
            parts.push(pip(0.0, 0.0, 0.025));
            // Marks for J, Q, K: one, two or three bars.
            for i in 0..(rank - 10) {
                parts.push(part(boxy(0.012, 0.002, 0.004), at(-0.02 + f32::from(i) * 0.02, 0.0035, -0.035), ink));
            }
        }
        n => {
            let n = usize::from(n.min(10));
            let cols = if n <= 3 { 1 } else { 2 };
            let rows = n.div_ceil(cols);
            for i in 0..n {
                let (c, r) = (i % cols, i / cols);
                let x = if cols == 1 { 0.0 } else { -0.02 + c as f32 * 0.04 };
                let z = (r as f32 - (rows - 1) as f32 / 2.0) * (0.1 / rows.max(1) as f32);
                parts.push(pip(x, z, 0.014));
            }
        }
    }
    combine(parts)
}

/// Felt printing for the blackjack table: a gold ring for each seat and the
/// dealer's chip rack. Positions are relative to the table's center.
pub fn blackjack_layout(seats: &[(f32, f32)], half_z: f32) -> Mesh {
    let gold = Color::srgb(0.9, 0.75, 0.35);
    let mut parts = Vec::new();
    for (x, _) in seats {
        parts.push(part(
            Torus::new(0.075, 0.085).mesh().minor_resolution(4).major_resolution(20).build(),
            Transform::from_xyz(*x, 0.0, half_z - 0.27).with_scale(Vec3::new(1.0, 0.2, 1.0)),
            gold,
        ));
    }
    parts.push(part(boxy(0.5, 0.03, 0.12), at(0.0, 0.015, -half_z + 0.14), Color::srgb(0.08, 0.05, 0.04)));
    let chips = [
        Color::srgb(0.85, 0.1, 0.1),
        Color::srgb(0.1, 0.6, 0.25),
        Color::srgb(0.08, 0.08, 0.1),
        Color::srgb(0.15, 0.3, 0.85),
        Color::srgb(0.95, 0.95, 0.95),
    ];
    for (i, c) in chips.into_iter().enumerate() {
        parts.push(part(tube(0.019, 0.04, 10), at(-0.18 + i as f32 * 0.09, 0.05, -half_z + 0.14), c));
    }
    combine(parts)
}

/// Felt printing for the roulette layout: a green zero, then three rows of
/// twelve numbers in red and black, on a white grid. `x0` is the zero's
/// west edge, relative to the table's center.
pub fn roulette_layout(x0: f32, is_red: impl Fn(u8) -> bool) -> Mesh {
    let (cw, rh) = (0.1, 0.13);
    let mut parts =
        vec![part(boxy(cw * 13.0 + 0.02, 0.001, rh * 3.0 + 0.02), at(x0 + cw * 6.5, 0.0, 0.0), Color::WHITE)];
    parts.push(part(
        boxy(cw - 0.01, 0.002, rh * 3.0 - 0.01),
        at(x0 + cw / 2.0, 0.001, 0.0),
        Color::srgb(0.0, 0.5, 0.2),
    ));
    for col in 0..12u8 {
        for row in 0..3u8 {
            let n = col * 3 + 3 - row;
            let c = if is_red(n) { Color::srgb(0.7, 0.05, 0.06) } else { Color::srgb(0.06, 0.06, 0.07) };
            let x = x0 + cw * (f32::from(col) + 1.5);
            let z = (f32::from(row) - 1.0) * rh;
            parts.push(part(boxy(cw - 0.01, 0.002, rh - 0.01), at(x, 0.001, z), c));
        }
    }
    combine(parts)
}

/// The roulette wheel's turning part: a dark wooden disc, 37 pockets in
/// wheel order round the rim, a gold ring inside them and a gold turret.
pub fn wheel(order: &[u8], is_red: impl Fn(u8) -> bool) -> Mesh {
    let gold = Color::srgb(0.95, 0.75, 0.3);
    let mut parts = vec![
        part(tube(0.38, 0.05, 37), Transform::IDENTITY, Color::srgb(0.22, 0.09, 0.04)),
        part(
            Torus::new(0.255, 0.27).mesh().minor_resolution(4).major_resolution(37).build(),
            at(0.0, 0.025, 0.0),
            gold,
        ),
        part(Cone::new(0.08, 0.08).mesh().resolution(12).build(), at(0.0, 0.065, 0.0), gold),
        part(tube(0.012, 0.08, 6), at(0.0, 0.12, 0.0), gold),
    ];
    for (i, n) in order.iter().enumerate() {
        let a = i as f32 / order.len() as f32 * std::f32::consts::TAU;
        let c = if *n == 0 {
            Color::srgb(0.0, 0.55, 0.2)
        } else if is_red(*n) {
            Color::srgb(0.7, 0.05, 0.05)
        } else {
            Color::srgb(0.05, 0.05, 0.05)
        };
        parts.push(part(
            boxy(0.06, 0.012, 0.075),
            Transform::from_xyz(a.sin() * 0.32, 0.03, a.cos() * 0.32).with_rotation(Quat::from_rotation_y(a)),
            c,
        ));
    }
    combine(parts)
}

/// The bowl the wheel turns in: a wide wooden rim.
pub fn wheel_bowl() -> Mesh {
    combine(vec![
        part(tube(0.46, 0.08, 37), Transform::IDENTITY, Color::srgb(0.3, 0.16, 0.07)),
        part(
            Torus::new(0.42, 0.47).mesh().minor_resolution(6).major_resolution(37).build(),
            at(0.0, 0.04, 0.0),
            Color::srgb(0.38, 0.2, 0.09),
        ),
    ])
}

/// The back bar's parts, each one mesh: wood (cabinets, shelf brackets),
/// bottles (glass, painted), glow (the warm strips under the shelves) and
/// mirror (the panels behind the shelves).
pub struct BackBar {
    pub wood: Mesh,
    pub bottles: Mesh,
    pub glow: Mesh,
    pub mirror: Mesh,
}

/// A back bar against a wall at `z` (the wall's face; the bar stands toward
/// +Z), along each `(x0, x1)` span: a low cabinet, a mirror, three glass
/// shelves full of bottles, a warm strip under each shelf.
pub fn back_bar(z: f32, spans: &[(f32, f32)]) -> BackBar {
    let wood = Color::srgb(0.22, 0.12, 0.06);
    let top = Color::srgb(0.45, 0.28, 0.14);
    let colors = [
        Color::srgb(0.55, 0.3, 0.08),
        Color::srgb(0.1, 0.35, 0.12),
        Color::srgb(0.85, 0.85, 0.82),
        Color::srgb(0.45, 0.12, 0.1),
        Color::srgb(0.15, 0.3, 0.6),
        Color::srgb(0.7, 0.55, 0.15),
        Color::srgb(0.08, 0.08, 0.08),
    ];
    let (mut w, mut b, mut g, mut m) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut seed = 17u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (seed >> 16) as f32 / 65_536.0
    };
    let mut bottle = |x: f32, y: f32, z: f32, rnd: &mut dyn FnMut() -> f32| {
        let c = colors[(rnd() * colors.len() as f32) as usize % colors.len()];
        let h = 0.18 + rnd() * 0.12;
        let r = 0.03 + rnd() * 0.012;
        b.push(part(tube(r, h * 0.65, 6), at(x, y + h * 0.325, z), c));
        b.push(part(tube(r * 0.4, h * 0.35, 5), at(x, y + h * 0.825, z), c));
        b.push(part(tube(r * 0.45, 0.02, 5), at(x, y + h + 0.01, z), Color::srgb(0.8, 0.7, 0.4)));
    };
    for &(x0, x1) in spans {
        let (cx, len) = ((x0 + x1) / 2.0, x1 - x0);
        // The cabinet with a top and door lines.
        w.push(part(boxy(len, 0.95, 0.45), at(cx, 0.475, z + 0.225), wood));
        w.push(part(boxy(len + 0.04, 0.05, 0.5), at(cx, 0.975, z + 0.25), top));
        let doors = (len / 0.6).floor().max(1.0) as usize;
        for i in 1..doors {
            let x = x0 + len * i as f32 / doors as f32;
            w.push(part(boxy(0.015, 0.8, 0.01), at(x, 0.48, z + 0.455), Color::srgb(0.1, 0.05, 0.03)));
        }
        // The mirror, framed.
        m.push(part(boxy(len - 0.1, 1.2, 0.01), at(cx, 1.68, z + 0.012), Color::WHITE));
        w.push(part(boxy(len, 0.06, 0.04), at(cx, 2.31, z + 0.02), wood));
        // Shelves: glass on brackets, a strip of light under each, bottles on top.
        for (k, y) in [1.3f32, 1.68, 2.06].into_iter().enumerate() {
            w.push(part(boxy(len - 0.12, 0.02, 0.24), at(cx, y, z + 0.13), Color::srgb(0.55, 0.6, 0.6)));
            g.push(part(boxy(len - 0.16, 0.012, 0.02), at(cx, y - 0.018, z + 0.22), Color::WHITE));
            let mut x = x0 + 0.12 + rnd() * 0.04;
            while x < x1 - 0.12 {
                bottle(x, y + 0.01, z + 0.13 + (rnd() - 0.5) * 0.06, &mut rnd);
                x += 0.085 + rnd() * 0.05 + if k == 1 { 0.02 } else { 0.0 };
            }
        }
        // A few bottles and glasses on the cabinet top.
        let mut x = x0 + 0.3;
        while x < x1 - 0.3 {
            if rnd() < 0.5 {
                bottle(x, 1.0, z + 0.3, &mut rnd);
            }
            x += 0.35;
        }
    }
    BackBar { wood: combine(w), bottles: combine(b), glow: combine(g), mirror: combine(m) }
}

/// The order marker: a frothy beer mug, about 0.25 m tall, centered.
pub fn order_mug() -> Mesh {
    let beer = Color::srgb(1.0, 0.68, 0.12);
    let foam = Color::srgb(1.0, 0.98, 0.92);
    combine(vec![
        part(tube(0.08, 0.16, 12), at(0.0, -0.03, 0.0), beer),
        part(tube(0.085, 0.03, 12), at(0.0, 0.065, 0.0), foam),
        part(ball(0.045), at(-0.03, 0.085, 0.02), foam),
        part(ball(0.05), at(0.03, 0.09, -0.01), foam),
        part(
            Torus::new(0.035, 0.055).mesh().minor_resolution(6).major_resolution(10).build(),
            Transform::from_xyz(0.1, -0.02, 0.0).with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            Color::srgb(0.95, 0.75, 0.3),
        ),
    ])
}

/// A beer tap tower for the counter top, centered on its foot: a drip
/// tray, a chrome column, a T bar with three faucets and colored handles.
pub fn tap_tower() -> Mesh {
    let chrome = Color::srgb(0.8, 0.8, 0.82);
    let mut parts = vec![
        part(boxy(0.5, 0.02, 0.16), at(0.0, 0.01, 0.12), Color::srgb(0.15, 0.15, 0.16)),
        part(tube(0.05, 0.42, 12), at(0.0, 0.21, 0.0), chrome),
        part(
            tube(0.04, 0.5, 12),
            Transform::from_xyz(0.0, 0.42, 0.0).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            chrome,
        ),
    ];
    for (i, c) in
        [Color::srgb(0.9, 0.7, 0.1), Color::srgb(0.1, 0.1, 0.1), Color::srgb(0.75, 0.1, 0.1)].into_iter().enumerate()
    {
        let x = -0.17 + i as f32 * 0.17;
        parts.push(part(tube(0.012, 0.08, 6), at(x, 0.36, 0.05), chrome));
        parts.push(part(boxy(0.035, 0.16, 0.035), at(x, 0.54, 0.02), c));
        parts.push(part(ball(0.022), at(x, 0.63, 0.02), c));
    }
    combine(parts)
}

// ---------- Small props (the cartoony set: flat colors, chunky shapes) ----------

const CHROME: Color = Color::srgb(0.78, 0.8, 0.84);
const DARK: Color = Color::srgb(0.1, 0.1, 0.12);

/// A bar stool around its physics body's center (0.38 m up): a round red
/// cushion on a chrome post, a foot ring and a heavy base.
pub fn bar_stool() -> Mesh {
    let red = Color::srgb(0.78, 0.12, 0.14);
    combine(vec![
        part(tube(0.21, 0.03, 16), at(0.0, -0.365, 0.0), DARK),
        part(tube(0.035, 0.66, 10), at(0.0, -0.05, 0.0), CHROME),
        part(
            Torus::new(0.14, 0.17).mesh().minor_resolution(6).major_resolution(16).build(),
            at(0.0, -0.12, 0.0),
            CHROME,
        ),
        part(tube(0.2, 0.04, 16), at(0.0, 0.27, 0.0), DARK),
        part(tube(0.21, 0.09, 16), at(0.0, 0.33, 0.0), red),
    ])
}

/// A pendant lamp hanging from its origin (the ceiling): a cord, a green
/// enamel shade. The bulb is separate (it glows).
pub fn pendant_lamp() -> (Mesh, Mesh) {
    let shade = Color::srgb(0.1, 0.35, 0.22);
    let body = combine(vec![
        part(tube(0.01, 0.7, 6), at(0.0, -0.35, 0.0), DARK),
        part(tube(0.05, 0.08, 10), at(0.0, -0.72, 0.0), Color::srgb(0.75, 0.6, 0.3)),
        part(Cone::new(0.28, 0.22).mesh().resolution(16).build(), at(0.0, -0.85, 0.0), shade),
    ]);
    (body, part(ball(0.08), at(0.0, -0.95, 0.0), Color::WHITE))
}

/// A dartboard facing +Z: black rim, alternating rings, a bullseye.
pub fn dartboard() -> Mesh {
    let flat = |r: f32, d: f32, c: Color| {
        part(tube(r, d, 20), Transform::from_xyz(0.0, 0.0, d / 2.0).with_rotation(Quat::from_rotation_x(FRAC_PI_2)), c)
    };
    combine(vec![
        flat(0.25, 0.04, DARK),
        flat(0.22, 0.045, Color::srgb(0.85, 0.15, 0.12)),
        flat(0.18, 0.05, Color::srgb(0.95, 0.9, 0.75)),
        flat(0.12, 0.055, Color::srgb(0.15, 0.55, 0.25)),
        flat(0.07, 0.06, Color::srgb(0.95, 0.9, 0.75)),
        flat(0.025, 0.065, Color::srgb(0.85, 0.15, 0.12)),
    ])
}

/// A wall clock facing +Z at ten past ten.
pub fn wall_clock() -> Mesh {
    let rot_x = Quat::from_rotation_x(FRAC_PI_2);
    combine(vec![
        part(tube(0.22, 0.05, 24), Transform::from_xyz(0.0, 0.0, 0.025).with_rotation(rot_x), DARK),
        part(
            tube(0.19, 0.055, 24),
            Transform::from_xyz(0.0, 0.0, 0.03).with_rotation(rot_x),
            Color::srgb(0.97, 0.95, 0.88),
        ),
        part(
            boxy(0.02, 0.13, 0.01),
            Transform::from_xyz(-0.05, 0.04, 0.06).with_rotation(Quat::from_rotation_z(1.0)),
            DARK,
        ),
        part(
            boxy(0.02, 0.17, 0.01),
            Transform::from_xyz(0.06, 0.06, 0.06).with_rotation(Quat::from_rotation_z(-0.6)),
            DARK,
        ),
    ])
}

/// A till for the counter top, facing +Z.
pub fn register() -> Mesh {
    let body = Color::srgb(0.3, 0.32, 0.36);
    combine(vec![
        part(boxy(0.45, 0.12, 0.4), at(0.0, 0.06, 0.0), body),
        part(
            boxy(0.42, 0.14, 0.25),
            Transform::from_xyz(0.0, 0.17, -0.05).with_rotation(Quat::from_rotation_x(-0.35)),
            body,
        ),
        part(
            boxy(0.36, 0.08, 0.02),
            Transform::from_xyz(0.0, 0.2, 0.07).with_rotation(Quat::from_rotation_x(-0.35)),
            Color::srgb(0.85, 0.85, 0.8),
        ),
        part(boxy(0.2, 0.1, 0.03), at(0.0, 0.32, -0.15), Color::srgb(0.1, 0.5, 0.25)),
    ])
}

/// A yellow wet-floor sign.
pub fn wet_floor_sign() -> Mesh {
    let yellow = Color::srgb(1.0, 0.8, 0.1);
    combine(vec![
        part(
            boxy(0.32, 0.6, 0.02),
            Transform::from_xyz(0.0, 0.3, 0.1).with_rotation(Quat::from_rotation_x(-0.32)),
            yellow,
        ),
        part(
            boxy(0.32, 0.6, 0.02),
            Transform::from_xyz(0.0, 0.3, -0.1).with_rotation(Quat::from_rotation_x(0.32)),
            yellow,
        ),
        part(
            boxy(0.12, 0.12, 0.03),
            Transform::from_xyz(0.0, 0.32, 0.115).with_rotation(Quat::from_rotation_x(-0.32)),
            DARK,
        ),
    ])
}

/// A TV on a low stand, facing +Z. The screen is separate (it glows).
pub fn tv() -> (Mesh, Mesh) {
    let wood = Color::srgb(0.35, 0.22, 0.13);
    let body = combine(vec![
        part(boxy(1.2, 0.5, 0.45), at(0.0, 0.25, 0.0), wood),
        part(boxy(1.1, 0.04, 0.4), at(0.0, 0.52, 0.0), Color::srgb(0.45, 0.3, 0.18)),
        part(boxy(0.2, 0.05, 0.15), at(0.0, 0.565, 0.0), DARK),
        part(boxy(0.05, 0.12, 0.05), at(0.0, 0.64, 0.0), DARK),
        part(boxy(1.05, 0.62, 0.07), at(0.0, 1.0, 0.0), DARK),
    ]);
    (body, part(boxy(0.95, 0.53, 0.01), at(0.0, 1.0, 0.04), Color::WHITE))
}

/// A wooden barrel with two iron bands.
pub fn barrel() -> Mesh {
    let wood = Color::srgb(0.48, 0.28, 0.14);
    combine(vec![
        part(tube(0.3, 0.9, 14), at(0.0, 0.45, 0.0), wood),
        part(tube(0.31, 0.06, 14), at(0.0, 0.2, 0.0), DARK),
        part(tube(0.31, 0.06, 14), at(0.0, 0.7, 0.0), DARK),
        part(tube(0.27, 0.01, 14), at(0.0, 0.905, 0.0), Color::srgb(0.38, 0.22, 0.1)),
    ])
}

/// A rooftop air conditioner, its fan facing +Z.
pub fn aircon() -> Mesh {
    let grey = Color::srgb(0.75, 0.77, 0.78);
    combine(vec![
        part(boxy(1.0, 0.75, 0.6), at(0.0, 0.42, 0.0), grey),
        part(boxy(1.04, 0.05, 0.64), at(0.0, 0.0, 0.0), DARK),
        part(
            tube(0.25, 0.02, 16),
            Transform::from_xyz(-0.15, 0.42, 0.31).with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
            DARK,
        ),
        part(boxy(0.2, 0.5, 0.02), at(0.3, 0.42, 0.31), Color::srgb(0.55, 0.57, 0.58)),
    ])
}

/// A lifebuoy facing +Z: white with four red quarters.
pub fn lifebuoy() -> Mesh {
    let mut m = part(
        Torus::new(0.17, 0.3).mesh().minor_resolution(8).major_resolution(24).build(),
        Transform::from_rotation(Quat::from_rotation_x(FRAC_PI_2)),
        Color::WHITE,
    );
    if let (Some(pos), Some(bevy::mesh::VertexAttributeValues::Float32x4(colors))) = (
        m.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a| a.as_float3()).map(|p| p.to_vec()),
        m.attribute_mut(Mesh::ATTRIBUTE_COLOR),
    ) {
        let red = Color::srgb(0.85, 0.1, 0.1).to_linear().to_f32_array();
        for (c, p) in colors.iter_mut().zip(pos) {
            let a = p[1].atan2(p[0]).rem_euclid(FRAC_PI_2);
            if a < FRAC_PI_2 / 2.0 {
                *c = red;
            }
        }
    }
    m
}

/// A laptop on its origin (a table top), facing +Z. The screen glows.
pub fn laptop() -> (Mesh, Mesh) {
    let body = combine(vec![
        part(boxy(0.38, 0.02, 0.26), at(0.0, 0.01, 0.0), Color::srgb(0.25, 0.26, 0.3)),
        part(
            boxy(0.38, 0.26, 0.015),
            Transform::from_xyz(0.0, 0.14, -0.14).with_rotation(Quat::from_rotation_x(-0.25)),
            Color::srgb(0.25, 0.26, 0.3),
        ),
    ]);
    let screen = part(
        boxy(0.34, 0.21, 0.005),
        Transform::from_xyz(0.0, 0.14, -0.13).with_rotation(Quat::from_rotation_x(-0.25)),
        Color::WHITE,
    );
    (body, screen)
}

/// The basement breaker box on its wall, facing +Z.
pub fn breaker_box() -> Mesh {
    combine(vec![
        part(boxy(0.5, 0.7, 0.15), at(0.0, 0.0, 0.075), Color::srgb(0.55, 0.58, 0.6)),
        part(boxy(0.42, 0.6, 0.01), at(0.0, 0.0, 0.155), Color::srgb(0.5, 0.53, 0.55)),
        part(boxy(0.05, 0.12, 0.03), at(0.15, 0.0, 0.17), DARK),
        part(boxy(0.3, 0.06, 0.01), at(0.0, 0.25, 0.16), Color::srgb(1.0, 0.8, 0.1)),
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
    fn the_back_bar_fits_the_budget() {
        let b = back_bar(-7.0, &[(-5.0, 0.8), (3.2, 5.8)]);
        let t = triangles(&b.bottles) + triangles(&b.wood) + triangles(&b.glow) + triangles(&b.mirror);
        assert!(t < 30_000, "{t}");
        assert!(triangles(&tap_tower()) < 600);
    }

    #[test]
    fn casino_pieces_stay_cheap() {
        for s in reel_symbols() {
            assert!(triangles(&s) < 300);
        }
        for rank in 1..=13 {
            assert!(triangles(&card_face(rank, rank % 2 == 0)) < 200, "{rank}");
        }
        let order: Vec<u8> = (0..37).collect();
        assert!(triangles(&wheel(&order, |n| n % 2 == 1)) < 2_000);
        assert!(triangles(&roulette_layout(-0.5, |n| n % 2 == 1)) < 600);
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
