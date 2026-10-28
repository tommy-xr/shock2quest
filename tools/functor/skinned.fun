// Skinned — a character (an `Lgmm` rig and `Motion` clips) as `Model.skinned`
// data, so the GPU skins it and `Anim` plays, blends and masks its clips.
// Scene axes (Y up), feet. Joints are named `joint<id>` by SS2 joint id.
//
// A blending ("stretchy") vertex on joint j mixes j with a helper bone: a
// never-animated child of j's parent at j's rest offset, so it turns with the
// parent but pivots at j — the blend `Lgmm.poseRig` computes.

// Native (Z up) to scene (Y up): (x, y, z) → (x, z, -y), as Mesh.toScene turns it.
let up = (p: List<float>): List<float> => [Affine.n(0.0, p), Affine.n(2.0, p), 0.0 - Affine.n(1.0, p)]

let vec = (p: List<float>): Vec3.t => let u = up(p) in Vec3.make(Affine.n(0.0, u), Affine.n(1.0, u), Affine.n(2.0, u))

// A packed normal: three signed 10-bit fields (bits 31-22, 21-12, 11-2), / 256.
let s10 = (f: float): float => if f >= 512.0 then f - 1024.0 else f

let normal = (u: float): List<float> =>
  [s10(Math.floor(u / 4194304.0)) / 256.0,
   s10(Math.mod(Math.floor(u / 4096.0), 1024.0)) / 256.0,
   s10(Math.mod(Math.floor(u / 4.0), 1024.0)) / 256.0]

// One `item` per vertex from its 12-byte record (offset at byte 60): f32 u,
// f32 v, packed normal.
let records = (what: string, item) =>
  Decode.map2((count, at) => (count, at), Lgmm.at16(26.0), Lgmm.at32(60.0))
    |> Decode.andThen((header) =>
      match header with
      | (count, at) => Decode.field(what, Decode.at(at, Decode.list(count, item))))

let normals = records("normals", Decode.skipThen(8.0, Decode.u32le) |> Decode.map(normal))

// Each vertex's (u, v) as stored: the game samples them as is, and so does
// Functor (0, 0 is the image's top-left in both), so no flip.
let uvs = records("uvs", Decode.list(2.0, Decode.f32le) |> Decode.andThen((uv) => Decode.skipThen(4.0, Decode.succeed(uv))))

// The mesh's texture, ss2/mesh/txt16/ + its lowercased name (e.g.
// ss2/mesh/txt16/og_pipe02.gif), else none: the model draws white.
let texture = (mesh: Bytes.t): Option.t<Asset.Texture> =>
  match Decode.run(Lgmm.textureName, mesh) with
  | Result.Ok(n) =>
    let path = Text.join("", ["ss2/mesh/txt16/", Text.toLower(n)]) in
    if File.exists(path) then Option.Some(Asset.texture(path)) else Option.None
  | Result.Error(_) => Option.None

let name = (j: float): string => Text.join("", ["joint", Text.fromFloat(j)])

let indexOf = (x: float, xs: List<float>): Option.t<float> =>
  List.indexedMap((k, y) => (k, y), xs)
    |> List.find((p) => match p with | (_, y) => y == x)
    |> Option.map((p) => match p with | (k, _) => k)

// The track driving joint `j`: track k drives `joints[k + 1]`; later tracks win.
let trackOf = (c: Motion.clip, j: float): Option.t<List<List<float>>> =>
  List.zip(c.tracks, List.drop(1.0, c.motion.joints))
    |> List.fold((found, pair) => match pair with | (joint, t) => if joint == j then Option.Some(t) else found, Option.None)

// A stored (w, x, y, z) key as the scene's (x, y, z, w) rotation: its conjugate
// (the rotation the game applies), turned into Y up. Bones rest unrotated, so
// this is also rest × key.
let key = (q0: List<float>): List<float> =>
  let q = Motion.unit(q0) in
  [0.0 - Affine.n(1.0, q), 0.0 - Affine.n(3.0, q), Affine.n(2.0, q), Affine.n(0.0, q)]

