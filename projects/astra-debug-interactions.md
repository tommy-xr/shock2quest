# Astra interaction workbench

Start the scene with `cargo dbgr --mission debug_interactions --vr`, or select
`debug_interactions` from the Developer scene launcher on desktop or Quest.
The scene also loads in flat presentation for inspecting its layout.

Ten actual pickups stand on labeled pedestals along a clear aisle: coffee mug,
printed magazine, basketball, wrench, pistol, shotgun, fusion cannon, worm
launcher, standard ammo clip, and psi amp. Walk along the aisle and squeeze to
grab with either hand; release squeeze to drop. Character stats are provisioned
to their caps so weapon requirements do not obstruct inspection.

Use **DebugReloadLevel** to restore the whole scene, including moved or held
items. Through the SDK: `await game.input.trigger("DebugReloadLevel")`, then
step. On Quest, reselect the scene in the Developer launcher to reset it.
This workbench intentionally resets rather than preserving inventory or progress.

The shared fixture list is
[`INTERACTION_FIXTURES`](../shock2vr/src/scenes/debug_interactions.rs): stable
template IDs and pickup model names for future Explorer grip previews. The
magazine archetype has no default model, so this instance uses the shipped
`magci` cover. Runtime IDs still change on each launch; discover by template.

Current baseline: the rigged glove has its original glove texture again.
Miscellaneous objects still use the existing generic held pose, and wielded
weapons still replace the glove with their baked hands. Automatic fitting,
Explorer grip editing, watch/lights, support grips, and body slots are subsequent
steps in the [interaction plan](astra-vr-hands-round-2.md).

The SDK regression `test/debug-interactions.e2e.test.ts` verifies that every
fixture renders, can be grabbed/released by each VR hand, and returns once after
a reset. Run with Node 22+ and `SHOCK2_E2E=1` after building the SDK.
