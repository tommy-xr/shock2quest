# Turret articulation

The 25AE replacements for `tu_f`, `tu_l`, and `tu_s` swap the cap and gun's
sub-object indices. Their authored parameters and hierarchy are unchanged:

| Part | Original index | 25AE index | Parameter | Motion |
| --- | --- | --- | --- | --- |
| Base | 0 | 0 | -1 | fixed |
| Cap | 2 | 1 | 0 | slide |
| Gun (child of cap) | 1 | 2 | 1 | rotate |

The old turret script translated index 2 and rotated index 1, including an
extra -90 degree correction. With the remaster it translated the gun and
rotated the cap. It also used a fixed muzzle offset unrelated to the model.

## Authored motion

`ObjectArticulation` preserves LGMD sub-object type, parameter ID, hierarchy,
axle transform and vhot ownership. Parameter IDs are independent of sub-object
indices and can drive multiple parts. `SetObjectParameters` resolves them to
the renderer's bone transforms. The same evaluator resolves the muzzle vhot
through its owning part and all parents, using the current frame's parameters.

The reference is Dark's
[`mds_subobj`](https://github.com/infernuslord/DarkEngine/blob/master/tech/libsrc/md/mds.h)
and [`md_start_subobj`](https://github.com/infernuslord/DarkEngine/blob/master/tech/libsrc/md/render.c):
enter the authored axle frame, then rotate or translate about local X. Our
coordinate conversion maps Dark `(x,y,z)` to `(-x,z,y)`, so that axle becomes
local -X. Rotations receive degrees; translations receive Dark feet. The
model's range metadata is not an animation fraction or a renderer clamp.

The turret reads `AIDevice` for activation/rotation parameter IDs, inactive and
active positions, activation speed, rotation activation, and facing tolerance.
The shipped turret settings are parameter 0, positions 0 and 2, speed 0.1,
rotation parameter 1, and tolerance 0.04 radians. Two Dark feet become 0.8
renderer units, raising the cap and its gun child together.

[`cAIJointSlideAction::Enact`](https://github.com/infernuslord/DarkEngine/blob/master/cam/src/ai/aiactjs.cpp)
increments by `activateSpeed` each AI frame and ignores `deltaTime`. We normalize
that legacy per-frame increment at the debug simulation's 60 Hz, then integrate
with elapsed time in every runtime. Thus these settings take 20 simulation
frames to open; this is an explicit frame-rate-independent timing policy,
not a claim that the original frame-dependent code had one fixed duration.

## Facing and closing

An LGMD object's authored forward is Dark +X (our model -X). World targets are
transformed into the mounted model's local frame. Only the FOV/projectile
orientation boundary converts that direction to the helpers' +Z forward;
there is no correction applied to the gun's bone.

Turning takes the shortest angular path at `AI_TurnRate` degrees/second, with
the original [`AIGetTurnRate`](https://github.com/infernuslord/DarkEngine/blob/master/cam/src/ai/aiprcore.h)
default of 380. Firing requires an open turret, visible target, and facing
within the authored epsilon. Loss of sight stops firing. The gun returns to
parameter zero before lowering, as in
[`cAIDevice::DeactivateSuggestActions`](https://github.com/infernuslord/DarkEngine/blob/master/cam/src/ai/aidev.cpp).
Reacquisition reverses opening from the current height. Turret phase, height,
facing and shot cooldown persist through BaseMonster's script-state payload.

## Verification

- Parser and pose tests cover parameter IDs, shared parameters, reordered
  parts, sparse attachment IDs, parent motion, and axis conversion.
- Turret tests cover time steps, facing relative to rotated mounts, firing
  gates, shortest-path rotation, return-before-close, reacquisition and state
  hydration through BaseMonster.
- `tools/shock2-sdk/test/turret-articulation.e2e.test.ts` checks real assets in
  both presentations: fixed base, both parts rising 0.8 units, and full closure.
- Before/after media uses identical fixed-step requests with the 25AE assets,
  approaching from -Z (old incorrect FOV) and +X (authored facing), then moving
  sideways and withdrawing. The player must remain alive throughout.
