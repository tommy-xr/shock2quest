// Door — the slice's objects on `Objects`: the properties a door needs,
// copied down per object, SwitchLinks, and the StdDoor and StdButton scripts
// run from the message queue. Positions stay in Dark feet (Z up).
//
// A frame: `send` this frame's frobs, `drain` the queue once, `tick` the
// moving doors. Scripts are pure `(world, self, msg) => world`: messages
// they send join the queue, sounds they play collect in `sounds` for the
// caller to turn into effects.

// `P$TransDoor`: a door that slides between two authored locations (Dark
// feet) at `speed` feet per second; `state` is the authored DOOR_STATE
// (0 closed, 1 open, 2 closing, 3 opening, 4 halted) and `base` where it is.
type transDoor = { closed: float, open: float, speed: float, axis: float, state: float,
                   baseClosed: List<float>, baseOpen: List<float>, base: List<float> }

// `P$RotDoor`: a door that swings `closed` → `open` degrees about `axis`
// (0 X, 1 Y, 2 Z, turned by `baseRotation`) at `speed` radians per second,
// the long way round when `clockwise` says so. `baseClosed`/`baseOpen` and
// the two rotations are where it sits at each end.
type rotDoor = { closed: float, open: float, speed: float, axis: float, state: float, clockwise: bool,
                 baseClosed: List<float>, baseOpen: List<float>, base: List<float>,
                 baseRotation: Affine.t, closedRotation: Affine.t, openRotation: Affine.t }

// `P$FrobInfo`: what frobbing does in the world, in inventory and as a tool
// (flag bits: 1 move, 2 script, 4 delete, 8 ignore, …).
type frobInfo = { world: float, inventory: float, tool: float }

// Every authored record, donors' and objects' alike; immutable after load.
type authored = { transDoor: Map<float, transDoor>, rotDoor: Map<float, rotDoor>, locked: Map<float, bool>,
                  frobInfo: Map<float, frobInfo>, scripts: Map<float, Objects.contribution<string>>,
                  switchLink: Objects.links<bool> }

// Live, per-object copied-down values. `scripts` caches the accumulated
// list, recomputed from `scriptsLocal` and the donors.
type props = { transDoor: Map<float, Objects.Cell<transDoor>>, rotDoor: Map<float, Objects.Cell<rotDoor>>,
               locked: Map<float, Objects.Cell<bool>>, frobInfo: Map<float, Objects.Cell<frobInfo>>,
               scriptsLocal: Map<float, Objects.contribution<string>>, scripts: Map<float, List<string>> }

type doorState =
  | Closed
  | Opening
  | Open
  | Closing

// StdDoor's own state per door: where it's headed and how far along
// (0 closed, 1 open).
type doorScript = { state: doorState, t: float }

type msg =
  | Frob(frobber: float)
  | TurnOn(from: float)
  | TurnOff(from: float)

type envelope = { dest: float, msg: msg }

type sound = { obj: float, name: string }

type world = {
  graph: Map<float, List<float>>,
  edits: Map<float, Objects.edits>,
  authored: authored,
  props: props,
  switchLink: Objects.links<bool>,
  door: Map<float, doorScript>,
  moving: Map<float, bool>,
  queue: Objects.queue<envelope>,
  sounds: List<sound>,
}

// Messages handled per frame; the rest wait (in order) for the next.
let cap = 1024.0

let n = Affine.n
let lerp = (a: List<float>, b: List<float>, t: float): List<float> =>
  [n(0.0, a) + (n(0.0, b) - n(0.0, a)) * t, n(1.0, a) + (n(1.0, b) - n(1.0, a)) * t, n(2.0, a) + (n(2.0, b) - n(2.0, a)) * t]
let distance = (a: List<float>, b: List<float>): float =>
  Math.sqrt((n(0.0, b) - n(0.0, a)) * (n(0.0, b) - n(0.0, a)) + (n(1.0, b) - n(1.0, a)) * (n(1.0, b) - n(1.0, a))
            + (n(2.0, b) - n(2.0, a)) * (n(2.0, b) - n(2.0, a)))

let noProps: props = { transDoor: Map.empty(), rotDoor: Map.empty(), locked: Map.empty(), frobInfo: Map.empty(),
                       scriptsLocal: Map.empty(), scripts: Map.empty() }

// No objects.
let empty: world = { graph: Map.empty(), edits: Map.empty(),
  authored: { transDoor: Map.empty(), rotDoor: Map.empty(), locked: Map.empty(), frobInfo: Map.empty(), scripts: Map.empty(),
              switchLink: Objects.noLinks() },
  props: noProps, switchLink: Objects.noLinks(), door: Map.empty(), moving: Map.empty(), queue: Objects.queue(), sounds: [] }

