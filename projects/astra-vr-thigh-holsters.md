# VR thigh holsters

The right thigh carries one weapon independently of backpack capacity. Pack Rat
adds a left thigh slot as well as its existing backpack capacity bonus. Either
hand can use either unlocked slot; the psi amp and ordinary pickups are excluded.

Bring a weapon into the cyan marker, then release the grip to stow it. Reaching
while still squeezing does not stow. A fresh squeeze with an empty hand draws
the same weapon, preserving ammunition and condition. The marker turns green
on approach. An occupied slot refuses with a red marker, message and sound,
retaining the held weapon until the player deliberately squeezes again. The
existing weapon is never swapped or discarded.

The slot owns the actual entity, with no backpack duplicate or loose physics
body. Ownership and the calibrated held size survive saves and level transitions.
The world model is displayed barrel-down, matching the held weapon's longest
extent, capped at 45 cm for large weapons. An occupied second slot remains
retrievable if Pack Rat is removed. Loading the save in flatscreen returns
holstered items to the backpack; overflow drops visibly in front of the player.

## Calibration and verification

Use `cargo dbgr --mission debug_interactions --vr`. Developer parameters:

- `vr_holster_zones`: show the full 14 cm grab radius.
- `vr_holster_drop`: vertical offset below the tracked head, default 0.78 m,
  range 0.55–1.10 m. Reduce this when testing seated reach.
- `vr_holster_side`: lateral offset, default 0.23 m, range 0.16–0.40 m.

Targets use horizontal heading with gentle following and freeze that heading
while a hand is in a slot. They are estimates from head/controller tracking,
not tracked legs. Opening the cyber interface disables gestures but keeps worn
weapons visible. Invalid tracking cannot invent a deposit or draw. A support or
climbing hand cannot draw, and drawing with the trigger already held is safe
until the trigger is released.

`/v1/info` publishes `player.hand_feedback.holsters`: world centers, grab radius,
enabled slots, nearby slot per hand, occupant IDs and refused-release latches.
Hand arrays are left/right; slot arrays are right/left. Discover entity IDs each
run rather than hardcoding them.

The SDK `vr-holsters.e2e.test.ts` exercises both hands, occupied refusal, full
backpack independence, trigger safety, condition/ammo preservation, real mission
save/load, level transitions, and flat accessibility. Pure tests cover tracking,
simultaneous hands and Pack Rat gating. Shoulder-backpack tests protect the
shared heading/refusal logic.

Headset acceptance still needs standing/seated reach, ordinary arm swings, and
weapon visibility checks. Additional per-model holstered orientation, a preferred-leg
selector, editor controls and haptics are follow-up refinements; the current
increment adapts the shotgun/wrench axis corrections from #1399 and uses
developer offsets.
