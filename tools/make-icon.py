"""Renders the JustCode app icon set (desktop + Android + iOS) and the small
copy the About box embeds.

The mark (`justcode-mark.png`) is the teal "jc" ribbon monogram cropped from
the brand artwork; this script does not draw it. What it does do is:

  1. Compose the mark onto a white rounded-square plate -> the desktop
     "default" icon (`src-tauri/icons/justcode-source.png`).
  2. Pad the mark onto a larger transparent square so it sits inside
     Android's adaptive-icon safe zone -> `justcode-mark-android-fg.png`.
  3. Hand both, plus a plain white Android background layer, to the Tauri
     CLI (`tauri icon`) via `icon-manifest.json` -- it knows the exact size
     and format rules for every platform (Windows ico/Store tiles, macOS
     icns, Android mipmap densities, iOS AppIcon set), so there's no point
     re-deriving those by hand.
  4. Inline a 96px copy of the desktop icon into `src/icons.js` for the
     About box.

    python tools/make-icon.py

Requires the Tauri CLI (`npx tauri`) and Pillow.
"""
import base64
import io
import math
import re
import subprocess
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
TOOLS = Path(__file__).resolve().parent
ICONS = ROOT / "src-tauri" / "icons"
MARK = TOOLS / "justcode-mark.png"
ICONS_JS = ROOT / "src" / "icons.js"
MANIFEST = TOOLS / "icon-manifest.json"
ANDROID_FG = TOOLS / "justcode-mark-android-fg.png"
ANDROID_BG = TOOLS / "android-bg-white.png"

MASTER = 1024
SUPERSAMPLE = 4
PLATE_RADIUS_RATIO = 118 / 512      # same corner roundness as the old plate
MARK_HEIGHT_RATIO = 0.78            # mark height as a fraction of the plate
MARK_Y_SHIFT_RATIO = 0.05           # nudge down from dead-center, as a fraction of the plate

# Android adaptive icons guarantee only a 66dp circle is visible inside the
# 108dp canvas across all launcher mask shapes (circle, squircle, teardrop).
# `android_fg_scale` in the Tauri manifest is documented but had no effect
# as of tauri-cli 2.11.4, so the safe zone is baked into the source image
# instead: pad a transparent square around the mark so its diagonal comes
# out to ANDROID_SAFE_RATIO of the canvas (some margin under the 66/108
# guarantee, since a wide/tall mark's corners are the first thing a
# circular mask clips).
ANDROID_SAFE_RATIO = 0.55


def render_master():
    """White rounded-square plate with the mark centered, supersampled for
    clean corners and a clean downscale of the mark's soft gradient edges."""
    W = MASTER * SUPERSAMPLE
    plate = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    mask = Image.new("L", (W, W), 0)
    radius = round(W * PLATE_RADIUS_RATIO)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, W - 1, W - 1], radius=radius, fill=255)
    white = Image.new("RGBA", (W, W), (255, 255, 255, 255))
    plate.paste(white, (0, 0), mask)

    mark = Image.open(MARK).convert("RGBA")
    target_h = round(W * MARK_HEIGHT_RATIO)
    target_w = round(mark.width * target_h / mark.height)
    mark = mark.resize((target_w, target_h), Image.LANCZOS)
    y = (W - target_h) // 2 + round(W * MARK_Y_SHIFT_RATIO)
    plate.alpha_composite(mark, ((W - target_w) // 2, y))

    return plate.resize((MASTER, MASTER), Image.LANCZOS)


def render_android_fg():
    mark = Image.open(MARK).convert("RGBA")
    w, h = mark.size
    S = round(math.hypot(w, h) / ANDROID_SAFE_RATIO)
    canvas = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    canvas.paste(mark, ((S - w) // 2, (S - h) // 2), mark)
    canvas.save(ANDROID_FG)


def render_android_bg():
    Image.new("RGBA", (MASTER, MASTER), (255, 255, 255, 255)).save(ANDROID_BG)


def write_manifest():
    MANIFEST.write_text(
        """{
  "default": "../src-tauri/icons/justcode-source.png",
  "bg_color": "#ffffff",
  "android_bg": "android-bg-white.png",
  "android_fg": "justcode-mark-android-fg.png"
}
""",
        encoding="utf-8",
    )


def data_uri(im, size):
    buf = io.BytesIO()
    im.resize((size, size), Image.LANCZOS).save(buf, format="PNG", optimize=True)
    return "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode("ascii")


def patch_icons_js(uri):
    text = ICONS_JS.read_text(encoding="utf-8")
    new_text, n = re.subn(
        r'const APP_LOGO =\n  "data:image/png;base64,[^"]*";',
        f'const APP_LOGO =\n  "{uri}";',
        text,
    )
    if n != 1:
        raise RuntimeError("could not find APP_LOGO constant to patch in src/icons.js")
    ICONS_JS.write_text(new_text, encoding="utf-8")


master = render_master()
master.save(ICONS / "justcode-source.png")
render_android_fg()
render_android_bg()
write_manifest()

subprocess.run(
    ["npx", "tauri", "icon", str(MANIFEST.relative_to(ROOT))],
    cwd=ROOT, check=True, shell=True,
)

uri = data_uri(master, 96)
(ICONS / "justcode-logo.txt").write_text(uri, encoding="ascii")
patch_icons_js(uri)

print(f"patched APP_LOGO in src/icons.js ({len(uri)} chars)")