let donorsOf = (w: world, id: float): List<float> =>
  Objects.donors(w.graph, Map.get(id, w.edits) |> Option.defaultValue(Objects.noEdits), id)

let scriptsOf = (w: world, ds: List<float>, id: float, p: props): props =>
  { p with scripts: Objects.cache(id, Objects.accumulate(p.scriptsLocal, w.authored.scripts, ds, id), p.scripts) }

// A translating door StdDoor moves; an object with both is a rotating door
// (Dark prefers RotDoor: rotating doors inherit a zero TransDoor).
let slider = (w: world, id: float): Option.t<transDoor> =>
  if Map.member(id, w.props.rotDoor) then Option.None else Objects.get(id, w.props.transDoor)

let rotator = (w: world, id: float): Option.t<rotDoor> => Objects.get(id, w.props.rotDoor)

// Signed degrees from closed to open: a clockwise door turns negative, and
// either may go the long way round through 0/360.
let travelDegrees = (d: rotDoor): float =>
  let t = d.open - d.closed in
  if Math.abs(t) <= 0.0000001192 then 0.0
  else if d.clockwise then (if t > 0.0 then t - 360.0 * Math.ceil(t / 360.0) else t)
  else if t < 0.0 then t + 360.0 * Math.ceil((0.0 - t) / 360.0) else t

// A rotating door's `axis` in its own (unturned) axes.
let hinge = (axis: float): List<float> =>
  if axis == 0.0 then [1.0, 0.0, 0.0] else if axis == 1.0 then [0.0, 1.0, 0.0] else [0.0, 0.0, 1.0]

let swings = (d: rotDoor): bool => Math.abs(travelDegrees(d)) > 0.0001

// How far along a door's authored location is (0 at closed, 1 at open).
let progressOf = (d: transDoor): float =>
  if d.state == 0.0 then 0.0
  else if d.state == 1.0 then 1.0
  else Math.clamp01(distance(d.baseClosed, d.base) / Math.max(distance(d.baseClosed, d.baseOpen), 0.000001))

// A halted door (state 4) counts as open past halfway, as StdDoor heads for
// the nearer end.
let scriptOf = (d: transDoor): doorScript =>
  let t = progressOf(d) in
  { state: if d.state == 1.0 then Open else if d.state == 2.0 then Closing else if d.state == 3.0 then Opening
           else if d.state == 4.0 && t >= 0.5 then Open else Closed,
    t: t }

// A rotating door starts at an end: closed unless authored open (opening
// or closing heads that way from closed; one without travel stays put).
let rotScriptOf = (d: rotDoor): doorScript =>
  if d.state == 1.0 then { state: Open, t: 1.0 }
  else { state: if not swings(d) then Closed else if d.state == 3.0 then Opening else if d.state == 2.0 then Closing else Closed, t: 0.0 }

let isMoving = (s: doorScript): bool => s.state == Opening || s.state == Closing

let hasScript = (w: world, id: float, name: string): bool =>
  Map.get(id, w.props.scripts) |> Option.defaultValue([]) |> List.any((s) => Text.toLower(s) == name)

// Start StdDoor's state when an object gains a door and the script (a
// running one keeps its own), and drop it when it loses either.
let syncDoor = (w: world, id: float): world =>
  let start = match rotator(w, id) with
    | Option.Some(r) => Option.Some(rotScriptOf(r))
    | Option.None => slider(w, id) |> Option.map(scriptOf) in
  match start with
  | Option.Some(s) =>
    if not hasScript(w, id, "stddoor") then { w with door: Map.remove(id, w.door), moving: Map.remove(id, w.moving) }
    else if Map.member(id, w.door) then w
    else setDoor(id, s, w)
  | Option.None => { w with door: Map.remove(id, w.door), moving: Map.remove(id, w.moving) }

// Copy an object's properties and links down from its donors, and start its door.
let spawn = (w: world, id: float): world =>
  let ds = donorsOf(w, id) in
  let a = w.authored in
  let p = w.props in
  let local = match Map.get(id, a.scripts) with
    | Option.Some(c) => Map.insert(id, c, p.scriptsLocal)
    | Option.None => p.scriptsLocal in
  let p2 = scriptsOf(w, ds, id, { p with
    transDoor: Objects.spawn(a.transDoor, ds, id, p.transDoor), rotDoor: Objects.spawn(a.rotDoor, ds, id, p.rotDoor),
    locked: Objects.spawn(a.locked, ds, id, p.locked), frobInfo: Objects.spawn(a.frobInfo, ds, id, p.frobInfo),
    scriptsLocal: local }) in
  syncDoor({ w with props: p2, switchLink: Objects.inherit(a.switchLink, [], ds, id, w.switchLink) }, id)

