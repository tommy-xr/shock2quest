// A synthetic LGMD file, written with `Encode`, to test `Lgmd` without game
// files: a unit cube whose top four vertices are a child sub-object turned 90°
// about X (y → z, z → -y) and moved (0, 1, 2) — every face stays planar.

let header = (version: float) =>
  Encode.sequence([
    Encode.latin1("LGMD"), Encode.u32le(version),
    Encode.zeros(52.0),                                    // name, radii, bounds, centre
    Encode.u16le(6.0), Encode.u16le(8.0), Encode.u16le(0.0), // polygons, vertices, params
    Encode.u8(0.0), Encode.u8(0.0), Encode.u8(0.0), Encode.u8(2.0), // materials … objects
    Encode.bytes(Bytes.fromU32le([122.0, 578.0, 578.0, 578.0, 308.0, 578.0, 578.0, 404.0, 578.0, 578.0])),
    Encode.bytes(Bytes.fromU32le([0.0, 578.0, 8.0]))       // v4 material extras
  ])

// A 93-byte sub-object.
let subObject = (matrix: List<float>, child: float, next: float, start: float, count: float) =>
  Encode.sequence([
    Encode.zeros(21.0), Encode.bytes(Bytes.fromF32le(matrix)),
    Encode.i16le(child), Encode.i16le(next), Encode.zeros(4.0),
    Encode.u16le(start), Encode.u16le(count), Encode.zeros(12.0)
  ])

// A 29-byte quad without UVs (version 4 has a trailing byte).
let quad = (ids: List<float>) =>
  Encode.sequence([
    Encode.zeros(4.0), Encode.u8(0.0), Encode.u8(4.0), Encode.zeros(6.0),
    Encode.bytes(Bytes.fromU16le(ids)), Encode.zeros(9.0)
  ])

// Corner i of the unit cube is (i mod 2, i / 2 mod 2, i / 4).
let corners =
  List.range(8.0)
    |> List.concatMap((i) => [Math.mod(i, 2.0), Math.mod(Math.floor(i / 2.0), 2.0), Math.floor(i / 4.0)])

// Rotation columns, then translation — Dark's layout.
let turned = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 1.0, 2.0]

let faces = [
  [0.0, 2.0, 3.0, 1.0], [4.0, 5.0, 7.0, 6.0], [0.0, 1.0, 5.0, 4.0],
  [2.0, 6.0, 7.0, 3.0], [0.0, 4.0, 6.0, 2.0], [1.0, 3.0, 7.0, 5.0]
]

let zeroMatrix = List.range(12.0) |> List.map((i) => 0.0)

// The cube with the root and child parts' transforms, the child's vertex
// count, and the corners and faces given.
let cubeRooted = (version: float, points: List<float>, root: List<float>, child: List<float>, childCount: float, quads: List<List<float>>): Bytes.t =>
  Encode.run(Encode.sequence([
    header(version),
    subObject(root, 1.0, -1.0, 0.0, 4.0),
    subObject(child, -1.0, -1.0, 4.0, childCount),
    Encode.bytes(Bytes.fromF32le(points)),
    Encode.list(quad, quads)
  ]))

let cubeWith = (version: float, points: List<float>, child: List<float>, childCount: float, quads: List<List<float>>): Bytes.t =>
  cubeRooted(version, points, zeroMatrix, child, childCount, quads)

let cube = (version: float): Bytes.t => cubeWith(version, corners, turned, 4.0, faces)

// The whole model lifted 5 by the root: the child composes after it, so the
// top lands at z = 5 + 3 (the other order would swing it to y = -5).
let lifted = cubeRooted(4.0, corners, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 5.0], turned, 4.0, faces)

// The top corners stored at z = 0 (on the bottom ones), lifted to 1 by the child.
let flatTop =
  cubeWith(4.0, List.indexedMap((k, v) => if Math.mod(k, 3.0) == 2.0 then 0.0 else v, corners),
    [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0], 4.0, faces)

