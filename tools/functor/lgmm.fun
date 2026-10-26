// Lgmm — System Shock 2 character meshes (".bin" files that start with
// "LGMM"), skinned on a `Cal` skeleton. Vertices are stored relative to their
// joint; each follows it rigidly, or blends towards its parent's rotation with
// a weight ("stretchy" segments, at elbows and knees).

type counts = {
  segments: float, smatsegs: float, triangles: float, vertices: float, weights: float
}
type offsets = { segmentsAt: float, smatsegsAt: float, trianglesAt: float, verticesAt: float, weightsAt: float }
type segment = { joint: float, stretchy: bool }
type run = { count: float, start: float, weightAt: float, segment: float }
// How one vertex follows the skeleton: its joint, and its weight when it blends.
type binding = { joint: float, blend: Option.t<float> }
type model = {
  positions: List<List<float>>, triangles: List<List<float>>,
  segments: List<segment>, runs: List<run>, weights: List<float>
}

let at8 = (offset: float) => Decode.at(offset, Decode.u8)
let at16 = (offset: float) => Decode.at(offset, Decode.u16le)
let at32 = (offset: float) => Decode.at(offset, Decode.u32le)

let counts =
  Decode.map5(
    (segments, smatsegs, triangles, vertices, weights): counts =>
      { segments: segments, smatsegs: smatsegs, triangles: triangles, vertices: vertices, weights: weights },
    Decode.field("segments", at8(21.0)), Decode.field("smatsegs", at8(23.0)),
    Decode.field("triangles", at16(24.0)), Decode.field("vertices", at16(26.0)),
    Decode.field("weights", at16(28.0)))

let offsets =
  Decode.map5(
    (segmentsAt, smatsegsAt, trianglesAt, verticesAt, weightsAt): offsets =>
      { segmentsAt: segmentsAt, smatsegsAt: smatsegsAt, trianglesAt: trianglesAt,
        verticesAt: verticesAt, weightsAt: weightsAt },
    Decode.field("segmentsAt", at32(36.0)), Decode.field("smatsegsAt", at32(44.0)),
    Decode.field("trianglesAt", at32(48.0)), Decode.field("verticesAt", at32(56.0)),
    Decode.field("weightsAt", at32(64.0)))

// A 20-byte segment: its joint, and bit 0 of its flags (stretchy).
let segment =
  Decode.map2(
    (joint, flags): segment => { joint: joint, stretchy: Math.mod(flags, 2.0) == 1.0 },
    Decode.skipThen(4.0, Decode.i8),
    Decode.skipThen(2.0, Decode.u8))
    |> Decode.andThen((s: segment) => Decode.skipThen(12.0, Decode.succeed(s)))

// A 16-byte run of vertices ("smatseg") bound to one segment.
let run =
  Decode.map4(
    (count, start, weightAt, seg): run => { count: count, start: start, weightAt: weightAt, segment: seg },
    Decode.skipThen(4.0, Decode.i16le), Decode.i16le, Decode.u16le,
    Decode.skipThen(4.0, Decode.i16le))

// A 16-byte triangle's three vertex indices.
let triangle = Decode.list(3.0, Decode.u16le) |> Decode.andThen((ids) => Decode.skipThen(10.0, Decode.succeed(ids)))

let model =
  Decode.field("magic", Decode.latin1(4.0))
    |> Decode.andThen((magic: string) =>
      if magic != "LGMM"
      then Decode.fail(Text.join("", ["not an LGMM character mesh (starts with \"", magic, "\")"]))
      else
        Decode.map2((c, o) => (c, o), counts, offsets)
          |> Decode.andThen((header) =>
            match header with
            | (c, o) =>
              Decode.map5(
                (positions, triangles, segments, runs, weights): model =>
                  { positions: positions, triangles: triangles, segments: segments, runs: runs, weights: weights },
                Decode.field("vertices", Decode.at(o.verticesAt, Decode.list(c.vertices, Decode.list(3.0, Decode.f32le)))),
                Decode.field("triangles", Decode.at(o.trianglesAt, Decode.list(c.triangles, triangle))),
                Decode.field("segments", Decode.at(o.segmentsAt, Decode.list(c.segments, segment))),
                Decode.field("runs", Decode.at(o.smatsegsAt, Decode.list(c.smatsegs, run))),
                Decode.field("weights", Decode.at(o.weightsAt, Decode.list(c.weights, Decode.f32le))))))

let rigid = (joint: float): binding => { joint: joint, blend: Option.None }

// Each vertex's binding, from the run that owns it (joint 0 when none does).
let bindings = (m: model): List<binding> =>
  let byRun =
    List.map((r: run) =>
      let seg = Option.defaultValue({ joint: 0.0, stretchy: false }, List.nth(r.segment, m.segments)) in
      let joint = if seg.joint < 0.0 || seg.joint >= 40.0 then 0.0 else seg.joint in
      let blends = seg.stretchy && r.weightAt + r.count <= List.length(m.weights) in
      (r, joint, blends), m.runs) in
  List.indexedMap((v, p) =>
    match List.find((entry) => match entry with | (r, _, _) => v >= r.start && v < r.start + r.count, byRun) with
    | Option.Some(entry) =>
      (match entry with
       | (r, joint, blends) =>
         if blends
         then { joint: joint, blend: Option.Some(Math.clamp(0.0, 1.0, Option.defaultValue(1.0, List.nth(r.weightAt + v - r.start, m.weights)))) }
         else rigid(joint))
    | Option.None => rigid(0.0), m.positions)

// The mesh posed on `skel`, each joint's motion given by `animate` (identity at
// rest): rigid vertices move with their joint; blending ones mix its rotation
// with the parent's, pivoting at the joint.
let pose = (m: model, skel: Cal.skeleton, animate: (float) => Affine.t): Mesh.t =>
  let joints = List.range(40.0) |> List.map((j) => Cal.globalOf(skel, animate, j)) in
  let global = (j: float) => Option.defaultValue(Affine.identity, List.nth(j, joints)) in
  let parentOf = (j: float) =>
    match Cal.bone(skel, j) with
    | Option.Some(b) => if b.parent < 0.0 then j else b.parent
    | Option.None => j in
  let posed =
    List.map((pair) =>
      match pair with
      | (b, v) =>
        match b.blend with
        | Option.None => Affine.apply(global(b.joint), v)
        | Option.Some(w) =>
          Affine.add(Affine.offset(global(b.joint)),
            Affine.add(Affine.scale(w, Affine.rotate(global(b.joint), v)),
              Affine.scale(1.0 - w, Affine.rotate(global(parentOf(b.joint)), v)))),
      List.zip(m.positions, bindings(m))) in
  Mesh.fromPolygons(Bytes.fromF32le(List.flatten(posed)), m.triangles)

// A character's mesh at rest (a T-pose), or where decoding failed.
let rest = (mesh: Bytes.t, cal: Bytes.t): Result.t<Mesh.t, Binary.error> =>
  Decode.run(model, mesh)
    |> Result.bind((m: model) =>
      Decode.run(Cal.skeleton, cal) |> Result.map((s: Cal.skeleton) => pose(m, s, (j) => Affine.identity)))
