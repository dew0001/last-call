//! `tools bake_lighting [--check]`: bake a lightmap for every room's floor
//! (plan section 7) into `web/assets/lightmaps/<room>.ktx2`.
//!
//! Each texel (4 per meter) gathers the room's lamps (warm, falling off with
//! distance, shadowed by walls, the counter and the tables) and, outdoors,
//! the cold moonlight (shadowed by the walls), plus a little ambient light,
//! darkened near walls (ambient occlusion). Rays test the same blocks the
//! walk code collides with. The output is uncompressed RGBA8 sRGB KTX2 (no
//! supercompression: the zstd path failed in WebKit, `docs/DECISIONS.md`).
//! With `--check` nothing is written; the command fails if a file is stale.

use shared::bar::{self, Block};
use shared::world::{self, AREAS, Area, Room};

/// Texels per meter.
pub const DENSITY: f32 = 4.0;
/// The largest lightmap side (the plan's 2048 atlas per room).
pub const MAX_SIDE: u32 = 2048;
pub const OUT_DIR: &str = "web/assets/lightmaps";

const LAMP_COLOR: [f32; 3] = [1.0, 0.78, 0.5];
const LAMP_POWER: f32 = 2.2;
const MOON_COLOR: [f32; 3] = [0.35, 0.45, 0.75];
const AMBIENT_IN: [f32; 3] = [0.10, 0.07, 0.05];
const AMBIENT_OUT: [f32; 3] = [0.05, 0.07, 0.12];

/// Every block light can hit: the walls and the bar's furniture.
fn blockers() -> Vec<Block> {
    world::walls().iter().chain(bar::BLOCKS.iter()).copied().collect()
}

/// The moon's direction, toward the moon (the client's directional light).
fn moon_dir() -> [f32; 3] {
    let d = [5.0f32, 20.0, 10.0];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    [d[0] / l, d[1] / l, d[2] / l]
}

/// Does the segment from `a` toward `dir` (length `len`) hit a block?
fn blocked(blocks: &[Block], a: [f32; 3], dir: [f32; 3], len: f32) -> bool {
    blocks.iter().any(|b| {
        let lo = [b.cx - b.hx, 0.0, b.cz - b.hz];
        let hi = [b.cx + b.hx, b.height, b.cz + b.hz];
        let (mut t0, mut t1) = (0.0f32, len);
        for i in 0..3 {
            if dir[i].abs() < 1e-9 {
                if a[i] < lo[i] || a[i] > hi[i] {
                    return false;
                }
                continue;
            }
            let (mut n, mut f) = ((lo[i] - a[i]) / dir[i], (hi[i] - a[i]) / dir[i]);
            if n > f {
                std::mem::swap(&mut n, &mut f);
            }
            t0 = t0.max(n);
            t1 = t1.min(f);
            if t0 > t1 {
                return false;
            }
        }
        true
    })
}

/// Light arriving at floor point (x, z).
fn light_at(blocks: &[Block], lamps: &[(Room, f32, f32)], x: f32, z: f32) -> [f32; 3] {
    let room = world::room_at(x, z);
    let outdoors = room.is_none_or(|r| r.outdoors());
    let p = [x, 0.02, z];
    let mut c = if outdoors { AMBIENT_OUT } else { AMBIENT_IN };
    for (_, lx, lz) in lamps {
        let d = [lx - x, world::LAMP_Y - p[1], lz - z];
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if dist > 14.0 {
            continue;
        }
        let dir = [d[0] / dist, d[1] / dist, d[2] / dist];
        if blocked(blocks, p, dir, dist - 0.05) {
            continue;
        }
        let k = LAMP_POWER * dir[1] / (1.0 + dist * dist * 0.12);
        for (ci, lc) in c.iter_mut().zip(LAMP_COLOR) {
            *ci += lc * k;
        }
    }
    if outdoors {
        let m = moon_dir();
        if !blocked(blocks, p, m, 60.0) {
            for (ci, mc) in c.iter_mut().zip(MOON_COLOR) {
                *ci += mc * m[1];
            }
        }
    }
    // Ambient occlusion: darker where walls are close.
    let mut near = 0;
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        if blocked(blocks, [x, 0.3, z], [a.cos(), 0.0, a.sin()], 0.7) {
            near += 1;
        }
    }
    let ao = 1.0 - 0.06 * near as f32;
    c.map(|v| v * ao)
}

