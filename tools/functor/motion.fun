// Motion — System Shock 2 animation clips: `motiondb.bin` (each motion's
// frame count, rate and which joint each track drives) and its `.mc` clip
// (the root's position and every joint's rotation, per frame).

type motion = { frames: float, fps: float, joints: List<float> }
type clip = { motion: motion, root: List<List<float>>, tracks: List<List<List<float>>> }
// A sampled pose: the root's vertical bob, and each joint's rotation.
type pose = { root: Affine.t, animate: (float) => Affine.t }

let nul = Binary.require(Bytes.latin1(0.0, 1.0, Bytes.zeros(1.0)))

// Fixed-size names stop at the first NUL (bytes after it can be junk).
let name = (length: float) =>
  Decode.latin1(length) |> Decode.map((s) => Option.defaultValue("", List.head(Text.split(nul, s))))

// One variable-size MpsMotion record.
let mps =
  Decode.map4(
    (frames, fps, components, flags) => (frames, fps, components, flags),
    Decode.skipThen(8.0, Decode.f32le), Decode.i32le,
    Decode.skipThen(80.0, Decode.i32le), Decode.skipThen(4.0, Decode.i32le))
    |> Decode.andThen((header) =>
      match header with
      | (frames, fps, components, flags) =>
        Decode.skipThen(4.0,
          Decode.list(components, Decode.skipThen(4.0, Decode.u32le) |> Decode.andThen((j) => Decode.skipThen(4.0, Decode.succeed(j))))
            |> Decode.andThen((joints) =>
              Decode.skipThen(8.0 * flags, Decode.succeed({ frames: Math.floor(frames), fps: fps, joints: joints })))))

// The MotDBase chunk's motion names and records (a chunk file: a table of
// contents names each chunk's offset; its data follows a 24-byte header).
let database =
  Decode.u32le
    |> Decode.andThen((toc) =>
      Decode.at(toc, Decode.u32le |> Decode.andThen((n) => Decode.list(n, Decode.map2((chunk, offset) => (chunk, offset), name(12.0), Decode.u32le) |> Decode.andThen((e) => Decode.skipThen(4.0, Decode.succeed(e)))))))
    |> Decode.andThen((chunks) =>
      match List.find((c) => match c with | (chunk, _) => chunk == "MotDBase", chunks) with
      | Option.None => Decode.fail("no MotDBase chunk")
      | Option.Some(found) =>
        match found with
        | (_, offset) =>
          Decode.at(offset + 24.0,
            Decode.skipThen(8.0, Decode.u32le)
              |> Decode.andThen((size) =>
                Decode.field("names",
                  Decode.list(size, Decode.u8 |> Decode.andThen((marker) => if marker == 43.0 then name(16.0) else Decode.succeed(""))))
                  |> Decode.andThen((names) =>
                    Decode.u32le
                      |> Decode.andThen((stuff) =>
                        Decode.skipThen(24.0 * stuff, Decode.u32le)
                          |> Decode.andThen((count) =>
                            Decode.field("motions", Decode.list(count, mps))
                              |> Decode.map((motions) => List.zip(motions, names))))))))

// A motion's record by name (case-insensitive).
let find = (db: Bytes.t, motionName: string): Result.t<motion, Binary.error> =>
  Decode.run(database, db)
    |> Result.bind((entries) =>
      match List.find((e) => match e with | (n, _) => Text.toLower(n) == Text.toLower(motionName), entries) with
      | Option.Some(e) => (match e with | (_, m) => Result.Ok(m))
      | Option.None => Result.Error({ offset: 0.0, path: [], message: Text.join("", ["no motion named ", motionName]) }))

// A clip's tracks: the root's positions, then each joint's (w, x, y, z) per frame.
let clipOf = (m: motion): Decode.t<clip> =>
  Decode.u32le
    |> Decode.andThen((n) => Decode.list(n, Decode.u32le))
    |> Decode.andThen((offsets) =>
      Decode.map2(
        (root, tracks): clip => { motion: m, root: root, tracks: tracks },
        Decode.field("root", Decode.at(Option.defaultValue(0.0, List.head(offsets)), Decode.list(m.frames, Decode.list(3.0, Decode.f32le)))),
        Decode.field("tracks", Decode.sequence(List.map((o) => Decode.at(o, Decode.list(m.frames, Decode.list(4.0, Decode.f32le))), List.drop(1.0, offsets))))))

let clip = (db: Bytes.t, mc: Bytes.t, motionName: string): Result.t<clip, Binary.error> =>
  find(db, motionName) |> Result.bind((m: motion) => Decode.run(clipOf(m), mc))

let dot = (a: List<float>, b: List<float>): float =>
  List.zip(b, a) |> List.fold((s, p) => match p with | (x, y) => s + x * y, 0.0)

// Shortest-path slerp of (w, x, y, z) quaternions, normalized.
let slerp = (a: List<float>, b0: List<float>, t: float): List<float> =>
  let d0 = dot(a, b0) in
  let b = if d0 < 0.0 then List.map((x) => 0.0 - x, b0) else b0 in
  let d = Math.abs(d0) in
  let mix =
    if d > 0.9995
    then List.zip(b, a) |> List.map((p) => match p with | (x, y) => x + (y - x) * t)
    else
      let theta = Math.acos(Math.min(1.0, d)) in
      let s = Math.sin(theta) in
      let wa = Math.sin((1.0 - t) * theta) / s in
      let wb = Math.sin(t * theta) / s in
      List.zip(b, a) |> List.map((p) => match p with | (x, y) => wa * x + wb * y) in
  let len = Math.sqrt(dot(mix, mix)) in
  if len == 0.0 then mix else List.map((x) => x / len, mix)

// A stored quaternion, with an all-zero one meaning identity (as the game reads it).
let unit = (q: List<float>): List<float> => if dot(q, q) <= 0.0000001 then [1.0, 0.0, 0.0, 0.0] else q

// The rotation a stored quaternion applies: its conjugate's.
let rotation = (q: List<float>): Affine.t =>
  Affine.quaternion(Affine.n(0.0, q), 0.0 - Affine.n(1.0, q), 0.0 - Affine.n(2.0, q), 0.0 - Affine.n(3.0, q))

// The pose at `seconds`, looping: frames interpolate (the last towards the
// first); the root bobs vertically only; a joint takes its last track.
let sample = (c: clip, seconds: float): pose =>
  let n = c.motion.frames in
  let p = Math.mod(seconds * c.motion.fps, n) in
  let f = Math.floor(p) in
  let a = p - f in
  let next = if f + 1.0 < n then f + 1.0 else 0.0 in
  let rootZ = (k: float) => Affine.n(2.0, Option.defaultValue([0.0, 0.0, 0.0], List.nth(k, c.root))) in
  let z = rootZ(f) + (rootZ(next) - rootZ(f)) * a in
  let rotations =
    List.map((track) =>
      let q0 = unit(Option.defaultValue([0.0, 0.0, 0.0, 0.0], List.nth(f, track))) in
      let q1 = unit(Option.defaultValue(q0, List.nth(next, track))) in
      rotation(slerp(q0, q1, a)), c.tracks) in
  // Track k (k ≥ 1) drives joint `joints[k]`; later tracks win.
  let driven = List.zip(rotations, List.drop(1.0, c.motion.joints)) in
  let perJoint =
    List.range(40.0)
      |> List.map((j) => List.fold((m, pair) => match pair with | (joint, r) => if joint == j then r else m, Affine.identity, driven)) in
  { root: Affine.translation(0.0, 0.0, z),
    animate: (j) => Option.defaultValue(Affine.identity, List.nth(j, perJoint)) }
