"""Draw one AI-reachability pass as a top-down (XZ) trail map.

    python3 render-trail-map.py <cells.json> <pass.json> <out.png>

Nav cells (from `cargo bn path dump`) are the faint background; each AI's
sampled trail is a colored polyline with a start dot and an end square, a red
ring wherever it wedged, and the player marked with a magenta cross.
"""

import json
import sys

from PIL import Image, ImageDraw

MAX_PIXELS = 2400
MARGIN = 40
LEGEND_WIDTH = 360

TRAIL_COLORS = [
    (255, 96, 96),
    (96, 200, 255),
    (140, 230, 140),
    (255, 200, 80),
    (210, 140, 255),
    (255, 140, 200),
    (120, 255, 220),
    (200, 200, 120),
]


def load(path):
    with open(path) as f:
        return json.load(f)


def main():
    cells_file, pass_file, out_file = sys.argv[1:4]
    cells = load(cells_file)["cells"]
    data = load(pass_file)

    polygons = [c["polygon"] for c in cells if len(c["polygon"]) >= 3]
    points = [(p[0], p[2]) for poly in polygons for p in poly]
    for track in data["tracks"]:
        points += [(s["position"][0], s["position"][2]) for s in track["samples"]]
    points.append((data["player"][0], data["player"][2]))
    if not points:
        raise SystemExit("nothing to draw")

    min_x = min(p[0] for p in points)
    max_x = max(p[0] for p in points)
    min_z = min(p[1] for p in points)
    max_z = max(p[1] for p in points)
    span = max(max_x - min_x, max_z - min_z, 1.0)
    scale = (MAX_PIXELS - 2 * MARGIN) / span

    width = int((max_x - min_x) * scale) + 2 * MARGIN + LEGEND_WIDTH
    height = int((max_z - min_z) * scale) + 2 * MARGIN

    def xy(pos_x, pos_z):
        # Flip Z so the map reads like the in-game overhead view.
        return (
            MARGIN + (pos_x - min_x) * scale,
            MARGIN + (max_z - pos_z) * scale,
        )

    image = Image.new("RGB", (width, height), (16, 16, 20))
    draw = ImageDraw.Draw(image)

    for polygon in polygons:
        draw.polygon([xy(p[0], p[2]) for p in polygon], outline=(56, 60, 70))

    px, pz = xy(data["player"][0], data["player"][2])
    for dx, dy in ((-9, -9, ), (-9, 9)):
        draw.line([px + dx, pz + dy, px - dx, pz - dy], fill=(255, 64, 220), width=3)

    legend = [f"{data['pass']} pass - {len(data['tracks'])} AIs"]
    for index, track in enumerate(data["tracks"]):
        samples = track["samples"]
        if not samples:
            continue
        color = TRAIL_COLORS[index % len(TRAIL_COLORS)]
        trail = [xy(s["position"][0], s["position"][2]) for s in samples]
        if len(trail) >= 2:
            draw.line([c for point in trail for c in point], fill=color, width=2)
        sx, sy = trail[0]
        draw.ellipse([sx - 4, sy - 4, sx + 4, sy + 4], fill=color)
        ex, ey = trail[-1]
        draw.rectangle([ex - 4, ey - 4, ex + 4, ey + 4], outline=color, width=2)

        verdict = track["classification"]["verdict"]
        wedge = track["classification"].get("wedge_at")
        if wedge:
            wx, wy = xy(wedge[0], wedge[2])
            draw.ellipse([wx - 12, wy - 12, wx + 12, wy + 12], outline=(255, 40, 40), width=3)
        legend.append(f"{track['name']} t{track['template_id']}: {verdict}")

    # Legend runs in columns so a crowded mission cannot push rows off the page.
    rows_per_column = max(1, (height - 2 * MARGIN) // 16)
    for index, line in enumerate(legend):
        column, row = divmod(index, rows_per_column)
        text_x = width - LEGEND_WIDTH + 12 + column * 170
        color = (255, 90, 90) if "wedged" in line else (220, 220, 225)
        draw.text((text_x, MARGIN + row * 16), line, fill=color)

    image.save(out_file)
    print(f"wrote {out_file} ({width}x{height})")


if __name__ == "__main__":
    main()
