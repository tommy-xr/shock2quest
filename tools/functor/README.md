# Printable System Shock 2 objects

A [Functor](https://github.com/tommy-xr/functor) project that decodes System Shock 2 static object meshes (`.bin` files starting `LGMD`) in Functor itself and turns them into printable solids.

```sh
mkdir -p tools/functor/ss2 && cp Data/res/obj/CRATE01.BIN tools/functor/ss2/model.bin  # gitignored
functor -d tools/functor run native                          # view it
functor -d tools/functor export -o tools/functor/crate.stl     # print it (60 mm tall)
functor -d tools/functor test                             # decoder tests (no game files)
```

- `lgmd.fun` — the decoder: header, vertices, polygons and the sub-object tree, posed at rest (each part's vertices moved by its parents' transforms, then its own). Positions stay in Dark's native Z-up axes.
- `fixture.fun` — a synthetic LGMD cube with a rotated, lifted child part, written with `Encode`, and `expect`s against it.
- `print.fun` — the part: `ss2/model.bin` if present, else the synthetic cube. Change `height` in it for flat objects (it scales the Z extent).

Every object in the shipped `obj/` set (1,444, all version 4) decodes; single-part models match their headers' bounding boxes. Articulated models' header boxes are motion envelopes, so they don't — their rest pose follows the game's math (each part's matrix, as stored, transposed: Dark keeps rotation columns), checked by the fixture's turned and lifted parts.
