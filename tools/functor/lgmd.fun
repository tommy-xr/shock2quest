// Lgmd — System Shock 2 static object meshes (".bin" files that start with
// "LGMD"), decoded in Functor: vertices, polygons and the sub-object tree,
// posed at rest. Dark's native axes are Z up, like a print, so positions are
// used as stored.
//
//   let part = "ss2/obj/CRATE01.BIN" |> Path.of |> File.bytes |> Lgmd.mesh |> Binary.require
//     |> Solid.fromMesh({ height: 60.0, nozzle: 0.4 })

type counts = { version: float, polygons: float, vertices: float, objects: float }
type header = { counts: counts, objectsAt: float, verticesAt: float, polygonsAt: float }

// A sub-object: its transform relative to its parent, its first child and
// next sibling (-1 for none), and the vertices it owns.
type part = { local: Affine.t, child: float, next: float, start: float, count: float }

// `polygons`: each surface's vertex indices; `faces`: every polygon record.
type model = { header: header, positions: List<List<float>>, polygons: List<List<float>>, faces: List<polygon>, parts: List<part> }

let at8 = (offset: float) => Decode.at(offset, Decode.u8)
let at16 = (offset: float) => Decode.at(offset, Decode.u16le)
let at32 = (offset: float) => Decode.at(offset, Decode.u32le)

let counts =
  Decode.map4(
    (version, polygons, vertices, objects): counts =>
      { version: version, polygons: polygons, vertices: vertices, objects: objects },
    Decode.field("version", at32(4.0)),
    Decode.field("polygons", at16(60.0)),
    Decode.field("vertices", at16(62.0)),
    Decode.field("objects", at8(69.0)))

let header =
  Decode.field("magic", Decode.latin1(4.0))
    |> Decode.andThen((magic: string) =>
      if magic != "LGMD"
      then Decode.fail(Text.join("", ["not an LGMD object mesh (starts with \"", magic, "\")"]))
      else
        Decode.map4(
          (c, objectsAt, verticesAt, polygonsAt): header =>
            { counts: c, objectsAt: objectsAt, verticesAt: verticesAt, polygonsAt: polygonsAt },
          counts,
          Decode.field("objectsAt", at32(70.0)),
          Decode.field("verticesAt", at32(86.0)),
          Decode.field("polygonsAt", at32(98.0))))

// A polygon record: its material slot, then per corner its vertex (each
// below `vertices`), lighting-normal and (when texture-mapped) uv indices.
type polygon = { slot: float, vertices: List<float>, normals: List<float>, uvs: List<float> }

let polygon = (version: float, vertices: float): Decode.t<polygon> =>
  Decode.map3((slot, kind, count) => (slot, kind, count), Decode.skipThen(2.0, Decode.u16le), Decode.u8, Decode.u8)
    |> Decode.andThen((h) => match h with
      | (slot, kind, count) =>
        Decode.map3((vs, ns, uvs): polygon => { slot: slot, vertices: vs, normals: ns, uvs: uvs },
          Decode.skipThen(6.0, Decode.field("vertices", Decode.list(count, Decode.u16le))), Decode.list(count, Decode.u16le),
          Decode.list(if Math.mod(kind, 4.0) == 3.0 then count else 0.0, Decode.u16le))
        |> Decode.andThen((p: polygon) =>
          if List.any((i) => i >= vertices, p.vertices)
          then Decode.fail("a polygon uses a vertex past the end of the vertex list")
          else Decode.skipThen(if version >= 4.0 then 1.0 else 0.0, Decode.succeed(p))))

// Dark stores rotation columns, then the translation; all zeros = identity.
let affineOf = (v: List<float>): Affine.t =>
  let n = (k: float) => Option.defaultValue(0.0, List.nth(k, v)) in
  if List.all((x) => x == 0.0, v)
  then Affine.identity
  else {
    a: n(0.0), b: n(3.0), c: n(6.0), x: n(9.0),
    d: n(1.0), e: n(4.0), f: n(7.0), y: n(10.0),
    g: n(2.0), h: n(5.0), i: n(8.0), z: n(11.0)
  }

// One 93-byte sub-object record.
let part =
  Decode.map5(
    (m, child, next, start, count): part =>
      { local: affineOf(m), child: child, next: next, start: start, count: count },
    Decode.skipThen(21.0, Decode.field("transform", Decode.list(12.0, Decode.f32le))),
    Decode.field("child", Decode.i16le),
    Decode.field("next", Decode.i16le),
    Decode.skipThen(4.0, Decode.field("pointStart", Decode.u16le)),
    Decode.field("pointCount", Decode.u16le))
    |> Decode.andThen((p: part) => Decode.skipThen(12.0, Decode.succeed(p)))

