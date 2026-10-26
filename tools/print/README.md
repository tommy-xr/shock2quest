# Printable System Shock 2 objects

A [Functor](https://github.com/tommy-xr/functor) project that decodes System Shock 2 static object meshes (`.bin` files starting `LGMD`) in Functor itself and turns them into printable solids.

```sh
cp Data/res/obj/CRATE01.BIN tools/print/ss2/model.bin   # ss2/ is gitignored
functor -d tools/print run native                       # view it
functor -d tools/print export -o crate.stl              # print it (60 mm tall)
functor -d tools/print test                             # decoder tests (no game files)
```

- `lgmd.fun` — the decoder: header, vertices, polygons and the sub-object tree, posed at rest (each part's vertices moved by its parents' transforms, then its own). Positions stay in Dark's native Z-up axes.
- `fixture.fun` — a synthetic LGMD cube with a rotated, lifted child part, written with `Encode`, and `expect`s against it.
- `print.fun` — the part: `ss2/model.bin` if present, else the synthetic cube. Change `height` in it for flat objects (it scales the Z extent).

Every object in the shipped `obj/` set (1,444, all version 4) decodes; single-part models match their headers' bounding boxes. Articulated models' header boxes are motion envelopes, so they don't.
