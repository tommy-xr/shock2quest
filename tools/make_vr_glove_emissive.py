#!/usr/bin/env python3
"""Generate `assets/vr_glove_emissive.png`, the glove's light mask.

White texels are the parts of the glove that glow when a hand light is on
(`hand_glove::HandLight`); black texels stay unlit. The mask is derived from
the glove *model* rather than hand-painted, so it lands on the mesh wherever
the atlas happens to pack it: a band around the wrist cuff and a pad on each
fingertip, both picked in model space and rasterized through the mesh's own
UVs.

    python3 tools/make_vr_glove_emissive.py      # from the repo root

Needs Pillow + numpy. Deterministic: same input, same output.
"""

import json
import struct

import numpy as np
from PIL import Image, ImageFilter

SRC, DST = "assets/vr_glove_model.glb", "assets/vr_glove_emissive.png"
SIZE = 1024

# The cuff band, in fractions of the model's wrist-to-fingertip span: solid to
# CUFF_SOLID, faded out by CUFF_FADE. Wide enough to read as a band from any
# angle, short enough to stay off the knuckles.
CUFF_SOLID, CUFF_FADE = 0.05, 0.11

# The fingertip pads, as a radius around each `finger_*_aux` node (SteamVR's
# fingertip markers), in the same span units - about one fingertip segment.
TIP_SOLID, TIP_FADE = 0.05, 0.09

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
    mask = rasterize(uvs, indices, vertex_weights(positions, tips, span))

    image = Image.fromarray((np.clip(mask, 0.0, 1.0) * 255).astype(np.uint8))
    image = image.filter(ImageFilter.MaxFilter(DILATE)).filter(
        ImageFilter.GaussianBlur(FEATHER)
    )
    image.save(DST)
    print(f"{DST}: {image.size[0]}x{image.size[1]}, {np.asarray(image).mean() / 255:.3%} lit")


if __name__ == "__main__":
    main()