fn srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// The floor areas that get a lightmap (the office shares the bar's floor).
pub fn areas() -> Vec<Area> {
    AREAS.iter().filter(|a| a.room != Room::Office).copied().collect()
}

pub fn size(a: &Area) -> (u32, u32) {
    let w = ((a.x1 - a.x0) * DENSITY).ceil() as u32;
    let h = ((a.z1 - a.z0) * DENSITY).ceil() as u32;
    (w.clamp(1, MAX_SIDE), h.clamp(1, MAX_SIDE))
}

/// RGBA8 sRGB texels, row 0 at the area's -Z edge (the floor plane's v = 0).
pub fn bake(a: &Area) -> (u32, u32, Vec<u8>) {
    let blocks = blockers();
    let lamps = world::lamps();
    let (w, h) = size(a);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for j in 0..h {
        for i in 0..w {
            let x = a.x0 + (i as f32 + 0.5) / w as f32 * (a.x1 - a.x0);
            let z = a.z0 + (j as f32 + 0.5) / h as f32 * (a.z1 - a.z0);
            let c = light_at(&blocks, &lamps, x, z);
            out.extend([srgb(c[0]), srgb(c[1]), srgb(c[2]), 255]);
        }
    }
    (w, h, out)
}

/// A KTX2 file: one level, R8G8B8A8_SRGB, no supercompression.
pub fn ktx2(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    const VK_FORMAT_R8G8B8A8_SRGB: u32 = 43;
    // Data format descriptor: one basic block with four 8-bit samples.
    let mut dfd_block = Vec::new();
    let block_size: u32 = 24 + 16 * 4;
    dfd_block.extend(0u32.to_le_bytes()); // vendor 0, descriptor type 0
    dfd_block.extend((2u32 | (block_size << 16)).to_le_bytes()); // version 2, size
    dfd_block.extend([1u8, 1, 2, 0]); // model RGBSDA, primaries BT709, transfer sRGB, flags
    dfd_block.extend([0u8; 4]); // texel block dimensions (1x1x1x1, stored minus one)
    dfd_block.extend([4u8, 0, 0, 0, 0, 0, 0, 0]); // bytes per plane
    for (ch, offset) in [(0u8, 0u16), (1, 8), (2, 16), (15 | 0x40, 24)] {
        dfd_block.extend(offset.to_le_bytes());
        dfd_block.push(7); // bit length minus one
        dfd_block.push(ch);
        dfd_block.extend([0u8; 4]); // sample position
        dfd_block.extend(0u32.to_le_bytes());
        dfd_block.extend(255u32.to_le_bytes());
    }
    let dfd_len = 4 + dfd_block.len() as u32;
    let header_len = 12 + 9 * 4 + 4 * 4 + 2 * 8;
    let level_index_len = 3 * 8;
    let dfd_offset = header_len + level_index_len;
    let data_offset = (dfd_offset + dfd_len).next_multiple_of(16);
    let mut f = Vec::new();
    f.extend([0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A]);
    for v in [VK_FORMAT_R8G8B8A8_SRGB, 1, w, h, 0, 0, 1, 1, 0] {
        f.extend(v.to_le_bytes());
    }
    for v in [dfd_offset, dfd_len, 0, 0] {
        f.extend(v.to_le_bytes());
    }
    f.extend(0u64.to_le_bytes()); // supercompression global data
    f.extend(0u64.to_le_bytes());
    for v in [u64::from(data_offset), rgba.len() as u64, rgba.len() as u64] {
        f.extend(v.to_le_bytes());
    }
    f.extend(dfd_len.to_le_bytes());
    f.extend(dfd_block);
    f.resize(data_offset as usize, 0);
    f.extend(rgba);
    f
}

