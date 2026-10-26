// A printable System Shock 2 object or character. Copy a model into ss2/
// (gitignored, so game files stay out of the repo) — a character also needs its
// skeleton beside it; without one, a synthetic test cube prints.
//
//   mkdir -p tools/functor/ss2 && cp Data/res/obj/CRATE01.BIN tools/functor/ss2/model.bin
//   cp Data/res/mesh/GRUNT_P.BIN tools/functor/ss2/model.bin     # a pipe hybrid …
//   cp Data/res/mesh/GRUNT_P.CAL tools/functor/ss2/model.cal     # … and its skeleton
//   functor -d tools/functor run native                           # view it
//   functor -d tools/functor export -o tools/functor/part.stl     # print it

let model = Path.of("ss2/model.bin")

let decode = (data: Bytes.t): Result.t<Mesh.t, Binary.error> =>
  if Bytes.latin1(0.0, 4.0, data) == Result.Ok("LGMM")
  then Lgmm.rest(data, File.bytes("ss2/model.cal"))
  else Lgmd.mesh(data)

let mesh = decode(if File.exists(model) then File.bytes(model) else Fixture.cube(4.0)) |> Binary.require

let part = mesh |> Solid.fromMesh({ height: 60.0, nozzle: 0.4 })
