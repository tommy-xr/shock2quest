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

- `vr_holster_zones`: show the full grab radius.
- `vr_holster_radius`: reach radius, default 0.35 m (previously 0.14 m),
  range 0.14–0.45 m. It enlarges the target without enlarging the holster mesh.
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
Where both leg regions overlap, the nearest unlocked/occupied slot wins. A
shoulder region takes priority over holsters and the pouch, including when seated calibration
brings their enlarged volumes together.

Body targets test sphere-to-sphere contact against the calibrated tracked palm,
using the glove kinematics already used for held-item fitting and support hands.
They no longer test the controller aim origin at the back of the glove. The
palm center can remain outside a region while the glove surface touches it.
`vr_glove_radius` adjusts the hand sphere (default 5 cm, range 2–9 cm), and
`vr_glove_spheres` draws the exact sampled spheres: amber outside, green while
touching a body target. `hand_feedback.glove_contacts` exposes their world
centers and radius. The scope is body inventory; ordinary world-object pickup
and weapon-support contacts retain their existing policies.

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
beside or behind that shoulder with an empty hand and squeeze freshly to retrieve the
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

## Shoulder reach and feedback

The shoulder centers are 24 cm to either side, 10 cm below the tracked head and
6 cm behind it. `vr_backpack_radius` defaults to 28 cm (range 18–35 cm).
This allows an approach beside/above the shoulder while the controller remains
in view of the headset cameras. The face is excluded: a hand must be more than
10 cm to the side and no more than 8 cm forward of the head. The debug wire
spheres show the reach radius; these face-safety limits still apply inside them.

Entering with an item, or with an empty hand at a remembered weapon, requests
one 60 ms controller pulse. This signals the target, not a successful deposit:
a full backpack still retains the item and plays its existing refusal sound.
Lingering or brief boundary jitter does not repeat the pulse; a tracked exit
of at least 150 ms rearms it. Tracking loss does not rearm feedback. The MFD,
pause and invalid tracking cannot emit pulses, and unfocused XR sessions discard
them. Headless `hand_feedback.haptics.sequence` counts requests per hand;
Quest logs `SHOCK2QUEST_HAPTIC` report submission or an OpenXR error.

`vr-reach-haptics.e2e.test.ts` verifies both hands' approach/stow/recall cues,
disabled input, and a wrench stow 30 cm below the holster mesh. Pure tests cover
nearest-slot arbitration, shoulder priority and tracking/boundary hysteresis.

## Extending haptics

Gameplay emits `Effect::HandHaptic { hand, pulse }`, with hardware-independent
amplitude and duration. Only the central effect handler updates the transient
per-hand output state. The Quest runtime consumes that state once and performs
the OpenXR call; pause/loading discard unconsumed output. Same-frame requests
resolve to the strongest pulse, then longest duration for equal strength.
The request counters remain available to headless assertions.

Next increments can translate semantic weapon events into named pulse profiles:

- Successful shot: short firing-hand recoil; a weaker optional support-hand
  pulse, with weapon-specific strength/duration. Do not cue a dry fire as a shot.
- Melee enemy hit: contact pulse in the primary hand and, when attached, the
  support hand. Use real impact/damage events, not overlap every physics frame.
- Melee wall block: a distinct, lighter profile, with per-contact cooldown so
  resting a weapon against a surface never produces continuous buzzing.
- Inventory refusal: distinguish it from the light shoulder-ready tick.

Before multiple sources ship, add arbitration over the lifetime of an active
pulse as well as within a frame, so a weaker later ready tick cannot interrupt a
strong impact. Keep profiles and cooldown policy in shared gameplay, leaving
only haptic submission in platform runtimes. These weapon cues are planned;
this increment emits shoulder-ready feedback only.
