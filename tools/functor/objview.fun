// Objview — the doors as drawn and collided: each StdDoor door's static
// model (its copied-down `P$ModelName`, from `ss2/obj/`) textured from
// `ss2/obj/txt16/`, with a kinematic box from its bounds. A sliding door
// sits at `Door.position`, turned to its `P$Position` heading (bank and
// pitch are ignored: every medsci1 door stands upright); a rotating door
// where `Door.swing` turns it. Scene space as `World` (metres, Y up).

// One `Model.static` drawn at one opacity (1 opaque).
type piece = { model: Model.t, opacity: float }

// A model's look: its pieces (scene axes, feet) and box (centre and size in
// its own Dark axes).
type look = { pieces: List<piece>, center: List<float>, size: List<float> }

type view = { id: float, look: look, heading: float }

let ft = World.metresPerFoot
// Some door meshes are a single sheet; their boxes get this thickness (feet).
let minThickness = 0.25

let tagOf = (id: float): Physics.tag => Physics.tag(Text.join("", ["obj", Text.fromFloat(id)]))

let modelPath = (name: string): Path.t => Path.of(Text.join("", ["ss2/obj/", name, ".bin"]))

// A material's texture, `ss2/obj/txt16/<name>` (8-bit PCX), if it's there.
let textureOf = (name: string): Option.t<Asset.Texture> =>
  let path = Path.of(Text.join("", ["ss2/obj/txt16/", name])) in
  if not File.exists(path) then Option.None
  else Texture.decode(File.bytes(path), Texture.Srgb) |> Result.toOption |> Option.map(Texture.fromPixels)

let unit = (v: List<float>): List<float> => Affine.scale(1.0 / Math.max(Math.sqrt(World.dot(v, v)), 0.000001), v)

// Faces as one static model, fan-triangulated counter-clockwise (Dark winds
// its polygons the other way), one group per run of a material slot.
let staticOf = (faces: List<Lgmd.face>, groupOf: (float) => Model.group): Model.t =>
  let corners = List.concatMap((f: Lgmd.face) => f.corners, faces) in
  let triangles = faces |> List.fold((acc, f: Lgmd.face) => match acc with
    | (first, ts) =>
      let k = List.length(f.corners) in
      (first + k, List.append(List.range(k - 2.0) |> List.concatMap((j) => [first, first + j + 2.0, first + j + 1.0]), ts)),
    (0.0, [])) in
  let groups = faces |> List.fold((acc, f: Lgmd.face) =>
      let tris = List.length(f.corners) - 2.0 in
      match List.last(acc) with
      | Option.Some(g) =>
        if g.slot == f.slot then List.append([{ g with count: g.count + tris }], List.take(List.length(acc) - 1.0, acc))
        else List.append([{ slot: f.slot, first: g.first + g.count, count: tris }], acc)
      | Option.None => [{ slot: f.slot, first: 0.0, count: tris }], [])
    |> List.map((g) => { groupOf(g.slot) with first: g.first, count: g.count }) in
  Model.static({
    positions: Bytes.fromF32le(List.concatMap((c: Lgmd.corner) => Skinned.up(c.p), corners)),
    normals: Bytes.fromF32le(List.concatMap((c: Lgmd.corner) => Skinned.up(unit(c.n)), corners)),
    uvs: Bytes.fromF32le(List.concatMap((c: Lgmd.corner) => c.uv, corners)),
    uvs2: Bytes.zeros(0.0),
    triangles: Bytes.fromU32le(match triangles with | (_, ts) => ts),
    groups: groups, lightmap: Option.None })

// A model's look, as shock2quest draws it: texture-mapped polygons whose
// material's texture is there (others are dropped), self-lit materials
// fullbright, see-through ones in their own piece at their opacity. Colour
// materials aren't drawn (no door uses one).
let lookOf = (textures: Map<string, Asset.Texture>, s: Lgmd.surface): look =>
  let materialOf = (slot: float) => List.find((m: Lgmd.material) => m.slot == slot, s.materials) in
  let textureFor = (slot: float) => materialOf(slot) |> Option.bind((m: Lgmd.material) => if m.color then Option.None else Map.get(m.name, textures)) in
  let drawn = s.faces |> List.filter((f: Lgmd.face) => f.mapped && Option.isSome(textureFor(f.slot))) |> List.sortBy((f: Lgmd.face) => f.slot) in
  let clearness = (f: Lgmd.face) => materialOf(f.slot) |> Option.map((m: Lgmd.material) => m.transparency) |> Option.defaultValue(0.0) in
  let levels = drawn |> List.fold((m, f) => Map.insert(clearness(f), true, m), Map.empty()) |> Map.keys in
  let groupOf = (slot: float): Model.group =>
    { first: 0.0, count: 0.0, texture: textureFor(slot),
      unlit: materialOf(slot) |> Option.map((m: Lgmd.material) => m.emissivity > 0.0) |> Option.defaultValue(false) } in
  let points = List.concatMap((f: Lgmd.face) => List.map((c: Lgmd.corner) => c.p, f.corners), s.faces) in
  let lo = [0.0, 1.0, 2.0] |> List.map((k) => List.minimum(List.map((p) => Affine.n(k, p), points)) |> Option.defaultValue(0.0)) in
  let hi = [0.0, 1.0, 2.0] |> List.map((k) => List.maximum(List.map((p) => Affine.n(k, p), points)) |> Option.defaultValue(0.0)) in
  { pieces: levels |> List.sortBy((t) => t) |> List.map((t) =>
      { model: staticOf(List.filter((f) => clearness(f) == t, drawn), groupOf), opacity: 1.0 - t }),
    center: Door.lerp(lo, hi, 0.5), size: World.sub(hi, lo) }

