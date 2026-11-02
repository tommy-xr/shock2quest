// Mis — System Shock 2 mission files (`.mis`, Dark chunk files), decoded in
// Functor: the chunk table, the world rep (`WRRGB`: cells, polygons,
// texturing, RGB555 lightmaps), the texture list (`TXLIST`), the ambient
// level (`RENDPARAMS`) and the player start (`L$PlayerFac` → `P$Position`).
// Positions stay in Dark's axes and feet; `World` turns them into a scene.
//
//   let m = "ss2data/medsci1.mis" |> Path.of |> File.bytes
//   let cells = Mis.cells(m) |> Binary.require

// A chunk: where its data starts (past the 24-byte header) and how long it is.
type chunk = { name: string, offset: float, length: float }

let entry =
  Decode.map3((n, o, l): chunk => { name: n, offset: o + 24.0, length: l },
    Motion.name(12.0), Decode.u32le, Decode.u32le)

// The table of contents sits where the first u32 says.
let toc =
  Decode.at(0.0, Decode.u32le)
    |> Decode.andThen((at) => Decode.at(at, Decode.u32le |> Decode.andThen((n) => Decode.list(n, entry))))

let find = (name: string, chunks: List<chunk>): Result.t<chunk, Binary.error> =>
  match List.find((c: chunk) => c.name == name, chunks) with
  | Option.Some(c) => Result.Ok(c)
  | Option.None => Result.Error(Decode.failure(0.0, Text.join("", ["no ", name, " chunk"])))

let vec3 = Cal.vec3

// A polygon: its vertex count and, for a portal, the cell it leads to.
type polygon = { count: float, target: float }

// How a rendered polygon is textured (the 48-byte WRRGB record): texture
// axes, the texture's offset in 1/4096ths of a tile, its TXLIST index and the
// polygon vertex (an index into its own vertex list) the axes start at.
type texturing = { axisU: List<float>, axisV: List<float>, u: float, v: float, texture: float, origin: float }

// A rendered polygon's lightmap: its texel offset, size, and which animated
// lights add layers (one per set bit) after the base.
type lightmap = { u: float, v: float, width: float, height: float, layers: float }

// One cell. `lightmaps` holds each rendered polygon's base lightmap (RGB555,
// row 0 first); animated layers are skipped.
type cell = { vertices: List<List<float>>, polygons: List<polygon>, texturing: List<texturing>,
              indices: List<List<float>>, lights: List<lightmap>, lightmaps: List<Bytes.t> }

// Cells are read with direct `Bytes` reads, about 3× cheaper per number
// than `Decode` combinators (medsci1 has ~350k). Each section's extent is
// checked against the data before it is read, so a read can't fail inside.
let u8At = (at: float, data: Bytes.t): float => Bytes.u8(at, data) |> Result.defaultValue(0.0)
let u16At = (at: float, data: Bytes.t): float => Bytes.u16le(at, data) |> Result.defaultValue(0.0)
let i16At = (at: float, data: Bytes.t): float => Bytes.i16le(at, data) |> Result.defaultValue(0.0)
let u32At = (at: float, data: Bytes.t): float => Bytes.u32le(at, data) |> Result.defaultValue(0.0)
let f32At = (at: float, data: Bytes.t): float => Bytes.f32le(at, data) |> Result.defaultValue(0.0)
let vec3At = (at: float, data: Bytes.t): List<float> => [f32At(at, data), f32At(at + 4.0, data), f32At(at + 8.0, data)]

