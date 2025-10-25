# Speech Step 1 – Surface Speech Metadata

## Objective

Expose all speech-related metadata (voice assignment, pause windows, AI sound tags) when loading Dark Engine data so later steps can resolve speech consistently.

## Prerequisites

- Reviewed `projects/speech.md` overview.
- Access to `dark/src/properties`, `dark/src/gamesys`, `references/env_sound.spew`, `references/darkengine/src/sound/spchprop.cpp`.
- Understanding of existing property parsing helpers (`define_prop`, accumulators).

## Deliverables

- New property structs/components for speech metadata (`PropSpeechVoice`, `PropSpeechVoiceIndex`, etc.).
- Integration into gamesys load producing a `VoiceRegistry` (struct capturing template → voice index/pause settings/base tags).
- Logging for missing or conflicting voice data.
- Unit/integration tests or debug assertions verifying properties parse for a representative sample (eg. security bot, hybrid, turrets).

## Detailed Tasks

1. **Inventory Property Chunks**

   - Inspect `references/env_sound.spew` and `references/darkengine/src/sound/spchprop.cpp` to map property names to chunk identifiers (`P$` names, confirm case).
   - Use `tools/dark_query` or targeted logging to list currently unparsed properties containing `"Speech"` or `"AI_..."`.
   - Document mapping (chunk name → intended struct) in this file for future reference.

2. **Define Property Types**

   - Extend `dark/src/properties/mod.rs` with new structs:
     - `PropSpeechVoice` (wraps voice archetype string).
     - `PropSpeechVoiceIndex` (optional u32/signed? confirm with dumps).
     - `PropSpeechPauseMin/Max` (durations).
     - `PropSpeechNextPlay` (timestamp – may be runtime only; verify if persisted).
     - `PropAISoundTags` (string of default tags).
   - Implement readers (`read_prop_string`, `read_i32`, conversions to `Duration`).
   - Add to `props` vector with suitable accumulators (likely `latest`).

3. **Populate `VoiceRegistry`**

   - Design a registry struct (eg. `struct VoiceInfo { voice_index: Option<u32>, voice_name: Option<String>, pause_min: Option<Duration>, pause_max: Option<Duration>, default_tags: Vec<(String, String)> }`).
   - During gamesys initialization (post-property load), iterate templates to fill registry keyed by template id and `PropSymName`.
   - Include inheritance handling: merge parent metadata where child overrides.
   - Provide lookup helper by template id and optionally by voice archetype name.

4. **Validation & Logging**

   - Log (debug) when templates have speech tags but no voice assignment, or duplicate voice indices.
   - Consider fail-fast (`panic!`) if core archetypes (player, guard) lack data unless intentionally absent.
   - Ensure registry accessible via `GlobalContext` (needed later steps).

5. **Testing / Verification**
   - Add targeted assertions in load path or small unit tests (e.g. using `.bin` fixture) verifying at least one known template resolves.
   - Add CLI tool command to dump voice registry for manual inspection.

## Open Questions / Research

- Does `Voice Index` persist in the gamesys or derived at runtime? Verify by scanning `references/darkengine/src/sound/speech.cpp` and dumps for field usage.
- Are pause min/max stored per template or runtime only? Cross-check property flags (`kPropertyTransient`).
- Confirm encoding for `AI_SndTags` (space-separated tags or key/value pairs?).

## Acceptance Criteria

- Running gamesys load reports zero speech-related properties remaining in the `unparsed_properties` map for core templates.
- `VoiceRegistry` accessible and populated for at least three AI archetypes (hybrid, rumbler, security bot).
- Code builds and existing tests pass.

## Handoff Checklist

- Provide list of files touched with rationale.
- Include any sample output (log snippet) demonstrating new registry entries.
- Note any blocked research items for next step.
