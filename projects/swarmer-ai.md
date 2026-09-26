# Swarmer particle-cloud AI

This layer builds on GrubAI's shared awareness and BaseMonster save envelope.
`SwarmerAI` is another sibling of `AnimatedMonsterAI`: it drives the position of
the existing authored particle group, not skeletal clips or individual bug bodies.

## Original data and behavior

The shipped Swarm (-183) has 36 HP, zero gravity, a zero-radius physical point,
70 particles using the `bug` model, and an Anti-Human radius stimulus. Its parsed
movement speed is 1.0 (the move enactor applies Dark's 7.5 multiplier) and its
`AI_MoveZO` hover offset is 2.0 world units. `AI_Swarm` supplies close/backoff
ranges when authored; the original defaults are one and ten Dark feet.

The original `cAISwarmer` adds a four-second sine wave with half a Dark foot of
amplitude to the ground offset. Its combat ability closes on the target, then
chooses a clear retreat direction. The separate `Swarm` object script slays it
after twenty seconds. These are separate responsibilities here too.

## Implementation

- `MobileAwareness` handles sight, noise, alertness caps and target memory.
- The existing chase/wander/fixed-point path followers supply horizontal
  navigation. Real navigation failures never become direct wall-crossing chase;
  scenes without navigation may steer directly toward a visible target.
- The hover motor tracks floor height and sweeps a finite core through the
  intended flight volume. Nearby headings allow local avoidance, and Rapier
  handles the remaining physical contacts. It does not open doors.
- The particle emitter moves with the cloud; particle animation is unchanged.
- Radius stimuli use the existing receptron, falloff and obstruction handling
  without blast force. The effect now carries its source identity so its own
  body cannot block the exposure ray. Existing ambient radiation keeps its
  previous source-less behavior.
- The AI snapshot preserves bob phase, retreat destination, action budget,
  awareness and pulse timing. Paths are recomputed on loading. The object
  script preserves its remaining lifetime and sends Slay once at expiry.
  Stasis pauses flight and damage through BaseMonster; the independent lifetime
  timer continues, like the original script timer.

## Explicit approximations

The authored zero-radius body is unsuitable as a stable, aimable Rapier body.
A radius-.3 core represents the cloud; individual visual bugs have no hitboxes.
The close threshold has a 1.0-unit minimum to account for the core and player
collision volumes. The action retry budget is a fixed four seconds within the
original three-to-five-second range. Proximity damage pulses every .5 seconds;
the generic Dark stimulus-source timing records are not implemented here.
Navigation remains ground routes plus hover, rather than a new volumetric flight
navigation system. These choices are local to the swarmer controller.

## Verification

- Unit checks cover authored distance conversion, hover period/amplitude,
  saved retreat/pulse state, wall/ceiling clearance and lifetime expiry/restore.
- `swarmer-ai.e2e.test.ts` checks natural hatch, hover/pursuit/backoff, player
  damage, expiry, and the real rec1 pod's save/load lifetime.
- Visual capture uses the debug_annelid station and rec1 swarmer pod264, with
  before/after sequences and both flat and VR presentations.

## References

- [Original swarmer behavior](https://github.com/DeathEngine2/LookingGlass-DarkEngine/blob/main/thief_2_service_release/rdrive/prj/thief2/src/SHOCK/SHKAISWM.CPP)
- [Original close/backoff combat](https://github.com/DeathEngine2/LookingGlass-DarkEngine/blob/main/thief_2_service_release/rdrive/prj/thief2/src/SHOCK/SHKAISWA.CPP)
- [Original swarm property defaults](https://github.com/DeathEngine2/LookingGlass-DarkEngine/blob/main/thief_2_service_release/rdrive/prj/thief2/src/SHOCK/SHKAIPR.CPP)
- [Telliamed's Swarm script reference](https://thiefmissions.com/telliamed/allscripts.html)
