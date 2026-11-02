// walk — the `walk` entry: medsci1's world rep, textured and lightmapped,
// walked with a capsule player on `Physics.character`, and its doors: E (or
// the right trigger) frobs what's ahead, and a StdDoor door slides open.
//
//   functor -d tools/functor run native --entry walk
//   functor -d tools/functor run vr --entry walk
//
// VR: the left thumbstick walks and strafes relative to where the head looks;
// the right one snap-turns 30°. Desktop: W/S walk, A/D strafe, Left/Right
// turn, Up/Down or a left-drag looks around.
//
// The player is shock2quest's: a 6 ft capsule 1.2 ft in radius, 2 ft steps,
// walkable up to a 0.7 normal (45.6°), snapping down a step, eyes 2.6 ft above
// the centre, 11 ft/s forward, 0.7× sideways and 0.5× back.
//
// Objects live in the model (`Door.world`): a frob is a ray from the eye to
// a door's body, sent to the door as a message; the queue is drained once a
// step and the moving doors advance, so seeking a frame restores them.

let loaded = Level.loaded
let level = Level.level

let ft = World.metresPerFoot
let radius = 1.2 * ft
let halfHeight = 3.0 * ft
let offset = 0.1 * ft
let eyeAboveCentre = 2.6 * ft
let runSpeed = 11.0 * ft
let turnRate = 2.0
let gravity = 32.0 * ft

let playerTag = Physics.tag("player")
// How far a frob reaches (metres).
let reach = 3.0
let levelTag = Physics.tag("level")

// The player factory, in scene space. Its heading turns from Dark's +X
// (scene +X) counter-clockwise; camera yaw 0 looks down +Z.
let marker = Skinned.vec(Affine.scale(ft, level.start.position))
let startYaw = level.start.heading + Math.pi / 2.0

let physics = (model) =>
  Physics.scene(Vec3.make(0.0, 0.0 - 9.81, 0.0), [
    Physics.fixed(levelTag, level.shape),
    Physics.character(playerTag, Physics.capsule(halfHeight - radius, radius),
                      { stepHeight: 2.0 * ft, maxSlope: Angle.radians(Math.acos(0.7)), snap: 2.0 * ft, offset: offset })
      |> Physics.at(marker),
    Physics.memo("doors", Objview.still(model.objects), (s) => Objview.placed(Level.doors, s, Objview.bodyAt)),
  ] |> List.append(Objview.moving(Level.doors, model.objects, Objview.bodyAt)))

// `placed`: the player stands on the floor under the marker. Until then the
// level isn't in the physics world yet (moves see the last step's bodies),
// so the player holds still and `tick` looks for the floor each step.
let init = { yaw: startYaw, pitch: 0.0, vy: 0.0, walk: 0.0, strafe: 0.0, turn: 0.0, look: 0.0,
             headYaw: 0.0, headPitch: 0.0, snap: 0.0, snapHeld: false, dragX: -1.0, dragY: -1.0, placed: false,
             objects: Level.world, frob: false, frobHeld: false }

let held = (k, snap) => snap.heldKeys |> List.any((h) => h == k)
let axis = (neg, pos, snap) =>
  (if held(pos, snap) then 1.0 else 0.0) - (if held(neg, snap) then 1.0 else 0.0)
let deadzone = (v) => if Math.abs(v) < 0.15 then 0.0 else v
let unit = (v) => Math.clamp(-1.0, 1.0, v)

// The head's turn about +Y from the rig's forward (-Z), left positive: the
// rotated +Z axis's heading. Looking nearly straight up or down it has none,
// so `previous` stands.
// The head's pitch (up positive): the height of its forward (-Z) axis.
let pitchOf = (q: Input.quaternion) => Math.asin(Math.clamp(-1.0, 1.0, 0.0 - Affine.quaternion(q.w, q.x, q.y, q.z).f))

let yawOf = (previous: float, q: Input.quaternion) =>
  let m = Affine.quaternion(q.w, q.x, q.y, q.z) in
  if m.c * m.c + m.i * m.i < 0.04 then previous else Math.atan2(m.c, m.i)

let sampledInput = (model, snap: Input.snapshot) =>
  let m = snap.mouse in
  let dragging = m.buttons.left && model.dragX >= 0.0 in
  let dx = if dragging then m.x - model.dragX else 0.0 in
  let dy = if dragging then m.y - model.dragY else 0.0 in
  let keys = { model with
    walk: unit(axis(Key.S, Key.W, snap)), strafe: unit(axis(Key.A, Key.D, snap)),
    turn: axis(Key.Right, Key.Left, snap) - dx * 0.3, look: axis(Key.Down, Key.Up, snap) - dy * 0.3,
    frob: List.any((k) => k == Key.E, snap.pressedKeys),
    dragX: if m.buttons.left then m.x else -1.0, dragY: if m.buttons.left then m.y else -1.0 } in
  match snap.xr with
  | Option.None => { keys with headYaw: 0.0, headPitch: 0.0, snap: 0.0 }
  | Option.Some(xr) =>
    let stick = xr.left.thumbstick in
    let flick = xr.right.thumbstick.x in
    // One 30° turn per flick: it re-arms once the stick returns near centre.
    let turnNow = Math.abs(flick) > 0.7 && not model.snapHeld in
    { keys with
      walk: unit(keys.walk + deadzone(stick.y)), strafe: unit(keys.strafe + deadzone(stick.x)),
      headYaw: Option.defaultValue(0.0, Option.map((h: Input.pose) => yawOf(model.headYaw, h.orientation), xr.head)),
      headPitch: Option.defaultValue(0.0, Option.map((h: Input.pose) => pitchOf(h.orientation), xr.head)),
      snap: if turnNow then 0.0 - Math.sign(flick) * Math.pi / 6.0 else 0.0,
      snapHeld: Math.abs(flick) > 0.3 && (model.snapHeld || turnNow),
      // One frob per pull, like the snap turn: it re-arms once released.
      frob: keys.frob || (xr.right.trigger > 0.7 && not model.frobHeld),
      frobHeld: xr.right.trigger > 0.3 && (model.frobHeld || xr.right.trigger > 0.7) }

