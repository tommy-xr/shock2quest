# Terrain material effects

The implementation follows `doc/material-format.txt` in the original
[NewDark 1.27 distribution](https://darkfate.org/view/details/files/projects/thief_2_v1_19/t2_v127.zip)
(`new_dark.zip` inside the download), cross-checked against installed 25AE materials.

- Pass `ani_rate` is milliseconds per frame, defaulting to 250.
- Pass animation defaults to `NORMAL`; `PINGPONG` reverses at the ends.
- A numbered `texture *_ N prefix` sequence starts with `prefix`, followed by
  `prefix_1`, etc. An existing trailing underscore is reused.
- Wave values are normalized to 0–1 before amplitude and bias are applied.
  Phase is measured in cycles; the period is milliseconds.
- UV scroll speeds are texture-coordinate units per second. Optional step
  counts of zero or one mean continuous movement.

Unsupported directives retain the original material. This is a bounded subset,
not a claim of complete NewDark material support.

The UV increment supports continuous SCROLL (including literal `[1 1]` in the
Nightdive assets) and independent UOFFSET_WAVE/VOFFSET_WAVE with SINE or
SAWTOOTH. Unsupported stepping, scale/rotation transforms, and other waveforms
retain fallback. Offset evaluation uses simulation time; lightmap UVs are
untouched. `ani_frames 1` explicitly suppresses legacy texture animation.

Layered terrain uses the shared MaterialStack draw path, retaining authored
pass order and blend factors. The supported subset is material-only plans of
one to eight passes with an ordinary depth-writing first pass; overlays keep
bitmap alpha and may have independent UV motion, animation and RGB/alpha
modulation. Only the base pass alpha-tests and writes depth. Every pass must
resolve, otherwise both original art and UV dimensions remain. Water,
transparent-only stacks, environment/incidence maps, clamping, mip bias,
force_opaque and replace_alpha are outside this terrain subset.

## Experimental Many wetness

The installed Nightdive egg and grub scripts reference dedicated
`OBJ/TXT16/ND-anegg_s` and `ND-grub_s` masks with `MATERIALS/ND-IR_SHINE`.
The ten `_ND/om*` and `_ND/ovm*` terrain includes inspected reference diffuse
textures, animation and UV effects, without corresponding specular,
normal or roughness-map directives.

`terrain_wetness` (Developer → Organic shine) is a **project-owned prototype**,
not recovered Nightdive terrain metadata. It defaults to **0** and ranges to 4.
It requires `--experimental upgraded_terrain` and selects only OvrMnd_1's
OM1/2/6/8, OMW and OVM003/005/010/012/015 surfaces. For a controlled comparison,
set `game.devParams.set('terrain_wetness', 1.5)` under a fixed spotlight, then
sweep the light separately. Setting it back to 0 disables the effect live.

The terrain reuses the egg/grub Blinn–Phong highlight and mask composition.
Its final layer's own diffuse RGB/alpha supplies the provisional gloss mask,
following that layer's UV motion and animation. Gloss is applied once, without
an extra geometry pass. The shared shine ramp is loaded as part of the existing
shine pipeline, but there are zero authored incidence-sheen passes: only the
light-driven highlight is enabled. Missing ramp art leaves diffuse rendering.
Object shine controls and materials retain their existing behavior.

This first prototype has no terrain normal/roughness map, procedural veins,
or environment reflection. Highlights follow the mesh normals and are tinted
by the diffuse art, so broad highlights can reveal the low-poly surface.
It needs an active light; the baked lightmap alone supplies no light direction.
The default is off pending visual review and Quest GPU measurements.
