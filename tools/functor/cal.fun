// Cal — System Shock 2 character skeletons (".cal"): torsos with fixed joints,
// and limbs chained from an attachment joint. Each becomes a bone (joint id,
// parent joint id, transform relative to the parent); a joint listed twice
// keeps its first bone, as in the game. Native axes, Z up, feet.

type bone = { joint: float, parent: float, local: Affine.t }

type torso = { joint: float, parent: float, fixed: List<float>, offsets: List<List<float>> }
type limb = { attachment: float, joints: List<float>, directions: List<List<float>>, lengths: List<float> }
type skeleton = { bones: List<bone> }

let vec3 = Decode.list(3.0, Decode.f32le)

// A 268-byte torso.
let torso =
  Decode.map4(
    (joint, parent, count, rest): torso =>
      match rest with
      | (fixed, offsets) => { joint: joint, parent: parent, fixed: List.take(count, fixed), offsets: List.take(count, offsets) },
    Decode.field("joint", Decode.u32le),
    Decode.field("parent", Decode.i32le),
    Decode.field("fixedCount", Decode.i32le),
    Decode.map2((fixed, offsets) => (fixed, offsets),
      Decode.field("fixed", Decode.list(16.0, Decode.u32le)),
      Decode.field("offsets", Decode.list(16.0, vec3))))

// A 302-byte limb.
let limb =
  Decode.map4(
    (count, attachment, joints, rest): limb =>
      match rest with
      | (directions, lengths) =>
        { attachment: attachment, joints: List.take(count, joints),
          directions: List.take(count, directions), lengths: List.take(count, lengths) },
    Decode.skipThen(8.0, Decode.field("segments", Decode.i32le)),
    Decode.field("attachment", Decode.u16le),
    Decode.field("joints", Decode.list(16.0, Decode.u16le)),
    Decode.map2((directions, lengths) => (directions, lengths),
      Decode.field("directions", Decode.list(16.0, vec3)),
      Decode.field("lengths", Decode.list(16.0, Decode.f32le))))

let at = (k: float, xs: List<'a>, fallback: 'a): 'a => Option.defaultValue(fallback, List.nth(k, xs))

// The bones in the game's order: each torso, then its fixed joints, then each
// limb's chain. The root torso's own transform is identity (Dark's model space;
// shock2quest turns it 90° for its renderer).
let bonesOf = (torsos: List<torso>, limbs: List<limb>): List<bone> =>
  let fromTorsos =
    List.concatMap((t: torso) =>
      let parentJoint =
        if t.parent < 0.0 then -1.0
        else Option.defaultValue(-1.0, List.nth(t.parent, torsos) |> Option.map((p: torso) => p.joint)) in
      List.append(
        List.indexedMap((k, j) =>
          let o = at(k, t.offsets, [0.0, 0.0, 0.0]) in
          { joint: j, parent: t.joint, local: Affine.translation(Affine.n(0.0, o), Affine.n(1.0, o), Affine.n(2.0, o)) }, t.fixed),
        [{ joint: t.joint, parent: parentJoint, local: Affine.identity }]), torsos) in
  let fromLimbs =
    List.concatMap((l: limb) =>
      List.indexedMap((k, j) =>
        let d = Affine.scale(at(k, l.lengths, 0.0), at(k, l.directions, [0.0, 0.0, 0.0])) in
        let parent = if k == 0.0 then l.attachment else at(k - 1.0, l.joints, l.attachment) in
        { joint: j, parent: parent, local: Affine.translation(Affine.n(0.0, d), Affine.n(1.0, d), Affine.n(2.0, d)) }, l.joints), limbs) in
  List.append(fromLimbs, fromTorsos)

let skeleton =
  Decode.map2(
    (torsoCount, limbCount) => (torsoCount, limbCount),
    Decode.skipThen(4.0, Decode.field("torsos", Decode.u32le)),
    Decode.field("limbs", Decode.u32le))
    |> Decode.andThen((counts) =>
      match counts with
      | (torsoCount, limbCount) =>
        Decode.map2(
          (torsos, limbs): skeleton => { bones: bonesOf(torsos, limbs) },
          Decode.field("torsos", Decode.list(torsoCount, torso)),
          Decode.field("limbs", Decode.list(limbCount, limb))))

// The first bone for a joint, as the game looks bones up.
let bone = (s: skeleton, joint: float): Option.t<bone> => List.find((b: bone) => b.joint == joint, s.bones)

// Each joint's bones from the root down (empty for joints the skeleton lacks):
// computed once, then composed per frame by `globals`.
let chains = (s: skeleton): List<List<bone>> =>
  List.range(40.0)
    |> List.map((joint) =>
      let walked =
        List.range(List.length(s.bones))
          |> List.fold((walk, step) =>
            match walk with
            | (at, chain) =>
              match bone(s, at) with
              | Option.Some(b) =>
                if List.length(chain) > List.length(s.bones) then walk else (b.parent, List.append(chain, [b]))
              | Option.None => walk, (joint, [])) in
      match walked with
      | (_, chain) => chain)

// Every joint's transform in the model for one pose: `root` (the clip's
// vertical bob), then each bone from the root down, followed by
// `animate(joint)` (identity at rest).
let globals = (chainsOf: List<List<bone>>, root: Affine.t, animate: (float) => Affine.t): List<Affine.t> =>
  List.map((chain) =>
    List.fold((m, b: bone) => Affine.compose(m, Affine.compose(b.local, animate(b.joint))), root, chain), chainsOf)
