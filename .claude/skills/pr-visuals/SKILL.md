---
name: pr-visuals
description: >-
  Capture a looping GIF + a still PNG of a visual/rendering change and embed them
  in its pull request. Uses the headless debug runtime (cargo dbgr + HTTP
  /v1/step + /v1/screenshot - no window interaction, deterministic fixed-timestep
  stepping), assembles a GIF with PIL, hosts the binaries in a gist (pushed via
  git, the only token-friendly host for binary images), and embeds them in the PR
  body. Use whenever a change adds or alters something visible (a viewmodel/HUD/
  rendering/material/lighting feature, a debug scene, an animation) and you're
  opening or updating a PR - see AGENTS.md "Visual Changes".
---

# pr-visuals — capture & embed PR screenshots / GIFs

Produce a short looping GIF **and** a still PNG of a visual change and embed them
in its PR. Everything runs headlessly through the debug runtime (no manual
clicking) and deterministically (fixed 60 Hz stepping).

## Context discipline (important)

Verifying the look means **reading PNGs** (image tokens) and running many capture
commands — that bloats the main context. When the host supports delegation,
**delegate the whole capture → assemble → upload → embed flow to a subagent** and
let it return only the final URLs + markdown block. Otherwise run the flow inline
and keep only the final artifacts and conclusions in the active context.

For a delegated run, tell the subagent to read
`.agents/skills/pr-visuals/SKILL.md` and execute it for the target. Pass the
scene/mission (for example `debug_weapons`), the exact HTTP sequence that shows
the feature, a one-line caption, and the PR number (or "current branch"). Ask it
to return the gist raw URLs and the markdown block it embedded.

The rest of this file is the technique the subagent (or you, if running inline)
follows.

## 1. Prerequisites

- Built runtime: `cargo build -p debug_runtime` (incremental after a change is
  seconds; a cold build takes minutes).
- `python3` with PIL (`Pillow`) for GIF assembly + verification.
- Run multi-step shell blocks under **`bash`** (heredoc), not the default zsh —
  zsh misparses hex gist IDs like `832ee3…` as math expressions and aborts.

## 2. Capture stills

Drive the debug runtime over HTTP (see AGENTS.md "Iterating on Visual Features"
for the full API):

```bash
mkdir -p /tmp/shots            # /v1/screenshot does NOT create directories
cargo dbgr --mission debug_weapons --port 8085 &   # NO extra `--` after dbgr
until curl -s -m 2 http://127.0.0.1:8085/v1/info >/dev/null; do sleep 2; done
# ...drive the feature: actions, inputs, stepping...
curl -s -X POST http://127.0.0.1:8085/v1/input/action -d '{"action":"CycleWeapon"}'
curl -s -X POST http://127.0.0.1:8085/v1/step -d '{"frames":10}'
curl -s -X POST http://127.0.0.1:8085/v1/screenshot -d '{"filename":"/tmp/shots/after.png"}'
# ALWAYS shut down when done:
curl -s -X POST http://127.0.0.1:8085/v1/shutdown
```

- **The screenshot path must be ABSOLUTE and its directory must already exist** —
  the endpoint won't `mkdir`, and `curl` exits 0 on the error response, so a
  missing dir fails silently. Echo the JSON response instead of `> /dev/null`.
- **Captures are 800x600** — the declared logical size, regardless of a HiDPI
  framebuffer. Pass a large `{"max_width": 4000}` when a detail (small HUD text,
  a thin seam) needs the native framebuffer resolution; it never upscales.
- `/v1/step` is a **fixed 60 Hz timestep** and blocks until the frames ran;
  `/v1/screenshot` captures the fully-rendered frame. The same request sequence
  from a fresh launch produces **byte-identical** images — that determinism is
  what makes before/after comparable (and `cmp` a cheap "did anything change?").
- Input channels are dotted paths in flat JSON: `{"right_hand.trigger": 1.0}`,
  `{"head.look": [yaw_deg, pitch_deg]}` — not nested objects.
