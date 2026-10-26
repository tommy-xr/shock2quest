// System Shock 2 objects and characters, decoded in Functor: view them (a
// character plays its clip) and print them. Copy a model into ss2/ (gitignored,
// so game files stay out of the repo); without one, a synthetic cube shows.
//
//   mkdir -p tools/functor/ss2 && cp Data/res/obj/CRATE01.BIN tools/functor/ss2/model.bin
//   # or a character: its mesh, skeleton, the motion database and a clip
//   cp Data/res/mesh/GRUNT_P.BIN tools/functor/ss2/model.bin
//   cp Data/res/mesh/GRUNT_P.CAL tools/functor/ss2/model.cal
//   cp Data/motiondb.bin Data/res/motions/ogpbzwkl_.mc tools/functor/ss2/
//   functor -d tools/functor run native                         # view it
//   functor -d tools/functor export -o tools/functor/part.stl   # print it

let model = Path.of("ss2/model.bin")

// A character plays this clip (looping); it prints posed at `seconds`.
let clipName = "ogpbzwkl"
let seconds = 0.0

let clipFile = Text.join("", ["ss2/", clipName, "_.mc"])

let data = if File.exists(model) then File.bytes(model) else Fixture.cube(4.0)

type character = { rig: Lgmm.rig, clip: Option.t<Motion.clip> }

let character: Option.t<character> =
  if Bytes.latin1(0.0, 4.0, data) != Result.Ok("LGMM")
  then Option.None
  else
    let clip =
      if File.exists("ss2/motiondb.bin") && File.exists(clipFile)
      then Option.Some(Motion.clip(File.bytes("ss2/motiondb.bin"), File.bytes(clipFile), clipName) |> Binary.require)
      else Option.None in
    Option.Some({
      rig: Lgmm.rig(Decode.run(Lgmm.model, data) |> Binary.require,
        Decode.run(Cal.skeleton, File.bytes("ss2/model.cal")) |> Binary.require),
      clip: clip
    })

// The character at `t` seconds of its clip (at rest without one).
let posedAt = (c: character, t: float): Mesh.t =>
  match c.clip with
  | Option.Some(clip) =>
    let p = Motion.sample(clip, t) in
    Lgmm.poseRig(c.rig, p.root, p.animate)
  | Option.None => Lgmm.poseRig(c.rig, Affine.identity, (j) => Affine.identity)

let mesh =
  match character with
  | Option.Some(c) => posedAt(c, seconds)
  | Option.None => Lgmd.mesh(data) |> Binary.require

let part = mesh |> Solid.fromMesh({ height: 60.0, nozzle: 0.4 })

// The viewer: PartViewer's camera and controls; a character animates.
let init = PartViewer.init
let tick = PartViewer.tick
let mouseMove = PartViewer.mouseMove
let mouseButton = PartViewer.mouseButton
let mouseWheel = PartViewer.mouseWheel

// Mesh space (Z up) to scene space (Y up), as Mesh.toScene draws it.
let toScene = (v: Vec3.t) => Vec3.make(Vec3.x(v), Vec3.z(v), 0.0 - Vec3.y(v))

let animate = (c: character, m, tts: float) =>
  let b = Mesh.bounds(mesh) in
  let lo = toScene(b.min) in
  let hi = toScene(b.max) in
  let center = Vec3.scale(0.5, Vec3.add(lo, hi)) in
  let size = Vec3.distance(lo, hi) in
  Frame.createLit(
    OrbitCamera.camera(m.orbit, center, size),
    Scene.group([
      Scene.plane()
        |> Scene.scale(size * 4.0)
        |> Scene.translate(Vec3.make(Vec3.x(center), Vec3.y(lo), Vec3.z(center)))
        |> Scene.lit(Color.rgb(0.55, 0.56, 0.6)),
      Mesh.toScene(posedAt(c, tts)) |> Scene.lit(Color.rgb(0.25, 0.5, 0.85)),
    ]),
    [
      Light.ambient(Color.rgb(0.18, 0.18, 0.22)),
      Light.directional(Vec3.make(0.4, -1.0, 0.3), Color.rgb(1.0, 0.97, 0.92), 0.9),
      Light.directional(Vec3.make(-0.6, -0.2, -0.5), Color.rgb(0.75, 0.8, 1.0), 0.45),
    ])

let draw = (m, tts: float) =>
  match character with
  | Option.Some(c) => animate(c, m, tts)
  | Option.None => PartViewer.draw(part, m, tts)
