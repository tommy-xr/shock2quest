// Checks against medsci1 and the gamesys when ss2data/ is mapped (see
// README): how many of the door slice's records each object ends up with
// after copy-down, as shock2quest's loader resolves them (52 sliding doors
// run StdDoor), and that no MetaProp link ranks below its archetype. Without the files these pass
// vacuously.

let gamesysFile = Path.of("ss2data/shock2.gam")
let missionFile = Path.of("ss2data/medsci1.mis")

expect not (File.exists(gamesysFile) && File.exists(missionFile))
  || (Props.load(File.bytes(gamesysFile), File.bytes(missionFile)) |> Result.map((l: Props.loaded) => l.stats))
       == Result.Ok({ objects: 1556.0, metaLinks: 6762.0, belowArchetype: 0.0, transDoors: 56.0, rotDoors: 5.0,
                      locked: 9.0, frobInfo: 346.0, scripts: 749.0, switchLinks: 336.0, doors: 52.0 })