let decoded = Lgmd.mesh(cube(4.0))

expect Bytes.length(cube(4.0)) == 578.0
expect Result.map(Mesh.triangleCount, decoded) == Result.Ok(12.0)
// Top corners (x, y, 1) → (x, 0, 2 + y): y stays within 0..1 (the transposed
// matrix would push it to 2) and z reaches 3.
expect (decoded |> Result.map((m) => Vec3.y(Mesh.bounds(m).max))) == Result.Ok(1.0)
expect (decoded |> Result.map((m) => Vec3.z(Mesh.bounds(m).max))) == Result.Ok(3.0)
expect (Decode.run(Lgmd.model, cube(4.0)) |> Result.map((m: Lgmd.model) => Lgmd.parents(m.parts))) == Result.Ok([-1.0, 0.0])

let failure = (data: Bytes.t): string =>
  match Lgmd.mesh(data) with
  | Result.Error(e) => Binary.describe(e)
  | Result.Ok(_) => "decoded"

expect failure(cube(7.0)) == "at byte 4: LGMD version 7 isn't supported (3 to 6 are)"
expect (Lgmd.mesh(lifted) |> Result.map((m) => [Vec3.y(Mesh.bounds(m).min), Vec3.z(Mesh.bounds(m).max)])) == Result.Ok([0.0, 8.0])
expect failure(Bytes.fromLatin1("LGMM and more")) == "header at byte 4: not an LGMD object mesh (starts with \"LGMM\")"
expect Text.contains("polygons", failure(Binary.require(Bytes.slice(0.0, 500.0, cube(4.0)))))
// Corners that only separate once posed keep their faces.
expect Result.map(Mesh.triangleCount, Lgmd.mesh(flatTop)) == Result.Ok(12.0)
// Bad indices and ranges are decode errors, not crashes.
expect Text.contains("polygons[0]", failure(cubeWith(4.0, corners, turned, 4.0, List.append(List.drop(1.0, faces), [[0.0, 1.0, 9.0, 4.0]]))))
expect Text.contains("past the end", failure(cubeWith(4.0, corners, turned, 5.0, faces)))

// A synthetic character: a root joint 0 and a limb to joint 1, 2 up. Three
// vertices follow joint 0 rigidly; three blend on joint 1 (weights 1, 1, 0).

let characterMesh =
  Encode.run(Encode.sequence([
    Encode.latin1("LGMM"), Encode.u32le(2.0), Encode.zeros(13.0),
    Encode.u8(2.0), Encode.u8(0.0), Encode.u8(2.0),               // segments, materials, runs
    Encode.u16le(2.0), Encode.u16le(6.0), Encode.u16le(3.0), Encode.u16le(0.0),
    // remap, segments, materials, runs, triangles, normals, vertices, uvs, weights
    Encode.bytes(Bytes.fromU32le([72.0, 72.0, 112.0, 112.0, 144.0, 176.0, 176.0, 248.0, 248.0])),
    Encode.zeros(4.0),
    // Segments @72: joint 0 rigid, joint 1 stretchy.
    Encode.zeros(4.0), Encode.i8(0.0), Encode.zeros(2.0), Encode.u8(0.0), Encode.zeros(12.0),
    Encode.zeros(4.0), Encode.i8(1.0), Encode.zeros(2.0), Encode.u8(1.0), Encode.zeros(12.0),
    // Runs @112: vertices 0..2 → segment 0; 3..5 → segment 1, weights from 0.
    Encode.zeros(4.0), Encode.i16le(3.0), Encode.i16le(0.0), Encode.u16le(0.0), Encode.zeros(4.0), Encode.i16le(0.0),
    Encode.zeros(4.0), Encode.i16le(3.0), Encode.i16le(3.0), Encode.u16le(0.0), Encode.zeros(4.0), Encode.i16le(1.0),
    // Triangles @144.
    Encode.bytes(Bytes.fromU16le([0.0, 1.0, 2.0])), Encode.zeros(10.0),
    Encode.bytes(Bytes.fromU16le([3.0, 4.0, 5.0])), Encode.zeros(10.0),
    // Vertices @176 (each relative to its joint), then 3 weights @248.
    Encode.bytes(Bytes.fromF32le([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                                  0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])),
    Encode.bytes(Bytes.fromF32le([1.0, 1.0, 0.0]))
  ]))

