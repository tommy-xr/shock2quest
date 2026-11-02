// Checks for `Objects` and `Door` on synthetic objects (no game files).

// --- A door, a button and a lock -----------------------------------------------
// Donors: -1 Door holds a TransDoor (2 ft up at 8 ft/s), -2 StdDoor (donor
// -1) the StdDoor script, -3 Locked a lock, -4 Button the StdButton script.
// Objects: 10 a door, 11 a button SwitchLinked to it, 12 a door whose own
// value unlocks it.

let slide: Door.transDoor = { closed: 0.0, open: 2.0, speed: 8.0, axis: 2.0, state: 0.0,
                              baseClosed: [0.0, 0.0, 0.0], baseOpen: [0.0, 0.0, 2.0], base: [0.0, 0.0, 0.0] }
let script = (name: string): Objects.contribution<string> => { items: [name], inherits: true }

let switches = Objects.linkAs(100.0, { src: 11.0, dst: 10.0, data: true, donor: Option.None }, Objects.noLinks())

let world0: Door.world =
  let w: Door.world = {
    graph: Map.fromList([(-2.0, [-1.0]), (10.0, [-2.0]), (11.0, [-4.0]), (12.0, [-2.0])]),
    edits: Map.empty(),
    authored: { transDoor: Map.fromList([(-1.0, slide)]), rotDoor: Map.empty(),
                locked: Map.fromList([(-3.0, true), (12.0, false)]), frobInfo: Map.empty(),
                scripts: Map.fromList([(-2.0, script("StdDoor")), (-4.0, script("StdButton"))]), switchLink: switches },
    props: Door.noProps, switchLink: switches,
    door: Map.empty(), moving: Map.empty(), queue: Objects.queue(), sounds: [],
  } in
  List.fold(Door.spawn, w, [10.0, 11.0, 12.0])

let step = (w: Door.world, frobs: List<float>): Door.world => Door.frame(0.1, frobs, w)
let frames = (inputs: List<List<float>>): List<Door.world> =>
  List.fold((hist, f) => match List.last(hist) with
    | Option.Some(w) => List.append([step(w, f)], hist)
    | Option.None => hist, [world0], inputs)

let inputs = [[], [10.0], [], [], [], []]
let hist = frames(inputs)
let at = (k: float): Door.world => Cal.at(k, hist, world0)
let doorZ = (w: Door.world, id: float): float => Door.position(w, id) |> Option.map((p) => Door.n(2.0, p)) |> Option.defaultValue(0.0)
let stateOf = (w: Door.world, id: float): Door.doorState => Door.doorOf(w, id).state

expect Map.get(10.0, world0.props.transDoor) == Option.Some(Objects.Inherited(slide))
expect Map.get(10.0, world0.props.scripts) == Option.Some(["StdDoor"])
expect Map.member(11.0, world0.props.transDoor) == false
expect doorZ(at(2.0), 10.0) == 0.8
expect stateOf(at(4.0), 10.0) == Door.Open
expect doorZ(at(5.0), 10.0) == 2.0
// Rewind is picking an earlier snapshot; stepping it with other input diverges.
expect doorZ(step(at(1.0), []), 10.0) == 0.0
expect frames(inputs) == hist
// Button → SwitchLink → TurnOn, handled in the same drain.
expect stateOf(step(world0, [11.0]), 10.0) == Door.Opening
// A runtime Locked metaproperty blocks the frob; removing it unlocks.
let lockedW = Door.setMeta(true, -3.0, 10.0, world0)
expect Door.isLocked(lockedW, 10.0)
expect stateOf(step(lockedW, [10.0]), 10.0) == Door.Closed
expect step(lockedW, [10.0]).sounds == [{ obj: 10.0, name: "hackfail" }]
expect Door.isLocked(Door.setMeta(false, -3.0, 10.0, lockedW), 10.0) == false
// The object's own value beats a metaproperty.
expect Door.isLocked(Door.setMeta(true, -3.0, 12.0, world0), 12.0) == false

