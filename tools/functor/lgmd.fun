// Lgmd — System Shock 2 static object meshes (".bin" files that start with
// "LGMD"), decoded in Functor: vertices, polygons and the sub-object tree,
// posed at rest. Dark's native axes are Z up, like a print, so positions are
// used as stored.
//
//   let part = File.bytes("ss2/model.bin") |> Lgmd.mesh |> Binary.require
//     |> Solid.fromMesh({ height: 60.0, nozzle: 0.4 })

type counts = { version: float, polygons: float, vertices: float, objects: float }
type header = { counts: counts, objectsAt: float, verticesAt: float, polygonsAt: float }

// A 3×4 affine transform, rows x, y, z, each with its translation last.
type affine = {
  a: float, b: float, c: float, x: float,
  d: float, e: float, f: float, y: float,
  g: float, h: float, i: float, z: float
}

// A sub-object: its transform relative to its parent, its first child and
// next sibling (-1 for none), and the vertices it owns.
type part = { local: affine, child: float, next: float, start: float, count: float }

type model = { header: header, positions: List<List<float>>, polygons: List<List<float>>, parts: List<part> }

let identity: affine = {
  a: 1.0, b: 0.0, c: 0.0, x: 0.0,
  d: 0.0, e: 1.0, f: 0.0, y: 0.0,
  g: 0.0, h: 0.0, i: 1.0, z: 0.0
}

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

type polygonPrefix = { kind: float, count: float }

// One polygon's vertex indices (each below `vertices`), skipping its normals,
// UVs and trailer.
let polygon = (version: float, vertices: float): Decode.t<List<float>> =>
  Decode.map2(
    (kind, count): polygonPrefix => { kind: kind, count: count },
    Decode.skipThen(4.0, Decode.u8),
    Decode.u8)
    |> Decode.andThen((p: polygonPrefix) =>
      Decode.skipThen(6.0, Decode.field("vertices", Decode.list(p.count, Decode.u16le)))
        |> Decode.andThen((ids: List<float>) =>
          let uvs = if Math.mod(p.kind, 4.0) == 3.0 then 2.0 * p.count else 0.0 in
          let tail = if version >= 4.0 then 1.0 else 0.0 in
          if List.any((i) => i >= vertices, ids)
          then Decode.fail("a polygon uses a vertex past the end of the vertex list")
          else Decode.skipThen(2.0 * p.count + uvs + tail, Decode.succeed(ids))))

// Dark stores rotation columns, then the translation; all zeros = identity.
let affineOf = (v: List<float>): affine =>
  let n = (k: float) => Option.defaultValue(0.0, List.nth(k, v)) in
  if List.all((x) => x == 0.0, v)
  then identity
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
            { header: h, positions: positions, parts: parts,
              polygons: List.filter((ids) => List.length(ids) >= 3.0, polygons) },
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

// `outer` after `inner`.
let compose = (outer: affine, inner: affine): affine => {
  a: outer.a * inner.a + outer.b * inner.d + outer.c * inner.g,
  b: outer.a * inner.b + outer.b * inner.e + outer.c * inner.h,
  c: outer.a * inner.c + outer.b * inner.f + outer.c * inner.i,
  x: outer.a * inner.x + outer.b * inner.y + outer.c * inner.z + outer.x,
  d: outer.d * inner.a + outer.e * inner.d + outer.f * inner.g,
  e: outer.d * inner.b + outer.e * inner.e + outer.f * inner.h,
  f: outer.d * inner.c + outer.e * inner.f + outer.f * inner.i,
  y: outer.d * inner.x + outer.e * inner.y + outer.f * inner.z + outer.y,
  g: outer.g * inner.a + outer.h * inner.d + outer.i * inner.g,
  h: outer.g * inner.b + outer.h * inner.e + outer.i * inner.h,
  i: outer.g * inner.c + outer.h * inner.f + outer.i * inner.i,
  z: outer.g * inner.x + outer.h * inner.y + outer.i * inner.z + outer.z
}

let rows = (m: affine): List<float> => [m.a, m.b, m.c, m.x, m.d, m.e, m.f, m.y, m.g, m.h, m.i, m.z]

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
let worlds = (parts: List<part>): List<affine> =>
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
    | (_, chain) => List.fold((m, local) => compose(m, local), identity, chain), parts)

let apply = (m: affine, v: List<float>): List<float> =>
  let n = (k: float) => Option.defaultValue(0.0, List.nth(k, v)) in
  [ m.a * n(0.0) + m.b * n(1.0) + m.c * n(2.0) + m.x,
    m.d * n(0.0) + m.e * n(1.0) + m.f * n(2.0) + m.y,
    m.g * n(0.0) + m.h * n(1.0) + m.i * n(2.0) + m.z ]

// The model's triangles, each vertex moved to its rest pose first (by the
// first part that owns it, else the root, as the game does) — corners that
// only separate once posed must not be merged as degenerate.
let pose = (m: model): Mesh.t =>
  let all = worlds(m.parts) in
  let poses = List.zip(all, m.parts) in
  let root = Option.defaultValue(identity, List.nth(0.0, all)) in
  let posed =
    List.indexedMap((i, v: List<float>) =>
      match List.find((pair) => match pair with | (p, w) => i >= p.start && i < p.start + p.count, poses) with
      | Option.Some(pair) => (match pair with | (_, w) => apply(w, v))
      | Option.None => apply(root, v), m.positions) in
  Mesh.fromPolygons(Bytes.fromF32le(List.flatten(posed)), m.polygons)

// An LGMD file's mesh at rest, or where decoding failed.
let mesh = (data: Bytes.t): Result.t<Mesh.t, Binary.error> =>
  Decode.run(model, data) |> Result.map(pose)
