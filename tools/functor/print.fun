// A printable System Shock 2 object. Copy a model into ss2/ (gitignored, so
// game files stay out of the repo); without one, a synthetic test cube prints.
//
//   mkdir -p tools/functor/ss2 && cp Data/res/obj/CRATE01.BIN tools/functor/ss2/model.bin
//   functor -d tools/functor run native                         # view it
//   functor -d tools/functor export -o tools/functor/crate.stl    # print it

let model = Path.of("ss2/model.bin")

let source = if File.exists(model) then File.bytes(model) else Fixture.cube(4.0)

let part =
  source
    |> Lgmd.mesh
    |> Binary.require
    |> Solid.fromMesh({ height: 60.0, nozzle: 0.4 })