// The doors whose model is there, each model and texture decoded once.
let views = (l: Props.loaded): List<view> =>
  let nameOf = (id: float) => Objects.first(l.modelNames, List.append(Door.donorsOf(l.world, id), [id])) in
  let ids = Map.keys(l.world.door) in
  let surfaces = ids |> List.fold((m, id) => match nameOf(id) with
    | Option.Some(name) =>
      if Map.member(name, m) || not File.exists(modelPath(name)) then m
      else (match Lgmd.surface(File.bytes(modelPath(name))) with
        | Result.Ok(s) => Map.insert(name, s, m)
        | Result.Error(_) => m)
    | Option.None => m, Map.empty()) in
  let textures = Map.fold((m, _, s: Lgmd.surface) => List.fold((acc, mat: Lgmd.material) =>
      if mat.color || Map.member(mat.name, acc) then acc
      else match textureOf(mat.name) with
        | Option.Some(t) => Map.insert(mat.name, t, acc)
        | Option.None => acc, m, s.materials), Map.empty(), surfaces) in
  let looks = Map.fold((m, name, s) => Map.insert(name, lookOf(textures, s), m), Map.empty(), surfaces) in
  ids |> List.concatMap((id) =>
    nameOf(id) |> Option.bind((name) => Map.get(name, looks)) |> Option.toList
      |> List.map((k: look) => { id: id, look: k, heading: Map.get(id, l.headings) |> Option.defaultValue(0.0) }))

let scenePos = (p: List<float>): Vec3.t => Skinned.vec(Affine.scale(ft, p))

// A Dark rotation as the scene's: bank about X, pitch about -Z (Dark's Y),
// heading about Y (Dark's Z), heading last.
let turned = (r: Affine.t, rx, rz, ry, node) =>
  let a = Affine.angles(r) in
  node |> rx(Angle.radians(Affine.n(0.0, a))) |> rz(Angle.radians(0.0 - Affine.n(1.0, a))) |> ry(Angle.radians(Affine.n(2.0, a)))

let bodyAt = (v: view, p: Door.pose): Physics.body =>
  let side = (k: float) => ft * Math.max(Affine.n(k, v.look.size), minThickness) in
  Physics.kinematic(tagOf(v.id), Physics.box(side(0.0), side(2.0), side(1.0)))
    |> turned(p.rotation, Physics.rotateX, Physics.rotateZ, Physics.rotateY)
    |> Physics.at(scenePos(Affine.add(p.at, Affine.rotate(p.rotation, v.look.center))))

let nodeAt = (v: view, p: Door.pose): Scene.t =>
  Scene.group(v.look.pieces |> List.map((pc: piece) =>
      if pc.opacity < 1.0 then Scene.opacity(pc.opacity, Scene.modelOf(pc.model)) else Scene.modelOf(pc.model)))
    |> Scene.scale(ft) |> turned(p.rotation, Scene.rotateX, Scene.rotateZ, Scene.rotateY) |> Scene.translate(scenePos(p.at))

// A door's pose at progress `t` from these door tables: a rotating door's
// swing, else a sliding door between its ends at its heading.
let poseOf = (td: Map<float, Objects.Cell<Door.transDoor>>, rd: Map<float, Objects.Cell<Door.rotDoor>>, v: view, t: float): Option.t<Door.pose> =>
  match Objects.get(v.id, rd) with
  | Option.Some(r) => Option.Some(Door.swing(r, t))
  | Option.None => Objects.get(v.id, td) |> Option.map((d: Door.transDoor): Door.pose =>
    { at: Door.lerp(d.baseClosed, d.baseOpen, t), rotation: Affine.facing(0.0, 0.0, v.heading) })

// Doors at rest change only when one starts or stops, so they are built
// once into a memo group (`Physics.memo`, `Scene.memo`) keyed on this: the
// door table without the moving doors, and the doors' TransDoor and RotDoor
// cells. While nothing moves all are last frame's maps, so the check is free.
type still = { door: Map<float, Door.doorScript>, transDoor: Map<float, Objects.Cell<Door.transDoor>>,
               rotDoor: Map<float, Objects.Cell<Door.rotDoor>> }

let still = (w: Door.world): still =>
  { door: Map.fold((m, id, _) => Map.remove(id, m), w.door, w.moving), transDoor: w.props.transDoor, rotDoor: w.props.rotDoor }

let placed = (views: List<view>, s: still, make: (view, Door.pose) => 'a): List<'a> =>
  views |> List.concatMap((v: view) =>
    Map.get(v.id, s.door) |> Option.bind((d: Door.doorScript) => poseOf(s.transDoor, s.rotDoor, v, d.t))
      |> Option.toList |> List.map((p) => make(v, p)))

// The moving doors, declared each frame.
let moving = (views: List<view>, w: Door.world, make: (view, Door.pose) => 'a): List<'a> =>
  views |> List.filter((v: view) => Map.member(v.id, w.moving))
    |> List.concatMap((v: view) => poseOf(w.props.transDoor, w.props.rotDoor, v, Door.doorOf(w, v.id).t) |> Option.toList |> List.map((p) => make(v, p)))
