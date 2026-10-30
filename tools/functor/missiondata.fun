// Checks against the real medsci1 when ss2data/ is mapped (see README): world
// rep counts cross-checked against a separate walk of the same bytes, and the
// player factory shock2quest starts at. Without the file these pass vacuously.

let medsci1 = Path.of("ss2data/medsci1.mis")

expect not File.exists(medsci1)
  || (World.load(File.bytes(medsci1)) |> Result.map((l: World.level) => (l.stats, l.start)))
       == Result.Ok(({ cells: 1953.0, vertices: 30132.0, renderPolygons: 9602.0, lightmapBytes: 1534934.0,
                       triangles: 26508.0, textures: 100.0 },
                     { position: [87.41897583007813, 44.64602279663086, -10.4399995803833], heading: 0.0 }))
