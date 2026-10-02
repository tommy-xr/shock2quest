"""Capture real CLI/TUI output to PNG. Requires Python with pyte and Pillow.

Run from the repo root, with DARK_ASSET_PATH set; pass command arguments after --.
"""

import argparse, copy, fcntl, json, os, pty, select, struct, subprocess, termios, time
from pathlib import Path
from datetime import datetime, timezone
import pyte
from PIL import Image, ImageDraw, ImageFont

p = argparse.ArgumentParser()
p.add_argument("--name", required=True)
p.add_argument("--title", required=True)
p.add_argument("--dashboard", action="store_true")
p.add_argument("command", nargs="+")
a = p.parse_args()
root = Path.cwd()
out = root / "screenshots/tools"
out.mkdir(parents=True, exist_ok=True)
cols = 108 if a.dashboard else 156
rows = 28
screen = pyte.Screen(cols, rows)
stream = pyte.ByteStream(screen)
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
env = dict(
    os.environ,
    TERM="xterm-256color",
    COLUMNS=str(cols),
    LINES=str(rows),
    RUST_LOG="error",
)
proc = subprocess.Popen(
    a.command, stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True
)
os.close(slave)
started = time.monotonic()
last_data = started
while time.monotonic() - started < 30:
    ready, _, _ = select.select([master], [], [], 0.1)
    if ready:
        try:
            chunk = os.read(master, 65536)
        except OSError:
            break
        if not chunk:
            break
        stream.feed(chunk)
        last_data = time.monotonic()
    if a.dashboard and time.monotonic() - started > 6:
        break
    if proc.poll() is not None and time.monotonic() - last_data > 0.3:
        break
# Preserve dashboard before it exits alternate screen. No fake status/device data.
lines = list(screen.display)
cells = copy.deepcopy(screen.buffer)
if a.dashboard:
    os.write(master, b"q")
    proc.wait(timeout=5)
elif proc.poll() is None:
    proc.terminate()
    proc.wait(timeout=5)
os.close(master)
(out / f"{a.name}.txt").write_text("\n".join(lines).rstrip() + "\n")
font = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 17)
cw = font.getlength("M")
lh = 25
pad = 25
top = 64
display_rows = (
    rows
    if a.dashboard
    else max(8, max((i + 1 for i, line in enumerate(lines) if line.strip()), default=1))
)
height = top + display_rows * lh + pad
im = Image.new("RGB", (round(cols * cw) + pad * 2, height), "#10151f")
d = ImageDraw.Draw(im)
d.rounded_rectangle((12, 12, im.width - 12, 48), 8, fill="#202b3e")
d.text((25, 18), a.title, font=font, fill="#7ce4e2")
colors = {
    "default": "#dce3f0",
    "black": "#131822",
    "red": "#f7768e",
    "green": "#9ece6a",
    "yellow": "#e0af68",
    "blue": "#7aa2f7",
    "magenta": "#bb9af7",
    "cyan": "#7dcfff",
    "white": "#dce3f0",
    "brightblack": "#68778d",
    "brightwhite": "#ffffff",
}
for y, line in enumerate(lines):
    for x, char in enumerate(line):
        cell = cells[y][x]
        color = colors.get(cell.fg, "#" + cell.fg if len(cell.fg) == 6 else "#dce3f0")
        if char != " ":
            d.text((pad + x * cw, top + y * lh), char, font=font, fill=color)
im.save(out / f"{a.name}.png", optimize=True)
(out / f"{a.name}.json").write_text(
    json.dumps(
        {
            "command": a.command,
            "capturedAt": datetime.now(timezone.utc).isoformat(),
            "revision": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], text=True
            ).strip(),
            "displayedCommand": a.title,
            "capture": "Actual PTY output rendered with pyte and Menlo; no desktop session required",
            "exitCode": proc.returncode,
            "columns": cols,
            "rows": rows,
            "renderedRows": display_rows,
        },
        indent=2,
    )
    + "\n"
)
print(a.name, proc.returncode)
if proc.returncode:
    raise SystemExit(proc.returncode)
