# VR thigh holsters

The right thigh carries one small weapon independently of backpack capacity. Pack Rat
adds a left thigh slot as well as its existing backpack capacity bonus. Either
hand can use either unlocked slot. Only melee weapons, the pistol and the laser
pistol fit; larger guns, the psi amp and ordinary pickups are excluded. A large
gun released at the slot is refused and retained, with a prompt to use a shoulder.

Bring a weapon into the cyan marker, then release the grip to stow it. Reaching
while still squeezing does not stow. A fresh squeeze with an empty hand draws
the same weapon, preserving ammunition and condition. The marker turns green
on approach. An occupied slot refuses with a red marker, message and sound,
retaining the held weapon until the player deliberately squeezes again. The
existing weapon is never swapped or discarded.

The slot owns the actual entity, with no backpack duplicate or loose physics
body. Ownership and the calibrated held size survive saves and level transitions.
The world model is displayed barrel-down, matching the held weapon's longest
extent, capped at 45 cm. An occupied second slot remains
retrievable if Pack Rat is removed. Loading the save in flatscreen returns
holstered items to the backpack; overflow drops visibly in front of the player.

## Calibration and verification

Use `cargo dbgr --mission debug_interactions --vr`. Developer parameters:

- `vr_holster_zones`: show the full 14 cm grab radius.
- `vr_holster_drop`: vertical offset below the tracked head, default 0.78 m,
  range 0.55–1.10 m. Reduce this when testing seated reach.
- `vr_holster_side`: lateral offset, default 0.23 m, range 0.16–0.40 m.
- `vr_holster_forward`: forward/back offset, default 0.04 m, range −0.20–0.30 m.
- `vr_belt_distance`: distance to the front of the belt, default 0.20 m,
  range 0.10–0.45 m. The ammo pouch and its target follow the belt, 5 cm farther
  forward. This compensates for the belt mesh's authored 30 cm offset.

All placement settings take effect live, in metres, without changing hand poses
or holstered weapon scale. Belt distance and thigh placement are independent.
If calibration overlaps the pouch and a holster, an empty hand with a gun in the
opposite hand uses the pouch there; one squeeze cannot draw both objects.

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

## Shoulder weapon recall

The backpack remembers the last weapon successfully stored at each shoulder:
left and right independently, regardless of which hand deposited it. Reach
behind that shoulder with an empty hand and squeeze freshly to retrieve the
actual weapon. Holding squeeze while entering the zone never retrieves it.

Weapons still occupy normal backpack cells. A full backpack refuses the deposit
and preserves the previous shortcut. A newer weapon replaces that shoulder's
shortcut but leaves the previous weapon in the normal inventory. Ordinary items
do not replace weapon shortcuts. Taking a remembered weapon out makes its
shortcut unavailable until it returns to the backpack; dropping it into the
world or another container clears the assignment. Depositing it at the
other shoulder reassigns it there. Empty shoulder draws
play a refusal cue, and a consumed squeeze cannot grab a stray world object when
the hand moves away. Support hands and untracked/disabled input cannot draw.

Bookmarks are per-entity runtime components saved and remapped with the normal
inventory through save/load and level transitions. Only current backpack members
can be recalled. The debug snapshot exposes `hand_feedback.body_gear.shoulder_weapons`
in left/right order. `vr-shoulder-weapon-recall.e2e.test.ts` exercises the complete
loop, replacement, independent sides, cross-hand recall and persistence.
