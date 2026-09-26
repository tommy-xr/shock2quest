# Shoulder backpack

Hold an item, move the controller behind either shoulder, and release grip to
put it in the backpack. Either hand can use either shoulder. Moving through a
zone while still squeezing does nothing. Retrieve stored items through the
existing inventory interface.

Successful stowing plays the inventory confirmation sound and displays “Stored
in backpack.” The existing entity moves into a real inventory cell, preserving
ammo and item state. Collectible credentials use the existing collection path.
If no cell fits, the item stays held with a refusal sound and message: squeeze
again to re-grip, then move it somewhere else and release normally.

## Placement and diagnostics

Each zone is a sphere of radius 18 cm, centered 18 cm to the side, 20 cm below,
and 18 cm behind the tracked eyes. A release must also be at least 3 cm behind
the eye plane. The zones follow head translation and a smoothed horizontal
heading; pitch and roll do not tilt them. Heading stops following while a held
hand is inside a zone. These are head-based estimates, not tracked shoulders.

The gesture is VR-only and disabled while the cyber interface is open, the
player is dead, or controls are disabled. It requires tracked head/hand poses
and a prior squeeze holding the same item. Tracking recovery alone cannot
cause a deposit.

Enable **Backpack zones** (`vr_backpack_zones`) to draw cyan targets, green
while reached, and red after a refused deposit. `/v1/info` exposes world-space
centers, radius, per-hand proximity and retention under
`player.hand_feedback.shoulder_backpack`. The arrays use left/right order.
The debug overlay is off by default; it can be viewed from an external debug
camera for automated captures.

## Verification

- Unit tests cover both hands and shoulders, release edges, invalid tracking,
  disabled gestures, heading/height changes, retention and simultaneous cell
  reservations.
- `tools/shock2-sdk/test/vr-shoulder-backpack.e2e.test.ts` exercises real pickup,
  stowing and retrieval with a mug and pistol, ammo preservation, full-pack
  refusal/re-grip and ordinary world drops.
- Existing cyber-interface deposit tests cover the shared release-to-inventory
  rewrite used by both interactions.

Before headset acceptance, check comfortable seated and standing reaches,
looking sideways while reaching, real controller tracking behind the shoulder,
and the refusal cue with a full backpack. Desktop VR emulation verifies the
state transitions but cannot establish the physical comfort or tracking envelope.

This increment adds depositing only. Shoulder retrieval, belt pouches, leg
holsters and editable body anchors remain separate work.
