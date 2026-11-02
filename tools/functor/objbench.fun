// objbench — the `objbench` entry: the object model's per-frame cost at
// medsci1's scale, with history on. Every property record in the mission
// (6,754 in 110 `P$` chunks) sits in a per-chunk `Map<id, Cell<Bytes>>`; each
// step writes 20 of them (spread over the chunks) and every 30th step sends
// a burst of TurnOn/TurnOff to all 52 doors, which the queue drains and the
// door system moves. Read `stages.sim_ms` from `/state` and the process's
// memory over the 900-frame history ring.
//
//   functor -d tools/functor run native --entry objbench --hidden --debug-port 8093

let config = Files.config

let mission = Level.mission
let data = if File.exists(mission) then File.bytes(mission) else Bytes.zeros(0.0)
let chunks = Decode.run(Mis.toc, data) |> Result.defaultValue([])

// Every `P$` chunk's records as own cells of their raw bytes.
let tables: Map<string, Map<float, Objects.Cell<Bytes.t>>> =
  chunks |> List.filter(Props.isProperty)
    |> List.fold((m, c: Mis.chunk) =>
      let t = Props.records(c.name, chunks, data) |> Result.defaultValue([])
        |> List.fold((acc, r: Props.record) => match Bytes.slice(r.at, r.length, data) with
          | Result.Ok(b) => Map.insert(r.id, Objects.Own(b), acc)
          | Result.Error(_) => acc, Map.empty()) in
      if Map.size(t) > 0.0 then Map.insert(c.name, t, m) else m, Map.empty())

// Every (chunk, object) record, to pick writes from.
let records: List<(string, float)> =
  Map.toList(tables) |> List.concatMap((e) => match e with | (name, t) => List.map((id) => (name, id), Map.keys(t)))
let count = List.length(records)
let doorIds = Map.keys(Level.world.door)

let writesPerStep = 20.0
let burstEvery = 30.0

type model = { tables: Map<string, Map<float, Objects.Cell<Bytes.t>>>, world: Door.world, step: float }

let init: model = { tables: tables, world: Level.world, step: 0.0 }

// Write `k`: a record far from the last (a multiplicative stride), set to
// 4 new bytes.
let write = (step: float, m: Map<string, Map<float, Objects.Cell<Bytes.t>>>, k: float) =>
  match List.nth(Math.mod((step * writesPerStep + k) * 2654.0, Math.max(count, 1.0)), records) with
  | Option.Some(r) => (match r with
    | (name, id) => Map.update(name, (t) => Option.map((x) => Objects.set(id, Bytes.fromF32le([step]), x), t), m))
  | Option.None => m

let tick = (m: model, dt, tts) =>
  let open = Math.mod(Math.floor(m.step / burstEvery), 2.0) == 0.0 in
  let burst = if Math.mod(m.step, burstEvery) == 0.0
    then List.fold((w, id) => Door.send(id, if open then Door.TurnOn(0.0) else Door.TurnOff(0.0), w), m.world, doorIds)
    else m.world in
  ({ tables: List.fold((t, k) => write(m.step, t, k), m.tables, List.range(writesPerStep)),
     world: Door.frame(dt, [], burst), step: m.step + 1.0 }, Effect.none())

let draw = (m: model, tts) =>
  Frame.createLit(Camera3D.firstPerson(Vec3.make(0.0, 0.0, 0.0), Angle.radians(0.0), Angle.radians(0.0), Angle.degrees(75.0)),
    Scene.group([]), [])

let ui = (m: model) =>
  Ui.text(Text.join("", ["records ", Text.fromFloat(count), "  step ", Text.fromFloat(m.step), "  doors moving ",
                         Text.fromFloat(Map.size(m.world.moving))]))
    |> Ui.panel(Ui.topLeft())