pub fn file_name(room: Room) -> String {
    format!("{room:?}").to_lowercase() + ".ktx2"
}

pub fn run(check: bool) -> i32 {
    let mut stale = 0;
    for a in areas() {
        let (w, h, rgba) = bake(&a);
        let bytes = ktx2(w, h, &rgba);
        let path = format!("{OUT_DIR}/{}", file_name(a.room));
        if check {
            if std::fs::read(&path).ok().as_deref() != Some(&bytes[..]) {
                eprintln!("stale: {path} (run: cargo run -p last_call_tools -- bake_lighting)");
                stale += 1;
            }
        } else {
            std::fs::create_dir_all(OUT_DIR).expect("create the lightmap folder");
            std::fs::write(&path, &bytes).expect("write the lightmap");
            println!("{path}: {w}x{h}, {} bytes", bytes.len());
        }
    }
    i32::from(stale > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texel(a: &Area, data: &(u32, u32, Vec<u8>), x: f32, z: f32) -> [u8; 3] {
        let (w, h, px) = data;
        let i = (((x - a.x0) / (a.x1 - a.x0)) * *w as f32) as u32;
        let j = (((z - a.z0) / (a.z1 - a.z0)) * *h as f32) as u32;
        let o = ((j * w + i) * 4) as usize;
        [px[o], px[o + 1], px[o + 2]]
    }

    #[test]
    fn lamps_light_the_bar_and_the_moon_lights_the_lot() {
        let bar = world::area_of(Room::Bar);
        let lit = bake(&bar);
        let under_lamp = texel(&bar, &lit, 0.0, 0.5);
        let corner = texel(&bar, &lit, -9.8, 6.8);
        assert!(under_lamp[0] > corner[0] + 30, "{under_lamp:?} {corner:?}");
        assert!(under_lamp[0] > under_lamp[2], "warm inside");
        let lot = world::area_of(Room::ParkingLot);
        let out = bake(&lot);
        let open = texel(&lot, &out, 0.0, 20.0);
        assert!(open[2] > open[0], "cold outside: {open:?}");
    }

    #[test]
    fn the_counter_casts_a_shadow() {
        // Behind the counter, a lamp's light is cut off where the counter is between.
        let blocks = blockers();
        let c = bar::BLOCKS.iter().find(|b| b.kind == bar::BlockKind::Counter).unwrap();
        assert!(blocked(&blocks, [c.cx, 0.3, c.cz + c.hz + 0.3], [0.0, 0.0, -1.0], 2.0));
        assert!(!blocked(&blocks, [0.0, 0.3, 3.0], [0.0, 1.0, 0.0], 2.0));
    }

    #[test]
    fn every_lightmap_fits_the_atlas_budget_and_is_valid_ktx2() {
        for a in areas() {
            let (w, h) = size(&a);
            assert!(w <= MAX_SIDE && h <= MAX_SIDE);
            let rgba = vec![9u8; (w * h * 4) as usize];
            let f = ktx2(w, h, &rgba);
            assert_eq!(&f[..12], &[0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A]);
            assert_eq!(u32::from_le_bytes(f[12..16].try_into().unwrap()), 43);
            assert_eq!(u32::from_le_bytes(f[20..24].try_into().unwrap()), w);
            let off = u64::from_le_bytes(f[80..88].try_into().unwrap()) as usize;
            let len = u64::from_le_bytes(f[88..96].try_into().unwrap()) as usize;
            assert_eq!(&f[off..off + len], &rgba[..]);
            assert_eq!(off % 16, 0);
        }
    }

    #[test]
    fn the_baked_files_are_up_to_date() {
        // The same check CI runs: re-baking gives the committed bytes.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/assets/lightmaps");
        for a in areas() {
            let (w, h, rgba) = bake(&a);
            let committed = std::fs::read(format!("{dir}/{}", file_name(a.room))).expect("baked file");
            assert!(committed == ktx2(w, h, &rgba), "{:?} is stale: run tools bake_lighting", a.room);
        }
    }
}