// `count` records `size` bytes apart from `at`, each read by `read`.
let table = (at: float, count: float, size: float, read: (float) => 'a): List<'a> =>
  List.range(count) |> List.map((i) => read(at + size * i))

// `next(at)` when `size` bytes from `at` are inside the data, else an error.
let within = (at: float, size: float, what: string, data: Bytes.t, next: (float) => Result.t<'a, Binary.error>): Result.t<'a, Binary.error> =>
  if at + size > Bytes.length(data) then Result.Error(Decode.failure(at, Text.join("", ["a cell's ", what, " run past the end of the data"])))
  else next(at)

// One cell from `at`: the cell and where the next one starts.
let cellAt = (at: float, data: Bytes.t): Result.t<(cell, float), Binary.error> =>
  within(at, 31.0, "header", data, (_) =>
    let nv = u8At(at, data) in
    let np = u8At(at + 1.0, data) in
    let nr = u8At(at + 2.0, data) in
    let npl = u8At(at + 4.0, data) in
    let animated = u8At(at + 13.0, data) in
    let vAt = at + 31.0 in
    let pAt = vAt + 12.0 * nv in
    let tAt = pAt + 8.0 * np in
    let iAt = tAt + 48.0 * nr + 4.0 in
    within(vAt, 12.0 * nv + 8.0 * np + 48.0 * nr + 4.0, "vertices, polygons and texturing", data, (_) =>
      let polygons = table(pAt, np, 8.0, (o): polygon => { count: u8At(o + 1.0, data), target: u16At(o + 4.0, data) }) in
      let counts = List.map((p: polygon) => p.count, polygons) in
      let total = List.sum(counts) in
      within(iAt, total, "polygon indices", data, (_) =>
        let starts = List.fold((acc, c) => match acc with | (next, ss) => (next + c, List.append([next], ss)), (iAt, []), counts) in
        let indices = match starts with
          | (_, ss) => List.zip(counts, ss) |> List.map((pair) => match pair with | (s, c) => table(s, c, 1.0, (o) => u8At(o, data))) in
        let lAt = iAt + total + 16.0 * npl + 2.0 * animated in
        within(lAt, 20.0 * nr, "lightmap records", data, (_) =>
          let lights = table(lAt, nr, 20.0, (o): lightmap =>
            { u: i16At(o, data), v: i16At(o + 2.0, data), width: u16At(o + 4.0, data), height: u8At(o + 6.0, data), layers: u32At(o + 16.0, data) }) in
          let mAt = lAt + 20.0 * nr in
          let sizes = List.map((l: lightmap) => 2.0 * l.width * l.height * (Bits.popcount(l.layers) + 1.0), lights) in
          within(mAt, List.sum(sizes) + 4.0, "lightmaps", data, (_) =>
            let maps = List.fold((acc, pair) => match acc with
                | (o, ms) => (match pair with
                  | (l, size) => (o + size, List.append([Bytes.slice(o, 2.0 * l.width * l.height, data) |> Result.defaultValue(Bytes.zeros(0.0))], ms))),
              (mAt, []), List.zip(sizes, lights)) in
            let cAt = mAt + List.sum(sizes) in
            let skip = 2.0 * u32At(cAt, data) in
            within(cAt + 4.0, skip, "trailer", data, (_) =>
              Result.Ok(({ vertices: table(vAt, nv, 12.0, (o) => vec3At(o, data)), polygons: polygons,
                           texturing: table(tAt, nr, 48.0, (o): texturing =>
                             { axisU: vec3At(o, data), axisV: vec3At(o + 12.0, data), u: u16At(o + 24.0, data), v: u16At(o + 26.0, data),
                               texture: u8At(o + 28.0, data), origin: u8At(o + 29.0, data) }),
                           indices: indices, lights: lights, lightmaps: match maps with | (_, ms) => ms },
                         cAt + 4.0 + skip))))))))

type walk = { at: float, left: float, cells: List<cell>, error: Option.t<Binary.error> }

// WRRGB: a u32, the cell count, then the cells.
let cellsIn = (chunks: List<chunk>, data: Bytes.t): Result.t<List<cell>, Binary.error> =>
  find("WRRGB", chunks) |> Result.bind((wr: chunk) =>
    Bytes.u32le(wr.offset + 4.0, data) |> Result.bind((count) =>
      let w = Loop.until((w: walk) => w.left <= 0.0, (w: walk) =>
          match cellAt(w.at, data) with
          | Result.Ok(r) => (match r with | (c, next) => { w with at: next, left: w.left - 1.0, cells: List.append([c], w.cells) })
          | Result.Error(e) => { w with left: 0.0, error: Option.Some(e) },
        { at: wr.offset + 8.0, left: count, cells: [], error: Option.None }) in
      match w.error with
      | Option.Some(e) => Result.Error(e)
      | Option.None => Result.Ok(w.cells)))

// A TXLIST texture: its family folder under `fam/` and its name.
type texture = { family: string, name: string }

let textures = (chunks: List<chunk>, data: Bytes.t): Result.t<List<texture>, Binary.error> =>
  find("TXLIST", chunks) |> Result.bind((c: chunk) =>
    Decode.run(Decode.at(c.offset + 4.0,
      Decode.map2((count, families) => (count, families), Decode.u32le, Decode.u32le)
        |> Decode.andThen((counts) => match counts with
          | (count, families) =>
            Decode.list(families, Motion.name(16.0)) |> Decode.andThen((names) =>
              Decode.list(count, Decode.map2((f, name): texture =>
                  { family: if f > 0.0 then Option.defaultValue("", List.nth(f - 1.0, names)) else "", name: name },
                Decode.skipThen(1.0, Decode.u8), Decode.skipThen(2.0, Motion.name(16.0))))))), data))

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

// A link chunk's links (14-byte records: id, source, destination, then a
// flavor this skips); none when the file has no such chunk.
type link = { id: float, src: float, dst: float }

let links = (name: string, chunks: List<chunk>, data: Bytes.t): Result.t<List<link>, Binary.error> =>
  match find(name, chunks) with
  | Result.Error(_) => Result.Ok([])
  | Result.Ok(c) =>
    Decode.run(Decode.at(c.offset, Decode.list(c.length / 14.0,
      Decode.map3((id, src, dst): link => { id: id, src: src, dst: dst }, Decode.i32le, Decode.i32le, Decode.i32le)
        |> Decode.andThen((l) => Decode.skipThen(2.0, Decode.succeed(l))))), data)

// Where the player starts: the last `L$PlayerFac` link's source, as
// shock2quest picks it (medsci1 has four factories, for multiplayer).
let start = (chunks: List<chunk>, data: Bytes.t): Result.t<start, Binary.error> =>
  links("L$PlayerFac", chunks, data) |> Result.bind((factories) =>
    positions(chunks, data) |> Result.bind((ps) =>
      match Option.bind((l: link) => Map.get(l.src, ps), List.last(factories)) with
      | Option.Some(s) => Result.Ok(s)
      | Option.None => Result.Error(Decode.failure(0.0, "no player factory position"))))
