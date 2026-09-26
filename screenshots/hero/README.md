# Hero captures

These are actual engine screenshots, captured with `debug_runtime --vr` and
Remaster assets. The README uses stills first; the same setups are the starting
point for short gameplay clips and website media.

## Regenerate the stills

Follow [DEVELOPMENT.md](../../DEVELOPMENT.md) to provide the game assets. Use
Node.js 22+ and the repository's Rust toolchain. From the repository root:

```sh
cargo build -p debug_runtime
cd tools/shock2-sdk
npm ci
npm run build
node scripts/hero/weapon-melee.mjs
node scripts/hero/environmental.mjs
```

Both scripts default to this directory. To review new captures separately, pass
`--output /tmp/shock2quest-hero` to either script. The SDK owns runtime startup
and shutdown. Windows remain hidden, and each launch gets its own port.

| Image | Setup |
| --- | --- |
| `medsci-weapon-melee.png` | MedSci1, attached player camera, pistol in the right hand and wrench in the left. Debug-provisioned weapons are acquired through normal VR grip inputs. Unused provisioning items remain at the distant spawn. |
| `hydroponics.png` | Hydro1, detached camera, authored lighting and maintenance robot. |
| `recreation-pool.png` | Rec1, detached camera, authored court lamps activated through their existing messages. |

The weapon still demonstrates the loadout and hand presentation; it does not
claim a successful combat encounter. Environmental cameras are detached for
unobstructed framing. No image generation, color grading, or lighting overrides
are applied.

Each adjacent JSON file records the engine revision and capture settings. Shot
positions and hand poses live in the scripts; runtime entity IDs are discovered
on each launch. Simulation stepping is fixed at 60 Hz, but AI scheduling,
physics, and GPU differences mean exact pixel equality is not guaranteed.

The captures request at most 1600 pixels in width. The current renderer has an
800×600 logical window; these images were captured from its native 1600×1200
HiDPI framebuffer. A standard-density display may produce smaller files. The
screenshot API never upscales. True selectable output resolution/aspect ratio
is a future renderer change.

## Weapon + melee motion prototype

[Looping GIF](prototype/weapon-melee.gif) · [MP4](prototype/weapon-melee.mp4) ·
[Combat evidence](prototype/combat-evidence.json)

The prototype is 4.5 seconds at 20 fps: two pistol shots, an approaching hybrid,
a tracked wrench strike, and recovery. The script checks ammunition consumption,
damage after each phase, the target's defeat, and that the player survives with
both weapons retained. The opponent is debug-provisioned; damage comes from
normal VR inputs. It sets Standard Weapons to level 1 and uses an 80-degree
capture field of view. The primary README still keeps the default projection.

With `ffmpeg` installed, run from `tools/shock2-sdk`:

```sh
node scripts/hero/weapon-melee.mjs --video --output /tmp/shock2quest-hero
```

This also generates the loadout still. Raw PNG frames are retained in a fresh
temporary directory printed by the script, and GIF/MP4 exports go to the chosen
output directory. The MP4 has no audio. The hand recovery and loop cut are still
abrupt, so this is a motion prototype rather than the README's primary hero.

## Choosing and refreshing media

Regenerate to a temporary directory and inspect the images before replacing the
README versions. Check that both hands and their items fit in frame, the camera
is clear of geometry, lighting is readable, and no debug-spawned clutter or
story spoilers appear. Keep the JSON alongside the selected PNG.

Future takes can build on the prototype's setup, timed head/hand poses, input
edges, and fixed capture cadence. A headset pose/input recording can later
replace the scripted motion without replacing the setup or export workflow.
