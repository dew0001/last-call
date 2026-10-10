#!/usr/bin/env python3
"""Download the game's CC0 textures and models from Poly Haven.

    python3 scripts/fetch-assets.py            # fetch what is missing
    python3 scripts/fetch-assets.py --force    # fetch everything again

Textures land in web/assets/tex/<id>_{d,n,a}.jpg: colour (d), OpenGL normal
(n), and AO/roughness/metal packed in R/G/B (a, Bevy's metallic-roughness
layout). They are scaled down for the web: colour to TEX_COLOR px, the others
to TEX_DETAIL px. Models land in web/assets/models/<id>/ as glTF with their
textures scaled to MODEL_TEX px.

The results are committed, so builds and CI need no network. Everything here
is CC0 (https://polyhaven.com/license); docs/CREDITS.md lists it.
"""

import io
import json
import os
import sys
import urllib.request

from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_TEX = os.path.join(ROOT, "web", "assets", "tex")
OUT_MODELS = os.path.join(ROOT, "web", "assets", "models")
API = "https://api.polyhaven.com/files/"

TEX_COLOR = 1024
TEX_DETAIL = 512
MODEL_TEX = 512

# Surface textures (crates/client/src/look.rs names them).
TEXTURES = [
    "wood_floor_worn",  # main bar floor
    "herringbone_parquet",  # office floor
    "floor_tiles_06",  # kitchen floor
    "dirty_carpet",  # back room floor
    "concrete_floor_worn_001",  # stairwell and basement floors
    "brushed_concrete",  # roof
    "asphalt_02",  # parking lot
    "wood_floor_deck",  # pier
    "brown_brick_02",  # bar walls
    "beige_wall_001",  # office walls
    "long_white_tiles",  # kitchen walls
    "wood_plank_wall",  # back room walls
    "concrete_block_wall",  # basement and stairwell walls
    "dark_brick_wall",  # outside walls
    "concrete_wall_003",  # low walls and parapets
    "dark_planks",  # the counter
    "metal_plate",  # the safe and machine bodies
    "fabric_leather_02",  # stool seats, booths
]

# Props and furniture.
MODELS = [
    "bar_chair_round_01",
    "wine_bottles_01",
    "CashRegister_01",
    "hanging_industrial_lamp",
    "dartboard",
    "street_lamp_01",
    "covered_car",
    "Barrel_01",
    "wooden_crate_01",
    "metal_trash_can",
    "WetFloorSign_01",
    "potted_plant_01",
    "Television_01",
    "Sofa_01",
    "wall_clock",
    "hanging_picture_frame_01",
    "metal_office_desk",
    "electric_stove",
    "steel_frame_shelves_01",
    "cardboard_box_01",
    "lifebuoy",
    "wooden_picnic_table",
    "exterior_aircon_unit",
    "fire_hydrant",
]


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "last-call-asset-fetch"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def scaled_jpg(data, size, quality=85):
    img = Image.open(io.BytesIO(data)).convert("RGB")
    if img.width > size:
        img = img.resize((size, size * img.height // img.width), Image.LANCZOS)
    out = io.BytesIO()
    img.save(out, "JPEG", quality=quality, optimize=True, progressive=False)
    return out.getvalue()


def fetch_texture(tid, force):
    paths = {k: os.path.join(OUT_TEX, f"{tid}_{k}.jpg") for k in "dna"}
    if not force and all(os.path.exists(p) for p in paths.values()):
        return
    files = json.loads(get(API + tid))
    pick = lambda key: files[key]["1k"]["jpg"]["url"]
    arm_key = "arm" if "arm" in files else None
    with open(paths["d"], "wb") as f:
        f.write(scaled_jpg(get(pick("Diffuse")), TEX_COLOR, 82))
    with open(paths["n"], "wb") as f:
        f.write(scaled_jpg(get(pick("nor_gl")), TEX_DETAIL, 90))
    if arm_key:
        arm = get(pick(arm_key))
    else:
        # Build AO/rough/metal from the separate maps.
        rough = Image.open(io.BytesIO(get(pick("Rough")))).convert("L")
        ao = Image.open(io.BytesIO(get(pick("AO")))).convert("L") if "AO" in files else Image.new("L", rough.size, 255)
        metal = Image.new("L", rough.size, 0)
        buf = io.BytesIO()
        Image.merge("RGB", (ao.resize(rough.size), rough, metal)).save(buf, "PNG")
        arm = buf.getvalue()
    with open(paths["a"], "wb") as f:
        f.write(scaled_jpg(arm, TEX_DETAIL, 90))
    print(f"texture {tid}")


def fetch_model(mid, force):
    out = os.path.join(OUT_MODELS, mid)
    gltf_path = os.path.join(out, f"{mid}.gltf")
    if not force and os.path.exists(gltf_path):
        return
    files = json.loads(get(API + mid))
    entry = files["gltf"]["1k"]["gltf"]
    os.makedirs(out, exist_ok=True)
    gltf = json.loads(get(entry["url"]))
    for rel, inc in entry.get("include", {}).items():
        data = get(inc["url"])
        dest = os.path.join(out, rel)
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        if rel.lower().endswith((".jpg", ".jpeg", ".png")):
            # Scale the model's textures down and store them as JPEG.
            img = Image.open(io.BytesIO(data))
            has_alpha = img.mode in ("RGBA", "LA") and img.getextrema()[-1][0] < 255
            if has_alpha:
                img = img.convert("RGBA")
                if img.width > MODEL_TEX:
                    img = img.resize((MODEL_TEX, MODEL_TEX * img.height // img.width), Image.LANCZOS)
                buf = io.BytesIO()
                img.save(buf, "PNG", optimize=True)
                new_rel = os.path.splitext(rel)[0] + ".png"
            else:
                buf = io.BytesIO(scaled_jpg(data, MODEL_TEX, 85))
                new_rel = os.path.splitext(rel)[0] + ".jpg"
            if new_rel != rel:
                for image in gltf.get("images", []):
                    if image.get("uri") == rel:
                        image["uri"] = new_rel
                        image["mimeType"] = "image/png" if new_rel.endswith(".png") else "image/jpeg"
                dest = os.path.join(out, new_rel)
            with open(dest, "wb") as f:
                f.write(buf.getvalue())
        else:
            with open(dest, "wb") as f:
                f.write(data)
    with open(gltf_path, "w") as f:
        json.dump(gltf, f, separators=(",", ":"))
    print(f"model {mid}")


def main():
    force = "--force" in sys.argv
    os.makedirs(OUT_TEX, exist_ok=True)
    os.makedirs(OUT_MODELS, exist_ok=True)
    for tid in TEXTURES:
        fetch_texture(tid, force)
    for mid in MODELS:
        fetch_model(mid, force)


if __name__ == "__main__":
    main()
