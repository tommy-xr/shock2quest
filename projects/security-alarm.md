# Security alarm and wrist integration

PR #1223 is rebased on main after #1515. Its older forearm rendering was
replaced with the current glove/wrist path. The local post-#1515 refinements
(centered toxin pips, optional-state rows, original-art toxin frame and 45°
lower-edge hinge) are included here because they were not in the merged commit.

## State and interactions

A camera's authored SwitchLinks deliver Alarm to its linked ecologies.
`SecurityAlarm` records the count and remaining badge deadline; it does not
broadcast Alarm to unrelated ecologies. A hacked linked ecology cannot admit a
camera alarm merely because another ecology elsewhere is normal. Durations
come from the linked ecology's authored alert recovery.

Original `references/darkengine/src/shock/shkalarm.cpp`:
- `ShockAlarmAdd` increments the count and writes HackTime on every add.
  The badge deadline therefore updates on a repeat alarm. Ecology recovery
  remains independently governed by `TriggerEcology` and does not restart.
- `ShockAlarmDisableAll` sends Reset to alerted ecologies, whose links reset
  associated cameras. The port also clears the bookkeeping when the deadline
  expires or the ecologies finish recovering.

Count and remaining time are saved per mission in
`EntitySaveData`. Held inventory carries no alarm snapshot, and cannot overwrite
mission state during load. Restoration treats message delivery as complete,
because queued script messages are not saved: a restored badge with no alerted
ecology clears on its next update. This also covers saves made just after an
ecology reset. Older saves without this optional field load without alarm bookkeeping;
their existing ecology/script state is preserved.

Security computers reuse the existing `ComputerGui` HRM board, skill checks,
node interactions and nanite payment. Opening the board does not clear the
alarm; a successful hack emits `ClearSecurityAlarm`. The console remains usable
for later alarms. This covers alarm stand-down, not a new timed player-invisibility
or security-shutdown power. Non-camera alarm sources and separate klaxon/audio
objects remain outside this PR.

## Presentation

The original `ALARM.PCX` supplies teal lettering and its red warning mark.
The countdown uses the shared teal `MFD_FONT` (MAINAA with the cyan palette),
not the default untinted font. Both renderers resolve the same font and layout.
The badge retains original 75/255 opacity; screenshots show that it is faint
over textured glove surfaces, so physical headset readability remains a
follow-up.

Flat and cyber draw the same 64×80 badge/countdown at (10,260). The older PR's
(10,278) origin would put its countdown into the toxin row at y345; the new
origin clears the hazard block without changing toxin/radiation placement.
Flat use mode delegates to the cyber canvas so the alarm is emitted once.

VR uses the same original badge and countdown layout on the hazard display's
45° lower-edge hinge. A pixel occupies 1.25 mm: hazards stay 16 cm wide, the
alarm is 8 cm wide. The alarm is centered when alone, or offset 13 cm beside
the hazards (a 1 cm gap). Weapon-skill warnings move higher when either warning
is active so they do not obscure it. Health/psi stay flush on the bracelet.

## Verification

- Natural camera identification, countdown ticking, paid HRM success, stand-down,
  and fresh re-alarm are exercised through the SDK.
- A fresh runtime loads a real MedSci save with the camera, alarm and countdown
  intact. Unit tests cover mission/held-data separation and the reset/save and pending-message edges.
- Geometry tests check the common lower hinge, 45° outward rise and handedness.
- Flat, cyber and VR captures include alarm-only and simultaneous rad/toxin states,
  countdown GIFs and an oblique wrist view. Countdown color is visibly teal.
- Strict core/runtime checks pass. Full regression results are recorded in the PR.
