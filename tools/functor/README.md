# Printable System Shock 2 objects

A [Functor](https://github.com/tommy-xr/functor) project that decodes System Shock 2 static object meshes (`.bin` files starting `LGMD`) in Functor itself and turns them into printable solids.

```sh
mkdir -p tools/functor/ss2 && cp Data/res/obj/CRATE01.BIN tools/functor/ss2/model.bin  # gitignored
# or a character playing a clip:
cp Data/res/mesh/GRUNT_P.BIN tools/functor/ss2/model.bin && cp Data/res/mesh/GRUNT_P.CAL tools/functor/ss2/model.cal
cp Data/motiondb.bin Data/res/motions/ogpbzwkl_.mc tools/functor/ss2/
functor -d tools/functor run native                          # view it
functor -d tools/functor export -o tools/functor/crate.stl     # print it (60 mm tall)
functor -d tools/functor test                             # decoder tests (no game files)
```

- `lgmd.fun` — static objects (`LGMD`): header, vertices, polygons and the sub-object tree, posed at rest.
- `lgmm.fun` + `cal.fun` — characters (`LGMM` mesh + `.cal` skeleton): joint-local vertices skinned rigidly or blended ("stretchy" elbows and knees); a `rig` precomputes everything that doesn't change per frame.
- `motion.fun` — animation: `motiondb.bin` (each motion's frames, rate and track→joint map) and `.mc` clips, sampled with slerp; the root bobs vertically.
- `affine.fun` — shared 3×4 transform math.
- `print.fun` — the entry: `ss2/model.bin` (object, or character with `ss2/model.cal`, playing `clipName` when `ss2/motiondb.bin` and the clip are there), else a synthetic cube. The viewer reuses Functor's `PartViewer` controls; a character animates via `Mesh.toScene`. Export prints the pose at `seconds`.
- `fixture.fun` — synthetic object, character, skeleton, motion database and clip, written with `Encode`, and their `expect`s (no game files).
- `realdata.fun` — checks against the real pipe hybrid when `ss2/hybrid.{bin,cal}`, `ss2/motiondb.bin` and `ss2/ogpstd_.mc` are present (values from shock2quest's own pipeline); pass vacuously otherwise.

Every object in the shipped `obj/` set (1,444, all version 4) decodes; single-part models match their headers' bounding boxes. The pipe hybrid's rest pose and `ogpstd` frame 0 match shock2quest's pipeline to 1e-3. Playing a clip costs ~4.4 ms/frame in a release build (posing runs in Functor).
