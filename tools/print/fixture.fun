// A synthetic LGMD file, written with `Encode`, to test `Lgmd` without game
// files: a unit cube whose top four vertices are a child sub-object turned 90°
// about Z and lifted 2.

let header = (version: float) =>
  Encode.sequence([
    Encode.latin1("LGMD"), Encode.u32le(version),
    Encode.zeros(52.0),                                    // name, radii, bounds
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

let cube = (version: float): Bytes.t =>
  Encode.run(Encode.sequence([
    header(version),
    subObject(List.range(12.0) |> List.map((i) => 0.0), 1.0, -1.0, 0.0, 4.0),
    subObject([0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0], -1.0, -1.0, 4.0, 4.0),
    Encode.bytes(Bytes.fromF32le(corners)),
    Encode.sequence([
      quad([0.0, 2.0, 3.0, 1.0]), quad([4.0, 5.0, 7.0, 6.0]), quad([0.0, 1.0, 5.0, 4.0]),
      quad([2.0, 6.0, 7.0, 3.0]), quad([0.0, 4.0, 6.0, 2.0]), quad([1.0, 3.0, 7.0, 5.0])
    ])
  ]))

let decoded = Lgmd.mesh(cube(4.0))

expect Bytes.length(cube(4.0)) == 578.0
expect Result.map(Mesh.triangleCount, decoded) == Result.Ok(12.0)
// The top turned 90° (x' = -y) and lifted to z = 1 + 2.
expect (decoded |> Result.map((m) => Vec3.x(Mesh.bounds(m).min))) == Result.Ok(-1.0)
expect (decoded |> Result.map((m) => Vec3.z(Mesh.bounds(m).max))) == Result.Ok(3.0)
expect (Decode.run(Lgmd.model, cube(4.0)) |> Result.map((m: Lgmd.model) => Lgmd.parents(m.parts))) == Result.Ok([-1.0, 0.0])

let failure = (data: Bytes.t): string =>
  match Lgmd.mesh(data) with
  | Result.Error(e) => Binary.describe(e)
  | Result.Ok(_) => "decoded"

expect failure(cube(6.0)) == "at byte 4: LGMD version 6 isn't supported (3 and 4 are)"
expect failure(Bytes.fromLatin1("LGMM and more")) == "header at byte 4: not an LGMD object mesh (starts with \"LGMM\")"
expect Text.contains("polygons", failure(Binary.require(Bytes.slice(0.0, 500.0, cube(4.0)))))
