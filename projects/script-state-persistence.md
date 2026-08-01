# Script state persistence

Entity properties and links remain the canonical home for state that other
systems inspect or mutate. They already participate in the Dark property/link
save pipeline and entity-ID remapping. Engine-owned animation, rendering,
physics, and collider state likewise stays with those systems.

`Script` state is only for private runtime data which has no canonical ECS
representation: a behavior mode, remaining timer, one-shot latch, cooldown, or
queued intent owned by that script. It is opt-in; scripts without a stable state
key keep the legacy fresh-construction behavior.

## Save identity and schema

Each saved envelope contains:

- the entity ID from the saved world;
- an explicit stable script key;
- a structural ordinal path through the entity's top-level scripts and every
  nested `CompositeScript`;
- the script-owned schema version and JSON payload.

The path makes duplicate script keys collision-safe. Entity creation sorts the
authored script list, while composite child order is authored in code, so those
ordinals are deterministic. Renaming a stable key or reordering stateful
scripts is therefore a save-schema change and must include migration handling.

An opting-in script implements `script_state_key`, `save_state`, and
`restore_state`. `ScriptState::encode`/`decode` centralize payload errors and
explicit version rejection. A script which changes schemas must either migrate
an older version in `restore_state` or return a clear unsupported-version error.

## Load lifecycle

The existing entity-population maps for mission and held entities are combined,
then every script payload is restored after all scripts have been constructed
and before the mission can update. A hydrated leaf skips `initialize()` so
fresh-session side effects cannot overwrite or replay its restored state.
Missing payloads, including all pre-feature saves, run normal initialization.
`CompositeScript` tracks this per child, so restored and legacy children can be
mixed at any nesting depth.

Payloads must never retain a serialized `shipyard::EntityId` directly. Store
its `u64` saved form and resolve it through `ScriptRestoreContext::remap_entity`
during hydration. A missing target is an explicit load error rather than a
stale handle into the rebuilt world.