// One torso (joint 0, no fixed joints) and one limb: joint 1, 2 up from 0.
let characterSkeleton =
  Encode.run(Encode.sequence([
    Encode.i32le(1.0), Encode.u32le(1.0), Encode.u32le(1.0),
    Encode.u32le(0.0), Encode.i32le(-1.0), Encode.i32le(0.0), Encode.zeros(256.0),
    Encode.u32le(0.0), Encode.i32le(0.0), Encode.i32le(1.0), Encode.u16le(0.0),
    Encode.bytes(Bytes.fromU16le(List.append(List.range(15.0) |> List.map((i) => 0.0), [1.0]))),
    Encode.bytes(Bytes.fromF32le(List.append(List.range(45.0) |> List.map((i) => 0.0), [0.0, 0.0, 1.0]))),
    Encode.bytes(Bytes.fromF32le(List.append(List.range(15.0) |> List.map((i) => 0.0), [2.0]))),
    Encode.zeros(4.0)
  ]))

let character = Lgmm.rest(characterMesh, characterSkeleton)

expect Result.map(Mesh.triangleCount, character) == Result.Ok(2.0)
// Joint 1's vertices sit 2 up; nothing goes below joint 0.
expect (character |> Result.map((m) => [Vec3.z(Mesh.bounds(m).min), Vec3.z(Mesh.bounds(m).max)])) == Result.Ok([0.0, 2.0])
expect (Decode.run(Cal.skeleton, characterSkeleton) |> Result.map((s: Cal.skeleton) => List.map((b: Cal.bone) => [b.joint, b.parent], s.bones))) == Result.Ok([[0.0, -1.0], [1.0, 0.0]])

