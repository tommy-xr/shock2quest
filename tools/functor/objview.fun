// Objview — the doors as drawn and collided: each StdDoor door's static
// mesh (its copied-down `P$ModelName`, from `ss2/obj/`), placed where
// `Door.position` has it and turned to its `P$Position` heading (bank and
// pitch are ignored: every medsci1 door stands upright), with a
// kinematic box from the mesh's bounds. Scene space as `World` (metres, Y up).

// A door's look: its mesh (Dark axes, feet), heading, and box (centre and
// size in its own axes).
type view = { id: float, mesh: Mesh.t, heading: float, center: List<float>, size: List<float> }

let ft = World.metresPerFoot
// Some door meshes are a single sheet; their boxes get this thickness (feet).
let minThickness = 0.25

let tagOf = (id: float): Physics.tag => Physics.tag(Text.join("", ["obj", Text.fromFloat(id)]))

let modelPath = (name: string): Path.t => Path.of(Text.join("", ["ss2/obj/", name, ".bin"]))

// The doors whose model is there.
let views = (l: Props.loaded): List<view> =>
  Map.keys(l.world.door) |> List.concatMap((id) =>
    let name = Objects.first(l.modelNames, List.append(Door.donorsOf(l.world, id), [id])) in
    match Option.map(modelPath, name) with
    | Option.Some(path) =>
      if not File.exists(path) then []
      else (match Lgmd.mesh(File.bytes(path)) with
        | Result.Ok(mesh) =>
          let b = Mesh.bounds(mesh) in
          let lo = [Vec3.x(b.min), Vec3.y(b.min), Vec3.z(b.min)] in
          let hi = [Vec3.x(b.max), Vec3.y(b.max), Vec3.z(b.max)] in
          [{ id: id, mesh: mesh, heading: Map.get(id, l.headings) |> Option.defaultValue(0.0),
             center: Door.lerp(lo, hi, 0.5), size: [Affine.n(0.0, hi) - Affine.n(0.0, lo), Affine.n(1.0, hi) - Affine.n(1.0, lo), Affine.n(2.0, hi) - Affine.n(2.0, lo)] }]
        | Result.Error(_) => [])
    | Option.None => [])

// A point in the door's own axes, turned to its heading, at `at` (Dark).
let place = (v: view, at: List<float>, p: List<float>): List<float> =>
  let c = Math.cos(v.heading) in
  let s = Math.sin(v.heading) in
  [Affine.n(0.0, at) + c * Affine.n(0.0, p) - s * Affine.n(1.0, p), Affine.n(1.0, at) + s * Affine.n(0.0, p) + c * Affine.n(1.0, p),
   Affine.n(2.0, at) + Affine.n(2.0, p)]

let scenePos = (p: List<float>): Vec3.t => Skinned.vec(Affine.scale(ft, p))

let bodyAt = (v: view, at: List<float>): Physics.body =>
  let side = (k: float) => ft * Math.max(Affine.n(k, v.size), minThickness) in
  Physics.kinematic(tagOf(v.id), Physics.box(side(0.0), side(2.0), side(1.0)))
    |> Physics.rotateY(Angle.radians(v.heading)) |> Physics.at(scenePos(place(v, at, v.center)))

let nodeAt = (v: view, at: List<float>): Scene.t =>
  Mesh.toScene(v.mesh) |> Scene.scale(ft) |> Scene.rotateY(Angle.radians(v.heading)) |> Scene.translate(scenePos(at))
    |> Scene.lit(Color.rgb(0.55, 0.58, 0.62))

// Doors at rest change only when one starts or stops, so they are built
// once into a memo group (`Physics.memo`, `Scene.memo`) keyed on this: the
// door table without the moving doors, and the doors' TransDoor cells.
// While nothing moves both are last frame's maps, so the check is free.
type still = { door: Map<float, Door.doorScript>, transDoor: Map<float, Objects.Cell<Door.transDoor>> }

let still = (w: Door.world): still =>
  { door: Map.fold((m, id, _) => Map.remove(id, m), w.door, w.moving), transDoor: w.props.transDoor }

let placed = (views: List<view>, s: still, make: (view, List<float>) => 'a): List<'a> =>
  views |> List.concatMap((v: view) =>
    match Map.get(v.id, s.door) with
    | Option.Some(d) =>
      Objects.get(v.id, s.transDoor) |> Option.toList |> List.map((td: Door.transDoor) => make(v, Door.lerp(td.baseClosed, td.baseOpen, d.t)))
    | Option.None => [])

// The moving doors, declared each frame.
let moving = (views: List<view>, w: Door.world, make: (view, List<float>) => 'a): List<'a> =>
  views |> List.filter((v: view) => Map.member(v.id, w.moving))
    |> List.concatMap((v: view) => Door.position(w, v.id) |> Option.toList |> List.map((at) => make(v, at)))
