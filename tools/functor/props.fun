// Props — a mission's objects and its gamesys's archetypes as a
// `Door.world`: the donor graph from `L$MetaProp` (priorities from
// `LD$MetaProp`), the door slice's property records (`P$TransDoor`,
// `P$RotDoor`, `P$Locked`, `P$FrobInfo`, `P$Scripts`) and SwitchLinks, then
// every mission object spawned (copied down).
//
//   let w = Props.load(File.bytes(Path.of("ss2data/shock2.gam")), File.bytes(Path.of("ss2data/medsci1.mis")))
//     |> Binary.require

// One record of a property chunk: the object, and where its data is.
type record = { id: float, at: float, length: float }

// A property chunk's records (object id, size, data), in file order; none
// when the file has no such chunk.
let records = (name: string, chunks: List<Mis.chunk>, data: Bytes.t): List<record> =>
  match Mis.find(name, chunks) with
  | Result.Error(_) => []
  | Result.Ok(c) =>
    let end = c.offset + c.length in
    let step = (s) => match s with
      | (at, rs) =>
        let id = Bytes.i32le(at, data) |> Result.defaultValue(0.0) in
        let size = Bytes.u32le(at + 4.0, data) |> Result.defaultValue(end) in
        (at + 8.0 + size, List.append([{ id: id, at: at + 8.0, length: size }], rs)) in
    match Objects.loop(step, (s) => match s with | (at, _) => at >= end, (c.offset, [])) with
    | (_, rs) => rs

