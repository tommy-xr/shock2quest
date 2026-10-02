# Energy Reflection

Energy Reflection (`AntiPsi`, gamesys template -3153) contributes its authored
receptron filters to the player while the sustained power is active. Contact
and radius stimuli both consume those filters before producing untyped Damage.
The existing power timer provides cost, refresh, expiry and save persistence.

The power authors Abort for Psi Stim and Amplify 0.5 for Incendiary, High
Explosive, Electricity, Cold, Droid Fusion and Energy Stim. The implementation
reads those links, including inherited links; it has no damage-type table.
Psycho-reflective Screen still applies its uniform factor once to final Damage,
including untyped damage. Immolate still contributes its fire immunity once.
Neither existing consumer is duplicated by the registry. Tier-five
Psycho-reflective Aura (`PsiShield`, -1019) also contributes its nine authored
Amplify 0.4 filters while active. Both defenses filter typed stimuli before
damage; untyped damage is unchanged.

## Authored-data corrections to #1276

The installed 25th Anniversary gamesys differs from the issue's suggested
acceptance fixture:

- Blue Monkey (-1431) fires Blue Monkey Shot (-2210), whose corpse Cryo
  Explosion (-1947) emits **Cold**, not Psi Stim. Red Monkey emits Incendiary.
  Reflection should halve these attacks, not eliminate them.
- Human Vulnerability (-1230), inherited by The Player (-384), has no Psi Stim
  damage response. Psi Mine Explosion (-3756) therefore already causes zero
  player HP damage before Reflection is cast. The regression preserves this;
  a resolver test with a synthetic vulnerable receiver verifies Abort priority.
- Laser Turret (-168) fires Turret Laser Bolt (-1414), which emits Energy Stim.
  A real hit is 10 HP unshielded and 5 HP with Reflection, then 10 after expiry.

[Telliamed's script reference](https://thiefmissions.com/telliamed/allscripts.html)
lists AntiPsi itself as an inert RootPsi script; the defense comes from the
metaproperty's receptrons, not a missing bespoke script implementation.

## Defense stations

`debug_psi` retains the original hybrid pen and adds two separated lanes:
Blue Monkey at (-12, 1, 40), and Laser Turret at (-12, 0.8, 80). Stand eight
world units in front of the attacker, facing -X. The monkey's radius explosion
has falloff, so a one-HP difference between repeated hits is expected as it moves.
The SDK regression measures live attacks before, during and after Reflection,
and verifies a pipe hybrid's WeaponBash remains unchanged.

The exact-base comparison uses the existing `debug_turret` scene, provisions a
psi amp and PSI/Endurance 6, casts AntiPsi, then enters the turret's firing lane.
This isolates the gameplay fix from the newly added scene fixtures.
