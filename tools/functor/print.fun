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

// A character's pose: this clip at this time, when `motiondb.bin` and the
// clip (`<clip>_.mc`, from Data/res/motions) are in ss2/; else its rest pose.
let clipName = "ogpstd"
let seconds = 0.0

let clipFile = Text.join("", ["ss2/", clipName, "_.mc"])

let decode = (data: Bytes.t): Result.t<Mesh.t, Binary.error> =>
  if Bytes.latin1(0.0, 4.0, data) != Result.Ok("LGMM")
  then Lgmd.mesh(data)
  else if File.exists("ss2/motiondb.bin") && File.exists(clipFile)
  then
    Motion.clip(File.bytes("ss2/motiondb.bin"), File.bytes(clipFile), clipName)
      |> Result.bind((c) => Lgmm.posed(data, File.bytes("ss2/model.cal"), c, seconds))
  else Lgmm.rest(data, File.bytes("ss2/model.cal"))

let mesh = decode(if File.exists(model) then File.bytes(model) else Fixture.cube(4.0)) |> Binary.require

let part = mesh |> Solid.fromMesh({ height: 60.0, nozzle: 0.4 })
