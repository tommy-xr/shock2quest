// A synthetic mission and PCX, written with `Encode`, to test `Mis`, `World`
// and `Pcx` without game files: one cell holding a 1 ft square floor (one
// render polygon, a 2×2 lightmap), a texture list, the ambient level and one
// player factory.

let chunkHeader = 24.0

let named = (size: float, name: string) => Encode.sequence([Encode.latin1(name), Encode.zeros(size - Text.length(name))])

// A chunk file: each chunk's 24-byte header (unread) and data, then the table
// of contents the first u32 points at.
let chunkFile = (chunks: List<(string, Bytes.t)>) =>
  let placed = chunks |> List.tryMapAccum((at, c) => match c with
    | (name, data) => Result.Ok((at + chunkHeader + Bytes.length(data), (name, at, data))), 4.0)
    |> Result.defaultValue((0.0, [])) in
  match placed with
  | (tocAt, entries) =>
    Encode.run(Encode.sequence(List.flatten([
      [Encode.u32le(tocAt)],
      entries |> List.concatMap((e) => match e with | (_, _, data) => [Encode.zeros(chunkHeader), Encode.bytes(data)]),
      [Encode.u32le(List.length(entries))],
      entries |> List.concatMap((e) => match e with
        | (name, at, data) => [named(12.0, name), Encode.u32le(at), Encode.u32le(Bytes.length(data))])])))

// A 1 ft square at z = 0 (Dark feet), texture axes 4 units a foot.
let cell = Encode.sequence([
  Encode.bytes(Bytes.fromU8([4.0, 1.0, 1.0, 0.0, 1.0, 1.0, 0.0])),     // verts, polys, rendered, portals, planes, medium, flags
  Encode.zeros(6.0), Encode.u8(0.0), Encode.u8(0.0), Encode.zeros(16.0), // nxn, map size, animated lights, flow, centre, radius
  Encode.bytes(Bytes.fromF32le([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0])),
  Encode.bytes(Bytes.fromU8([0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])),  // polygon: 4 vertices
  Encode.bytes(Bytes.fromF32le([4.0, 0.0, 0.0, 0.0, 4.0, 0.0])),         // texture axes
  Encode.u16le(0.0), Encode.u16le(0.0), Encode.u8(1.0), Encode.u8(0.0), Encode.zeros(18.0),
  Encode.u32le(4.0), Encode.bytes(Bytes.fromU8([0.0, 1.0, 2.0, 3.0])),
  Encode.zeros(16.0),                                                      // plane
  Encode.i16le(0.0), Encode.i16le(0.0), Encode.u16le(2.0), Encode.u8(2.0), Encode.zeros(9.0), Encode.u32le(0.0),
  Encode.bytes(Bytes.fromU16le([31.0, 992.0, 31744.0, 32767.0])),         // red, green, blue, white
  Encode.u32le(1.0), Encode.u16le(0.0)
])

let mission = chunkFile([
  ("WRRGB", Encode.run(Encode.sequence([Encode.u32le(0.0), Encode.u32le(1.0), cell]))),
  ("TXLIST", Encode.run(Encode.sequence([
    Encode.u32le(0.0), Encode.u32le(2.0), Encode.u32le(1.0), named(16.0, "testfam"),
    Encode.bytes(Bytes.fromU8([1.0, 0.0, 0.0, 0.0])), named(16.0, "null"),
    Encode.bytes(Bytes.fromU8([1.0, 1.0, 0.0, 0.0])), named(16.0, "wall")]))),
  ("RENDPARAMS", Encode.run(Encode.sequence([named(16.0, "pal"), Encode.f32le(0.25)]))),
  ("L$PlayerFac", Encode.run(Encode.sequence([Encode.i32le(1.0), Encode.i32le(7.0), Encode.i32le(-1.0), Encode.u16le(0.0)]))),
  ("P$Position", Encode.run(Encode.sequence([
    Encode.i32le(7.0), Encode.u32le(22.0), Encode.bytes(Bytes.fromF32le([2.0, 2.0, 3.0])),
    Encode.u16le(0.0), Encode.i16le(0.0), Encode.bytes(Bytes.fromU16le([0.0, 0.0, 16384.0]))])))
])

let near = (a: List<float>, b: List<float>): bool =>
  List.length(a) == List.length(b)
    && List.all((p) => match p with | (x, y) => Math.abs(x - y) < 0.0001, List.zip(b, a))

let chunks = Decode.run(Mis.toc, mission) |> Binary.require
let cells = Mis.cellsIn(chunks, mission) |> Binary.require

expect List.map((c: Mis.chunk) => c.name, chunks) == ["WRRGB", "TXLIST", "RENDPARAMS", "L$PlayerFac", "P$Position"]
expect Mis.textures(chunks, mission) == Result.Ok([{ family: "", name: "null" }, { family: "testfam", name: "wall" }])
expect Mis.ambient(chunks, mission) == Result.Ok(0.25)
expect Mis.start(chunks, mission) == Result.Ok({ position: [2.0, 2.0, 3.0], heading: Math.pi / 2.0 })
expect List.length(cells) == 1.0
expect (match List.head(cells) with
  | Option.Some(c) => c.indices == [[0.0, 1.0, 2.0, 3.0]] && c.lights == [{ u: 0.0, v: 0.0, width: 2.0, height: 2.0, layers: 0.0 }]
                      && List.map(Bytes.length, c.lightmaps) == [8.0]
  | Option.None => false)

