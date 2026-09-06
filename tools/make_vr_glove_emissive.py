#!/usr/bin/env python3
"""Bootstrap `assets/vr_glove_emissive.png`, the glove's light mask.

White texels are the parts of the glove that glow when a hand light is on
(`hand_glove::HandLight`); black texels stay unlit.

**The PNG is the authored source, not this script's output.** This only paints a
first draft so the feature had something to render; the asset is hand-painted
from here on and hand edits win. Do NOT rerun this over an edited file - it
overwrites it. If the draft ever needs regenerating, do it to a scratch path and
merge by hand.

What the draft contains, in the glove's own UV atlas:

- Front: a band around the wrist cuff and a pad on each fingertip, picked in
  model space (depth along the fingers; a radius around the five `finger_*_aux`
  fingertip markers) and rasterized through the mesh's own UVs.
- Back: the light stripes running down the back-of-hand panel. Those are an
  albedo detail, not a shape, so model space can't separate them from the panel
  around them - they are picked off the colour map instead, as texels standing
  well above a dark neighbourhood inside the back-of-hand box.

    python3 tools/make_vr_glove_emissive.py      # from the repo root

Needs Pillow + numpy. Deterministic: same input, same output.
"""

import json
import struct

import numpy as np
from PIL import Image, ImageFilter

SRC = "assets/vr_glove_model.glb"
COLOR = "assets/vr_glove_color.jpg"
DST = "assets/vr_glove_emissive.png"

# The colour map's resolution, so the two line up texel for texel and the mask
# stays paintable by hand.
SIZE = 1024

# The cuff band, in fractions of the model's wrist-to-fingertip span: solid to
# CUFF_SOLID, faded out by CUFF_FADE. Wide enough to read as a band from any
# angle, short enough to stay off the knuckles.
CUFF_SOLID, CUFF_FADE = 0.05, 0.11

# The fingertip pads, as a radius around each `finger_*_aux` node (SteamVR's
# fingertip markers), in the same span units - about one fingertip segment.
TIP_SOLID, TIP_FADE = 0.05, 0.09

# The back-of-hand panel, in atlas texels: the box the light stripes live in.
BACK_BOX = (90, 385, 345, 915)  # left, top, right, bottom

# How a stripe is told from the panel it sits on: brighter than its own
# neighbourhood by BACK_CONTRAST, on a neighbourhood at most BACK_PANEL_MAX
# bright. The darkness test is what keeps the pale hex mesh either side of the
# panel out - it is bright, but so is everything around it.
BACK_BLUR = 12.0
BACK_CONTRAST, BACK_PANEL_MAX = 1.55, 0.35

# The panel is also perforated and stitched, and those specks clear the contrast
# test too. An opening (erode, then dilate back) drops anything thinner than a
# stripe while leaving the stripes their own width.
BACK_ERODE = 5

# Closes the hairline seams left where a UV island's edge falls between texel
# centres, then softens the mask's own edges.
DILATE, FEATHER = 5, 2.0


def read_glb(path):
    data = open(path, "rb").read()
    assert data[:4] == b"glTF", path
    offset, chunks = 12, []
    while offset < len(data):
        length, _kind = struct.unpack_from("<II", data, offset)
        offset += 8
        chunks.append(data[offset : offset + length])
        offset += length
    return json.loads(chunks[0].decode("utf-8")), chunks[1]


def accessor(gltf, blob, index):
    spec = gltf["accessors"][index]
    view = gltf["bufferViews"][spec["bufferView"]]
    component = {5126: "f", 5123: "H", 5125: "I", 5121: "B"}[spec["componentType"]]
    count = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[spec["type"]]
    packed = component * count
    stride = view.get("byteStride", struct.calcsize(packed))
    base = view.get("byteOffset", 0) + spec.get("byteOffset", 0)
    return np.array(
        [struct.unpack_from("<" + packed, blob, base + i * stride) for i in range(spec["count"])]
    )


