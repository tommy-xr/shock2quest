// World — a mission's world rep as one textured, lightmapped `Model.static`
// and one `Physics.trimesh`, in scene space: Dark's (x, y, z) feet become
// (x, z, -y) metres (Y up, as `Skinned` turns characters). Polygons are
// fan-triangulated and grouped by texture (one draw each); every rendered
// polygon's base lightmap goes into one atlas. The sky (texture 249) is left
// out, as shock2quest leaves it out.
//
//   let level = World.load(File.bytes("ss2data/medsci1.mis")) |> Binary.require

let metresPerFoot = 0.3048
let sky = 249.0

let n = Mis.n
let dot = (a: List<float>, b: List<float>): float => n(0.0, a) * n(0.0, b) + n(1.0, a) * n(1.0, b) + n(2.0, a) * n(2.0, b)
let sub = (a: List<float>, b: List<float>): List<float> => [n(0.0, a) - n(0.0, b), n(1.0, a) - n(1.0, b), n(2.0, a) - n(2.0, b)]
let cross = (a: List<float>, b: List<float>): List<float> =>
  [n(1.0, a) * n(2.0, b) - n(2.0, a) * n(1.0, b), n(2.0, a) * n(0.0, b) - n(0.0, a) * n(2.0, b),
   n(0.0, a) * n(1.0, b) - n(1.0, a) * n(0.0, b)]

// Dark axes to scene axes; `scale` is metres per foot for points, 1 for directions.
let toScene = (scale: float, p: List<float>): List<float> => [n(0.0, p) * scale, n(2.0, p) * scale, 0.0 - n(1.0, p) * scale]

// A base texture: what the texturing math needs (its size) and what draws.
type surface = { width: float, height: float, texture: Option.t<Asset.Texture> }

// `ss2/fam/<family>/<name>.pcx`, matched ignoring case (see `files.fun`).
let texturePath = (t: Mis.texture): string => Text.join("", ["ss2/fam/", t.family, "/", t.name, ".pcx"])

let surfaceOf = (t: Mis.texture): surface =>
  let path = texturePath(t) in
  if t.name == "null" || not File.exists(path) then { width: 64.0, height: 64.0, texture: Option.None }
  else
    match Pcx.decode(File.bytes(path)) with
    | Result.Ok(image) =>
      { width: image.width, height: image.height,
        texture: Option.Some(Texture.fromPixels({ width: image.width, height: image.height, format: Texture.Rgb8,
                                                  colorSpace: Texture.Srgb, pixels: image.pixels })) }
    | Result.Error(_) => { width: 64.0, height: 64.0, texture: Option.None }

// One rendered polygon, with what its vertices need.
type face = { index: float, cell: Mis.cell, polygon: float, texturing: Mis.texturing, light: Mis.lightmap, lightmap: Bytes.t }

let facesOf = (cells: List<Mis.cell>): List<face> =>
  cells
    |> List.concatMap((c: Mis.cell) =>
      List.zip(c.lights, c.texturing) |> List.indexedMap((i, pair) => match pair with
        | (t, l) => { index: 0.0, cell: c, polygon: i, texturing: t, light: l,
                      lightmap: Option.defaultValue(Bytes.zeros(2.0), List.nth(i, c.lightmaps)) }))
    |> List.indexedMap((i, f: face) => { f with index: i })

// A face's vertices as plain floats: scene position, normal, texture uv and
// atlas uv per vertex, in its polygon's order.
type built = { texture: float, count: float, positions: List<float>, normals: List<float>, uvs: List<float>, uvs2: List<float> }

// SS2's texturing (as shock2quest computes it): a vertex's offset from the
// origin projected on the texture axes (correcting for skewed axes), in
// 64-texel tiles for the texture and 16 texels per tile for the lightmap.
// Lightmap uvs are clamped into the polygon's atlas rect.
let build = (surfaces: List<surface>, f: face, rect: Texture.rect): built =>
  let t = f.texturing in
  let ids = Option.defaultValue([], List.nth(f.polygon, f.cell.indices)) in
  let vertex = (k: float) => Option.defaultValue([0.0, 0.0, 0.0], List.nth(n(k, ids), f.cell.vertices)) in
  let origin = vertex(t.origin) in
  let size = Option.defaultValue({ width: 64.0, height: 64.0, texture: Option.None }, List.nth(t.texture, surfaces)) in
  let mu = dot(t.axisU, t.axisU) in
  let mv = dot(t.axisV, t.axisV) in
  let uv = dot(t.axisU, t.axisV) in
  let corr = 1.0 / (mu * mv - uv * uv) in
  let project = (d: List<float>) =>
    let pu = dot(t.axisU, d) in
    let pv = dot(t.axisV, d) in
    if Math.abs(uv) < 0.0001 then (pu / mu, pv / mv)
    else (corr * (pu * mv - pv * uv), corr * (pv * mu - pu * uv)) in
  let normal = toScene(1.0, cross(t.axisV, t.axisU)) in
  let len = Math.sqrt(dot(normal, normal)) in
  let unitNormal = List.map((x) => x / Math.max(len, 0.000001), normal) in
  let corners = ids |> List.indexedMap((k, _) =>
    let p = vertex(k) in
    match project(sub(p, origin)) with
    | (pu, pv) =>
      let lu = (4.0 * pu + 0.5 - f.light.u + t.u / 1024.0) / f.light.width in
      let lv = (4.0 * pv + 0.5 - f.light.v + t.v / 1024.0) / f.light.height in
      { p: toScene(metresPerFoot, p),
        uv: [(pu + t.u / 4096.0) / (size.width / 64.0), (pv + t.v / 4096.0) / (size.height / 64.0)],
        uv2: [rect.u0 + Math.clamp01(lu) * (rect.u1 - rect.u0), rect.v0 + Math.clamp01(lv) * (rect.v1 - rect.v0)] }) in
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
      if Map.member(i, used) then surfaceOf(t) else { width: 64.0, height: 64.0, texture: Option.None }) in
    let atlas = Texture.atlas({
      images: faces |> List.map((f: face) =>
        { pixels: { width: f.light.width, height: f.light.height, format: Texture.Rgb555le, colorSpace: Texture.Srgb,
                    pixels: f.lightmap }, layers: [] }),
      padding: 1.0, maxSize: 4096.0 }) in
    let drawn = faces |> List.zip(atlas.rects)
      |> List.filter((pair) => match pair with | (f, _) => f.texturing.texture != sky)
      |> List.sortBy((pair) => match pair with | (f, _) => f.texturing.texture * 100000.0 + f.index) in
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
          { first: first, count: count, unlit: false,
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
