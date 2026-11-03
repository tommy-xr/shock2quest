// World — a mission's world rep as one textured, lightmapped `Model.static`
// and one `Physics.trimesh`, in scene space: Dark's (x, y, z) feet become
// (x, z, -y) metres (Y up, as `Skinned` turns characters). Polygons are
// fan-triangulated and grouped by texture (one draw each); every rendered
// polygon's base lightmap goes into one atlas. The sky (texture 249) is left
// out, as shock2quest leaves it out, and so is water (medsci1 has none).
//
//   let level = World.load(File.bytes(Path.of("ss2data/medsci1.mis"))) |> Binary.require

let metresPerFoot = 0.3048
// Textures 247 and 248 are water surfaces and 249 the sky: none is in the
// texture list, and none is drawn or collides here.
let water = 247.0

let n = Affine.n
let dot = (a: List<float>, b: List<float>): float => n(0.0, a) * n(0.0, b) + n(1.0, a) * n(1.0, b) + n(2.0, a) * n(2.0, b)
let sub = (a: List<float>, b: List<float>): List<float> => [n(0.0, a) - n(0.0, b), n(1.0, a) - n(1.0, b), n(2.0, a) - n(2.0, b)]
// `v` at length 1 (a zero vector stays zero).
let unit = (v: List<float>): List<float> => Affine.scale(1.0 / Math.max(Math.sqrt(dot(v, v)), 0.000001), v)
let cross = (a: List<float>, b: List<float>): List<float> =>
  [n(1.0, a) * n(2.0, b) - n(2.0, a) * n(1.0, b), n(2.0, a) * n(0.0, b) - n(0.0, a) * n(2.0, b),
   n(0.0, a) * n(1.0, b) - n(1.0, a) * n(0.0, b)]

// A base texture: what the texturing math needs (its size) and what draws.
type surface = { width: float, height: float, texture: Option.t<Asset.Texture> }

// No texture: drawn white, textured as if 64 texels square.
let blank: surface = { width: 64.0, height: 64.0, texture: Option.None }

// `ss2/fam/<family>/<name>.pcx`, matched ignoring case (see `files.fun`).
let texturePath = (t: Mis.texture): Path.t => Path.of(Text.join("", ["ss2/fam/", t.family, "/", t.name, ".pcx"]))

let surfaceOf = (t: Mis.texture): surface =>
  let path = texturePath(t) in
  if t.name == "null" || not File.exists(path) then blank
  else
    match Texture.decode(File.bytes(path), Texture.Srgb) with
    | Result.Ok(image) => { width: image.width, height: image.height, texture: Option.Some(Texture.fromPixels(image)) }
    | Result.Error(_) => blank

// One rendered polygon, with what its vertices need.
type face = { cell: Mis.cell, polygon: float, texturing: Mis.texturing, light: Mis.lightmap, lightmap: Bytes.t }

let facesOf = (cells: List<Mis.cell>): List<face> =>
  cells
    |> List.concatMap((c: Mis.cell) =>
      List.zip(c.lights, c.texturing) |> List.indexedMap((i, pair) => match pair with
        | (t, l) => { cell: c, polygon: i, texturing: t, light: l,
                      lightmap: Option.defaultValue(Bytes.zeros(2.0), List.nth(i, c.lightmaps)) }))

// A face's vertices as plain floats: scene position, normal, texture uv and
// atlas uv per vertex, in its polygon's order.
type built = { texture: float, count: float, positions: List<float>, normals: List<float>, uvs: List<float>, uvs2: List<float> }