// Re-derive an object's inherited values and links after its donors changed
// from `before`.
let recompose = (w: world, before: List<float>, id: float): world =>
  let ds = donorsOf(w, id) in
  let a = w.authored in
  let p = w.props in
  syncDoor({ w with
    props: scriptsOf(w, ds, id, { p with
      transDoor: Objects.recompose(a.transDoor, ds, id, p.transDoor), rotDoor: Objects.recompose(a.rotDoor, ds, id, p.rotDoor),
      locked: Objects.recompose(a.locked, ds, id, p.locked), frobInfo: Objects.recompose(a.frobInfo, ds, id, p.frobInfo) }),
    switchLink: Objects.inherit(a.switchLink, before, ds, id, w.switchLink) }, id)

// Add or remove a metaproperty on an object at runtime (no-op when that
// changes nothing; see `Objects.relate`).
let setMeta = (add: bool, meta: float, id: float, w: world): world =>
  match Objects.relate(w.graph, add, meta, id, Map.get(id, w.edits) |> Option.defaultValue(Objects.noEdits)) with
  | Option.Some(e) => recompose({ w with edits: Map.insert(id, e, w.edits) }, donorsOf(w, id), id)
  | Option.None => w

let isLocked = (w: world, id: float): bool => Objects.get(id, w.props.locked) |> Option.defaultValue(false)

let send = (dest: float, m: msg, w: world): world => { w with queue: Objects.push({ dest: dest, msg: m }, w.queue) }
let play = (id: float, name: string, w: world): world => { w with sounds: List.append([{ obj: id, name: name }], w.sounds) }

let doorOf = (w: world, id: float): doorScript => Map.get(id, w.door) |> Option.defaultValue({ state: Closed, t: 0.0 })

let setDoor = (id: float, s: doorScript, w: world): world =>
  { w with door: Map.insert(id, s, w.door), moving: if isMoving(s) then Map.insert(id, true, w.moving) else Map.remove(id, w.moving) }

// Head open or closed; already headed there changes nothing (no sound).
let head = (open: bool, id: float, w: world): world =>
  let s = doorOf(w, id) in
  let opening = s.state == Open || s.state == Opening in
  if open == opening then w
  else play(id, if open then "opening" else "closing", setDoor(id, { s with state: if open then Opening else Closing }, w))

// StdDoor: a frob toggles the door unless it's locked and closed; TurnOn
// and TurnOff (from a switch) open and close it, locked or not. A sliding
// door without travel never moves (a locked one still answers a frob), and
// one authored open without travel is a doorway that ignores everything; a
// rotating door without travel ignores everything.
let stdDoor = (w: world, self: float, m: msg): world =>
  let kind = match rotator(w, self) with
    | Option.Some(r) => Option.Some((swings(r), not swings(r)))
    | Option.None => slider(w, self) |> Option.map((d: transDoor) =>
      let travels = distance(d.baseClosed, d.baseOpen) >= 0.001 in (travels, not travels && d.state == 1.0)) in
  match kind with
  | Option.None => w
  | Option.Some(k) => match k with | (travels, deaf) =>
    let go = (open: bool) => if travels then head(open, self, w) else w in
    if deaf then w
    else match m with
      | Frob(_) =>
        let s = doorOf(w, self) in
        if s.state == Open || s.state == Opening then go(false)
        else if isLocked(w, self) then play(self, "hackfail", w)
        else go(true)
      | TurnOn(_) => go(true)
      | TurnOff(_) => go(false)

// StdButton: a frob turns on everything its SwitchLinks lead to.
let stdButton = (w: world, self: float, m: msg): world =>
  match m with
  | Frob(_) => Objects.from(self, w.switchLink) |> List.fold((acc, l: Objects.link<bool>) => send(l.dst, TurnOn(self), acc), w)
  | _ => w

// Scripts by name (Dark's names ignore case); unknown ones do nothing yet.
let run = (name: string, w: world, self: float, m: msg): world =>
  let key = Text.toLower(name) in
  if key == "stddoor" then stdDoor(w, self, m)
  else if key == "stdbutton" then stdButton(w, self, m)
  else w

