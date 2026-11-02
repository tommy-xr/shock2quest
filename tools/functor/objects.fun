// Objects — Dark's object model as plain data. Ids are Dark's: concrete
// objects positive, archetypes and metaproperties (donors) negative.
//
// - Properties are copied down: each object's effective value is resolved
//   through its donors at spawn and when a metaproperty is added or removed,
//   never per read, so a read is one `Map.get`.
// - Links live in one table per flavor, indexed by id, source and destination.
// - Messages wait in a FIFO that the game drains once per frame, capped.

// A copied-down property value: the object's own (authored, or written
// live), or inherited from a donor and re-derived when its donors change.
type Cell<'t> =
  | Own(value: 't)
  | Inherited(value: 't)

let value = (c: Cell<'t>): 't =>
  match c with
  | Own(v) => v
  | Inherited(v) => v

let isOwn = (c: Cell<'t>): bool =>
  match c with
  | Own(_) => true
  | Inherited(_) => false

// An object's effective value, and a live write (which then outranks donors).
let get = (id: float, table: Map<float, Cell<'t>>): Option.t<'t> => Map.get(id, table) |> Option.map(value)
let set = (id: float, v: 't, table: Map<float, Cell<'t>>): Map<float, Cell<'t>> => Map.insert(id, Own(v), table)

// Repeat `step` until `done`. Functor has no loop and only shallow recursion,
// so steps run in folds whose length doubles each round: at most twice the
// steps needed, and log2(steps) recursion.
let loop = (step: ('s) => 's, done: ('s) => bool, s: 's): 's => rounds(step, done, s, 4.0)
let rounds = (step: ('s) => 's, done: ('s) => bool, s: 's, n: float): 's =>
  if done(s) then s
  else rounds(step, done, List.fold((acc, _) => if done(acc) then acc else step(acc), s, List.range(n)), n * 2.0)

let has = (x: float, xs: List<float>): bool => List.any((y) => y == x, xs)

// --- Donors -----------------------------------------------------------------

// `graph` holds each object's and donor's direct donors (its `L$MetaProp`
// links), highest priority first: metaproperties above the archetype, the
// archetype link (priority 0), then any below it.
//
// Runtime relation changes per object: `added` (newest first) rank above
// every authored donor; `removed` donors are excluded wherever they appear.
type edits = { added: List<float>, removed: List<float> }
let noEdits: edits = { added: [], removed: [] }

let direct = (graph: Map<float, List<float>>, id: float): List<float> => Map.get(id, graph) |> Option.defaultValue([])

type walk = { stack: List<float>, seen: Map<float, bool>, out: List<float> }

// The donors `id` inherits from, nearest first: a depth-first preorder from
// its direct donors. The visited set drops shared ancestors and ends cycles;
// an excluded donor is skipped along with everything only it donates.
let donors = (graph: Map<float, List<float>>, e: edits, id: float): List<float> =>
  let excluded = Map.fromList(List.map((d) => (d, true), e.removed)) in
  let start: walk = { stack: List.append(direct(graph, id), e.added), seen: Map.fromList([(id, true)]), out: [] } in
  let step = (w: walk): walk =>
    match w.stack with
    | [] => w
    | [d, ..rest] =>
      if Map.member(d, w.seen) || Map.member(d, excluded) then { w with stack: rest }
      else { stack: List.append(rest, direct(graph, d)), seen: Map.insert(d, true, w.seen), out: List.append([d], w.out) } in
  loop(step, (w: walk) => List.isEmpty(w.stack), start).out

// Add (`add`) or remove the relation `id → meta` at runtime, by
// shock2quest's rules: adding a present relation or removing an absent one
// changes nothing (`None`); removing one `id` inherits from its authored
// donors records an exclusion; adding it back clears the exclusion.
let relate = (graph: Map<float, List<float>>, add: bool, meta: float, id: float, e: edits): Option.t<edits> =>
  let authored = has(meta, donors(graph, noEdits, id)) in
  let present = has(meta, e.added) || (authored && not has(meta, e.removed)) in
  let others = (xs: List<float>) => List.filter((x) => x != meta, xs) in
  if add == present then Option.None
  else if add then
    Option.Some({ added: if authored then e.added else List.append(e.added, [meta]), removed: others(e.removed) })
  else Option.Some({ added: others(e.added), removed: List.append([meta], others(e.removed)) })

// --- Properties ---------------------------------------------------------------

// The first of `ids` with a value in `table`.
let first = (table: Map<float, 't>, ids: List<float>): Option.t<'t> =>
  List.fold((found, d) => match found with
    | Option.Some(_) => found
    | Option.None => Map.get(d, table), Option.None, ids)

// A "latest" property at spawn: the object's authored value is its own,
// else its nearest donor's is inherited. `authored` holds every authored
// record, objects' and donors' alike.
let spawn = (authored: Map<float, 't>, ds: List<float>, id: float, live: Map<float, Cell<'t>>): Map<float, Cell<'t>> =>
  match Map.get(id, authored) with
  | Option.Some(v) => Map.insert(id, Own(v), live)
  | Option.None => recompose(authored, ds, id, live)

// Re-derive an inherited cell from the object's donors now; an own cell
// stays, so a metaproperty never overrides the object's own value.
let recompose = (authored: Map<float, 't>, ds: List<float>, id: float, live: Map<float, Cell<'t>>): Map<float, Cell<'t>> =>
  let own = match Map.get(id, live) with
    | Option.Some(c) => isOwn(c)
    | Option.None => false in
  if own then live
  else match first(authored, ds) with
    | Option.Some(v) => Map.insert(id, Inherited(v), live)
    | Option.None => Map.remove(id, live)

// An accumulating property's contribution (scripts): its items, and whether
// donors' items follow them.
type contribution<'t> = { items: List<'t>, inherits: bool }

// The effective value: the object's own contribution (`local`), then each
// donor's in order, up to and including the first that doesn't inherit.
let accumulate = (local: Map<float, contribution<'t>>, authored: Map<float, contribution<'t>>, ds: List<float>, id: float): List<'t> =>
  let add = (acc, c: contribution<'t>) => match acc with
    | (items, open) => if open then (List.append(c.items, items), c.inherits) else acc in
  let own = match Map.get(id, local) with
    | Option.Some(c) => add(([], true), c)
    | Option.None => ([], true) in
  match List.fold((acc, d) => match Map.get(d, authored) with
      | Option.Some(c) => add(acc, c)
      | Option.None => acc, own, ds) with
  | (items, _) => items

// Store `id`'s effective accumulated value, or drop it when empty.
let cache = (id: float, items: List<'t>, table: Map<float, List<'t>>): Map<float, List<'t>> =>
  if List.isEmpty(items) then Map.remove(id, table) else Map.insert(id, items, table)

// --- Links --------------------------------------------------------------------

// One link: source, destination, the flavor's data, and the donor it was
// copied down from (`None` for the object's own).
type link<'d> = { src: float, dst: float, data: 'd, donor: Option.t<float> }

// A flavor's links by id, with source and destination indexes (nested maps
// as multimaps). `next` is the id the next new link gets.
type links<'d> = { byId: Map<float, link<'d>>, bySrc: Map<float, Map<float, bool>>, byDst: Map<float, Map<float, bool>>, next: float }

let noLinks = (): links<'d> => { byId: Map.empty(), bySrc: Map.empty(), byDst: Map.empty(), next: 1.0 }

let index = (key: float, id: float, on: bool, m: Map<float, Map<float, bool>>): Map<float, Map<float, bool>> =>
  Map.update(key, (ids) =>
    let next = if on then Map.insert(id, true, Option.defaultValue(Map.empty(), ids)) else Map.remove(id, Option.defaultValue(Map.empty(), ids)) in
    if Map.size(next) == 0.0 then Option.None else Option.Some(next), m)

// Add a link under a given id (an authored link keeps its own).
let linkAs = (id: float, l: link<'d>, t: links<'d>): links<'d> =>
  { byId: Map.insert(id, l, t.byId), bySrc: index(l.src, id, true, t.bySrc), byDst: index(l.dst, id, true, t.byDst),
    next: Math.max(t.next, id + 1.0) }

let link = (l: link<'d>, t: links<'d>): links<'d> => linkAs(t.next, l, t)

let unlink = (id: float, t: links<'d>): links<'d> =>
  match Map.get(id, t.byId) with
  | Option.Some(l) => { t with byId: Map.remove(id, t.byId), bySrc: index(l.src, id, false, t.bySrc), byDst: index(l.dst, id, false, t.byDst) }
  | Option.None => t

let fromIndex = (key: float, m: Map<float, Map<float, bool>>, t: links<'d>): List<link<'d>> =>
  Map.get(key, m) |> Option.defaultValue(Map.empty()) |> Map.keys
    |> List.concatMap((id) => Option.toList(Map.get(id, t.byId)))

// Links out of / into an object, in id order.
let from = (src: float, t: links<'d>): List<link<'d>> => fromIndex(src, t.bySrc, t)
let into = (dst: float, t: links<'d>): List<link<'d>> => fromIndex(dst, t.byDst, t)

// Inherited links follow the donor delta: links copied from donors that
// left go, links of donors that joined are copied down tagged with their
// donor, and the object's own (and live) links stay.
let inherit = (authored: links<'d>, before: List<float>, after: List<float>, id: float, t: links<'d>): links<'d> =>
  let left = (lid: float) => match Map.get(lid, t.byId) |> Option.bind((l: link<'d>) => l.donor) with
    | Option.Some(d) => not has(d, after)
    | Option.None => false in
  let dropped = Map.get(id, t.bySrc) |> Option.defaultValue(Map.empty()) |> Map.keys |> List.filter(left)
    |> List.fold((acc, lid) => unlink(lid, acc), t) in
  after |> List.filter((d) => not has(d, before))
    |> List.concatMap((d) => from(d, authored))
    |> List.fold((acc, l: link<'d>) => link({ l with src: id, donor: Option.Some(l.src) }, acc), dropped)

// --- Messages -----------------------------------------------------------------

// A FIFO keyed by sequence number: `head` is the next to handle, `tail` the
// next free. `carried` counts the drains that left messages for a later
// frame, so a replay sees the same carry-over.
type queue<'m> = { head: float, tail: float, items: Map<float, 'm>, carried: float }

let queue = (): queue<'m> => { head: 0.0, tail: 0.0, items: Map.empty(), carried: 0.0 }
let push = (m: 'm, q: queue<'m>): queue<'m> => { q with tail: q.tail + 1.0, items: Map.insert(q.tail, m, q.items) }
let pending = (q: queue<'m>): float => q.tail - q.head

// The frame's one drain: handle up to `cap` messages in order, including
// those the handled ones send. `handle` gets the world with the message
// already taken off the queue; what's left waits for the next frame.
let drain = (cap: float, handle: ('w, 'm) => 'w, read: ('w) => queue<'m>, write: (queue<'m>, 'w) => 'w, w0: 'w): 'w =>
  let step = (s) => match s with
    | (w, n) =>
      let q = read(w) in
      match Map.get(q.head, q.items) with
      | Option.Some(m) => (handle(write({ q with head: q.head + 1.0, items: Map.remove(q.head, q.items) }, w), m), n + 1.0)
      | Option.None => (w, cap) in
  match loop(step, (s) => match s with | (w, n) => n >= cap || pending(read(w)) == 0.0, (w0, 0.0)) with
  | (w, _) =>
    let q = read(w) in
    if pending(q) > 0.0 then write({ q with carried: q.carried + 1.0 }, w) else w
