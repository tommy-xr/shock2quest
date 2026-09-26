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

## Choosing and refreshing media

Regenerate to a temporary directory and inspect the images before replacing the
README versions. Check that both hands and their items fit in frame, the camera
is clear of geometry, lighting is readable, and no debug-spawned clutter or
story spoilers appear. Keep the JSON alongside the selected PNG.

Future motion takes can build on these setups with timed head/hand poses and
input edges, capturing at a fixed simulation cadence. Verify ammunition use and
target damage when a clip demonstrates combat. Headset pose/input recordings
can later replace scripted motion without replacing setup and export.
