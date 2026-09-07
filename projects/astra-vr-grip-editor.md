# Astra VR grip editor

Explorer's **VR Grips** tab edits the prepared pickup and weapon grips used by the game.
It uses the game renderer, calibrated glove, and model importers directly.

From the repository root:

```sh
cargo run -p dark_explorer -- ui --grip mug
```

Select a model on the left and a hand above the preview. Shared implant models
list every fixture using them. Edits apply to all items using that model; left
and right poses are independent. Drafts stay in memory when switching models.

- **Front / back / top / oblique** frame the glove and item. **Palm** is a
  contact close-up: large objects may extend outside it. Drag to orbit; scroll
  to zoom.
- **Position**, **XYZ rotation**, **finger curls**, and **uniform item scale**
  have sliders and **− / +** nudge buttons. Click any displayed number for an
  exact final adjustment; typing is optional. Rotation values are XYZ Euler
  angles in degrees; the separately labeled nudge step controls button size.
- **Open / point / closed / ball** provide starting finger amounts. Ball cups
  the fingers; tune it to the actual object's size. Each curl runs from 0
  (open) to 1 (closed).
- Uniform item scale affects the held model, preserving glove calibration.
  Dropping the item restores its normal world size before loose physics resumes.
- **Save all edits** writes the selected library atomically: pickups use
  `assets/vr-grips.json`; weapons use `assets/vr-weapon-grips.json`. Restart the
  game/debug runtime process to load the saved resource (scene reloads can retain
  cached assets). Closing Explorer with unsaved drafts offers save, discard, or keep editing.
- **Save As…** writes a separate JSON file and makes it the active save target.
  It refuses to overwrite an existing file. Use a new filename; the original
  stays untouched. Reopen the copy with `--grip-library /path/to/file.json`.
- **Revert this hand to saved** discards just that hand's draft, if a saved
  baseline exists.
- **Automatic fitting → Replace draft with auto fit** explicitly replaces this
  hand's draft using the model's existing region/orientation hints. An optional
  family choice (cylindrical, pinch, broad grasp, trigger) guides the fit. This
  must still be reviewed and saved. Model defaults remove the manual override.

In the **Files** model picker, select a model and choose **Edit VR Grip**.
Existing entries open directly. Missing hands are fitted into new unsaved
entries using a background worker, keeping the preview responsive. Review and
save the pair with Save or Save As. `--grip <model>` also prepares missing
entries, so the same path is available for automated captures.

Manual poses carry `authored: true`. For rack models the bulk baker preserves
them if mesh, rig, hints and solver revision still match; otherwise it stops
without writing and asks for review. Entries for additional models outside the
rack are kept verbatim with their original fingerprints; gameplay still rejects
stale geometry/rig/hints. A solver revision change requires their review. Manual edits clear old solver contact diagnostics, since
those samples no longer describe the edited pose. A successful load is not a
visual approval. The scene regression still verifies holding, motion and
release, but does not assert old automatic contact counts for authored poses.

The editor rejects malformed resources and detects concurrent changes to the
resource before saving. Stale selected poses are visible for diagnosis but must
be refitted before editing/saving. Use `--grip-library /path/to/grips.json` to
work on a copy; automatic fitting still uses this checkout's model hints.

For repeatable screenshots:

```sh
cargo run -p dark_explorer -- ui --grip icepick --grip-hand left \
  --grip-view oblique --screenshot /tmp/astra-ice-editor.png
```

The native Explorer capture is used here because the debug runtime does not host
the desktop egui tool. The standalone gallery remains available for sharing and
multi-item review. Use **Pickups / Weapons** to switch libraries after saving or
reverting drafts. `--grip atek_h` opens the weapon library directly. All fourteen
weapon models preview with their authored arms removed and the calibrated glove
in their place. Melee previews use the same posed mesh as gameplay. The psi amp
is a reference-only preview retaining its integrated forearm.

Weapon auto-fitting uses authored arm geometry or the posed melee fist as a
starting guide; it cannot distinguish a support grip from a firing grip. Manual
position, rotation, curls, and uniform scale remain editable. The initial manual
weapon overrides correct primary/support-grip ambiguities; inspect the gallery
for remaining contact and clearance concerns.
Weapon scale does not resize the glove or multiply the old melee scale again.

From Files, opening a supported weapon imports its prepared default pair into the
active document when missing. This also permits a custom weapon override in the
pickup resource: a valid matching entry there takes precedence over the shipped
weapon default. A separate Save As file must be copied to a runtime resource path
to be loaded by gameplay.

## Rest and Trigger pressed poses

The finger controls offer **Rest** and **Trigger pressed** for primary and support
hands. Select Trigger pressed, then **Customize pressed pose** to start with a
copy of Rest. Adjust any finger: a gun can curl its index while a hypo can press
with its thumb. **Use Rest for both** removes the pressed override.

The **Trigger preview** slider blends the two poses without changing the saved
values. Item position, wrist rotation, and item scale are shared by both poses.
Copy/paste and mirroring carry both finger poses. Primary poses save optional
`trigger_curls` in the pickup/weapon library; support poses save it in
`vr-support-grips.json`. If absent, the resting pose remains unchanged at any
trigger value. Existing resources therefore keep their authored fit.

Gameplay blends using analog controller trigger input. Fire/use thresholds are
unchanged, and the supporting hand can animate without firing or using an item.
The debug runtime's hand-grip diagnostics report `visual_trigger` and
`finger_curls` for the primary hand and its support hand.

## Support regions for large weapons

In **Support hand**, choose **Fixed socket** or **Support region**. A region has
editable XYZ start/end points in the displayed hand's frame, plus a grab radius
in centimeters. The cyan wire capsule shows the eligible area. **Position along
region** previews the support glove along it without editing the saved pose.
Wrist rotation and both finger poses apply everywhere on that region.

The fusion cannon (`fsn_h`) and worm launcher (`al_h`) use regions; the wrench,
pistol, and shotgun retain fixed sockets by default. Their existing primary
poses and uniform scales are preserved. A support squeeze selects the closest
point on the region and locks that contact until release. Moving the second
hand steers the weapon; it does not slide the contact or resize the model.
Releasing and squeezing again selects a new nearest point. The primary hand
retains ownership, and releasing it drops the weapon.

Regions save as optional `region: {start: [x,y,z], end: [x,y,z]}` in
`vr-support-grips.json`, in normalized right-primary model coordinates before
item scale. The editor mirrors them through the same model frame as gameplay.
Omitting the region uses `palm_anchor` as a fixed socket. A zero-length region
also behaves as a socket. Runtime diagnostics expose world-space
`support.region_endpoints` and the locked scaled-model-space `support_anchor`.
