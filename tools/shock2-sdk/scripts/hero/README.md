# Hero capture recipes

Build the SDK (`npm ci && npm run build` in `tools/shock2-sdk`) and provide
Remaster assets as described in the root `DEVELOPMENT.md`. Requires Node.js 22+.

From `tools/shock2-sdk`:

```sh
node scripts/hero/weapon-melee.mjs --output /tmp/shock2quest-hero
node scripts/hero/environmental.mjs --output /tmp/shock2quest-hero
```

Without `--output`, files go to `screenshots/hero/`. Each script launches hidden
VR debug runtimes with the SDK, uses fixed simulation steps, and shuts them down
in `finally`. Inspect the captures before selecting them for publication.

The MedSci recipe provisions a pistol and wrench, acquires both with normal VR
grip inputs, and frames them in a medical room. It asserts the player is alive,
both weapons remain held, and the player settles on the expected floor. The
still demonstrates the loadout rather than a combat encounter.

Environmental recipes use detached cameras in Hydro1 and Rec1. Rec1's existing
court lamps are activated via their authored messages. No lighting overrides or
image postprocessing are applied.

Adjacent JSON files retain the engine revision and camera/setup provenance.
The requested width is a cap, never an upscale: a native HiDPI framebuffer
produces 1600×1200; a standard-density display may produce less. Fixed stepping
makes the setup repeatable, but AI scheduling, physics and GPU differences mean
exact pixel equality is not guaranteed.

## Combat prototype

With `ffmpeg` installed, add `--video` to the weapon recipe:

```sh
node scripts/hero/weapon-melee.mjs --video --output /tmp/shock2quest-hero
```

It records two pistol shots and a tracked wrench strike against a staged hybrid,
asserts ammo use and separate damage phases, and exports a silent MP4 and looping
GIF at 20 fps. Raw frames stay in a fresh temporary directory printed at the end.
The motion and loop cut are prototypes; the still remains the primary hero image.