// The door a frob reaches: the first body on a ray from the eye along the
// view (the head's on a headset, from the eye's rest point), if it is a door.
let frobbed = (model): List<float> =>
  let heading = model.yaw + model.headYaw in
  let pitch = model.pitch + model.headPitch in
  let dir = Vec3.make(Math.sin(heading) * Math.cos(pitch), Math.sin(pitch), Math.cos(heading) * Math.cos(pitch)) in
  let hit = Physics.castExcluding(playerTag, eye(model), dir, reach) in
  if not hit.hit then []
  else Level.doors |> List.filter((v: Objview.view) => Objview.tagOf(v.id) == hit.tag) |> List.map((v: Objview.view) => v.id)

// A door's sounds, from SS2's own (`ss2/snd/Devices/`), where the door is.
let soundFile = (name: string): Option.t<string> =>
  if name == "opening" then Option.Some("DOOR1OP.WAV")
  else if name == "closing" then Option.Some("DOOR1CL.WAV")
  else if name == "closed" then Option.Some("DOOR1SLM.WAV")
  else if name == "hackfail" then Option.Some("HACKFAIL.WAV")
  else Option.None

let play = (w: Door.world, s: Door.sound): List<Effect.t> =>
  match soundFile(s.name) with
  | Option.Some(file) =>
    Door.position(w, s.obj) |> Option.toList
      |> List.map((at) => Effect.playAt(Asset.sound(Text.join("", ["ss2/snd/Devices/", file])), Objview.scenePos(at)))
  | Option.None => []

let tick = (model, dt, tts) =>
  if not model.placed then
    let down = Physics.castExcluding(playerTag, marker, Vec3.make(0.0, -1.0, 0.0), 20.0) in
    if down.hit then
      let feet = Vec3.make(Vec3.x(marker), down.y + halfHeight + offset + 0.01, Vec3.z(marker)) in
      ({ model with placed: true, frob: false }, Physics.teleport(playerTag, feet))
    else ({ model with frob: false }, Effect.none())
  else
    let grounded = Physics.grounded(playerTag) in
    let yaw = model.yaw + model.turn * turnRate * dt + model.snap in
    let pitch = Math.clamp(-1.4, 1.4, model.pitch + model.look * turnRate * dt) in
    // Walk where the head looks (on a headset), else where the body faces.
    let heading = yaw + model.headYaw in
    let forward = runSpeed * model.walk * (if model.walk < 0.0 then 0.5 else 1.0) in
    let side = runSpeed * 0.7 * model.strafe in
    let vx = forward * Math.sin(heading) - side * Math.cos(heading) in
    let vz = forward * Math.cos(heading) + side * Math.sin(heading) in
    let vy = (if grounded && model.vy <= 0.0 then 0.0 else model.vy) - gravity * dt in
    let objects = Door.frame(dt, if model.frob then frobbed(model) else [], model.objects) in
    ({ model with yaw: yaw, pitch: pitch, vy: vy, snap: 0.0, objects: objects, frob: false },
     Effect.batch(List.append(List.concatMap((s) => play(objects, s), objects.sounds),
                              [Physics.move(playerTag, Vec3.make(vx * dt, vy * dt, vz * dt))])))

let eye = (model) =>
  let p = Physics.position(playerTag) in
  Vec3.make(p.x, p.y + eyeAboveCentre, p.z)

let world = Scene.modelOf(level.model)

// Light for the objects only (the world's is in its lightmaps): the
// mission's ambient level and a soft key light, both marked baked.
let objectLights = [
  Light.ambient(Color.rgb(level.ambient + 0.25, level.ambient + 0.25, level.ambient + 0.3)) |> Light.baked,
  Light.directional(Vec3.make(0.3, -1.0, 0.4), Color.rgb(0.9, 0.9, 1.0), 0.6) |> Light.baked,
]

let draw = (model, tts) =>
  Frame.createLit(
    Camera3D.firstPerson(eye(model), Angle.radians(model.yaw), Angle.radians(model.pitch), Angle.degrees(75.0))
      |> Camera3D.clip(0.05, 400.0),
    Scene.group(List.flatten([[world, Scene.memo("doors", Objview.still(model.objects), (s) => Scene.group(Objview.placed(Level.doors, s, Objview.nodeAt)))],
                              Objview.moving(Level.doors, model.objects, Objview.nodeAt)])), objectLights)

let ui = (model) =>
  let p = Physics.position(playerTag) in
  Ui.column([
    Ui.text(match loaded with | Result.Ok(_) => "medsci1" | Result.Error(e) => Binary.describe(e)),
    Ui.text(match Level.objects with | Result.Ok(_) => "objects" | Result.Error(e) => Binary.describe(e)),
    Ui.text(Text.join("", ["grounded  ", if Physics.grounded(playerTag) then "yes" else "no"])),
    Ui.text(Text.join("", ["doors  ", Text.fromFloat(List.length(Level.doors)), "  moving ", Text.fromFloat(Map.size(model.objects.moving)), "  (E frobs)"])),
    Ui.text(Text.join("", ["position  ", Text.fixed(p.x, 2.0), ", ", Text.fixed(p.y, 2.0), ", ", Text.fixed(p.z, 2.0)])),
  ])
    |> Ui.panel(Ui.topLeft())
