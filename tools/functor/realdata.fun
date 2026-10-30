// Checks against real game data, when it's in ss2/ (gitignored): the pipe
// hybrid at rest, in `ogpstd` frame 0 and at animated frames, against values
// from shock2quest's own pipeline. Without the files these pass vacuously.

let have = (paths: List<string>): bool => List.all((p) => File.exists(Path.of(p)), paths)

let near = (a: List<float>, b: List<float>): bool =>
  List.length(a) == List.length(b)
    && List.all((p) => match p with | (x, y) => Math.abs(x - y) < 0.001, List.zip(b, a))

let hybrid = ["ss2/hybrid.bin", "ss2/hybrid.cal"]
let standing = List.append(hybrid, ["ss2/motiondb.bin", "ss2/ogpstd_.mc"])

let bounds = (m: Mesh.t): List<float> =>
  let b = Mesh.bounds(m) in
  [Vec3.x(b.min), Vec3.y(b.min), Vec3.z(b.min), Vec3.x(b.max), Vec3.y(b.max), Vec3.z(b.max)]

expect not have(hybrid)
  || near(bounds(Lgmm.rest(File.bytes(Path.of("ss2/hybrid.bin")), File.bytes(Path.of("ss2/hybrid.cal"))) |> Binary.require),
          [-0.2934, -5.5243, -4.0758, 1.5229, 3.6003, 3.4440])

expect not have(standing)
  || (let clip = Motion.clip(File.bytes(Path.of("ss2/motiondb.bin")), File.bytes(Path.of("ss2/ogpstd_.mc")), "ogpstd") |> Binary.require in
      let skel = Decode.run(Cal.skeleton, File.bytes(Path.of("ss2/hybrid.cal"))) |> Binary.require in
      let p = Motion.sample(clip, 0.0) in
      let joints = Cal.globals(Cal.chains(skel), p.root, p.animate) in
      let joint = (j) => Affine.offset(Option.defaultValue(Affine.identity, List.nth(j, joints))) in
      let posed = Lgmm.posed(File.bytes(Path.of("ss2/hybrid.bin")), File.bytes(Path.of("ss2/hybrid.cal")), clip, 0.0) |> Binary.require in
      let b = bounds(posed) in
      clip.motion.frames == 43.0
        && near(joint(19.0), [0.7125, 0.0920, 3.4325])
        && near(joint(0.0), [0.9357, 0.3809, -4.0218])
        && near([Affine.n(2.0, b), Affine.n(5.0, b)], [-4.2535, 3.3984]))

// Animated frames against shock2quest's own pipeline (its `dark` crate's
// skeleton `animate` on the same clips, in native axes without its 90° turn):
// the right toe, pipe tip and head (joints 1, 17, 19). A joined walk's frame 8
// is `ogpbzwkr`'s frame 0.
let moving = List.append(hybrid, ["ss2/motiondb.bin", "ss2/ogpidle1_.mc", "ss2/ogpbzwkl_.mc", "ss2/ogpbzwkr_.mc"])

let motionOf = (n: string): Motion.clip =>
  Motion.clip(File.bytes(Path.of("ss2/motiondb.bin")), File.bytes(Path.of(Text.join("", ["ss2/", n, "_.mc"]))), n) |> Binary.require

let pinned = (c: Motion.clip, frame: float, expected: List<List<float>>): bool =>
  let skel = Decode.run(Cal.skeleton, File.bytes(Path.of("ss2/hybrid.cal"))) |> Binary.require in
  let p = Motion.sample(c, frame / c.motion.fps) in
  let g = Cal.globals(Cal.chains(skel), p.root, p.animate) in
  near(List.concatMap((j) => Affine.offset(Option.defaultValue(Affine.identity, List.nth(j, g))), [1.0, 17.0, 19.0]), List.flatten(expected))

expect not have(moving)
  || (let idle = motionOf("ogpidle1") in
      let step = motionOf("ogpbzwkl") in
      let walk = Motion.join(step, motionOf("ogpbzwkr")) |> Binary.require in
      idle.motion.frames == 110.0 && idle.motion.fps == 30.0 && walk.motion.frames == 17.0
        && pinned(idle, 13.0, [[0.8611, -0.1854, -3.8019], [3.3290, -0.5510, -1.1791], [0.8263, -0.1327, 3.3809]])
        && pinned(idle, 50.37, [[0.6770, 0.1046, -3.9656], [1.8343, 1.3263, -2.2129], [0.6625, 0.2866, 3.4092]])
        && pinned(idle, 80.0, [[0.6542, 0.1373, -3.9596], [2.3922, 1.8026, -1.5853], [0.6147, 0.2925, 3.3845]])
        && pinned(step, 3.0, [[0.2034, -0.0692, -4.3332], [2.7345, -3.1202, -0.3832], [1.3792, -0.3667, 3.2450]])
        && pinned(walk, 5.0, [[-0.7474, -0.0180, -4.4203], [2.9725, -2.6329, -0.8665], [1.4373, -0.2332, 3.1814]])
        && pinned(walk, 8.0, [[-2.0629, -0.2480, -3.9182], [3.3499, -1.5721, -1.2631], [1.4933, 0.0758, 3.1569]]))
