# Material and effects gallery

`debug_nd_materials` is a walkable preview of material effects using the
installed remaster's art. Six labeled stations show metal sparks, plasticrete
dust/chips, glass shards, blood, goo, and an electrical core with arcs/glow.
Bursts repeat every three seconds. Walk closer to inspect the samples; no weapon
or controller gesture is required. Metal and plasticrete decal samples sit
below their effects.

```bash
cargo dr --mission debug_nd_materials --vr
# Headless capture / HTTP control:
cargo dbgr --mission debug_nd_materials --vr --port 8080
# Omit --vr to compare the flatscreen presentation.
```

The stations share world positions and simulation timing in both presentations.
They preview independently tuned effects rather than changing campaign bullet
impacts. The decal samples use the current model importer and base material;
additional view-angle shine passes are future work. Glow is a world-space sprite
and does not cast light on surrounding surfaces.

The gallery exercises per-particle texture animation, growth/shrink, fade-in,
and existing fade-out. It owns and resets a bounded set of particle systems;
effect bursts do not accumulate entities. Installed particle/decal assets are
loaded through the regular asset cache. A missing particle sprite logs a warning
and uses the existing glow disk fallback; missing decal models are reported.

For a repeatable capture, place the free camera at `[8.5, 2, 0]`, looking at
`[-4, 1.6, 0]`. The six stations run along Z from `5.25` to `-5.25`, spaced
`2.1` units apart, with effects at Y `1.65`. Capture early and late within each
three-second cycle to see motion and disappearance. Particle randomization is
not seeded, so fixed simulation steps reproduce timing but not exact pixels.

Verification uses flatscreen and headless `--vr` presentation for now. It does
not establish headset stereo quality or device GPU cost.