let model =
  Decode.field("header", header)
    |> Decode.andThen((h: header) =>
      if h.counts.version < 3.0 || h.counts.version > 6.0
      then Decode.fail(Text.join("", ["LGMD version ", Text.fromFloat(h.counts.version), " isn't supported (3 to 6 are)"]))
      else
        Decode.map3(
          (positions, polygons, parts): model =>
            // Points and lines (under 3 corners) have no surface.
            { header: h, positions: positions, parts: parts, faces: polygons,
              polygons: polygons |> List.map((p: polygon) => p.vertices) |> List.filter((ids) => List.length(ids) >= 3.0) },
          Decode.field("vertices",
            Decode.at(h.verticesAt, Decode.list(h.counts.vertices, Decode.list(3.0, Decode.f32le)))),
          Decode.field("polygons",
            Decode.at(h.polygonsAt,
              Decode.list(h.counts.polygons, polygon(h.counts.version, h.counts.vertices)))),
          Decode.field("parts",
            Decode.at(h.objectsAt, Decode.list(h.counts.objects, part))
              |> Decode.andThen((parts: List<part>) =>
                if List.any((p: part) => p.start + p.count > h.counts.vertices, parts)
                then Decode.fail("a part owns vertices past the end of the vertex list")
                else Decode.succeed(parts)))))

let partAt = (parts: List<part>, k: float): Option.t<part> =>
  if k < 0.0 then Option.None else List.nth(k, parts)

// The sub-objects `first` and its `next` siblings (bounded, so a cycle stops).
let siblings = (parts: List<part>, first: float): List<float> =>
  let walked =
    List.range(List.length(parts))
      |> List.fold((walk, i) =>
        match walk with
        | (at, found) =>
          match partAt(parts, at) with
          | Option.Some(p) =>
            if List.any((k) => k == at, found) then (-1.0, found) else (p.next, List.append([at], found))
          | Option.None => walk, (first, [])) in
  match walked with
  | (_, found) => found

// Each sub-object's parent index, -1 for roots.
let parents = (parts: List<part>): List<float> =>
  let children = List.map((q: part) => siblings(parts, q.child), parts) in
  List.indexedMap((j, p: part) =>
    List.indexedMap((i, kids) => (i, List.any((k) => k == j, kids)), children)
      |> List.fold((found, pair) =>
        match pair with
        | (i, isChild) => if isChild && found < 0.0 then i else found, -1.0), parts)

// Each sub-object's rest pose in the model: its ancestors' transforms from the
// root down, then its own.
let worlds = (parts: List<part>): List<Affine.t> =>
  let up = parents(parts) in
  List.indexedMap((j, p: part) =>
    let walked =
      List.range(List.length(parts))
        |> List.fold((walk, step) =>
          match walk with
          | (at, chain) =>
            match partAt(parts, at) with
            | Option.Some(q) =>
              (Option.defaultValue(-1.0, List.nth(at, up)), List.append(chain, [q.local]))
            | Option.None => walk, (j, [])) in
    match walked with
    | (_, chain) => List.fold((m, local) => Affine.compose(m, local), Affine.identity, chain), parts)

// Each vertex's rest transform: that of the first part that owns it, else
// the root's, as the game does.
let placements = (m: model): List<Affine.t> =>
  let all = worlds(m.parts) in
  let poses = List.zip(all, m.parts) in
  let root = Option.defaultValue(Affine.identity, List.nth(0.0, all)) in
  List.indexedMap((i, _) =>
    match List.find((pair) => match pair with | (p, w) => i >= p.start && i < p.start + p.count, poses) with
    | Option.Some(pair) => (match pair with | (_, w) => w)
    | Option.None => root, m.positions)

// The model's triangles, each vertex moved to its rest pose first — corners
// that only separate once posed must not be merged as degenerate.
let pose = (m: model): Mesh.t =>
  let posed = List.zip(placements(m), m.positions) |> List.map((pair) => match pair with | (v, w) => Affine.apply(w, v)) in
  Mesh.fromPolygons(Bytes.fromF32le(List.flatten(posed)), m.polygons)

// An LGMD file's mesh at rest, or where decoding failed.
let mesh = (data: Bytes.t): Result.t<Mesh.t, Binary.error> =>
  Decode.run(model, data) |> Result.map(pose)

// --- Surfaces: materials, uvs and normals, as drawn ---------------------------

// A material: its texture's file name (in `obj/txt16/`), the slot polygons
// name it by, and from the extended records how see-through (0 opaque) and
// self-lit it is. `color` materials carry a colour instead of a texture.
type material = { name: string, slot: float, color: bool, transparency: float, emissivity: float }

// A polygon corner at rest: position and normal (Dark axes, feet) and uv.
type corner = { p: List<float>, n: List<float>, uv: List<float> }

// A polygon (3+ corners) and its material slot; `mapped` when it has uvs.
type face = { slot: float, mapped: bool, corners: List<corner> }

type surface = { faces: List<face>, materials: List<material> }