// SS2's texturing (as shock2quest computes it): a vertex's offset from the
// origin projected on the texture axes (correcting for skewed axes), in
// 64-texel tiles for the texture and 16 texels per tile for the lightmap.
// Lightmap uvs are clamped into the polygon's atlas rect. Vector components
// are read once per face or corner (list access dominates the load).
let build = (surfaces: List<surface>, f: face, rect: Texture.rect): built =>
  let t = f.texturing in
  let ids = Option.defaultValue([], List.nth(f.polygon, f.cell.indices)) in
  let vertex = (id: float) => Option.defaultValue([0.0, 0.0, 0.0], List.nth(id, f.cell.vertices)) in
  let origin = vertex(n(t.origin, ids)) in
  let ox = n(0.0, origin) in
  let oy = n(1.0, origin) in
  let oz = n(2.0, origin) in
  let ux = n(0.0, t.axisU) in
  let uy = n(1.0, t.axisU) in
  let uz = n(2.0, t.axisU) in
  let vx = n(0.0, t.axisV) in
  let vy = n(1.0, t.axisV) in
  let vz = n(2.0, t.axisV) in
  let size = Option.defaultValue(blank, List.nth(t.texture, surfaces)) in
  let mu = ux * ux + uy * uy + uz * uz in
  let mv = vx * vx + vy * vy + vz * vz in
  let uv = ux * vx + uy * vy + uz * vz in
  let corr = 1.0 / (mu * mv - uv * uv) in
  let square = Math.abs(uv) < 0.0001 in
  let normal = Skinned.up(cross(t.axisV, t.axisU)) in
  let len = Math.sqrt(dot(normal, normal)) in
  let unitNormal = List.map((x) => x / Math.max(len, 0.000001), normal) in
  let tileU = size.width / 64.0 in
  let tileV = size.height / 64.0 in
  let rectU = rect.u1 - rect.u0 in
  let rectV = rect.v1 - rect.v0 in
  let corners = ids |> List.map((id) =>
    let p = vertex(id) in
    let px = n(0.0, p) in
    let py = n(1.0, p) in
    let pz = n(2.0, p) in
    let dx = px - ox in
    let dy = py - oy in
    let dz = pz - oz in
    let au = ux * dx + uy * dy + uz * dz in
    let av = vx * dx + vy * dy + vz * dz in
    let pu = if square then au / mu else corr * (au * mv - av * uv) in
    let pv = if square then av / mv else corr * (av * mu - au * uv) in
    let lu = (4.0 * pu + 0.5 - f.light.u + t.u / 1024.0) / f.light.width in
    let lv = (4.0 * pv + 0.5 - f.light.v + t.v / 1024.0) / f.light.height in
    { p: [metresPerFoot * px, metresPerFoot * pz, metresPerFoot * (0.0 - py)],
      uv: [(pu + t.u / 4096.0) / tileU, (pv + t.v / 4096.0) / tileV],
      uv2: [rect.u0 + Math.clamp01(lu) * rectU, rect.v0 + Math.clamp01(lv) * rectV] }) in
  { texture: t.texture, count: List.length(ids),
    positions: List.concatMap((c) => c.p, corners),
    normals: List.concatMap((_) => unitNormal, corners),
    uvs: List.concatMap((c) => c.uv, corners),
    uvs2: List.concatMap((c) => c.uv2, corners) }

type stats = { cells: float, vertices: float, renderPolygons: float, lightmapBytes: float, triangles: float, textures: float }

type level = { model: Model.t, shape: Physics.shape, start: Mis.start, ambient: float, stats: stats }

