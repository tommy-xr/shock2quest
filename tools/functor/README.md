# System Shock 2 in Functor

A [Functor](https://github.com/tommy-xr/functor) project that decodes System Shock 2 data in Functor itself: static object meshes (`.bin` files starting `LGMD`) as printable solids, characters as skinned models, and medsci1's world rep as a level you walk, on a desktop or a Quest.

Game files never enter the repo. `files.fun` declares `ss2/` external in `config` (`Config.external("ss2/", Source.mount("ss2"))`): map the `ss2` mount to the game's `data/res` once per machine in `~/.config/functor/mounts.json` (or pass `--mount ss2=PATH`), either inside the 25th Anniversary remaster's archive (read in place, no extraction) or a folder with the same layout (`mesh/GRUNT_P.BIN`, `motions/ogpstd_.mc`, `mschema/motiondb.bin`, `mesh/txt16/og_pipe02.gif`, `obj/CRATE01.BIN`, …):

```json
{ "ss2": "~/ss2-25th/sshock2.kpf!data/res/", "ss2data": "~/ss2-25th/sshock2.kpf!data/" }
```

`ss2data` is the folder above `res/`, where the missions (`medsci1.mis`) are. SS2 names files in any case (a mission's texture list says `Medsci_1`, the archive `MedSci_1`), so both prefixes match ignoring case (`Source.ignoreCase`).

```sh
functor -d tools/functor run native                          # view ss2/mesh/GRUNT_P.BIN (set `model` in print.fun)
functor -d tools/functor run native --entry crowd            # 100 hybrids
functor -d tools/functor run vr --entry crowd                # the same on Quest: the headset gets ss2/ files from this machine
functor -d tools/functor run native --entry walk             # walk medsci1 (WASD, arrows, left-drag looks)
functor -d tools/functor run vr --entry walk                 # walk it on Quest: left stick walks where you look, right stick snap-turns
functor -d tools/functor export -o tools/functor/crate.stl     # print it (60 mm tall)
functor -d tools/functor test                             # decoder tests (no game files)
```

- `lgmd.fun` — static objects (`LGMD`): header, vertices, polygons and the sub-object tree, posed at rest.
- `lgmm.fun` + `cal.fun` — characters (`LGMM` mesh + `.cal` skeleton): joint-local vertices skinned rigidly or blended ("stretchy" elbows and knees); a `rig` precomputes everything that doesn't change per frame.
- `motion.fun` — animation: `motiondb.bin` (each motion's frames, rate and track→joint map) and `.mc` clips, sampled with slerp; the root bobs vertically. `join` plays clips back to back: most SS2 clips aren't loops (a walk is two half-step clips), so looping one alone snaps at its seam.
- `skinned.fun` — a character as `Model.skinned` data (Y up), so the GPU skins it and `Anim` plays and blends its clips: SS2 joints become a parents-first palette, each blending joint gets a never-animated helper bone (its parent's rotation at its pivot), and clip keys are the conjugated quaternions with the root's vertical bob. `texture` finds the mesh's texture in `ss2/mesh/txt16/` by its first material's name, lowercased (`og_pipe02.gif`); uvs pass through unflipped, as shock2quest's loader does. One texture per model: a mesh with several materials (the assassins' visors, the bots' guns) draws every triangle with the first.
- `mis.fun` — mission files (Dark chunk files): the chunk table, the world rep (`WRRGB`: cells, polygons, texturing, RGB555 lightmaps; animated layers skipped), the texture list, the ambient level and the player start (the last `L$PlayerFac` link's source, as shock2quest picks it).
- `pcx.fun` — 8-bit PCX (the world textures): run-length decode, palette to RGB8.
- `world.fun` — a mission's world rep as ONE `Model.static` and ONE `Physics.trimesh`, in metres with Y up: polygons fan-triangulated and grouped by texture (one draw each), texture and lightmap uvs computed as shock2quest does, every polygon's lightmap packed into one `Texture.atlas` (sRGB, so it multiplies in gamma space like the game). The sky (texture 249) is left out.
- `walk.fun` — the `walk` entry: shock2quest's player (6 ft capsule, 2 ft steps, 0.7 walkable normal, 11 ft/s) on `Physics.character`, placed on the floor under the player factory once the level is in the physics world. VR walks where the head looks; desktop uses keys.
- `misfixture.fun` — a synthetic mission (one cell, one lit square) and PCX written with `Encode`, and their `expect`s; `missiondata.fun` checks medsci1's counts and start when `ss2data/` is mapped.
- `affine.fun` — shared 3×4 transform math.
- `files.fun` — the `config` every entry shares: `ss2/` and `ss2data/` are external, from the `ss2` and `ss2data` mounts.
- `print.fun` — the entry: `model` (object, or character with its `skeleton`, playing `clipNames` in turn when `ss2/mschema/motiondb.bin` and the clips are there), else a synthetic cube. The viewer reuses Functor's `PartViewer` controls; a character plays its clips as a skinned model (`Scene.modelOf`). Export prints the pose at `seconds`.
- `crowd.fun` — the `crowd` entry: `count` hybrids, each blending two clips at its own phase (the synthetic character without the game files).
- `fixture.fun` — synthetic object, character, skeleton, motion database and clip, written with `Encode`, and their `expect`s (no game files), including the skinned model against `Lgmm`.
- `realdata.fun` — checks against the real pipe hybrid when `ss2/mesh/GRUNT_P.{BIN,CAL}`, `ss2/mschema/motiondb.bin` and `ss2/motions/ogpstd_.mc` are present (values from shock2quest's own pipeline, also at animated frames of `ogpidle1` and the `ogpbzwkl`+`ogpbzwkr` walk), and the skinned model against `Lgmm` in `ogpstd`, `ogpbzwkl` and `ogpidle1` when their clips are too; pass vacuously otherwise.

Every object in the shipped `obj/` set (1,444, all version 4) decodes; single-part models match their headers' bounding boxes. The pipe hybrid's rest pose and `ogpstd` frame 0 match shock2quest's pipeline to 1e-3. Posed in Functor (`Lgmm`), a character costs ~4.4 ms/frame in a release build; skinned on the GPU it matches that pose to 1e-6 ft, and 100 hybrids cost ~1 ms of render and ~1 ms of sim per frame (M3, release).