// The model data for a rig, its mesh's bytes (for uvs and normals) and named
// clips. Untextured: set `texture` (see `texture`) to sample one.
let data = (mesh: Bytes.t, r: Lgmm.rig, clips: List<(string, Motion.clip)>): Result.t<Model.data, Binary.error> =>
  Decode.run(Decode.map2((ns, ts) => (ns, ts), normals, uvs), mesh)
    |> Result.map((vertexData) =>
      match vertexData with
      | (ns, ts) =>
      let chainOf = (j: float) => Option.defaultValue([], List.nth(j, r.chains)) in
      // SS2 joints with a bone, parents first.
      let ids = List.range(40.0) |> List.filter((j) => List.length(chainOf(j)) > 0.0) |> List.sortBy((j) => List.length(chainOf(j))) in
      // A vertex on a joint without a bone, or a blend whose parent has none,
      // follows palette joint 0 / rigidly here (Lgmm uses the root bob); the
      // shipped characters have neither.
      let slot = (j: float) => Option.defaultValue(0.0, indexOf(j, ids)) in
      let parentSlot = (j: float) =>
        let p = Option.defaultValue(j, List.nth(j, r.parents)) in
        if p == j then Option.None else indexOf(p, ids) in
      // A root joint's blend is rigid (its "parent rotation" is its own).
      let helpers =
        List.filter((j) => Option.isSome(parentSlot(j))
          && List.any((b: Lgmm.binding) => b.joint == j && Option.isSome(b.blend), r.bindings), ids) in
      let helperSlot = (j: float) => List.length(ids) + Option.defaultValue(0.0, indexOf(j, helpers)) in
      let rest = Cal.globals(r.chains, Affine.identity, (j) => Affine.identity) in
      let restOf = (j: float) => Option.defaultValue(Affine.identity, List.nth(j, rest)) in
      // Cal bones only translate.
      let offsetOf = (j: float) =>
        Option.defaultValue([0.0, 0.0, 0.0], List.last(chainOf(j)) |> Option.map((b: Cal.bone) => Affine.offset(b.local))) in
      let identity = { x: 0.0, y: 0.0, z: 0.0, w: 1.0 } in
      let bone = (label: string, j: float) =>
        { name: label, parent: parentSlot(j), translation: vec(offsetOf(j)), rotation: identity } in
      let blended = (b: Lgmm.binding) => List.any((j) => j == b.joint, helpers) && Option.isSome(b.blend) in
      let w = (b: Lgmm.binding) => Option.defaultValue(1.0, b.blend) in
      let clipOf = (named) =>
        match named with
        | (clipName, c) =>
          let frames = List.range(c.motion.frames) in
          let rootZ = (f: float) => Affine.n(2.0, Option.defaultValue([0.0, 0.0, 0.0], List.nth(f, c.root))) in
          { name: clipName, fps: c.motion.fps, frames: c.motion.frames,
            tracks:
              ids
                |> List.filter((j) => Option.isSome(trackOf(c, j)) || Option.isNone(parentSlot(j)))
                |> List.map((j) =>
                  { joint: name(j),
                    rotations:
                      Bytes.fromF32le(match trackOf(c, j) with
                        | Option.Some(t) => List.concatMap((f) => key(Option.defaultValue([0.0, 0.0, 0.0, 0.0], List.nth(f, t))), frames)
                        | Option.None => []),
                    // The root's translation key is its rest offset plus the clip's vertical bob.
                    translations:
                      Bytes.fromF32le(if Option.isSome(parentSlot(j)) then []
                        else List.concatMap((f) => up(Affine.add(offsetOf(j), [0.0, 0.0, rootZ(f)])), frames)) }) } in
      let bound = List.zip(r.model.positions, r.bindings) in
      {
        positions: Bytes.fromF32le(List.concatMap((p) => match p with | (b, v) => up(Affine.apply(restOf(b.joint), v)), bound)),
        // Bind normals are the stored ones: rest poses don't rotate.
        normals: Bytes.fromF32le(List.concatMap(up, ns)),
        uvs: Bytes.fromF32le(List.flatten(ts)),
        joints: Bytes.fromU8(List.concatMap((b: Lgmm.binding) =>
          if blended(b) then [slot(b.joint), helperSlot(b.joint), slot(b.joint), slot(b.joint)]
          else [slot(b.joint), slot(b.joint), slot(b.joint), slot(b.joint)], r.bindings)),
        weights: Bytes.fromF32le(List.concatMap((b: Lgmm.binding) =>
          if blended(b) then [w(b), 1.0 - w(b), 0.0, 0.0] else [1.0, 0.0, 0.0, 0.0], r.bindings)),
        triangles: Bytes.fromU32le(List.flatten(r.model.triangles)),
        skeleton: List.append(
          List.map((j) => bone(Text.join("", [name(j), ".blend"]), j), helpers),
          List.map((j) => bone(name(j), j), ids)),
        clips: List.map(clipOf, clips),
        texture: Option.None
      })

// The largest coordinate difference between `m` posed by `anim` and `points`
// (native axes, the mesh's vertex order) — to check it against `Lgmm.points`.
let deviation = (m: Model.t, anim: Anim.t, points: List<List<float>>): float =>
  let expected = List.concatMap(up, points) in
  let posed = Model.pose(anim, m) in
  match Decode.run(Decode.list(List.length(expected), Decode.f32le), posed) with
  | Result.Ok(actual) =>
    if Bytes.length(posed) != 4.0 * List.length(expected) then 1000000.0
    else List.zip(actual, expected) |> List.map((p) => match p with | (a, b) => Math.abs(a - b)) |> List.maximum |> Option.defaultValue(0.0)
  | Result.Error(_) => 1000000.0
