#!/usr/bin/env python3
"""Regenerate `assets/vr_hand_skin.png` from `assets/vr_glove_color.jpg`.

The VR hands are the SteamVR glove *mesh*, so their skin has to live in the
glove's own UV atlas - see `projects/vr-gloves.md`. This recolours that atlas:
it keeps the map's skin-scale relief, tints it with the game's own hand tone,
and flattens the glove's hardware (straps, buckles, stitching, panel edges,
perforations) so the skin doesn't inherit strap ridges.

    python3 tools/make_vr_hand_skin.py        # from the repo root

Needs Pillow + numpy. Deterministic: same input, same output.
"""

import numpy as np
from PIL import Image, ImageFilter

SRC, DST = "assets/vr_glove_color.jpg", "assets/vr_hand_skin.png"

R_BASE = 32.0  # px on the 1024 atlas: the scale glove *panels* live at
R_FINE = 4.0  # the scale skin grain lives at
T0, T1 = 0.18, 0.45  # |ln contrast| band that fades skin -> hardware
DILATE, FEATHER = 9, 3.0  # grow the mask over its own edge halos, then soften
LO, HI, GAMMA, SOFTEN = 0.86, 1.14, 0.6, 1.5

# Mean skin colour of the game's first-person hand texture (HRPistArm.gif),
# divided by 1.5: both material shaders composite `texel * 0.5` (ambient) +
# `texel * emissivity`, and the hands render at emissivity 1.0, so an undivided
# tone clips at white.
TONE = np.array([185.0, 139.0, 124.0]) / 1.5


def blur(x, radius):
    packed = (np.clip(x, 0.0, 4.0) * 63.75).astype(np.uint8)
    return np.asarray(Image.fromarray(packed).filter(ImageFilter.GaussianBlur(radius))).astype(
        np.float32
    ) / 63.75


def dilate(x, size):
    packed = (np.clip(x, 0.0, 1.0) * 255).astype(np.uint8)
    return np.asarray(Image.fromarray(packed).filter(ImageFilter.MaxFilter(size))).astype(
        np.float32
    ) / 255.0


def main():
    rgb = np.asarray(Image.open(SRC).convert("RGB")).astype(np.float32) / 255.0
    luminance = 0.2126 * rgb[..., 0] + 0.7152 * rgb[..., 1] + 0.0722 * rgb[..., 2]

    # Glove hardware is whatever stands far off its own neighbourhood - no
    # hand-drawn regions needed. Dilating (max filter) before feathering is what
    # covers the *edges* of a strap; a plain blur leaves them as etched outlines.
    structure = np.abs(
        np.log(np.maximum(luminance / np.maximum(blur(luminance, R_BASE), 1e-3), 1e-3))
    )
    structure = np.clip((structure - T0) / (T1 - T0), 0.0, 1.0)
    structure = blur(dilate(structure, DILATE), FEATHER)

    # Relief at skin scale only (a size/256 high pass is too fine to carry a
    # strap), erased wherever the mask says "glove hardware".
    fine = luminance / np.maximum(blur(luminance, R_FINE), 1e-3)
    detail = np.clip(1.0 + (fine - 1.0) * (1.0 - structure), LO, HI) ** GAMMA

    texel = np.clip(TONE[None, None, :] * detail[..., None], 0, 255).astype(np.uint8)
    out = Image.fromarray(texel).filter(ImageFilter.GaussianBlur(SOFTEN))
    out.resize((512, 512), Image.LANCZOS).save(DST)
    print(f"wrote {DST}")


if __name__ == "__main__":
    main()