expect (match World.load(mission) with
  | Result.Ok(level) => level.stats == { cells: 1.0, vertices: 4.0, renderPolygons: 1.0, lightmapBytes: 8.0, triangles: 2.0, textures: 1.0 }
  | Result.Error(_) => false)

// The square's vertices: scene positions (x, z, -y) in metres, texture uvs in
// 64-texel tiles (a 64-texel stand-in: the texture file isn't there), and
// lightmap uvs on texel centres, mapped into the atlas rect (here its lower right quarter).
let built = World.build([World.blank, World.blank],
  Option.defaultValue({ cell: Option.defaultValue({ vertices: [], polygons: [], texturing: [], indices: [], lights: [], lightmaps: [] }, List.head(cells)),
                        polygon: 0.0, texturing: { axisU: [], axisV: [], u: 0.0, v: 0.0, texture: 0.0, origin: 0.0 },
                        light: { u: 0.0, v: 0.0, width: 1.0, height: 1.0, layers: 0.0 }, lightmap: Bytes.zeros(2.0) },
    List.head(World.facesOf(cells))),
  { x: 0.0, y: 0.0, u0: 0.5, v0: 0.5, u1: 1.0, v1: 1.0 })
expect near(built.positions, [0.0, 0.0, 0.0, 0.3048, 0.0, 0.0, 0.3048, 0.0, -0.3048, 0.0, 0.0, -0.3048])
expect near(built.uvs, [0.0, 0.0, 0.25, 0.0, 0.25, 0.25, 0.0, 0.25])
expect near(built.uvs2, [0.625, 0.625, 0.875, 0.625, 0.875, 0.875, 0.625, 0.875])
// The normal is -(axisU × axisV), as shock2quest takes it: these axes give -Y.
expect near(built.normals, [0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0])

// Skewed axes, the origin at the polygon's second vertex, and texture and
// lightmap offsets: the general case of the texturing math (values from
// shock2quest's formulas).
let skewed = World.build([World.blank, { width: 128.0, height: 64.0, texture: Option.None }],
  { cell: { vertices: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]], polygons: [],
            texturing: [], indices: [[0.0, 1.0, 2.0, 3.0]], lights: [], lightmaps: [] },
    polygon: 0.0, texturing: { axisU: [4.0, 0.0, 0.0], axisV: [2.0, 4.0, 0.0], u: 1024.0, v: 2048.0, texture: 1.0, origin: 1.0 },
    light: { u: -2.0, v: 2.0, width: 4.0, height: 3.0, layers: 0.0 }, lightmap: Bytes.zeros(24.0) },
  { x: 0.0, y: 0.0, u0: 0.0, v0: 0.0, u1: 1.0, v1: 1.0 })
expect near(skewed.uvs, [0.0, 0.5, 0.125, 0.5, 0.0625, 0.75, -0.0625, 0.75])
expect near(skewed.uvs2, [0.625, 0.166667, 0.875, 0.166667, 0.75, 0.5, 0.5, 0.5])

// A 3×2 PCX: row 0 is a run of three index-1 pixels (0xC3 0x01), row 1 is
// 0, 2, 0xC1 (an index ≥ 0xC0 needs a run of one); 4-byte rows.
let palette = List.range(256.0) |> List.concatMap((i) => [i, 255.0 - i, 7.0])
let pcx = Encode.run(Encode.sequence([
  Encode.bytes(Bytes.fromU8([10.0, 5.0, 1.0, 8.0])), Encode.bytes(Bytes.fromU16le([0.0, 0.0, 2.0, 1.0])),
  Encode.zeros(53.0), Encode.u8(1.0), Encode.u16le(4.0), Encode.zeros(60.0),
  Encode.bytes(Bytes.fromU8([195.0, 1.0, 0.0, 0.0, 2.0, 193.0, 193.0, 0.0])),
  Encode.u8(12.0), Encode.bytes(Bytes.fromU8(palette))]))
expect (Pcx.decode(pcx) |> Result.map((img) => (img.width, img.height, Bytes.equals(img.pixels,
  Bytes.fromU8([1.0, 254.0, 7.0, 1.0, 254.0, 7.0, 1.0, 254.0, 7.0, 0.0, 255.0, 7.0, 2.0, 253.0, 7.0, 193.0, 62.0, 7.0]))))) == Result.Ok((3.0, 2.0, true))
// Cut short, it is an error (not too few pixels).
expect Result.isError(Pcx.decode(Encode.run(Encode.sequence([Encode.bytes(Bytes.slice(0.0, 130.0, pcx) |> Binary.require),
  Encode.u8(12.0), Encode.bytes(Bytes.fromU8(palette))]))))
