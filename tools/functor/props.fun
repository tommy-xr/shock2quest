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
// when the file has no such chunk. A record running past the chunk fails.
type walk = { at: float, records: List<record>, error: Option.t<Binary.error> }

let records = (name: string, chunks: List<Mis.chunk>, data: Bytes.t): Result.t<List<record>, Binary.error> =>
  match Mis.find(name, chunks) with
  | Result.Error(_) => Result.Ok([])
  | Result.Ok(c) =>
    let end = c.offset + c.length in
    let step = (w: walk): walk =>
      let header = Bytes.i32le(w.at, data) |> Result.bind((id) => Bytes.u32le(w.at + 4.0, data) |> Result.map((size) => (id, size))) in
      match header with
      | Result.Ok(h) => (match h with
        | (id, size) =>
          if w.at + 8.0 + size > end then { w with at: end, error: Option.Some(Decode.failure(w.at, Text.join("", [name, " record runs past its chunk"]))) }
          else { w with at: w.at + 8.0 + size, records: List.append([{ id: id, at: w.at + 8.0, length: size }], w.records) })
      | Result.Error(e) => { w with at: end, error: Option.Some(e) } in
    let w = Objects.loop(step, (w: walk) => w.at >= end, { at: c.offset, records: [], error: Option.None }) in
    match w.error with
    | Option.Some(e) => Result.Error(e)
    | Option.None => Result.Ok(w.records)

