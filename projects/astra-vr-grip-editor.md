# Astra VR grip editor

Explorer's **VR Grips** tab edits the prepared pickup grips used by the game.
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
- Drag or type **item position** in centimeters in the calibrated hand frame.
- Set a rotation step, then use **X− / X+**, **Y− / Y+**, or **Z− / Z+** to rotate
  the item about its origin along hand-local axes.
- **Open / point / closed** provide starting finger amounts. Each finger can
  be adjusted independently from 0 (open) to 1 (closed).
- **Save all edits** writes `assets/astra-vr-grips.json` atomically. Restart the
  game/debug runtime process to load the saved resource (scene reloads can retain
  cached assets). Closing Explorer with unsaved drafts offers save, discard, or keep editing.
- **Revert this hand to saved** discards just that hand's draft.
- **Automatic fitting → Replace draft with auto fit** explicitly replaces this
  hand's draft using the model's existing region/orientation hints. An optional
  family choice (cylindrical, pinch, broad grasp, trigger) guides the fit. This
  must still be reviewed and saved. Model defaults remove the manual override.

Manual poses carry `authored: true`. The bulk baker preserves them if the mesh,
rig, hints, and solver revision still match; otherwise it stops without writing
and asks for review. Manual edits clear old solver contact diagnostics, since
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
multi-item review. The `_h` weapon workstream will expand both tools after
stripping authored weapon hands and fitting the gloves; psi amp keeps its
integrated forearm. This first editor lists models with prepared pickup grips.