// A chunk's records decoded as `Map<id, value>`.
let table = (name: string, d: Decode.t<'t>, chunks: List<Mis.chunk>, data: Bytes.t): Result.t<Map<float, 't>, Binary.error> =>
  records(name, chunks, data)
    |> List.tryMapAccum((u, r: record) => Decode.run(Decode.at(r.at, d), data) |> Result.map((v) => (u, (r.id, v))), 0.0)
    |> Result.map((acc) => match acc with | (_, pairs) => Map.fromList(pairs))

let vec3 = Cal.vec3
let bool32 = Decode.u32le |> Decode.map((b) => b != 0.0)

// `P$TransDoor` (94 bytes): type, closed, open, speed, axis, state, then
// limits and blocking (16 bytes), the closed, open and current locations.
let transDoor: Decode.t<Door.transDoor> =
  Decode.map5((closed, open, speed, axis, state) => (closed, open, speed, axis, state),
    Decode.skipThen(4.0, Decode.f32le), Decode.f32le, Decode.f32le, Decode.i32le, Decode.i32le)
    |> Decode.andThen((h) => match h with
      | (closed, open, speed, axis, state) =>
        Decode.map3((bc, bo, b): Door.transDoor =>
            { closed: closed, open: open, speed: speed, axis: axis, state: state, baseClosed: bc, baseOpen: bo, base: b },
          Decode.skipThen(16.0, vec3), vec3, vec3))

// `P$RotDoor` (98 or 110 bytes): as a TransDoor up to the locations, then
// the base angle, rooms and the clockwise flag at byte 94.
let rotDoor: Decode.t<Door.rotDoor> =
  Decode.map5((closed, open, speed, axis, state) => (closed, open, speed, axis, state),
    Decode.skipThen(4.0, Decode.f32le), Decode.f32le, Decode.f32le, Decode.i32le, Decode.i32le)
    |> Decode.andThen((h) => match h with
      | (closed, open, speed, axis, state) =>
        Decode.map4((bc, bo, b, cw): Door.rotDoor =>
            { closed: closed, open: open, speed: speed, axis: axis, state: state, clockwise: cw,
              baseClosed: bc, baseOpen: bo, base: b },
          Decode.skipThen(16.0, vec3), vec3, vec3, Decode.skipThen(18.0, bool32)))

let frobInfo: Decode.t<Door.frobInfo> =
  Decode.map3((w, i, t): Door.frobInfo => { world: w, inventory: i, tool: t }, Decode.u32le, Decode.u32le, Decode.u32le)

// `P$Scripts`: four 32-byte names (blank ones dropped) and "don't inherit".
let scripts: Decode.t<Objects.contribution<string>> =
  Decode.map2((names, dont): Objects.contribution<string> =>
      { items: List.filter((s) => s != "", names), inherits: dont == 0.0 },
    Decode.list(4.0, Motion.name(32.0)), Decode.u32le)

// A link chunk's records (14 bytes: id, source, destination, flavor).
type rawLink = { id: float, src: float, dst: float }
let links = (name: string, chunks: List<Mis.chunk>, data: Bytes.t): Result.t<List<rawLink>, Binary.error> =>
  match Mis.find(name, chunks) with
  | Result.Error(_) => Result.Ok([])
  | Result.Ok(c) =>
    Decode.run(Decode.at(c.offset, Decode.list(c.length / 14.0,
      Decode.map3((id, src, dst): rawLink => { id: id, src: src, dst: dst }, Decode.i32le, Decode.i32le, Decode.i32le)
        |> Decode.andThen((l) => Decode.skipThen(2.0, Decode.succeed(l))))), data)

// `LD$MetaProp`: each link's priority (a u32 record size, then id + i32).
let priorities = (chunks: List<Mis.chunk>, data: Bytes.t): Result.t<Map<float, float>, Binary.error> =>
  match Mis.find("LD$MetaProp", chunks) with
  | Result.Error(_) => Result.Ok(Map.empty())
  | Result.Ok(c) =>
    Decode.run(Decode.at(c.offset, Decode.u32le |> Decode.andThen((size) =>
      Decode.list((c.length - 4.0) / (4.0 + size),
        Decode.map2((id, p) => (id, p), Decode.i32le, Decode.i32le) |> Decode.andThen((r) => Decode.skipThen(size - 4.0, Decode.succeed(r)))))), data)
      |> Result.map(Map.fromList)

// A file's MetaProp links: each source's donors, highest priority first
// (ties in link order), and how many rank below the archetype (priority < 0).
type metaProps = { donors: Map<float, List<float>>, links: float, belowArchetype: float }

let metaProps = (chunks: List<Mis.chunk>, data: Bytes.t): Result.t<metaProps, Binary.error> =>
  links("L$MetaProp", chunks, data) |> Result.bind((ls) =>
  priorities(chunks, data) |> Result.map((ps) =>
    let pri = (l: rawLink) => Map.get(l.id, ps) |> Option.defaultValue(0.0) in
    { donors: ls |> List.sortBy((l: rawLink) => l.id - pri(l) * 10000000000.0)
                 |> List.fold((m, l: rawLink) => Map.update(l.src, (ds) => Option.Some(List.append([l.dst], Option.defaultValue([], ds))), m), Map.empty()),
      links: List.length(ls), belowArchetype: List.length(List.filter((l) => pri(l) < 0.0, ls)) }))

// The tables both files author, gamesys first so the mission's win.
let authoredIn = (chunks: List<Mis.chunk>, data: Bytes.t): Result.t<Door.authored, Binary.error> =>
  table("P$TransDoor", transDoor, chunks, data) |> Result.bind((td) =>
  table("P$RotDoor", rotDoor, chunks, data) |> Result.bind((rd) =>
  table("P$Locked", bool32, chunks, data) |> Result.bind((lk) =>
  table("P$FrobInfo", frobInfo, chunks, data) |> Result.bind((fi) =>
  table("P$Scripts", scripts, chunks, data) |> Result.bind((sc) =>
  links("L$SwitchLin", chunks, data) |> Result.map((sw) =>
    { transDoor: td, rotDoor: rd, locked: lk, frobInfo: fi, scripts: sc,
      switchLink: List.fold((t, l: rawLink) => Objects.linkAs(l.id, { src: l.src, dst: l.dst, data: true, donor: Option.None }, t),
                            Objects.noLinks(), sw) }))))))

let union = (a: Map<float, 't>, b: Map<float, 't>): Map<float, 't> => Map.fold((m, k, v) => Map.insert(k, v, m), a, b)

// Every object id with a property record in the file (any `P$` chunk).
let objectIds = (chunks: List<Mis.chunk>, data: Bytes.t): List<float> =>
  chunks |> List.filter((c: Mis.chunk) => Text.length(c.name) > 2.0 && List.take(2.0, Text.chars(c.name)) == ["P", "$"])
    |> List.fold((m, c: Mis.chunk) => List.fold((acc, r: record) => Map.insert(r.id, true, acc), m, records(c.name, chunks, data)), Map.empty())
    |> Map.keys

type stats = { objects: float, metaLinks: float, belowArchetype: float, transDoors: float, rotDoors: float,
               locked: float, frobInfo: float, scripts: float, switchLinks: float }

type loaded = { world: Door.world, objects: List<float>, stats: stats }

// The gamesys's donors and records under the mission's (a mission's links
// replace the gamesys's for the same source, as shock2quest merges them),
// then the mission's objects spawned.
let load = (gamesys: Bytes.t, mission: Bytes.t): Result.t<loaded, Binary.error> =>
  Decode.run(Mis.toc, gamesys) |> Result.bind((gc) =>
  Decode.run(Mis.toc, mission) |> Result.bind((mc) =>
  metaProps(gc, gamesys) |> Result.bind((gm) =>
  metaProps(mc, mission) |> Result.bind((mm) =>
  authoredIn(gc, gamesys) |> Result.bind((ga) =>
  authoredIn(mc, mission) |> Result.map((ma) =>
    let a: Door.authored = { transDoor: union(ga.transDoor, ma.transDoor), rotDoor: union(ga.rotDoor, ma.rotDoor),
      locked: union(ga.locked, ma.locked), frobInfo: union(ga.frobInfo, ma.frobInfo), scripts: union(ga.scripts, ma.scripts),
      switchLink: Map.fold((t, id, l) => Objects.linkAs(id, l, t), ga.switchLink, ma.switchLink.byId) } in
    let ids = objectIds(mc, mission) in
    let w0: Door.world = { graph: union(gm.donors, mm.donors), edits: Map.empty(), authored: a, props: Door.noProps,
      switchLink: a.switchLink, door: Map.empty(), moving: Map.empty(), queue: Objects.queue(), sounds: [] } in
    let w = List.fold(Door.spawn, w0, ids) in
    { world: w, objects: ids,
      stats: { objects: List.length(ids), metaLinks: gm.links + mm.links, belowArchetype: gm.belowArchetype + mm.belowArchetype,
               transDoors: Map.size(w.props.transDoor), rotDoors: Map.size(w.props.rotDoor), locked: Map.size(w.props.locked),
               frobInfo: Map.size(w.props.frobInfo), scripts: Map.size(w.props.scripts),
               switchLinks: Map.size(w.switchLink.byId) } }))))))
