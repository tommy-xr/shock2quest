"""Prepare Meshy body-gear exports for the game's metre-scale mounts.

Requires Pillow and NumPy. See assets/source/vr-body-gear.md for usage.
Keeps the source mesh and PBR maps; only placement and texture pixels change.
"""

import argparse
import io
import json
from pathlib import Path
import struct

import numpy as np
from PIL import Image, ImageEnhance


def prepare_body_gear(source, destination, kind, texture_size, gear_brightness=0.55, gear_contrast=0.85):
    raw = source.read_bytes()
    magic, version, length = struct.unpack_from("<III", raw)
    if (magic, version, length) != (0x46546C67, 2, len(raw)):
        raise ValueError(f"Not a GLB v2 file: {source}")
    json_length, json_type = struct.unpack_from("<II", raw, 12)
    assert json_type == 0x4E4F534A
    doc = json.loads(raw[20:20 + json_length])
    binary_length, binary_type = struct.unpack_from("<II", raw, 20 + json_length)
    assert binary_type == 0x004E4942
    binary = bytearray(raw[28 + json_length:28 + json_length + binary_length])
    # These source exports are single, untransformed static meshes. Fail rather
    # than silently mishandle a different export layout (e.g. nested transforms).
    assert len(doc["nodes"]) == len(doc["meshes"]) == len(doc["buffers"]) == 1
    assert set(doc["nodes"][0]) <= {"mesh", "name"}
    assert len(doc["meshes"][0]["primitives"]) == 1
    primitive = doc["meshes"][0]["primitives"][0]
    position = doc["accessors"][primitive["attributes"]["POSITION"]]
    minimum, maximum = np.array(position["min"]), np.array(position["max"])
    center = (minimum + maximum) / 2
    # Meshy faces +Z; gameplay faces -Z. Match the previous belt's 42 cm
    # width and front at -30 cm, and the holster's 24 cm height / +1.7 cm center.
    scale = (0.42 / (maximum[0] - minimum[0]) if kind == "belt"
             else (0.44 if kind == "backpack" else 0.24) / (maximum[1] - minimum[1]))
    if kind == "belt":
        translation = [0.0, 0.0, -0.30 + (maximum[2] - minimum[2]) * scale / 2]
    elif kind == "backpack":
        # Centre-height attachment on its flat back: the bag extends toward -Z.
        translation = [0.0, 0.0, -(maximum[2] - minimum[2]) * scale / 2]
    else:
        translation = [0.0, 0.017, 0.0]
    translation = np.array(translation)
    for semantic in ("POSITION", "NORMAL", "TANGENT"):
        if semantic not in primitive["attributes"]:
            continue
        accessor = doc["accessors"][primitive["attributes"][semantic]]
        view = doc["bufferViews"][accessor["bufferView"]]
        components = 4 if semantic == "TANGENT" else 3
        assert accessor["componentType"] == 5126 and "sparse" not in accessor
        offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        stride = view.get("byteStride", components * 4)
        values = np.ndarray((accessor["count"], components), dtype="<f4",
                            buffer=binary, offset=offset, strides=(stride, 4))
        if semantic == "POSITION":
            values[:] = (values - center) * [-scale, scale, -scale] + translation
            accessor["min"] = values.min(axis=0).tolist()
            accessor["max"] = values.max(axis=0).tolist()
        else:
            values[:, :3] *= [-1, 1, -1]

    color_images = {
        doc["textures"][material["pbrMetallicRoughness"]["baseColorTexture"]["index"]]["source"]
        for material in doc["materials"]
    }
    normal_images = {
        doc["textures"][material["normalTexture"]["index"]]["source"]
        for material in doc["materials"] if "normalTexture" in material
    }
    replacements = {}
    for index, image in enumerate(doc["images"]):
        view_index = image["bufferView"]
        view = doc["bufferViews"][view_index]
        offset = view.get("byteOffset", 0)
        pixels = Image.open(io.BytesIO(binary[offset:offset + view["byteLength"]])).convert("RGB")
        if kind == "belt" and index in color_images:
            # Remove baked green lights before filtering so their bright fringes
            # cannot bleed into neighboring texels during downsampling.
            colors = np.array(pixels)
            red, green, blue = colors.astype(np.int16).transpose(2, 0, 1)
            mask = (green > red * 1.15) & (green > blue * 1.15) & (green - blue > 4)
            colors[mask] = 0
            pixels = Image.fromarray(colors)
            print(f"{kind}: mapped {int(mask.sum())} green texels to black")
        pixels.thumbnail((texture_size, texture_size), Image.Resampling.LANCZOS)
        if kind == "holster" and index in color_images:
            # Match the belt's base-color luminance (0.116 vs 0.191 before
            # adjustment), preserving the holster's hue and worn detail.
            pixels = pixels.point(lambda value: round(value * 0.61))
        if kind == "backpack" and index in color_images:
            # Source mean luminance 0.194; belt/holster both 0.116. Match their
            # charcoal finish and soften the brighter baked wear highlights.
            # Keep UV islands, scratches and all PBR data in their authored places.
            pixels = ImageEnhance.Contrast(pixels).enhance(0.90)
            pixels = ImageEnhance.Brightness(pixels).enhance(0.60)
        if index in color_images:
            # Shared outfit finish after matching the three source exports.
            # Compress pale baked wear, then darken to the hacker's charcoal.
            # Only base color changes; preserve UVs and PBR data.
            pixels = ImageEnhance.Contrast(pixels).enhance(gear_contrast)
            pixels = ImageEnhance.Brightness(pixels).enhance(gear_brightness)
        if index in normal_images:
            normals = np.array(pixels).astype(np.float32) / 127.5 - 1
            normals /= np.maximum(np.linalg.norm(normals, axis=2, keepdims=True), 1e-6)
            pixels = Image.fromarray(np.clip((normals + 1) * 127.5, 0, 255).astype(np.uint8))
        encoded = io.BytesIO()
        pixels.save(encoded, format="PNG", optimize=True)
        image["mimeType"] = "image/png"
        replacements[view_index] = encoded.getvalue()
    for material in doc["materials"]:
        if kind == "belt":
            material.pop("emissiveTexture", None)
            material["emissiveFactor"] = [0, 0, 0]

    # Repack all views to discard the original 4K images, preserving indices.
    output = bytearray()
    for index, view in enumerate(doc["bufferViews"]):
        output.extend(b"\0" * (-len(output) % 4))
        offset = view.get("byteOffset", 0)
        data = replacements.get(index, binary[offset:offset + view["byteLength"]])
        view["byteOffset"], view["byteLength"] = len(output), len(data)
        output.extend(data)
    doc["buffers"][0]["byteLength"] = len(output)
    doc["nodes"][0]["name"] = kind
    encoded = json.dumps(doc, separators=(",", ":")).encode()
    encoded += b" " * (-len(encoded) % 4)
    output.extend(b"\0" * (-len(output) % 4))
    result = struct.pack("<III", 0x46546C67, 2, 28 + len(encoded) + len(output))
    result += struct.pack("<II", len(encoded), 0x4E4F534A) + encoded
    result += struct.pack("<II", len(output), 0x004E4942) + output
    destination.write_bytes(result)
    print(f"{destination}: {len(raw):,} -> {len(result):,} bytes; bounds {position['min']} .. {position['max']}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--belt", type=Path)
    parser.add_argument("--holster", type=Path)
    parser.add_argument("--backpack", type=Path)
    parser.add_argument("--texture-size", type=int, default=1024)
    parser.add_argument("--gear-brightness", type=float, default=0.55,
                        help="shared base-color multiplier after source matching (default: 0.55; legacy: 1)")
    parser.add_argument("--gear-contrast", type=float, default=0.85,
                        help="shared base-color contrast after source matching (default: 0.85; legacy: 1)")
    parser.add_argument("--output-dir", type=Path, default=Path(__file__).resolve().parents[1] / "assets")
    args = parser.parse_args()
    kinds = [kind for kind in ("belt", "holster", "backpack") if getattr(args, kind)]
    if not kinds:
        parser.error("provide at least one of --belt, --holster or --backpack")
    if args.texture_size < 1:
        parser.error("--texture-size must be positive")
    if not (0 < args.gear_brightness <= 1 and 0 < args.gear_contrast <= 1):
        parser.error("--gear-brightness and --gear-contrast must be in (0, 1]")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for kind in kinds:
        prepare_body_gear(getattr(args, kind), args.output_dir / f"{kind}.glb", kind, args.texture_size,
                          args.gear_brightness, args.gear_contrast)