let load = (data: Bytes.t): Result.t<level, Binary.error> =>
  Decode.run(Mis.toc, data) |> Result.bind((chunks) =>
  Mis.cellsIn(chunks, data) |> Result.bind((cells) =>
  Mis.textures(chunks, data) |> Result.bind((textures) =>
  Mis.start(chunks, data) |> Result.bind((start) =>
  Mis.ambient(chunks, data) |> Result.map((ambient) =>
    let faces = facesOf(cells) in
    let used = faces |> List.fold((m, f: face) => Map.insert(f.texturing.texture, true, m), Map.empty()) in
    let surfaces = textures |> List.indexedMap((i, t) =>
      if Map.member(i, used) then surfaceOf(t) else blank) in
    let atlas = Texture.atlas({
      images: faces |> List.map((f: face) =>
        { pixels: { width: f.light.width, height: f.light.height, format: Texture.Rgb555le, colorSpace: Texture.Srgb,
                    pixels: f.lightmap }, layers: [] }),
      padding: 1.0, maxSize: 4096.0 }) in
    let drawn = faces |> List.zip(atlas.rects)
      |> List.filter((pair) => match pair with | (f, _) => f.texturing.texture < water)
      |> List.sortBy((pair) => match pair with | (f, _) => f.texturing.texture) in
    let builts = drawn |> List.map((pair) => match pair with | (f, r) => build(surfaces, f, r)) in
    // Fan triangles, counter-clockwise from inside the cell (Dark winds its
    // polygons the other way).
    let triangles = builts
      |> List.tryMapAccum((first, b: built) =>
           Result.Ok((first + b.count, List.range(b.count - 2.0) |> List.concatMap((k) => [first, first + k + 2.0, first + k + 1.0]))),
         0.0)
      |> Result.map((acc) => match acc with | (_, ts) => List.flatten(ts))
      |> Result.defaultValue([]) in
    // One group per run of a texture.
    let groups = builts
      |> List.fold((acc, b: built) =>
           let tris = b.count - 2.0 in
           match acc with
           | (gs, last, first, count) =>
             if b.texture == last then (gs, last, first, count + tris)
             else (if count > 0.0 then List.append([(last, first, count)], gs) else gs, b.texture, first + count, tris),
         ([], -1.0, 0.0, 0.0))
      |> ((acc) => match acc with | (gs, last, first, count) => List.append([(last, first, count)], gs))
      |> List.map((g) => match g with
        | (tex, first, count) =>
          { first: first, count: count, unlit: false, emissive: 0.0,
            texture: Option.bind((s: surface) => s.texture, List.nth(tex, surfaces)) }) in
    let positions = Bytes.fromF32le(List.concatMap((b: built) => b.positions, builts)) in
    let tris = Bytes.fromU32le(triangles) in
    { model: Model.static({
        positions: positions,
        normals: Bytes.fromF32le(List.concatMap((b: built) => b.normals, builts)),
        uvs: Bytes.fromF32le(List.concatMap((b: built) => b.uvs, builts)),
        uvs2: Bytes.fromF32le(List.concatMap((b: built) => b.uvs2, builts)),
        triangles: tris, groups: groups,
        lightmap: Option.Some({ texture: atlas.texture, scale: 1.0, floor: Color.rgb(ambient, ambient, ambient) }) }),
      shape: Physics.trimesh({ positions: positions, triangles: tris }),
      start: start, ambient: ambient,
      stats: { cells: List.length(cells),
               vertices: List.fold((a, c: Mis.cell) => a + List.length(c.vertices), 0.0, cells),
               renderPolygons: List.length(faces),
               lightmapBytes: List.fold((a, c: Mis.cell) => a + List.fold((s, l: Mis.lightmap) => s + 2.0 * l.width * l.height * (Bits.popcount(l.layers) + 1.0), 0.0, c.lights), 0.0, cells),
               triangles: List.length(triangles) / 3.0,
               textures: Map.size(used) } })))))

// A 20 m floor at y = 0 with its start on it, for when the mission isn't
// there (unmapped, or on a headset still fetching it).
let placeholder: level =
  let positions = Bytes.fromF32le([-10.0, 0.0, -10.0, -10.0, 0.0, 10.0, 10.0, 0.0, 10.0, 10.0, 0.0, -10.0]) in
  let triangles = Bytes.fromU32le([0.0, 1.0, 2.0, 0.0, 2.0, 3.0]) in
  { model: Model.static({ positions: positions, normals: Bytes.fromF32le(List.flatten(List.map((i) => [0.0, 1.0, 0.0], List.range(4.0)))),
                          uvs: Bytes.zeros(0.0), uvs2: Bytes.zeros(0.0), triangles: triangles, groups: [], lightmap: Option.None }),
    shape: Physics.trimesh({ positions: positions, triangles: triangles }),
    start: { position: [0.0, 0.0, 1.0 / metresPerFoot], heading: 0.0 }, ambient: 1.0,
    stats: { cells: 0.0, vertices: 0.0, renderPolygons: 0.0, lightmapBytes: 0.0, triangles: 2.0, textures: 0.0 } }
