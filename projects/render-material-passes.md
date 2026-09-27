# Authored object material passes

LGMD objects (including skinned weapon/viewmodel parts), LGMM creatures and
PMNM creatures can render ordered `.mtl` passes. Each scene object retains its
stack when posed or cloned; a whole-material replacement explicitly clears it.
Material passes share the object's geometry, projection, skinning, culling,
lighting, depth bias and per-instance transparency.

The parser follows the existing include resolver and supports `$TEXTURE`,
line comments (`//` and `#`), `render_material_only`, `force_opaque`, all ten
source/destination blend factors, scalar `RGB`/`ALPHA`, `alpha func INCIDENCE`,
`replace_alpha`, `shaded`, `mipmap_bias`, pass `uv_clamp`, and `uv_source
ENVIRONMENT` with authored DDS cubemaps. An omitted pass texture is white.

`replace_alpha` replaces the surface's **vertex** alpha; bitmap alpha still
multiplies it. `force_opaque` ignores alpha in the material's original bitmap.
Without `render_material_only`, the original surface remains underneath the
passes. Texture selection for a later pass never changes what `$TEXTURE` means.
Plans load transactionally: an unsupported directive, missing texture or invalid
cubemap keeps the complete previous material rather than enabling some layers.
A script with no passes cannot suppress its original surface.

Replacement stacks select their render phase from their own full-opacity base
passes and the object's current opacity. A pass with `replace_alpha` can remain
visible when the object fades out. Only the first full-opacity diffuse/replace
pass establishes depth; subsequent layers compare at equal depth without
writing it. The base uses the renderer's existing bitmap cutout threshold.
Objects with depth writes disabled keep that override throughout the stack,
and every stack restores the renderer's phase state before another object draws.

The existing explicitly tuned metal decal, organic creature, wet-growth and
organic weapon profiles retain their previous rendering path. They include
project-authored lighting/vein adaptations; applying the generic stack as well
would double their effects. Generic materials and those profiles share the same
asset namespace rules: using a skinned shader does not turn an LGMD object into
a `mesh/` asset.

## Deliberate fallback

This is not universal NewDark material parity. Animation metadata/sequences,
wave functions, RGB incidence functions, UV scroll/scale/projection, location
based environment maps, `env_map`/`illum_map` shorthand, `force_alpha_key`,
`force_full_alpha`, edge padding and preprocessor conditions remain on the
previous fallback. Unknown blend tokens reject the plan; they are never treated
as an opaque base. World/terrain materials use their existing path.

An audit of the installed 25AE object/mesh scripts expanded 608 material files:
238 parsed as supported plans and 370 retained fallback (143 of the latter
contain explicit render passes). One additional file had an unresolved include.
This measures parser coverage, not a claim that every supported texture loads
or that each file is a unique mounted model. The effect regression covers actual
mounted pistol, chemical glass and Command2 forcefield art.

## Format sources and verification

The original NewDark `new_dark.zip/doc/material-format.txt` inside
[Le Corbeau's SS2 2.48 release](https://darkfate.org/view/details/files/projects/thief_2_v1_19/ss2_v248.zip)
defines pass order, vertex-alpha replacement, shading, texture aliases and blend
factors. The installed Remaster `base.kpf/progs/DarkMaterial.inc` and
`DarkModelPixelShader.inc` specify 25AE's incidence lookup and fragment
composition. The Remaster uses the red incidence channel and inverse normalized
angle; the older format document describes a different channel convention.

`render-material.e2e.test.ts` verifies the complete held-pistol stack in flat
and VR, the chemical glass's modulation/cubemap/rim order and both real
Command2 forcefield layers. Matched screenshots show the actual effects;
parser and renderer-state unit tests cover alpha replacement, asset families,
unsupported state, layer cloning and complete material replacement.