// A two-frame clip driving joint 1 from identity to 180° about Z, its root
// rising from 0 to 1: halfway is 90° and 0.5 up.
let twoFrames: Motion.clip = {
  motion: { frames: 2.0, fps: 2.0, joints: [0.0, 1.0] },
  root: [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
  tracks: [[[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]]]
}

let halfway = Motion.sample(twoFrames, 0.25)

expect Affine.offset(halfway.root) == [0.0, 0.0, 0.5]
// The stored quaternion's conjugate: +90° stored → -90° applied, so +X → -Y.
expect (Affine.apply(halfway.animate(1.0), [1.0, 0.0, 0.0]) |> List.map((x) => Math.round(x * 1000.0) / 1000.0)) == [0.0, -1.0, 0.0]
expect Affine.apply(halfway.animate(0.0), [1.0, 0.0, 0.0]) == [1.0, 0.0, 0.0]

// An all-zero quaternion is identity, and still interpolates: from it to 180°
// about Z, halfway is 90°.
let fromZero = Motion.sample({ twoFrames with tracks: [[[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]]] }, 0.25)
expect (Affine.apply(fromZero.animate(1.0), [1.0, 0.0, 0.0]) |> List.map((x) => Math.round(x * 1000.0) / 1000.0)) == [0.0, -1.0, 0.0]

// Blending, posed: `twoFrames` turns joint 1 by -90° about Z and lifts the root
// 0.5. Vertex 4 (weight 1) follows joint 1 to (0, -1, 2.5); vertex 5 (weight 0)
// keeps the parent's rotation at (0, 1, 2.5). Reversed weights would give y 0..1.
let blended =
  Decode.run(Lgmm.model, characterMesh)
    |> Result.bind((m: Lgmm.model) =>
      Decode.run(Cal.skeleton, characterSkeleton)
        |> Result.map((s: Cal.skeleton) =>
          let p = Motion.sample(twoFrames, 0.25) in
          Lgmm.poseRig(Lgmm.rig(m, s), p.root, p.animate)))

expect (blended |> Result.map((m) => [Vec3.y(Mesh.bounds(m).min), Vec3.y(Mesh.bounds(m).max), Vec3.z(Mesh.bounds(m).max)]))
  == Result.Ok([-1.0, 1.0, 2.5])

// A synthetic motion database: a chunk file holding MotDBase with two motions —
// "junk" (one frame event, so its record is longer) and "TestClip" (whose name
// field has junk after its NUL) — and a clip for TestClip with a gap before its
// tracks. Track 1 drives joint 1.
let name16 = (s: string) =>
  Encode.sequence([Encode.u8(43.0), Encode.latin1(s), Encode.zeros(16.0 - Text.length(s))])

let record = (frames: float, joints: List<float>, events: float) =>
  Encode.sequence([
    Encode.zeros(8.0), Encode.f32le(frames), Encode.i32le(30.0), Encode.zeros(80.0),
    Encode.i32le(List.length(joints)), Encode.zeros(4.0), Encode.i32le(events), Encode.zeros(4.0),
    Encode.list((j) => Encode.sequence([Encode.zeros(4.0), Encode.u32le(j), Encode.zeros(4.0)]), joints),
    Encode.zeros(8.0 * events)
  ])

let motionData =
  Encode.sequence([
    Encode.i32le(1.0), Encode.i32le(0.0), Encode.u32le(2.0),
    name16("junk"),
    Encode.sequence([Encode.u8(43.0), Encode.latin1("TestClip"), Encode.zeros(1.0), Encode.latin1("xyz"), Encode.zeros(4.0)]),
    Encode.u32le(2.0), Encode.zeros(48.0),           // MotStuff
    Encode.u32le(2.0), record(2.0, [8.0], 1.0), record(3.0, [8.0, 1.0], 0.0)
  ])

let database = (data: Encode.t) =>
  let body = Encode.run(data) in
  Encode.run(Encode.sequence([
    Encode.u32le(272.0 + 24.0 + Bytes.length(body)), Encode.zeros(268.0),
    Encode.latin1("MotDBase"), Encode.zeros(16.0), Encode.bytes(body),
    Encode.u32le(1.0), Encode.latin1("MotDBase"), Encode.zeros(4.0), Encode.u32le(272.0), Encode.u32le(Bytes.length(body))
  ]))

// Track 0 (root): z 0, 1, 2; track 1: identity, 90°, 180° about Z.
let testClip =
  Encode.run(Encode.sequence([
    Encode.u32le(2.0), Encode.u32le(16.0), Encode.u32le(52.0), Encode.zeros(4.0),
    Encode.bytes(Bytes.fromF32le([0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0])),
    Encode.bytes(Bytes.fromF32le([1.0, 0.0, 0.0, 0.0, 0.70710678, 0.0, 0.0, 0.70710678, 0.0, 0.0, 0.0, 1.0]))
  ]))

let decodedClip = Motion.clip(database(motionData), testClip, "testclip")

expect (decodedClip |> Result.map((c: Motion.clip) => [c.motion.frames, c.motion.fps, List.length(c.tracks)])) == Result.Ok([3.0, 30.0, 1.0])
// Halfway through the last frame (2.5 of 3) wraps towards frame 0: root z 1, and
// 180° slerped halfway back to identity is 90° (applied: +X → -Y).
expect (decodedClip |> Result.map((c: Motion.clip) =>
  let p = Motion.sample(c, 2.5 / 30.0) in
  List.append(Affine.apply(p.animate(1.0), [1.0, 0.0, 0.0]) |> List.map((x) => Math.round(x * 1000.0) / 1000.0),
    [Affine.n(2.0, Affine.offset(p.root))])))
  == Result.Ok([1.0, 0.0, -1.0, 0.0])
expect Result.isError(Motion.clip(database(motionData), testClip, "missing"))
