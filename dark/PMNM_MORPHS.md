# PMNM morph foundation

Verified against the owned 25AE install `/Users/bryan/ss2-25th`, sentinel
`sshock2.kpf`, upgraded `mods/sshock2ee.kpf`: all 66 PMNM meshes parse; 31 have
morphs, 26 have MouthShapeA–H. Names are fixed 16-byte strings. Each target has
`morph_vertices` 32-byte records, in target-major order, packed as position xyz,
normal xyz, UV. The final uint16 array identifies the affected **zero-based base
vertices**, not triangle corners or LGMD's one-based shape indices.

Every shipped Basis record is zero. Records are additive offsets, including
normals, not replacement positions or normals. The shipped
`base.kpf/progs/DarkEMMModel.shader`, in both shader configuration branches,
weights and sums position/normal/UV offsets before four-bone skinning. Its GPU
texture packing differs from the file: two vec4 rows hold position+U and normal+V.
Position deltas use the existing Dark (-x,z,y) conversion and SCALE_FACTOR;
normal deltas only change axes. Bone indices/weights and bind matrices are kept.

The shared MorphMesh starts each changed draw from immutable base vertices.
Named PMNM channels generalize the existing three-channel LGMD plant buffer;
material splits and extra material/depth passes share geometry but provide their
own instance weights. No facial timing, gameplay driver, collision deformation,
or retail animation activation is claimed. This implements the file and render
foundation; the authored names do not establish when retail gameplay uses them.

## Focused preview

Launch the debug runtime against the explicit 25AE asset root. Discover a visible
entity by name/template and step first. GET `/v1/scene?entity_id=N` lists
`morph_names` in channel order. POST `/v1/scene/morphs` with:

```json
{"entity_id":123,"weights":{"MouthShapeA":0.5,"MouthShapeH":0.5,"worm":1.0}}
```

The number above is illustrative; discover it every run. Each request replaces
that entity's blend; omitted names have zero weight. An empty `weights` object
releases the preview. Unknown names, invisible/nonmorph entities, non-finite
weights, and weights outside [-1,1] are rejected. Preview weights are session-only
and cleared on scene changes. Skinning continues normally. The same endpoint
works in flat and `--vr`; ordinary entities remain neutral until a driver supplies
weights. No asset is modified.

Drivers: `worm` (`shock2vr/src/monster_idle.rs`) and, while an `ND-lipsync.dml`
`Face` archetype speaks, the authored `facepos.str` mouth shapes
(`shock2vr/src/lip_sync.rs`). A preview overrides both.
