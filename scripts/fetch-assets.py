#!/usr/bin/env python3
"""Download the game's CC0 art and prepare it for the web.

    python3 scripts/fetch-assets.py            # fetch what is missing
    python3 scripts/fetch-assets.py --force    # fetch everything again

- web/assets/kk/{furniture,restaurant,city}/: KayKit Furniture, Restaurant
  and City Builder Bits (Kay Lousberg, CC0), glTF with one shared gradient
  texture per pack, from their GitHub repositories.
- web/assets/people/: Quaternius "Ultimate Modular Men/Women" (CC0),
  rigged, with Idle, Walk, Run, Interact and Wave clips. The copies found
  online are meshopt-compressed and quantized, which Bevy cannot read: they
  are decoded with glTF-Transform and repacked with gltfpack (no quantizing).
- web/assets/sky/night.jpg: a Poly Haven sky (CC0), darkened for night.
- web/assets/sfx/: chosen sounds from Kenney's CC0 audio packs, as MP3
  (Safari plays MP3 everywhere; Ogg only lately). Needs ffmpeg.

Floors and walls use textures generated in code (crates/client/src/look.rs).
The results are committed, so builds and CI need no network. Needs git, npx
(Node) and Pillow. docs/CREDITS.md lists everything.
"""

import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request

from PIL import Image, ImageEnhance

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS = os.path.join(ROOT, "web", "assets")

# (pack folder, GitHub repository, path of its glTF folder)
KAYKIT = [
    ("furniture", "KayKit-Game-Assets/KayKit-Furniture-Bits-1.0", "addons/kaykit_furniture_bits/Assets/gltf"),
    ("restaurant", "KayKit-Game-Assets/KayKit-Restaurant-Bits-1.0", "addons/kaykit_restaurant_bits/Assets/gltf"),
    ("city", "KayKit-Game-Assets/KayKit-City-Builder-Bits-1.0", "addons/kaykit_city_builder_bits/Assets/gltf"),
]

# The Quaternius characters, as a public project ships them (with their
# license file).
PEOPLE_REPO = "smallkhk/busy"
PEOPLE_DIR = "public/models"
PEOPLE = ["casual_2", "w_casual", "casual_hoodie", "w_formal", "suit", "w_suit", "worker", "beach"]

SKY = "qwantani_moonrise_puresky"

# Kenney audio packs, and the takes the game plays: (pack, file, name).
KENNEY_PACKS = ["casino-audio", "impact-sounds", "interface-sounds", "rpg-audio"]
SOUNDS = (
    [("impact-sounds", f"footstep_{kind}_00{i}", f"step-{kind}-{i}") for kind in ("wood", "concrete", "carpet") for i in range(5)]
    + [("impact-sounds", f"impactGlass_light_00{i}", f"clink-{i}") for i in range(3)]
    + [("casino-audio", f"chips-stack-{i + 1}", f"chips-{i}") for i in range(3)]
    + [("casino-audio", f"card-place-{i + 1}", f"card-{i}") for i in range(3)]
    + [
        ("impact-sounds", "impactGlass_heavy_000", "glass-break"),
        ("impact-sounds", "impactPunch_medium_000", "punch-0"),
        ("impact-sounds", "impactPunch_medium_001", "punch-1"),
        ("impact-sounds", "impactSoft_heavy_000", "thud"),
        ("impact-sounds", "impactBell_heavy_000", "bell"),
        ("rpg-audio", "handleCoins", "coins-0"),
        ("rpg-audio", "handleCoins2", "coins-1"),
        ("casino-audio", "card-shuffle", "shuffle"),
        ("casino-audio", "card-slide-1", "card-slide"),
        ("casino-audio", "dice-throw-1", "dice"),
        ("rpg-audio", "doorOpen_1", "door"),
        ("interface-sounds", "click_002", "click"),
        ("interface-sounds", "confirmation_001", "confirm"),
        ("interface-sounds", "error_004", "error"),
    ]
)


