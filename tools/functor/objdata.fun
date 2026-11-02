// Checks for `Props`: a synthetic gamesys and mission written with `Encode`
// (no game files), then medsci1 and the gamesys when ss2data/ is mapped.

let i32s = (xs: List<float>) => Encode.bytes(Bytes.fromI32le(xs))
// A link record: id, source, destination, flavor.
let link = (id: float, src: float, dst: float) => Encode.sequence([i32s([id, src, dst]), Encode.u16le(1.0)])
// A property record: object, size, data.
let prop = (id: float, data: Bytes.t) => Encode.sequence([i32s([id]), Encode.u32le(Bytes.length(data)), Encode.bytes(data)])
let chunk = (name: string, parts: List<Encode.t>) => (name, Encode.run(Encode.sequence(parts)))
let scripts = (name: string) => Encode.run(Encode.sequence([Misfixture.named(32.0, name), Encode.zeros(96.0), Encode.u32le(0.0)]))

// Gamesys: -2 Door (donor -1, which holds StdDoor), -3 Locked.
let gamesys = Misfixture.chunkFile([
  chunk("L$MetaProp", [link(1.0, -2.0, -1.0), link(2.0, -4.0, -1.0)]),
  chunk("LD$MetaProp", [Encode.u32le(4.0), i32s([1.0, 0.0, 2.0, 0.0])]),
  chunk("P$Scripts", [prop(-1.0, scripts("StdDoor"))]),
  chunk("P$Locked", [prop(-3.0, Bytes.fromU32le([1.0]))]),
])
// Mission: object 5 is a Door with Locked added at priority 1024 (written
// after its archetype link), 6 a Door whose own value unlocks it, and a
// SwitchLink 5 → 6.
let missionWith = (locked: List<Encode.t>) => Misfixture.chunkFile([
  chunk("L$MetaProp", [link(10.0, 5.0, -2.0), link(11.0, 5.0, -3.0), link(12.0, 6.0, -2.0)]),
  chunk("LD$MetaProp", [Encode.u32le(4.0), i32s([10.0, 0.0, 11.0, 1024.0, 12.0, 0.0])]),
  chunk("P$FrobInfo", [prop(5.0, Bytes.fromU32le([2.0, 0.0, 0.0, 0.0]))]),
  chunk("P$Locked", locked),
  chunk("L$SwitchLin", [link(20.0, 5.0, 6.0)]),
])
let synthetic = Props.load(gamesys, missionWith([prop(6.0, Bytes.fromU32le([0.0]))])) |> Binary.require
let sw = synthetic.world

expect synthetic.objects == [5.0, 6.0]
expect Map.get(5.0, sw.graph) == Option.Some([-3.0, -2.0])
expect Door.donorsOf(sw, 5.0) == [-3.0, -2.0, -1.0]
expect (Map.get(5.0, sw.props.locked), Map.get(6.0, sw.props.locked)) == (Option.Some(Objects.Inherited(true)), Option.Some(Objects.Own(false)))
expect Map.get(5.0, sw.props.scripts) == Option.Some(["StdDoor"])
expect Objects.get(5.0, sw.props.frobInfo) == Option.Some({ world: 2.0, inventory: 0.0, tool: 0.0 })
expect List.map((l: Objects.link<bool>) => l.dst, Objects.from(5.0, sw.switchLink)) == [6.0]
// Door layouts: a TransDoor's locations at bytes 40, 52 and 64; a RotDoor's
// clockwise flag at byte 94.
let doorBytes = (cw: float) => Encode.run(Encode.sequence([i32s([0.0]), Encode.bytes(Bytes.fromF32le([0.0, 6.0, 12.0])),
  i32s([1.0, 0.0]), Encode.zeros(16.0), Encode.bytes(Bytes.fromF32le([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0])),
  Encode.zeros(18.0), Encode.u32le(cw)]))
let doors = Props.load(gamesys, Misfixture.chunkFile([chunk("L$MetaProp", []), chunk("P$TransDoor", [prop(8.0, doorBytes(0.0))]),
  chunk("P$RotDoor", [prop(9.0, doorBytes(1.0))])])) |> Binary.require
expect Objects.get(8.0, doors.world.props.transDoor) == Option.Some({ closed: 0.0, open: 6.0, speed: 12.0, axis: 1.0, state: 0.0,
  baseClosed: [1.0, 2.0, 3.0], baseOpen: [4.0, 5.0, 6.0], base: [7.0, 8.0, 9.0] })
expect (Objects.get(9.0, doors.world.props.rotDoor) |> Option.map((d: Door.rotDoor) => (d.clockwise, d.base))) == Option.Some((true, [7.0, 8.0, 9.0]))
// A record too short for its property, or running past its chunk, fails.
expect Result.isError(Props.load(gamesys, missionWith([prop(6.0, Bytes.zeros(0.0)), prop(7.0, Bytes.fromU32le([1.0]))])))
expect Result.isError(Props.load(gamesys, missionWith([Encode.sequence([i32s([6.0]), Encode.u32le(100.0), Encode.u32le(0.0)])])))

// medsci1: how many of the door slice's records objects end up with after
// copy-down, as shock2quest's loader resolves them (52 sliding doors run
// StdDoor), and that no MetaProp link ranks below its archetype. Without
// the files this passes vacuously.
let gamesysFile = Path.of("ss2data/shock2.gam")
let missionFile = Path.of("ss2data/medsci1.mis")

expect not (File.exists(gamesysFile) && File.exists(missionFile))
  || (Props.load(File.bytes(gamesysFile), File.bytes(missionFile)) |> Result.map((l: Props.loaded) => l.stats))
       == Result.Ok({ objects: 1556.0, metaLinks: 6762.0, belowArchetype: 0.0, transDoors: 56.0, rotDoors: 5.0,
                      locked: 9.0, frobInfo: 346.0, scripts: 749.0, switchLinks: 336.0, doors: 52.0 })