// A metaproperty that brings a door starts it as authored (open here), and
// one that brings a SwitchLink copies it down, tagged with its donor; both go
// when the metaproperty does.
let openSlide = { slide with state: 1.0, base: [0.0, 0.0, 2.0] }
let rich = { world0 with authored: { world0.authored with transDoor: Map.insert(-5.0, openSlide, world0.authored.transDoor),
  switchLink: Objects.linkAs(200.0, { src: -6.0, dst: 10.0, data: true, donor: Option.None }, world0.authored.switchLink) } }
let gained = rich |> Door.setMeta(true, -5.0, 11.0) |> Door.setMeta(true, -6.0, 12.0)
expect Door.position(gained, 11.0) == Option.Some([0.0, 0.0, 2.0])
expect List.map((l: Objects.link<bool>) => (l.dst, l.donor), Objects.from(12.0, gained.switchLink)) == [(10.0, Option.Some(-6.0))]
let lost = gained |> Door.setMeta(false, -5.0, 11.0) |> Door.setMeta(false, -6.0, 12.0)
expect Map.member(11.0, lost.door) == false
expect Objects.from(12.0, lost.switchLink) == []

// Sounds: once when it starts, once when it arrives; none while it moves.
expect List.map((w: Door.world) => List.map((s: Door.sound) => s.name, w.sounds), hist) == [[], [], ["opening"], [], ["open"], [], []]

// --- Donors ---------------------------------------------------------------------
// -10 ← {-20 (meta), -30 (archetype)}; both hold `hp`, and -20 ranks first.
// -30 → -40 and -20 → -40: a shared ancestor, visited once, where first met.
let g = Map.fromList([(1.0, [-20.0, -30.0]), (-20.0, [-40.0]), (-30.0, [-40.0, -50.0]), (-40.0, [-60.0])])
expect Objects.donors(g, Objects.noEdits, 1.0) == [-20.0, -40.0, -60.0, -30.0, -50.0]
let hp = Map.fromList([(-20.0, 5.0), (-30.0, 7.0), (-50.0, 9.0)])
expect Objects.get(1.0, Objects.spawn(hp, Objects.donors(g, Objects.noEdits, 1.0), 1.0, Map.empty())) == Option.Some(5.0)
// A cycle ends.
expect Objects.donors(Map.fromList([(1.0, [-1.0]), (-1.0, [-2.0]), (-2.0, [-1.0, 1.0])]), Objects.noEdits, 1.0) == [-1.0, -2.0]
// A chain deeper than 8 is walked in full.
let chain = Map.fromList(List.map((k) => (0.0 - k, [0.0 - k - 1.0]), List.range(20.0)) |> List.append([(5.0, [0.0])]))
expect List.length(Objects.donors(chain, Objects.noEdits, 5.0)) == 21.0
expect List.last(Objects.donors(chain, Objects.noEdits, 5.0)) == Option.Some(-20.0)

// Relation changes.
let e0 = Objects.noEdits
// Adding an absent donor ranks it first; adding it again changes nothing.
let added = Objects.relate(g, true, -70.0, 1.0, e0) |> Option.defaultValue(e0)
expect added == { added: [-70.0], removed: [] }
expect Objects.donors(g, added, 1.0) == [-70.0, -20.0, -40.0, -60.0, -30.0, -50.0]
expect Objects.relate(g, true, -70.0, 1.0, added) == Option.None
// Removing an absent donor changes nothing.
expect Objects.relate(g, false, -99.0, 1.0, e0) == Option.None
// Adding an inherited donor changes nothing.
expect Objects.relate(g, true, -40.0, 1.0, e0) == Option.None
// Removing an inherited donor excludes it (and what only it donates);
// adding it back clears the exclusion.
let without = Objects.relate(g, false, -20.0, 1.0, e0) |> Option.defaultValue(e0)
expect without == { added: [], removed: [-20.0] }
expect Objects.donors(g, without, 1.0) == [-30.0, -40.0, -60.0, -50.0]
expect Objects.get(1.0, Objects.recompose(hp, Objects.donors(g, without, 1.0), 1.0, Map.fromList([(1.0, Objects.Inherited(5.0))]))) == Option.Some(7.0)
expect Objects.relate(g, true, -20.0, 1.0, without) == Option.Some(e0)
// Removing a runtime-added donor drops it.
expect Objects.relate(g, false, -70.0, 1.0, added) == Option.Some({ added: [], removed: [-70.0] })
// Recompose keeps a live own write and drops an inherited value with no donor left.
expect Objects.recompose(hp, [], 1.0, Objects.set(1.0, 3.0, Map.empty())) == Map.fromList([(1.0, Objects.Own(3.0))])
expect Objects.recompose(hp, [], 1.0, Map.fromList([(1.0, Objects.Inherited(5.0))])) == Map.empty()

