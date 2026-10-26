// Checks against real game data, when it's in ss2/ (gitignored): the pipe
// hybrid at rest and in `ogpstd` frame 0, against values from shock2quest's
// own pipeline (see its character spec). Without the files these pass vacuously.

let have = (paths: List<string>): bool => List.all((p) => File.exists(p), paths)

let near = (a: List<float>, b: List<float>): bool =>
  List.length(a) == List.length(b)
    && List.all((p) => match p with | (x, y) => Math.abs(x - y) < 0.001, List.zip(b, a))

let hybrid = ["ss2/hybrid.bin", "ss2/hybrid.cal"]
let standing = List.append(hybrid, ["ss2/motiondb.bin", "ss2/ogpstd_.mc"])

let bounds = (m: Mesh.t): List<float> =>
  let b = Mesh.bounds(m) in
  [Vec3.x(b.min), Vec3.y(b.min), Vec3.z(b.min), Vec3.x(b.max), Vec3.y(b.max), Vec3.z(b.max)]

expect not have(hybrid)
  || near(bounds(Lgmm.rest(File.bytes("ss2/hybrid.bin"), File.bytes("ss2/hybrid.cal")) |> Binary.require),
          [-0.2934, -5.5243, -4.0758, 1.5229, 3.6003, 3.4440])

expect not have(standing)
  || (let clip = Motion.clip(File.bytes("ss2/motiondb.bin"), File.bytes("ss2/ogpstd_.mc"), "ogpstd") |> Binary.require in
      let skel = Decode.run(Cal.skeleton, File.bytes("ss2/hybrid.cal")) |> Binary.require in
      let p = Motion.sample(clip, 0.0) in
      let joints = Cal.globals(Cal.chains(skel), p.root, p.animate) in
      let joint = (j) => Affine.offset(Option.defaultValue(Affine.identity, List.nth(j, joints))) in
      let posed = Lgmm.posed(File.bytes("ss2/hybrid.bin"), File.bytes("ss2/hybrid.cal"), clip, 0.0) |> Binary.require in
      let b = bounds(posed) in
      clip.motion.frames == 43.0
        && near(joint(19.0), [0.7125, 0.0920, 3.4325])
        && near(joint(0.0), [0.9357, 0.3809, -4.0218])
        && near([Affine.n(2.0, b), Affine.n(5.0, b)], [-4.2535, 3.3984]))
