// Affine — 3×4 transforms (rotation/scale rows with a translation) for posing
// Dark models in native axes (Z up). Points are `[x, y, z]` lists.

type t = {
  a: float, b: float, c: float, x: float,
  d: float, e: float, f: float, y: float,
  g: float, h: float, i: float, z: float
}

let identity: t = {
  a: 1.0, b: 0.0, c: 0.0, x: 0.0,
  d: 0.0, e: 1.0, f: 0.0, y: 0.0,
  g: 0.0, h: 0.0, i: 1.0, z: 0.0
}

let translation = (x: float, y: float, z: float): t =>
  { identity with x: x, y: y, z: z }

let n = (k: float, v: List<float>): float => Option.defaultValue(0.0, List.nth(k, v))

// `outer` after `inner`.
let compose = (outer: t, inner: t): t => {
  a: outer.a * inner.a + outer.b * inner.d + outer.c * inner.g,
  b: outer.a * inner.b + outer.b * inner.e + outer.c * inner.h,
  c: outer.a * inner.c + outer.b * inner.f + outer.c * inner.i,
  x: outer.a * inner.x + outer.b * inner.y + outer.c * inner.z + outer.x,
  d: outer.d * inner.a + outer.e * inner.d + outer.f * inner.g,
  e: outer.d * inner.b + outer.e * inner.e + outer.f * inner.h,
  f: outer.d * inner.c + outer.e * inner.f + outer.f * inner.i,
  y: outer.d * inner.x + outer.e * inner.y + outer.f * inner.z + outer.y,
  g: outer.g * inner.a + outer.h * inner.d + outer.i * inner.g,
  h: outer.g * inner.b + outer.h * inner.e + outer.i * inner.h,
  i: outer.g * inner.c + outer.h * inner.f + outer.i * inner.i,
  z: outer.g * inner.x + outer.h * inner.y + outer.i * inner.z + outer.z
}

let apply = (m: t, v: List<float>): List<float> =>
  [ m.a * n(0.0, v) + m.b * n(1.0, v) + m.c * n(2.0, v) + m.x,
    m.d * n(0.0, v) + m.e * n(1.0, v) + m.f * n(2.0, v) + m.y,
    m.g * n(0.0, v) + m.h * n(1.0, v) + m.i * n(2.0, v) + m.z ]

// The rotation part only (no translation).
let rotate = (m: t, v: List<float>): List<float> => apply({ m with x: 0.0, y: 0.0, z: 0.0 }, v)

let offset = (m: t): List<float> => [m.x, m.y, m.z]

let add = (p: List<float>, q: List<float>): List<float> =>
  [n(0.0, p) + n(0.0, q), n(1.0, p) + n(1.0, q), n(2.0, p) + n(2.0, q)]

let scale = (k: float, p: List<float>): List<float> => [k * n(0.0, p), k * n(1.0, p), k * n(2.0, p)]

// The rotation of a unit quaternion (w, x, y, z).
let quaternion = (w: float, x: float, y: float, z: float): t => {
  a: 1.0 - 2.0 * (y * y + z * z), b: 2.0 * (x * y - w * z), c: 2.0 * (x * z + w * y), x: 0.0,
  d: 2.0 * (x * y + w * z), e: 1.0 - 2.0 * (x * x + z * z), f: 2.0 * (y * z - w * x), y: 0.0,
  g: 2.0 * (x * z - w * y), h: 2.0 * (y * z + w * x), i: 1.0 - 2.0 * (x * x + y * y), z: 0.0
}