// Accumulators: own items first, then donors', up to one that doesn't inherit.
let scripts = Map.fromList([(-20.0, { items: ["A"], inherits: true }), (-40.0, { items: ["B", "C"], inherits: false }),
                            (-60.0, { items: ["D"], inherits: true })])
let ds1 = Objects.donors(g, Objects.noEdits, 1.0)
expect Objects.accumulate(Map.fromList([(1.0, { items: ["Own"], inherits: true })]), scripts, ds1, 1.0) == ["Own", "A", "B", "C"]
expect Objects.accumulate(Map.fromList([(1.0, { items: ["Own"], inherits: false })]), scripts, ds1, 1.0) == ["Own"]
expect Objects.accumulate(Map.empty(), scripts, [-60.0], 1.0) == ["D"]

// --- Links ------------------------------------------------------------------------
// Donor -20 owns receptron link 5 (→ -900); object 1 also has a live link.
let receptrons = Objects.linkAs(5.0, { src: -20.0, dst: -900.0, data: 1.0, donor: Option.None }, Objects.noLinks())
let live = Objects.link({ src: 1.0, dst: 2.0, data: 0.0, donor: Option.None }, Objects.noLinks())
let spawned = Objects.inherit(receptrons, [], ds1, 1.0, live)
expect List.map((l: Objects.link<float>) => (l.dst, l.donor), Objects.from(1.0, spawned)) == [(2.0, Option.None), (-900.0, Option.Some(-20.0))]
expect List.length(Objects.into(-900.0, spawned)) == 1.0
// -20 leaves: its copy goes, the live link stays; it comes back: copied again.
let gone = Objects.inherit(receptrons, ds1, Objects.donors(g, without, 1.0), 1.0, spawned)
expect List.map((l: Objects.link<float>) => l.dst, Objects.from(1.0, gone)) == [2.0]
expect Objects.into(-900.0, gone) == []
expect List.map((l: Objects.link<float>) => l.dst, Objects.from(1.0, Objects.inherit(receptrons, Objects.donors(g, without, 1.0), ds1, 1.0, gone))) == [2.0, -900.0]

// Re-linking an id replaces the old link in every index.
let relinked = Objects.noLinks() |> Objects.linkAs(5.0, { src: 1.0, dst: 2.0, data: 0.0, donor: Option.None })
  |> Objects.linkAs(5.0, { src: 3.0, dst: 4.0, data: 0.0, donor: Option.None })
expect (Objects.from(1.0, relinked), Objects.into(2.0, relinked), List.length(Objects.from(3.0, relinked))) == ([], [], 1.0)

// --- Queue --------------------------------------------------------------------------
// Each message n < 6 sends 2n+1 and 2n+2 (a sustained fan-out); a cap of 4
// per drain carries the rest over, in order.
type fan = { q: Objects.queue<float>, seen: List<float> }
let fanOut = (f: fan, m: float): fan =>
  let f2 = { f with seen: List.append([m], f.seen) } in
  if m < 6.0 then { f2 with q: f2.q |> Objects.push(2.0 * m + 1.0) |> Objects.push(2.0 * m + 2.0) } else f2
let fanDrain = (f: fan): fan => Objects.drain(4.0, fanOut, (x: fan) => x.q, (q, x: fan) => { x with q: q }, f)
let fan0: fan = { q: Objects.push(0.0, Objects.queue()), seen: [] }
let fan1 = fanDrain(fan0)
let fan2 = fanDrain(fan1)
let fan4 = fanDrain(fanDrain(fan2))
expect fan1.seen == [0.0, 1.0, 2.0, 3.0]
expect Objects.pending(fan1.q) == 5.0
expect fan2.seen == [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]
expect fan4.seen == List.range(13.0)
expect Objects.pending(fan4.q) == 0.0
// Drains that left messages behind are counted; an empty drain changes nothing.
expect fan4.q.carried == 3.0
expect fanDrain(fan4) == fan4
