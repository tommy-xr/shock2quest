# Nightdive grub model frames

The 25AE install used for verification is `/Users/bryphe/ss2-25th`, with
`sshock2.kpf` and `mods/sshock2ee.kpf` (plus 400, patch_ext, scp and shtup).

Registration evidence in that installation:

- `SystemShock2Remastered.exe` contains the native loader paths `sq_scripts`,
  `%s/squirrel.ini`, and `%s/*.nut`, and its script-class discovery diagnostic.
- `mods/sshock2ee.kpf:sq_scripts/squirrel.ini` loads `libs/ND.nut`,
  `libs/archetype_system.nut`, and `libs/animation_handler.nut` in that order
  before the top-level object scripts.
- `sq_scripts/object_grub.nut` actively attaches `ND_Grub` to both `Grub` and
  `Grub Rotatable`. Its class extends `ND_MeshAnimated`, with 20 animation
  frames, 20 model frames, 20 Hz and the `NDgrub` prefix. There is no mission or
  SCP fingerprint on these registrations. The base campaign's `Grub` template
  is -182; flinders descend from the separate `Grub Parts` family (-2665).
- All twenty `obj/NDgrub0.bin` through `obj/NDgrub19.bin` files ship in the same
  archive. The alternative corpse/debris patch in `dbmods/ND-grub.dml` is
  commented out and is deliberately not applied.

This is installed script/asset and native loader evidence, not a visual
comparison with a running Windows remaster.

`ND_MeshAnimated` selects a frame from global simulation milliseconds modulo
its animation period. `model_frame_animation` reproduces that selection with
integer duration arithmetic, using the existing asset cache for immutable
meshes. The shared mission render path substitutes only the drawn mesh and
reports that actual mesh in scene inspection. It retains the original entity
model, AI, physical sphere, articulated damage proxies and authored death
flinders. Consequently frames never rebuild physics or restart AI. Missing
frame art falls back to the original mesh. Both flat and VR use this path.

Save/load reconstructs the mesh from the live simulation clock and template
ancestry; no model property or transient runtime identity is persisted. Stasis
holds its captured simulation phase in the existing saved stasis state, just
as it holds the skeletal pose, and rejoins the global animation phase on expiry.
The sequence clock and cached-model selection can be reused by the Vein
animation tracked by #2186; its rest interval and attachment are not implemented
here.