type surfaceAt = { materials: float, materialsAt: float, uvsAt: float, vhotsAt: float, lightsAt: float, normalsAt: float,
                   extraAt: float, extraSize: float }

let surfaceAt =
  Decode.map4((count, mats, uvs, vhots): surfaceAt =>
      { materials: count, materialsAt: mats, uvsAt: uvs, vhotsAt: vhots, lightsAt: 0.0, normalsAt: 0.0, extraAt: 0.0, extraSize: 0.0 },
    at8(66.0), at32(74.0), at32(78.0), at32(82.0))
    |> Decode.andThen((s: surfaceAt) => Decode.map4((lights, normals, extraAt, extraSize): surfaceAt =>
         { s with lightsAt: lights, normalsAt: normals, extraAt: extraAt, extraSize: extraSize },
         at32(90.0), at32(94.0), at32(114.0), at32(118.0)))

// A 26-byte material record: name, type (1 = colour), slot.
let materialRecord =
  Decode.map3((name, kind, slot): material => { name: name, slot: slot, color: kind == 1.0, transparency: 0.0, emissivity: 0.0 },
    Motion.name(16.0), Decode.u8, Decode.u8)
    |> Decode.andThen((m: material) => Decode.skipThen(8.0, Decode.succeed(m)))

// Each material's transparency and emissivity, `size` bytes apart (v4+).
// A transparency of exactly 1 means opaque (re-exported models write it so).
let extras = (at: float, size: float, ms: List<material>): Decode.t<List<material>> =>
  Decode.at(at, Decode.list(List.length(ms), Decode.map2((t, e) => (t, e), Decode.f32le, Decode.f32le)
    |> Decode.andThen((te) => Decode.skipThen(size - 8.0, Decode.succeed(te)))))
    |> Decode.map((tes) => List.zip(tes, ms) |> List.map((pair) => match pair with
      | (m, te) => (match te with | (t, e) => { m with transparency: if t == 1.0 then 0.0 else t, emissivity: e })))

// An LGMD file's polygons as drawn, with their materials: corners posed at
// rest, normals (the per-corner lighting normals) turned with them, as
// shock2quest builds an object's vertices. A uv or normal index past its
// table fails.
let surface = (data: Bytes.t): Result.t<surface, Binary.error> =>
  Decode.run(model, data) |> Result.bind((m: model) =>
  Decode.run(surfaceAt, data) |> Result.bind((s: surfaceAt) =>
    let mats = Decode.at(s.materialsAt, Decode.list(s.materials, materialRecord))
      |> Decode.andThen((ms) => if m.header.counts.version > 3.0 && s.extraSize >= 8.0 then extras(s.extraAt, s.extraSize, ms) else Decode.succeed(ms)) in
    let uvs = Decode.at(s.uvsAt, Decode.list(Math.floor((s.vhotsAt - s.uvsAt) / 8.0), Decode.list(2.0, Decode.f32le))) in
    let normals = Decode.at(s.lightsAt, Decode.list(Math.floor((s.normalsAt - s.lightsAt) / 8.0),
      Decode.skipThen(4.0, Decode.u32le) |> Decode.map(Skinned.normal))) in
    Decode.run(Decode.map3((ms, us, ns) => (ms, us, ns), Decode.field("materials", mats), Decode.field("uvs", uvs),
                           Decode.field("normals", normals)), data)
      |> Result.bind((r) => match r with
        | (ms, us, ns) =>
          let at = placements(m) in
          let corner = (f: polygon, k: float): Option.t<corner> =>
            let v = Option.defaultValue(0.0, List.nth(k, f.vertices)) in
            let w = Option.defaultValue(Affine.identity, List.nth(v, at)) in
            List.nth(v, m.positions) |> Option.bind((p) =>
              Option.bind((i) => List.nth(i, ns), List.nth(k, f.normals)) |> Option.map((nrm): corner =>
                { p: Affine.apply(w, p), n: Affine.rotate(w, nrm),
                  uv: Option.defaultValue([0.0, 0.0], Option.bind((u) => List.nth(u, us), List.nth(k, f.uvs))) })) in
          m.faces |> List.filter((f: polygon) => List.length(f.vertices) >= 3.0)
            |> List.tryMapAccum((u, f: polygon) =>
                 let cs = List.range(List.length(f.vertices)) |> List.map((k) => corner(f, k)) in
                 if List.any(Option.isNone, cs) || List.any((u) => u >= List.length(us), f.uvs)
                 then Result.Error(Decode.failure(m.header.polygonsAt, "a polygon names a vertex, normal or uv past its table"))
                 else Result.Ok((u, { slot: f.slot, mapped: List.length(f.uvs) >= List.length(f.vertices),
                                      corners: List.concatMap(Option.toList, cs) })), 0.0)
            |> Result.map((acc) => match acc with | (_, faces) => { faces: faces, materials: ms }))))
