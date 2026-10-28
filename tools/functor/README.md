# Printable System Shock 2 objects

A [Functor](https://github.com/tommy-xr/functor) project that decodes System Shock 2 static object meshes (`.bin` files starting `LGMD`) in Functor itself and turns them into printable solids.

Game files never enter the repo. `ss2/` is an external prefix (`functor.json`): map it to the game's `data/res` in a gitignored `tools/functor/functor.local.json`, either inside the 25th Anniversary remaster's archive (read in place, no extraction) or a folder with the same layout (`mesh/GRUNT_P.BIN`, `motions/ogpstd_.mc`, `mschema/motiondb.bin`, `mesh/txt16/og_pipe02.gif`, `obj/CRATE01.BIN`, …):

```json
{ "external": { "ss2/": "~/ss2-25th/sshock2.kpf!data/res/" } }
```

```sh
functor -d tools/functor run native                          # view ss2/mesh/GRUNT_P.BIN (set `model` in print.fun)
functor -d tools/functor run native --entry crowd            # 100 hybrids
functor -d tools/functor run vr --entry crowd                # the same on Quest: the headset gets ss2/ files from this machine
functor -d tools/functor export -o tools/functor/crate.stl     # print it (60 mm tall)
functor -d tools/functor test                             # decoder tests (no game files)
```

- `lgmd.fun` — static objects (`LGMD`): header, vertices, polygons and the sub-object tree, posed at rest.
- `lgmm.fun` + `cal.fun` — characters (`LGMM` mesh + `.cal` skeleton): joint-local vertices skinned rigidly or blended ("stretchy" elbows and knees); a `rig` precomputes everything that doesn't change per frame.
- `motion.fun` — animation: `motiondb.bin` (each motion's frames, rate and track→joint map) and `.mc` clips, sampled with slerp; the root bobs vertically. `join` plays clips back to back: most SS2 clips aren't loops (a walk is two half-step clips), so looping one alone snaps at its seam.
- `skinned.fun` — a character as `Model.skinned` data (Y up), so the GPU skins it and `Anim` plays and blends its clips: SS2 joints become a parents-first palette, each blending joint gets a never-animated helper bone (its parent's rotation at its pivot), and clip keys are the conjugated quaternions with the root's vertical bob. `texture` finds the mesh's texture in `ss2/mesh/txt16/` by its first material's name, lowercased (`og_pipe02.gif`); uvs pass through unflipped, as shock2quest's loader does. One texture per model: a mesh with several materials (the assassins' visors, the bots' guns) draws every triangle with the first.
- `affine.fun` — shared 3×4 transform math.
- `print.fun` — the entry: `model` (object, or character with its `skeleton`, playing `clipNames` in turn when `ss2/mschema/motiondb.bin` and the clips are there), else a synthetic cube. The viewer reuses Functor's `PartViewer` controls; a character plays its clips as a skinned model (`Scene.modelOf`). Export prints the pose at `seconds`.
- `crowd.fun` — the `crowd` entry: `count` hybrids, each blending two clips at its own phase (the synthetic character without the game files).
- `fixture.fun` — synthetic object, character, skeleton, motion database and clip, written with `Encode`, and their `expect`s (no game files), including the skinned model against `Lgmm`.
- `realdata.fun` — checks against the real pipe hybrid when `ss2/mesh/GRUNT_P.{BIN,CAL}`, `ss2/mschema/motiondb.bin` and `ss2/motions/ogpstd_.mc` are present (values from shock2quest's own pipeline, also at animated frames of `ogpidle1` and the `ogpbzwkl`+`ogpbzwkr` walk), and the skinned model against `Lgmm` in `ogpstd`, `ogpbzwkl` and `ogpidle1` when their clips are too; pass vacuously otherwise.

Every object in the shipped `obj/` set (1,444, all version 4) decodes; single-part models match their headers' bounding boxes. The pipe hybrid's rest pose and `ogpstd` frame 0 match shock2quest's pipeline to 1e-3. Posed in Functor (`Lgmm`), a character costs ~4.4 ms/frame in a release build; skinned on the GPU it matches that pose to 1e-6 ft, and 100 hybrids cost ~1 ms of render and ~1 ms of sim per frame (M3, release).