// Each script once, in order: a name inherited twice (19 medsci1 objects)
// runs once.
let handle = (w: world, e: envelope): world =>
  match Map.get(e.dest, w.props.scripts) |> Option.defaultValue([])
    |> List.fold((acc, name) => match acc with
      | (x, ran) =>
        let key = Text.toLower(name) in
        if List.any((r) => r == key, ran) then acc else (run(name, x, e.dest, e.msg), List.append([key], ran)), (w, [])) with
  | (x, _) => x

let drain = (w: world): world =>
  Objects.drain(cap, handle, (x: world) => x.queue, (q, x: world) => { x with queue: q }, w)

// How far along (0..1) a door moves per second: a sliding door's speed over
// its travel, a rotating door's angular speed over its arc.
let pace = (w: world, id: float): Option.t<float> =>
  match rotator(w, id) with
  | Option.Some(r) => Option.Some(Math.abs(r.speed) / Math.max(Math.abs(travelDegrees(r)) * Math.pi / 180.0, 0.000001))
  | Option.None => slider(w, id) |> Option.map((d: transDoor) => d.speed / Math.max(distance(d.baseClosed, d.baseOpen), 0.000001))

// Advance only the moving doors; one reaching its end stops and sounds.
let tick = (dt: float, w: world): world =>
  Map.keys(w.moving) |> List.fold((acc, id) =>
    match pace(acc, id) with
    | Option.None => { acc with moving: Map.remove(id, acc.moving) }
    | Option.Some(perSecond) =>
      let s = doorOf(acc, id) in
      let rate = dt * perSecond in
      if s.state == Opening then
        (if s.t + rate >= 1.0 then play(id, "open", setDoor(id, { state: Open, t: 1.0 }, acc))
         else setDoor(id, { s with t: s.t + rate }, acc))
      else if s.t - rate <= 0.0 then play(id, "closed", setDoor(id, { state: Closed, t: 0.0 }, acc))
      else setDoor(id, { s with t: s.t - rate }, acc), w)

// A rotating door's place and rotation (Dark) at progress `t`, as
// shock2quest poses it: the ends exactly as authored; between them, the turn
// about the hinge axis through the pivot both ends share (plus any slide
// along the axis).
type pose = { at: List<float>, rotation: Affine.t }

let swing = (d: rotDoor, t: float): pose =>
  let eps = 0.0000001192 in
  if t <= eps || not swings(d) then { at: d.baseClosed, rotation: d.closedRotation }
  else if 1.0 - t <= eps then { at: d.baseOpen, rotation: d.openRotation }
  else
    let total = travelDegrees(d) * Math.pi / 180.0 in
    let axis = World.unit(Affine.rotate(d.baseRotation, hinge(d.axis))) in
    let disp = World.sub(d.baseOpen, d.baseClosed) in
    let axial = Affine.scale(World.dot(disp, axis), axis) in
    let perp = World.sub(disp, axial) in
    let alpha = Math.cos(total) - 1.0 in
    let beta = Math.sin(total) in
    let den = alpha * alpha + beta * beta in
    let turn = Affine.axisAngle(axis, total * t) in
    let at = if den > 0.000001 then
        let radius = Affine.scale(1.0 / den, World.sub(Affine.scale(alpha, perp), Affine.scale(beta, World.cross(axis, perp)))) in
        Affine.add(Affine.add(d.baseClosed, World.sub(Affine.rotate(turn, radius), radius)), Affine.scale(t, axial))
      else Affine.add(d.baseClosed, Affine.scale(t, disp)) in
    { at: at, rotation: Affine.compose(turn, d.closedRotation) }

// Where a door is now (Dark feet), if it's a door: where StdDoor has it, or
// as authored without the script.
let position = (w: world, id: float): Option.t<List<float>> =>
  match rotator(w, id) with
  | Option.Some(r) => Option.Some(swing(r, Map.get(id, w.door) |> Option.map((s: doorScript) => s.t) |> Option.defaultValue(rotScriptOf(r).t)).at)
  | Option.None =>
    slider(w, id) |> Option.map((d: transDoor) =>
      lerp(d.baseClosed, d.baseOpen, Map.get(id, w.door) |> Option.map((s: doorScript) => s.t) |> Option.defaultValue(progressOf(d))))

// One frame: this frame's frobs, the drain, the doors. Sounds are the
// frame's own.
let frame = (dt: float, frobs: List<float>, w: world): world =>
  List.fold((acc, id) => send(id, Frob(0.0), acc), { w with sounds: [] }, frobs) |> drain |> tick(dt)
