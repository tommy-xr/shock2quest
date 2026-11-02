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

// `P$RotDoor`: a door that swings `closed` → `open` degrees about `axis`,
// at `speed` radians per second. Decoded and compared, not yet animated.
type rotDoor = { closed: float, open: float, speed: float, axis: float, state: float, clockwise: bool,
                 baseClosed: List<float>, baseOpen: List<float>, base: List<float> }

// `P$FrobInfo`: what frobbing does in the world, in inventory and as a tool
// (flag bits: 1 move, 2 script, 4 delete, 8 ignore, …).
type frobInfo = { world: float, inventory: float, tool: float }

// Every authored record, donors' and objects' alike; immutable after load.
type authored = { transDoor: Map<float, transDoor>, rotDoor: Map<float, rotDoor>, locked: Map<float, bool>,
                  frobInfo: Map<float, frobInfo>, scripts: Map<float, Objects.contribution<string>> }

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

let n = (k: float, v: List<float>): float => Option.defaultValue(0.0, List.nth(k, v))
let lerp = (a: List<float>, b: List<float>, t: float): List<float> =>
  [n(0.0, a) + (n(0.0, b) - n(0.0, a)) * t, n(1.0, a) + (n(1.0, b) - n(1.0, a)) * t, n(2.0, a) + (n(2.0, b) - n(2.0, a)) * t]
let distance = (a: List<float>, b: List<float>): float =>
  Math.sqrt((n(0.0, b) - n(0.0, a)) * (n(0.0, b) - n(0.0, a)) + (n(1.0, b) - n(1.0, a)) * (n(1.0, b) - n(1.0, a))
            + (n(2.0, b) - n(2.0, a)) * (n(2.0, b) - n(2.0, a)))

let noProps: props = { transDoor: Map.empty(), rotDoor: Map.empty(), locked: Map.empty(), frobInfo: Map.empty(),
                       scriptsLocal: Map.empty(), scripts: Map.empty() }

let donorsOf = (w: world, id: float): List<float> =>
  Objects.donors(w.graph, Map.get(id, w.edits) |> Option.defaultValue(Objects.noEdits), id)

let scriptsOf = (w: world, ds: List<float>, id: float, p: props): props =>
  { p with scripts: Objects.cache(id, Objects.accumulate(p.scriptsLocal, w.authored.scripts, ds, id), p.scripts) }

// A translating door StdDoor moves: a rotating one is left alone (Dark
// prefers RotDoor when an object has both).
let slider = (w: world, id: float): Option.t<transDoor> =>
  if Map.member(id, w.props.rotDoor) then Option.None else Objects.get(id, w.props.transDoor)

// How far along a door's authored location is (0 at closed, 1 at open).
let progressOf = (d: transDoor): float =>
  if d.state == 0.0 then 0.0
  else if d.state == 1.0 then 1.0
  else Math.clamp01(distance(d.baseClosed, d.base) / Math.max(distance(d.baseClosed, d.baseOpen), 0.000001))

let scriptOf = (d: transDoor): doorScript =>
  { state: if d.state == 1.0 then Open else if d.state == 2.0 then Closing else if d.state == 3.0 then Opening else Closed,
    t: progressOf(d) }

let isMoving = (s: doorScript): bool => s.state == Opening || s.state == Closing

// Copy an object's properties down from its donors, and start its door.
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
  let w2 = { w with props: p2 } in
  match slider(w2, id) with
  | Option.Some(d) =>
    let s = scriptOf(d) in
    { w2 with door: Map.insert(id, s, w2.door), moving: if isMoving(s) then Map.insert(id, true, w2.moving) else w2.moving }
  | Option.None => w2

// Re-derive an object's inherited values after its donors changed.
let recompose = (w: world, id: float): world =>
  let ds = donorsOf(w, id) in
  let a = w.authored in
  let p = w.props in
  { w with props: scriptsOf(w, ds, id, { p with
    transDoor: Objects.recompose(a.transDoor, ds, id, p.transDoor), rotDoor: Objects.recompose(a.rotDoor, ds, id, p.rotDoor),
    locked: Objects.recompose(a.locked, ds, id, p.locked), frobInfo: Objects.recompose(a.frobInfo, ds, id, p.frobInfo) }) }

// Add or remove a metaproperty on an object at runtime (no-op when that
// changes nothing; see `Objects.relate`).
let setMeta = (add: bool, meta: float, id: float, w: world): world =>
  match Objects.relate(w.graph, add, meta, id, Map.get(id, w.edits) |> Option.defaultValue(Objects.noEdits)) with
  | Option.Some(e) => recompose({ w with edits: Map.insert(id, e, w.edits) }, id)
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
// and TurnOff (from a switch) open and close it, locked or not. A door
// without travel never moves.
let stdDoor = (w: world, self: float, m: msg): world =>
  match slider(w, self) with
  | Option.None => w
  | Option.Some(d) =>
    if distance(d.baseClosed, d.baseOpen) < 0.001 then w
    else match m with
      | Frob(_) =>
        let s = doorOf(w, self) in
        if s.state == Open || s.state == Opening then head(false, self, w)
        else if isLocked(w, self) then play(self, "hackfail", w)
        else head(true, self, w)
      | TurnOn(_) => head(true, self, w)
      | TurnOff(_) => head(false, self, w)

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

let handle = (w: world, e: envelope): world =>
  Map.get(e.dest, w.props.scripts) |> Option.defaultValue([])
    |> List.fold((acc, name) => run(name, acc, e.dest, e.msg), w)

let drain = (w: world): world =>
  Objects.drain(cap, handle, (x: world) => x.queue, (q, x: world) => { x with queue: q }, w)

// Advance only the moving doors; one reaching its end stops and sounds.
let tick = (dt: float, w: world): world =>
  Map.keys(w.moving) |> List.fold((acc, id) =>
    match slider(acc, id) with
    | Option.None => { acc with moving: Map.remove(id, acc.moving) }
    | Option.Some(d) =>
      let s = doorOf(acc, id) in
      let rate = dt * d.speed / Math.max(distance(d.baseClosed, d.baseOpen), 0.000001) in
      if s.state == Opening then
        (if s.t + rate >= 1.0 then play(id, "open", setDoor(id, { state: Open, t: 1.0 }, acc))
         else setDoor(id, { s with t: s.t + rate }, acc))
      else if s.t - rate <= 0.0 then play(id, "closed", setDoor(id, { state: Closed, t: 0.0 }, acc))
      else setDoor(id, { s with t: s.t - rate }, acc), w)

// Where a door is now (Dark feet), if it's a door.
let position = (w: world, id: float): Option.t<List<float>> =>
  slider(w, id) |> Option.map((d: transDoor) => lerp(d.baseClosed, d.baseOpen, doorOf(w, id).t))

// One frame: this frame's frobs, the drain, the doors. Sounds are the
// frame's own.
let frame = (dt: float, frobs: List<float>, w: world): world =>
  List.fold((acc, id) => send(id, Frob(0.0), acc), { w with sounds: [] }, frobs) |> drain |> tick(dt)