def fetch_sounds(force):
    import re
    import zipfile

    out = os.path.join(ASSETS, "sfx")
    if not force and all(os.path.exists(os.path.join(out, f"{n}.mp3")) for _, _, n in SOUNDS):
        return
    os.makedirs(out, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for pack in KENNEY_PACKS:
            page = get(f"https://kenney.nl/assets/{pack}").decode("utf-8", "replace")
            url = re.search(r"https://kenney\.nl/media/pages/assets/[^\"]+\.zip", page).group(0)
            zipfile.ZipFile(io.BytesIO(get(url))).extractall(os.path.join(tmp, pack))
        for pack, file, name in SOUNDS:
            src = os.path.join(tmp, pack, "Audio", f"{file}.ogg")
            subprocess.run(
                ["ffmpeg", "-loglevel", "error", "-y", "-i", src, "-ac", "1", "-ar", "44100", "-b:a", "80k", os.path.join(out, f"{name}.mp3")],
                check=True,
            )
        print(f"{len(SOUNDS)} sounds")


def sparse_checkout(repo, path, dest):
    """Clone just `path` of a GitHub repository into `dest`."""
    subprocess.run(
        ["git", "clone", "-q", "--depth", "1", "--filter=blob:none", "--no-checkout", f"https://github.com/{repo}", dest],
        check=True,
    )
    subprocess.run(["git", "-C", dest, "checkout", "-q", "HEAD", "--", path], check=True)


def fetch_kaykit(force):
    for folder, repo, path in KAYKIT:
        out = os.path.join(ASSETS, "kk", folder)
        if not force and os.path.isdir(out) and os.listdir(out):
            continue
        with tempfile.TemporaryDirectory() as tmp:
            sparse_checkout(repo, path, os.path.join(tmp, "repo"))
            shutil.rmtree(out, ignore_errors=True)
            shutil.copytree(os.path.join(tmp, "repo", path), out)
        print(f"kaykit {folder}")


def fetch_people(force):
    out = os.path.join(ASSETS, "people")
    if not force and all(os.path.exists(os.path.join(out, f"{p}.glb")) for p in PEOPLE):
        return
    os.makedirs(out, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        repo = os.path.join(tmp, "repo")
        sparse_checkout(PEOPLE_REPO, PEOPLE_DIR, repo)
        shutil.copy(os.path.join(repo, PEOPLE_DIR, "LICENSE-Quaternius.txt"), out)
        for p in PEOPLE:
            src = os.path.join(repo, PEOPLE_DIR, f"{p}.glb")
            plain = os.path.join(tmp, f"{p}.glb")
            subprocess.run(["npx", "--yes", "@gltf-transform/cli@4.1.1", "dequantize", src, plain], check=True)
            subprocess.run(
                ["npx", "--yes", "gltfpack@1.3.0", "-i", plain, "-o", os.path.join(out, f"{p}.glb"), "-si", "0.5", "-noq"],
                check=True,
            )
            print(f"person {p}")


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "last-call-asset-fetch"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def fetch_sky(force):
    path = os.path.join(ASSETS, "sky", "night.jpg")
    if not force and os.path.exists(path):
        return
    os.makedirs(os.path.dirname(path), exist_ok=True)
    url = json.loads(get(f"https://api.polyhaven.com/files/{SKY}"))["tonemapped"]["url"]
    img = Image.open(io.BytesIO(get(url))).convert("RGB").resize((2048, 1024), Image.LANCZOS)
    img = ImageEnhance.Brightness(img).enhance(0.38)
    img = ImageEnhance.Contrast(img).enhance(1.25)
    img.save(path, "JPEG", quality=85, optimize=True)
    print(f"sky {SKY}")


def main():
    force = "--force" in sys.argv
    fetch_kaykit(force)
    fetch_people(force)
    fetch_sky(force)
    fetch_sounds(force)


if __name__ == "__main__":
    main()
