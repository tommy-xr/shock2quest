# Egg goo cloud

EggGooCloud (-438) is a one-shot burst, not a persistent toxin zone. The
shipped gamesys `LD$arSrcDes` record links it to Venom (-387) and authors:

| Offset | Field | Value |
| --- | --- | --- |
| 0 | Propagator | 2 (radius) |
| 4 | Intensity | 2 |
| 8 | Valid fields | 3 (shape and lifecycle) |
| 12 | Radius | 10 Dark units / 4 world units |
| 16 | Shape flags | 1 (raycast) |
| 20 | Dispersion | 0 (constant intensity) |
| 44 | Lifecycle flags | 0 (finite, no lifecycle-triggered destruction) |
| 48 | Period | 5000 ms |
| 52 | Maximum firings | 1 |
| 56 | Intensity slope | 0 |

The original [periodic lifecycle](https://github.com/DeathEngine2/LookingGlass-DarkEngine/blob/main/thief_2_service_release/rdrive/prj/thief2/src/ACTREACT/SSRCLIFE.CPP)
fires at birth, then at each period. With max_firings=1, this is one immediate
pulse; 5000ms is neither a delay nor the cloud lifetime. The
[radius propagator](https://github.com/DeathEngine2/LookingGlass-DarkEngine/blob/main/thief_2_service_release/rdrive/prj/thief2/src/ACTREACT/RADIAG8R.CPP)
uses constant intensity for dispersion0 and rejects obstructed targets.
The port reuses its existing multi-ray body exposure calculation: partial
cover reduces exposure instead of using retail's single all-or-nothing ray.

An internal EggGooCloud script supplies this engine-owned behavior without
inventing an authored object script or enabling unimplemented periodic stimuli
on unrelated objects. It reads intensity/radius from the existing parsed link.
The generic radius-effect applier now supports constant intensity while
retaining existing falloff for explosions, radiation sources and swarmers.
Venom reaches the existing player receptron/toxin system, including resistance,
Toxin Shield, persistent poisoning and detox treatment.

The particle group authors animation_type0 (one-shot), ten particles, and
lifetimes0.9–1.3sec. The renderer already stops emitting once that burst ends.
The original [particle-group simulation](https://github.com/DeathEngine2/LookingGlass-DarkEngine/blob/main/thief_2_service_release/rdrive/prj/thief2/src/RENDER/PGROUP.C)
returns expiration once its one-shot particles are exhausted. The cloud script
cleans up at the longest authored particle lifetime (a conservative upper
bound, rather than the random final particle's exact death). Its saved state
retains the spent pulse and remaining lifetime, preventing replay on load.

General arSrcDesc lifecycle and dispersion parsing remains outside this focused
fix. Other radius source types retain their existing controllers.