def smooth_falloff(distance, solid, fade):
    """1 inside `solid`, 0 beyond `fade`, smoothstepped between."""
    t = np.clip((fade - distance) / (fade - solid), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def vertex_weights(positions, tips, span):
    """How lit each vertex is: the wrist cuff, plus a pad at every fingertip."""
    depth = (positions[:, 2] - positions[:, 2].min()) / span
    weight = smooth_falloff(depth, CUFF_SOLID, CUFF_FADE)
    for tip in tips:
        distance = np.linalg.norm(positions - tip, axis=1) / span
        weight = np.maximum(weight, smooth_falloff(distance, TIP_SOLID, TIP_FADE))
    return weight


def back_of_hand_stripes():
    """The light stripes down the back-of-hand panel, read off the colour map.

    A hand seen from behind shows none of the cuff and only the far edge of the
    fingertip pads, so without these the light is invisible from the side the
    player looks at most.
    """
    color = Image.open(COLOR).convert("L").resize((SIZE, SIZE), Image.LANCZOS)
    luminance = np.asarray(color).astype(np.float32) / 255.0
    neighbourhood = np.asarray(
        color.filter(ImageFilter.GaussianBlur(BACK_BLUR * SIZE / 1024.0))
    ).astype(np.float32) / 255.0

    stripes = (luminance > neighbourhood * BACK_CONTRAST) & (
        neighbourhood < BACK_PANEL_MAX
    )

    box = np.zeros_like(stripes)
    left, top, right, bottom = (round(v * SIZE / 1024.0) for v in BACK_BOX)
    box[top:bottom, left:right] = True

    opened = Image.fromarray(((stripes & box) * 255).astype(np.uint8))
    opened = opened.filter(ImageFilter.MinFilter(BACK_ERODE)).filter(
        ImageFilter.MaxFilter(BACK_ERODE)
    )
    return np.asarray(opened).astype(np.float32) / 255.0


def rasterize(uvs, indices, weights):
    """Draw the per-vertex weights into the atlas through the mesh's UVs."""
    mask = np.zeros((SIZE, SIZE), np.float32)
    # The atlas wraps: this model authors its v above 1.
    pixels = np.stack([uvs[:, 0] % 1.0, uvs[:, 1] % 1.0], axis=1) * (SIZE - 1)

    for triangle in indices.reshape(-1, 3):
        corners = pixels[triangle]
        weight = weights[triangle]
        if weight.max() <= 0.0:
            continue
        x0, y0 = np.floor(corners.min(axis=0)).astype(int)
        x1, y1 = np.ceil(corners.max(axis=0)).astype(int) + 1
        # A triangle straddling the atlas seam covers the whole atlas once
        # wrapped; it carries no light of its own worth chasing.
        if x1 - x0 > SIZE // 2 or y1 - y0 > SIZE // 2:
            continue
        xs, ys = np.meshgrid(np.arange(x0, x1), np.arange(y0, y1))
        (ax, ay), (bx, by), (cx, cy) = corners
        area = (bx - ax) * (cy - ay) - (cx - ax) * (by - ay)
        if abs(area) < 1e-9:
            continue
        u = ((xs - ax) * (cy - ay) - (cx - ax) * (ys - ay)) / area
        v = ((bx - ax) * (ys - ay) - (xs - ax) * (by - ay)) / area
        inside = (u >= 0) & (v >= 0) & (u + v <= 1)
        if not inside.any():
            continue
        value = weight[0] + u * (weight[1] - weight[0]) + v * (weight[2] - weight[0])
        target = mask[y0:y1, x0:x1]
        np.maximum(target, np.where(inside, value, 0.0), out=target)

    return mask


def main():
    gltf, blob = read_glb(SRC)
    primitive = gltf["meshes"][0]["primitives"][0]
    positions = accessor(gltf, blob, primitive["attributes"]["POSITION"])
    uvs = accessor(gltf, blob, primitive["attributes"]["TEXCOORD_0"])
    indices = accessor(gltf, blob, primitive["indices"]).reshape(-1)

    tips = np.array(
        [
            node["translation"]
            for node in gltf["nodes"]
            if node.get("name", "").endswith("_aux") and "translation" in node
        ]
    )
    assert len(tips) == 5, f"expected 5 fingertip markers, found {len(tips)}"

    span = positions[:, 2].max() - positions[:, 2].min()
    mask = np.maximum(
        rasterize(uvs, indices, vertex_weights(positions, tips, span)),
        back_of_hand_stripes(),
    )

    image = Image.fromarray((np.clip(mask, 0.0, 1.0) * 255).astype(np.uint8))
    image = image.filter(ImageFilter.MaxFilter(DILATE)).filter(
        ImageFilter.GaussianBlur(FEATHER)
    )
    image.save(DST)
    print(f"{DST}: {image.size[0]}x{image.size[1]}, {np.asarray(image).mean() / 255:.3%} lit")


if __name__ == "__main__":
    main()