- **`debug_weapons` gotcha:** `CycleWeapon` drops the previously wielded weapon
  into the world in the look direction, so repeated cycling piles weapon models
  on the floor — they photobomb screenshots and look like broken viewmodels.
  Face away before shooting (`{"head.look": [180.0, 0.0]}`) or teleport
  (`/v1/player/teleport`). To confirm something is camera-anchored (viewmodel)
  vs world litter, rotate the camera: the viewmodel stays screen-fixed.

## 3. Capture a looping GIF

Step a fixed number of frames between screenshots and stitch:

```bash
for i in $(seq -w 0 23); do
  curl -s -X POST http://127.0.0.1:8085/v1/step -d '{"frames":3}' >/dev/null
  curl -s -X POST http://127.0.0.1:8085/v1/screenshot -d "{\"filename\":\"/tmp/frames/frame_$i.png\"}" >/dev/null
done
```

- 3 sim frames per GIF frame at 60 Hz ≈ 20 fps playback with `duration=50`.
- **Seamless loop:** span one cycle of the animation (e.g. a full melee swing
  from idle back to idle, one tweq period). If the motion doesn't loop, a small
  seam is fine for a demo — or bracket it with a few idle frames on each end.
- Assemble + downscale with PIL (~600px wide is plenty):

```python
from PIL import Image
import glob
files = sorted(glob.glob('/tmp/frames/frame_*.png'))
W = 600
frames = []
for f in files:
    im = Image.open(f).convert('RGB')
    frames.append(im.resize((W, int(im.height * W / im.width)), Image.LANCZOS))
frames[0].save('/tmp/demo.gif', save_all=True, append_images=frames[1:],
               duration=50, loop=0, optimize=True, disposal=2)
```

## 4. Before/after (when the change MODIFIES an existing visual)

If the change alters an *existing* rendered element (not a net-new feature),
include a **before/after** so reviewers see the delta — a one-sided "after"
can't show what improved. Capture both with the **same request sequence** from a
fresh launch (deterministic stepping makes them frame-comparable).

- **Cheapest "before":** if you captured baseline screenshots from the pre-change
  build while iterating, reuse them.
