// Mis — System Shock 2 mission files (`.mis`, Dark chunk files), decoded in
// Functor: the chunk table, the world rep (`WRRGB`: cells, polygons,
// texturing, RGB555 lightmaps), the texture list (`TXLIST`), the ambient
// level (`RENDPARAMS`) and the player start (`L$PlayerFac` → `P$Position`).
// Positions stay in Dark's axes and feet; `World` turns them into a scene.
//
//   let m = File.bytes("ss2data/medsci1.mis")
//   let cells = Mis.cells(m) |> Binary.require

// A chunk: where its data starts (past the 24-byte header) and how long it is.
type chunk = { name: string, offset: float, length: float }

// A NUL-padded name of `size` bytes.
let paddedName = (size: float): Decode.t<string> =>
  Decode.bytes(size) |> Decode.andThen((b: Bytes.t) =>
    let used = List.range(size) |> List.fold((n, i) =>
      if n < 0.0 && Bytes.u8(i, b) == Result.Ok(0.0) then i else n, -1.0) in
    Decode.succeed(Bytes.latin1(0.0, if used < 0.0 then size else used, b) |> Binary.require))

let entry =
  Decode.map3((n, o, l): chunk => { name: n, offset: o + 24.0, length: l },
    paddedName(12.0), Decode.u32le, Decode.u32le)

// The table of contents sits where the first u32 says.
let toc =
  Decode.at(0.0, Decode.u32le)
    |> Decode.andThen((at) => Decode.at(at, Decode.u32le |> Decode.andThen((n) => Decode.list(n, entry))))

let find = (name: string, chunks: List<chunk>): Result.t<chunk, Binary.error> =>
  match List.find((c: chunk) => c.name == name, chunks) with
  | Option.Some(c) => Result.Ok(c)
  | Option.None => Result.Error(Decode.failure(0.0, Text.join("", ["no ", name, " chunk"])))

let vec3 = Decode.map3((x, y, z) => [x, y, z], Decode.f32le, Decode.f32le, Decode.f32le)

// A polygon: its vertex count and, for a portal, the cell it leads to.
type polygon = { count: float, target: float }
let polygon =
  Decode.map2((count, target): polygon => { count: count, target: target },
    Decode.skipThen(1.0, Decode.u8), Decode.skipThen(2.0, Decode.u16le))
    |> Decode.andThen((p: polygon) => Decode.skipThen(2.0, Decode.succeed(p)))

// How a rendered polygon is textured (the 48-byte WRRGB record): texture
// axes, the texture's offset in 1/4096ths of a tile, its TXLIST index and the
// polygon vertex (an index into its own vertex list) the axes start at.
type texturing = { axisU: List<float>, axisV: List<float>, u: float, v: float, texture: float, origin: float }
let texturing =
  Decode.map5((au, av, uv, tex, origin): texturing =>
      { axisU: au, axisV: av, u: Option.defaultValue(0.0, List.nth(0.0, uv)),
        v: Option.defaultValue(0.0, List.nth(1.0, uv)), texture: tex, origin: origin },
    vec3, vec3, Decode.list(2.0, Decode.u16le), Decode.u8, Decode.u8)
    |> Decode.andThen((t: texturing) => Decode.skipThen(18.0, Decode.succeed(t)))

// A rendered polygon's lightmap: its texel offset, size, and which animated
// lights add layers (one per set bit) after the base.
type lightmap = { u: float, v: float, width: float, height: float, layers: float }
let lightmapInfo =
  Decode.map5((u, v, w, h, layers): lightmap => { u: u, v: v, width: w, height: h, layers: layers },
    Decode.i16le, Decode.i16le, Decode.u16le, Decode.u8, Decode.skipThen(9.0, Decode.u32le))

// One cell. `lightmaps` holds each rendered polygon's base lightmap (RGB555,
// row 0 first); animated layers are skipped.
type cell = { vertices: List<List<float>>, polygons: List<polygon>, texturing: List<texturing>,
              indices: List<List<float>>, lights: List<lightmap>, lightmaps: List<Bytes.t> }

let n = (k: float, xs: List<float>): float => Option.defaultValue(0.0, List.nth(k, xs))

