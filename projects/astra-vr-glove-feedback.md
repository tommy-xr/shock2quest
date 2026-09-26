# Glove feedback lights

This layer sits above `astra-vr-grip-device-validation`. It adapts the emissive
mask and skinned-material support from Fable #1373, and the eligibility-light
idea from #1376, while keeping Astra fitting, trigger poses and support grips.

| Light | Meaning |
| --- | --- |
| Off | No recognized action in reach, or the hand is holding/supporting an item |
| Green | The current ray target can be grabbed or has a recognized world-use action |
| Amber | A recognized use target is locked against the player |
| Red | A Frob attempt was made at that blocked target; lasts 0.25 seconds |

The empty hand resolves one ray target. Squeeze behavior and feedback share
`HandTarget`; hover messages read the same hit. Eligibility reuses production
pickup/script and credential-aware lock checks. Green describes an available
interaction, not a guarantee that every arbitrary object script will succeed.
Red currently covers known lock refusals, not all possible script failures.

Losing reach/visibility clears the hover immediately. Failure timing uses
elapsed simulation seconds, not frame counts. Supporting, climbing-suppressed,
untracked and occupied hands do not advertise a world pickup action. The
frontend and Explorer previews retain unlit gloves. No finger preshape is added.

`assets/vr_glove_emissive.png` is a paintable mask in the existing glove UV atlas;
its red channel multiplies the selected RGB tint. It marks small fingertip accents and
back-of-hand stripes; the cuff interior stays unlit. `tools/make_vr_glove_emissive.py` is the borrowed bootstrap,
not a build step: preserve manual edits to the PNG. `HandLight::tint` controls
brightness/colour. Immutable material sets prevent one hand recolouring another.
Missing mask falls back to the existing textured glove.

Debug inspection: `/v1/info` → `player.hand_feedback` reports each hand's target,
affordance and displayed light. The SDK regression tests use the mug and the
visible ready/locked BaseButton fixtures in `debug_interactions`, checking that
the red pulse accompanies the real `hackfail` refusal. A real-mission smoke test
covers the mission wrapper; invisible eng1 proxy buttons are not ray fixtures.

Headless captures verify mask placement and state changes. Headset review is
still needed for perceived brightness and readability in peripheral vision.
