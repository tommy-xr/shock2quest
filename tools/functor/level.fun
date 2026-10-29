// level — medsci1's world, decoded once. Its own module so edits to `walk.fun`
// leave the decode alone.

let mission = "ss2data/medsci1.mis"
// Without the mission (no `ss2data` mount, or a headset still fetching it) a
// plain floor stands in, and the panel says why.
let loaded = if File.exists(mission) then World.load(File.bytes(mission))
             else Result.Error(Decode.failure(0.0, "ss2data/medsci1.mis isn't there: map the ss2data mount (see README)"))
let level = Result.defaultValue(World.placeholder, loaded)