let cell: Decode.t<cell> =
  Decode.list(7.0, Decode.u8) |> Decode.andThen((h) =>
  Decode.skipThen(6.0, Decode.u8) |> Decode.andThen((animated) =>
  Decode.skipThen(17.0, Decode.list(n(0.0, h), vec3)) |> Decode.andThen((vertices) =>
  Decode.list(n(1.0, h), polygon) |> Decode.andThen((polygons) =>
  Decode.list(n(2.0, h), texturing) |> Decode.andThen((tex) =>
  Decode.skipThen(4.0, Decode.sequence(List.map((p: polygon) => Decode.list(p.count, Decode.u8), polygons))) |> Decode.andThen((ids) =>
  Decode.skipThen(16.0 * n(4.0, h) + 2.0 * animated, Decode.list(n(2.0, h), lightmapInfo)) |> Decode.andThen((lights) =>
  Decode.sequence(List.map((l: lightmap) =>
      let size = 2.0 * l.width * l.height in
      Decode.bytes(size) |> Decode.andThen((base) => Decode.skipThen(size * Bits.popcount(l.layers), Decode.succeed(base))),
    lights)) |> Decode.andThen((maps) =>
  Decode.u32le |> Decode.andThen((count) =>
  Decode.skipThen(2.0 * count, Decode.succeed(
    { vertices: vertices, polygons: polygons, texturing: tex, indices: ids, lights: lights, lightmaps: maps })))))))))))

// WRRGB: a u32, the cell count, then the cells.
let cellsIn = (chunks: List<chunk>, data: Bytes.t): Result.t<List<cell>, Binary.error> =>
  find("WRRGB", chunks) |> Result.bind((wr: chunk) =>
    Decode.run(Decode.at(wr.offset + 4.0, Decode.u32le |> Decode.andThen((count) => Decode.list(count, cell))), data))

// A TXLIST texture: its family folder under `fam/` and its name.
type texture = { family: string, name: string }

let textures = (chunks: List<chunk>, data: Bytes.t): Result.t<List<texture>, Binary.error> =>
  find("TXLIST", chunks) |> Result.bind((c: chunk) =>
    Decode.run(Decode.at(c.offset + 4.0,
      Decode.map2((count, families) => (count, families), Decode.u32le, Decode.u32le)
        |> Decode.andThen((counts) => match counts with
          | (count, families) =>
            Decode.list(families, paddedName(16.0)) |> Decode.andThen((names) =>
              Decode.list(count, Decode.map2((f, name): texture =>
                  { family: if f > 0.0 then Option.defaultValue("", List.nth(f - 1.0, names)) else "", name: name },
                Decode.skipThen(1.0, Decode.u8), Decode.skipThen(2.0, paddedName(16.0))))))), data))

// RENDPARAMS: the mission's ambient light (a minimum surface brightness).
let ambient = (chunks: List<chunk>, data: Bytes.t): Result.t<float, Binary.error> =>
  find("RENDPARAMS", chunks) |> Result.bind((c: chunk) => Bytes.f32le(c.offset + 16.0, data))

// A start: the player factory's position (Dark feet) and heading (radians,
// counter-clockwise about Dark's +Z from +X).
type start = { position: List<float>, heading: float }

// Every object's position record (`P$Position`: id, size, then x y z, cell,
// and bank/pitch/heading as u16 angles).
let positions = (chunks: List<chunk>, data: Bytes.t): Result.t<Map<float, start>, Binary.error> =>
  find("P$Position", chunks) |> Result.bind((c: chunk) =>
    Decode.run(Decode.at(c.offset, Decode.list(c.length / 30.0,
      Decode.map3((id, p, heading) => (id, { position: p, heading: heading * 2.0 * Math.pi / 65536.0 }),
        Decode.skipThen(0.0, Decode.i32le), Decode.skipThen(4.0, vec3), Decode.skipThen(8.0, Decode.u16le)))), data)
      |> Result.map(Map.fromList))

// Where the player starts: the last `L$PlayerFac` link's source, as
// shock2quest picks it (medsci1 has four factories, for multiplayer).
let start = (chunks: List<chunk>, data: Bytes.t): Result.t<start, Binary.error> =>
  find("L$PlayerFac", chunks) |> Result.bind((c: chunk) =>
    Decode.run(Decode.at(c.offset, Decode.list(c.length / 14.0,
      Decode.skipThen(4.0, Decode.i32le) |> Decode.andThen((src) => Decode.skipThen(6.0, Decode.succeed(src))))), data)
      |> Result.bind((sources) =>
        positions(chunks, data) |> Result.bind((ps) =>
          match Option.bind((src) => Map.get(src, ps), List.last(sources)) with
          | Option.Some(s) => Result.Ok(s)
          | Option.None => Result.Error(Decode.failure(c.offset, "no player factory position")))))