// A chunk's records decoded as `Map<id, value>`, each within its own bytes.
let table = (name: string, d: Decode.t<'t>, chunks: List<Mis.chunk>, data: Bytes.t): Result.t<Map<float, 't>, Binary.error> =>
  records(name, chunks, data) |> Result.bind((rs) => rs
    |> List.tryMapAccum((u, r: record) =>
         Bytes.slice(r.at, r.length, data) |> Result.bind((bytes) => Decode.run(d, bytes)) |> Result.map((v) => (u, (r.id, v))), 0.0)
    |> Result.map((acc) => match acc with | (_, pairs) => Map.fromList(pairs)))

let bool32 = Decode.u32le |> Decode.map((b) => b != 0.0)

// A door's first 24 bytes: type, closed, open, speed, axis, state.
type doorHead = { closed: float, open: float, speed: float, axis: float, state: float }
let doorHead: Decode.t<doorHead> =
  Decode.map5((closed, open, speed, axis, state): doorHead => { closed: closed, open: open, speed: speed, axis: axis, state: state },
    Decode.skipThen(4.0, Decode.f32le), Decode.f32le, Decode.f32le, Decode.i32le, Decode.i32le)

// `P$TransDoor` (94 bytes): the head, limits and blocking (16 bytes), then
// the closed, open and current locations.
let transDoor: Decode.t<Door.transDoor> =
  doorHead |> Decode.andThen((h: doorHead) =>
    Decode.map3((bc, bo, b): Door.transDoor =>
        { closed: h.closed, open: h.open, speed: h.speed, axis: h.axis, state: h.state, baseClosed: bc, baseOpen: bo, base: b },
      Decode.skipThen(16.0, Mis.vec3), Mis.vec3, Mis.vec3))

// `P$RotDoor` (98 or 110 bytes): as a TransDoor up to the locations, then
// the base angle, rooms and the clockwise flag at byte 94.
let rotDoor: Decode.t<Door.rotDoor> =
  doorHead |> Decode.andThen((h: doorHead) =>
    Decode.map4((bc, bo, b, cw): Door.rotDoor =>
        { closed: h.closed, open: h.open, speed: h.speed, axis: h.axis, state: h.state, clockwise: cw,
          baseClosed: bc, baseOpen: bo, base: b },
      Decode.skipThen(16.0, Mis.vec3), Mis.vec3, Mis.vec3, Decode.skipThen(18.0, bool32)))

let frobInfo: Decode.t<Door.frobInfo> =
  Decode.map3((w, i, t): Door.frobInfo => { world: w, inventory: i, tool: t }, Decode.u32le, Decode.u32le, Decode.u32le)

// `P$Scripts`: four 32-byte names (blank ones dropped) and "don't inherit".
let scripts: Decode.t<Objects.contribution<string>> =
  Decode.map2((names, dont): Objects.contribution<string> =>
      { items: List.filter((s) => s != "", names), inherits: dont == 0.0 },
    Decode.list(4.0, Motion.name(32.0)), Decode.u32le)

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
// (ties in file order: the sort is stable), and how many rank below the
// archetype (priority < 0).
type metaProps = { donors: Map<float, List<float>>, links: float, belowArchetype: float }

let metaProps = (chunks: List<Mis.chunk>, data: Bytes.t): Result.t<metaProps, Binary.error> =>
  Mis.links("L$MetaProp", chunks, data) |> Result.bind((ls) =>
  priorities(chunks, data) |> Result.map((ps) =>
    let pri = (l: Mis.link) => Map.get(l.id, ps) |> Option.defaultValue(0.0) in
    { donors: ls |> List.sortBy((l: Mis.link) => 0.0 - pri(l))
                 |> List.fold((m, l: Mis.link) => Map.update(l.src, (ds) => Option.Some(List.append([l.dst], Option.defaultValue([], ds))), m), Map.empty()),
      links: List.length(ls), belowArchetype: List.length(List.filter((l) => pri(l) < 0.0, ls)) }))

// The door slice's records and SwitchLinks one file authors.
let authoredIn = (chunks: List<Mis.chunk>, data: Bytes.t): Result.t<Door.authored, Binary.error> =>
  table("P$TransDoor", transDoor, chunks, data) |> Result.bind((td) =>
  table("P$RotDoor", rotDoor, chunks, data) |> Result.bind((rd) =>
  table("P$Locked", bool32, chunks, data) |> Result.bind((lk) =>
  table("P$FrobInfo", frobInfo, chunks, data) |> Result.bind((fi) =>
  table("P$Scripts", scripts, chunks, data) |> Result.bind((sc) =>
  Mis.links("L$SwitchLin", chunks, data) |> Result.map((sw) =>
    { transDoor: td, rotDoor: rd, locked: lk, frobInfo: fi, scripts: sc,
      switchLink: List.fold((t, l: Mis.link) => Objects.linkAs(l.id, { src: l.src, dst: l.dst, data: true, donor: Option.None }, t),
                            Objects.noLinks(), sw) }))))))

let union = (a: Map<float, 't>, b: Map<float, 't>): Map<float, 't> => Map.fold((m, k, v) => Map.insert(k, v, m), a, b)

// A property chunk (`P$…`).
let isProperty = (c: Mis.chunk): bool => Text.length(c.name) > 2.0 && List.take(2.0, Text.chars(c.name)) == ["P", "$"]

// Every object id with a property record in the file (any `P$` chunk).
let objectIds = (chunks: List<Mis.chunk>, data: Bytes.t): List<float> =>
  chunks |> List.filter(isProperty)
    |> List.fold((m, c: Mis.chunk) => List.fold((acc, r: record) => Map.insert(r.id, true, acc), m,
         records(c.name, chunks, data) |> Result.defaultValue([])), Map.empty())
    |> Map.keys

type stats = { objects: float, metaLinks: float, belowArchetype: float, transDoors: float, rotDoors: float,
               locked: float, frobInfo: float, scripts: float, switchLinks: float, doors: float }

// `modelNames`: every authored `P$ModelName` (resolve through donors);
// `headings`: each mission object's `P$Position` heading.
type loaded = { world: Door.world, objects: List<float>, stats: stats, modelNames: Map<float, string>, headings: Map<float, float> }

// A chunk's records as text (NUL-terminated within each record).
let names = (name: string, chunks: List<Mis.chunk>, data: Bytes.t): Result.t<Map<float, string>, Binary.error> =>
  records(name, chunks, data) |> Result.bind((rs) => rs
    |> List.tryMapAccum((u, r: record) => Decode.run(Decode.at(r.at, Motion.name(r.length)), data) |> Result.map((v) => (u, (r.id, v))), 0.0)
    |> Result.map((acc) => match acc with | (_, pairs) => Map.fromList(pairs)))

// The gamesys's donors and records under the mission's (the mission's win;
// its links replace the gamesys's for the same source, as shock2quest merges
// them), then the mission's objects spawned.
let load = (gamesys: Bytes.t, mission: Bytes.t): Result.t<loaded, Binary.error> =>
  Decode.run(Mis.toc, gamesys) |> Result.bind((gc) =>
  Decode.run(Mis.toc, mission) |> Result.bind((mc) =>
  metaProps(gc, gamesys) |> Result.bind((gm) =>
  metaProps(mc, mission) |> Result.bind((mm) =>
  authoredIn(gc, gamesys) |> Result.bind((ga) =>
  authoredIn(mc, mission) |> Result.bind((ma) =>
  names("P$ModelName", gc, gamesys) |> Result.bind((gn) =>
  names("P$ModelName", mc, mission) |> Result.bind((mn) =>
  (match Mis.find("P$Position", mc) with
   | Result.Ok(_) => Mis.positions(mc, mission)
   | Result.Error(_) => Result.Ok(Map.empty())) |> Result.map((ps) =>
    let a: Door.authored = { transDoor: union(ga.transDoor, ma.transDoor), rotDoor: union(ga.rotDoor, ma.rotDoor),
      locked: union(ga.locked, ma.locked), frobInfo: union(ga.frobInfo, ma.frobInfo), scripts: union(ga.scripts, ma.scripts),
      switchLink: Map.fold((t, id, l) => Objects.linkAs(id, l, t), ga.switchLink, ma.switchLink.byId) } in
    let ids = objectIds(mc, mission) in
    let w0: Door.world = { Door.empty with graph: union(gm.donors, mm.donors), authored: a, switchLink: a.switchLink } in
    let w = List.fold(Door.spawn, w0, ids) in
    { world: w, objects: ids, modelNames: union(gn, mn), headings: Map.fold((m, id, p: Mis.start) => Map.insert(id, p.heading, m), Map.empty(), ps),
      stats: { objects: List.length(ids), metaLinks: gm.links + mm.links, belowArchetype: gm.belowArchetype + mm.belowArchetype,
               transDoors: Map.size(w.props.transDoor), rotDoors: Map.size(w.props.rotDoor), locked: Map.size(w.props.locked),
               frobInfo: Map.size(w.props.frobInfo), scripts: Map.size(w.props.scripts),
               switchLinks: Map.size(w.switchLink.byId), doors: Map.size(w.door) } })))))))))