- **Otherwise build the base ref in a throwaway worktree** (doesn't disturb your
  branch; point it at the main checkout's game data):

  ```bash
  BASE=$(gh pr view --json baseRefName -q .baseRefName 2>/dev/null || echo main)
  git worktree add /tmp/before "$BASE"
  ( cd /tmp/before && cargo build -p debug_runtime \
      && DARK_ASSET_PATH="$(git -C "$(git rev-parse --show-toplevel)" rev-parse --show-toplevel)/Data" \
         ./target/debug/debug_runtime --mission debug_weapons --port 8086 & )
  # ...same capture sequence against port 8086, then /v1/shutdown...
  git worktree remove /tmp/before --force
  ```

  (The worktree gets its own cold `target/` — budget several minutes.)

- Compose the two into one labeled side-by-side PNG (renders everywhere, one
  attachment):

  ```python
  from PIL import Image, ImageDraw, ImageFont
  b = Image.open('/tmp/before.png').convert('RGB'); a = Image.open('/tmp/after.png').convert('RGB')
  H = 440; fit = lambda im: im.resize((int(im.width*H/im.height), H), Image.LANCZOS)
  b, a = fit(b), fit(a); gap = 8
  c = Image.new('RGB', (b.width+gap+a.width, H+28), (18,8,28)); c.paste(b,(0,28)); c.paste(a,(b.width+gap,28))
  d = ImageDraw.Draw(c); f = ImageFont.truetype("/System/Library/Fonts/Supplemental/Arial Bold.ttf", 20)
  d.text((8,4), "BEFORE", fill=(255,255,255), font=f); d.text((b.width+gap+8,4), "AFTER", fill=(255,255,255), font=f)
  c.save('/tmp/beforeafter.png', optimize=True)
  ```

  Host it with the GIF (host step) and embed it under a "Before → after" heading
  (embed step). **Skip before/after for net-new features** — there's no prior state.

## 5. Verify

Read a couple of frames (and/or the GIF) back to confirm the look, and confirm the
animation actually moves (diff two frames with `PIL.ImageChops.difference(...).getbbox()`
— `None` means identical). Fix framing/timing and re-capture before uploading.
Watch for the `debug_weapons` floor-litter gotcha (step 2) — make sure what you
captured is the feature, not a dropped world object.

## 6. Host the media in a gist (binary-safe)

The browser drag-drop `user-attachments` host needs your web session cookies — not
reachable with a token. A **gist works** as a token-based equivalent, BUT you must
push binaries via **git** — `gh gist create` flat-out **rejects** a binary file
(`binary file not supported`), so create the gist with a small **text placeholder**
first, then git-push the images into it. Gist raw URLs serve the correct `image/*`
content-type, so they render inline in markdown.

```bash
bash <<'EOF'
set -e
# Create with a TEXT placeholder (gh gist create rejects binary), then git-push the media.
printf '# shock2quest <feature> — media for PR #<N>\n' > /tmp/gist-readme.md
URL=$(gh gist create --desc "shock2quest <feature> — media for PR #<N>" /tmp/gist-readme.md 2>&1 | tail -1)
GID=$(basename "$URL"); USER=$(gh api user -q .login); TOKEN=$(gh auth token)
rm -rf /tmp/gist && git clone "https://x-access-token:$TOKEN@gist.github.com/$GID.git" /tmp/gist
cp /tmp/demo.gif /tmp/beforeafter.png /tmp/gist/
git -C /tmp/gist add -A
git -C /tmp/gist -c user.email="$(git config user.email)" -c user.name="$(git config user.name)" commit -qm "media"
git -C /tmp/gist push -q
for f in demo.gif beforeafter.png; do
  RAW="https://gist.githubusercontent.com/$USER/$GID/raw/$f"
  echo "$RAW  [$(curl -sIL "$RAW" | grep -i '^content-type:' | tr -d '\r')]"
done
EOF
```

Confirm each raw URL reports `content-type: image/gif` / `image/png` (proof the
binary survived and markdown will render it).

## 7. Embed in the PR body

**Do NOT use `gh pr edit --body-file`** here — it hits a Projects-classic GraphQL
path that errors out and silently leaves the body unchanged. Use the REST API:

```bash
# fetch current body, insert the media markdown, write to /tmp/body.md, then:
gh api -X PATCH repos/<owner>/<repo>/pulls/<N> -F body=@/tmp/body.md -q .html_url
```

Markdown to embed (a caption reads nicely under the GIF):

```markdown
![<feature> demo](<gif raw url>)

<sub>Short caption. Captured headlessly via the debug runtime (`/v1/step` + `/v1/screenshot`, deterministic fixed-timestep).</sub>

## Before → after

![before/after](<png raw url>)
```

## 8. Report

Return the gist URL, the raw image URLs, **and the local media paths** (keep the
captured files on disk, e.g. `/tmp/demo.gif` + the stills, don't delete them), plus
a confirmation that the PR body was updated (verify with
`gh pr view <N> --json body -q .body | grep gist`).

## 9. Hand the media to the review (visual verification)

The point of the media isn't just decoration — a reviewer should confirm the
render **actually exercises the feature and looks correct**. After embedding,
use the available review workflow and pass the local media paths to every
image-capable reviewer. For example, when `/xreview` is installed:

```
/xreview --media /tmp/demo.gif,/tmp/shot-t0.png,/tmp/shot-t2.png
```

Pass the GIF **plus two stills captured at different sim times** — a single image
is one frame, so motion must be evidenced by distinct stills. Treat a reviewer
that cannot see the claimed feature in the media as a finding to resolve.
