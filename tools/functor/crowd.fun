// A crowd of GPU-skinned pipe hybrids: `count` in a grid, each on its own
// playhead blending two of its clips, so every figure is posed independently
// every frame. Shadows are on (the engine's shadow map covers the near rows). Uses ss2/hybrid.{bin,cal}, ss2/motiondb.bin and
// each clip's .mc (see README).
//
//   functor -d tools/functor run native --entry crowd
//
// Set `count` (1 / 25 / 50 / 100) and save: hot reload applies it. `GET /state`
// reports the frame's `skinned` draws / pose evaluations and `stages` timings.

let count = 100.0

// Clips that loop: "walk" is the stride pair `ogpbzwkl` + `ogpbzwkr` (either
// alone snaps at its seam); one-shot gestures like `ogpidle1` would too.
let clipNames = ["walk", "ogpstd"]

let files = List.append(List.map((n) => Text.join("", ["ss2/", n, "_.mc"]), ["ogpbzwkl", "ogpbzwkr", "ogpstd"]),
  ["ss2/hybrid.bin", "ss2/hybrid.cal", "ss2/motiondb.bin"])

// Without the game files, the synthetic character from fixture.fun stands in.
let real = List.all((p) => File.exists(p), files)

let hybrid =
  let mesh = if real then File.bytes("ss2/hybrid.bin") else Fixture.characterMesh in
  let cal = if real then File.bytes("ss2/hybrid.cal") else Fixture.characterSkeleton in
  let load = (n: string): Motion.clip =>
    Motion.clip(File.bytes("ss2/motiondb.bin"), File.bytes(Text.join("", ["ss2/", n, "_.mc"])), n) |> Binary.require in
  let clipOf = (n: string): Motion.clip =>
    if not real then Fixture.twisting
    else if n == "walk" then Motion.join(load("ogpbzwkl"), load("ogpbzwkr")) |> Binary.require
    else load(n) in
  let r = Lgmm.rig(Decode.run(Lgmm.model, mesh) |> Binary.require, Decode.run(Cal.skeleton, cal) |> Binary.require) in
  Skinned.data(mesh, r, List.map((n) => (n, clipOf(n)), clipNames)) |> Binary.require |> Model.skinned

// Feet: soles sit about 4.25 below the root.
let spacing = 5.0
let ground = 0.0 - 4.25

let columns = Math.ceil(Math.sqrt(count))
let rows = Math.ceil(count / columns)

// A deterministic per-figure fraction in [0, 1): the golden-ratio sequence.
let fraction = (i: float): float => Math.mod(i * 0.618034, 1.0)

let clipAt = (k: float): string => Option.defaultValue("ogpstd", List.nth(Math.mod(k, List.length(clipNames)), clipNames))

let init = {}

let tick = (model, dt: float, tts: float) => model

// Figure i blends clips i and i + 1 (weights in [0.25, 0.75], so both are
// sampled) at its own phase.
let figure = (i: float, tts: float): Scene.t =>
  let row = Math.floor(i / columns) in
  let column = i - row * columns in
  let playhead = tts + fraction(i + 0.5) * 4.0 in
  let w = 0.25 + 0.5 * fraction(i) in
  Scene.modelOf(hybrid)
    |> Scene.animate(Anim.blend([
         (Anim.clip(clipAt(i), playhead), 1.0 - w),
         (Anim.clip(clipAt(i + 1.0), playhead), w),
       ]))
    // The hybrid faces +X; turn it to face the camera (-Z).
    |> Scene.rotateY(Angle.degrees(90.0))
    |> Scene.translate(Vec3.make((column - (columns - 1.0) / 2.0) * spacing, 0.0, row * spacing))

let extent = Math.max(columns, rows) * spacing

let camera =
  Camera3D.lookAt(
    Vec3.make(0.0, 4.0 + extent * 0.6, 0.0 - extent * 0.6),
    Vec3.make(0.0, ground, (rows - 1.0) * spacing / 2.0))

let draw = (model, tts: float) =>
  Frame.createLit(
    camera,
    Scene.group([
      Scene.plane() |> Scene.scale(extent + 20.0) |> Scene.translate(Vec3.make(0.0, ground, (rows - 1.0) * spacing / 2.0))
        |> Scene.lit(Color.rgb(0.42, 0.47, 0.55)),
      Scene.group(List.range(count) |> List.map((i) => figure(i, tts))),
    ]),
    [
      Light.ambient(Color.rgb(0.25, 0.25, 0.3)),
      Light.directional(Vec3.make(-0.5, -1.0, 0.4), Color.rgb(1.0, 0.96, 0.88), 1.0) |> Light.castShadows,
    ])
