// System Shock 2 objects and characters, decoded in Functor: view them (a
// character plays its clip) and print them. `ss2/` is the game's data/res,
// mapped on your machine (see README), so game files stay out of the repo;
// without it, a synthetic cube shows.
//
//   functor -d tools/functor run native                         # view it
//   functor -d tools/functor export -o tools/functor/part.stl   # print it

// The model to show: a character with its skeleton (the .CAL beside it), or
// an object such as "ss2/obj/CRATE01.BIN".
let model = Path.of("ss2/mesh/GRUNT_P.BIN")
let skeleton = Path.of("ss2/mesh/GRUNT_P.CAL")

// A character plays these clips in turn, looping (a walk is two half-step
// clips; either alone snaps at its seam); it prints posed at `seconds`.
let clipNames = ["ogpbzwkl", "ogpbzwkr"]
let seconds = 0.0

let clipFile = (n: string) => Path.of(Text.join("", ["ss2/motions/", n, "_.mc"]))

let data = if File.exists(model) then File.bytes(model) else Fixture.cube(4.0)

type character = { rig: Lgmm.rig, clip: Option.t<Motion.clip> }

let character: Option.t<character> =
  if Bytes.latin1(0.0, 4.0, data) != Result.Ok("LGMM")
  then Option.None
  else
    let clip =
      if File.exists(Path.of("ss2/mschema/motiondb.bin")) && List.all((n) => File.exists(clipFile(n)), clipNames)
      then
        let clips = List.map((n) => Motion.clip(File.bytes(Path.of("ss2/mschema/motiondb.bin")), File.bytes(clipFile(n)), n) |> Binary.require, clipNames) in
        List.head(clips)
          |> Option.map((first) => List.fold((joined, next) => Motion.join(joined, next) |> Binary.require, first, List.drop(1.0, clips)))
      else Option.None in
    let cal =
      if File.exists(skeleton) then File.bytes(skeleton)
      else Binary.invalid(Text.join("", [Path.toString(model), " is a character: set `skeleton` to its .CAL"])) in
    Option.Some({
      rig: Lgmm.rig(Decode.run(Lgmm.model, data) |> Binary.require,
        Decode.run(Cal.skeleton, cal) |> Binary.require),
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

// The character as a GPU-skinned model (built once): the viewer plays its
// joined clips on it (named after them), at rest without them. Textured when
// its texture is in ss2/mesh/txt16/.
let clipName = Text.join("+", clipNames)

let skinned: Option.t<Model.t> =
  character
    |> Option.map((c: character) =>
      let d = Skinned.data(data, c.rig, Option.toList(Option.map((clip) => (clipName, clip), c.clip))) |> Binary.require in
      Model.skinned({ d with texture: Skinned.texture(data) }))

let playing = Option.isSome(Option.bind((c: character) => c.clip, character))

// The viewer: PartViewer's camera and controls; a character animates.
let init = PartViewer.init
let tick = PartViewer.tick
let mouseMove = PartViewer.mouseMove
let mouseButton = PartViewer.mouseButton
let mouseWheel = PartViewer.mouseWheel

// Mesh space (Z up) to scene space (Y up), as the skinned model is built.
let toScene = (v: Vec3.t) => Vec3.make(Vec3.x(v), Vec3.z(v), 0.0 - Vec3.y(v))

// Framing for the animated view, from the printed pose.
let lo = toScene(Mesh.bounds(mesh).min)
let hi = toScene(Mesh.bounds(mesh).max)
let center = Vec3.scale(0.5, Vec3.add(lo, hi))
let size = Vec3.distance(lo, hi)

let animate = (model: Model.t, m, tts: float) =>
  Frame.createLit(
    OrbitCamera.camera(m.orbit, center, size),
    Scene.group([
      Scene.plane()
        |> Scene.scale(size * 4.0)
        |> Scene.translate(Vec3.make(Vec3.x(center), Vec3.y(lo), Vec3.z(center)))
        |> Scene.lit(Color.rgb(0.55, 0.56, 0.6)),
      Scene.modelOf(model) |> Scene.animate(if playing then Anim.clip(clipName, tts) else Anim.rest()),
    ]),
    [
      Light.ambient(Color.rgb(0.18, 0.18, 0.22)),
      Light.directional(Vec3.make(0.4, -1.0, 0.3), Color.rgb(1.0, 0.97, 0.92), 0.9),
      Light.directional(Vec3.make(-0.6, -0.2, -0.5), Color.rgb(0.75, 0.8, 1.0), 0.45),
    ])

let draw = (m, tts: float) =>
  match skinned with
  | Option.Some(model) => animate(model, m, tts)
  | Option.None => PartViewer.draw(part, m, tts)
