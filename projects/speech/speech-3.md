# Speech Step 3 – Runtime Tag Assembly

## Objective
Translate in-game AI state into the tag tuples required for speech selection, combining static metadata and dynamic context.

## Prerequisites
- Steps 1–2 complete (`VoiceRegistry`, selection API).
- Understanding of AI components/state in `shock2vr` (behaviors, sensors, inventory).
- Knowledge of existing `EnvSoundQuery` builders.

## Deliverables
- Utility module (likely under `shock2vr/src/scripts/ai`) that produces `Vec<(&'static str, String)>` or similar for speech tags.
- Integrations to fetch baseline tags from `PropClassTag`, `PropAISoundTags`, and voice metadata.
- Accessors exposing relevant runtime flags (alertness, `investigate`, senses, weapon mode, `carrybody`, `nearbyfriends` distance).
- Documentation of tag mapping logic (kept alongside utility).

## Detailed Tasks
1. **Tag Inventory**
   - Compile list of tag keys used in `voices.spew` (e.g. `event`, `investigate`, `sense`, `carrybody`, `weaponmode`).
   - Map each to data sources (property, AI state, derived calculation). Note gaps requiring new signals.

2. **Baseline Tag Extraction**
   - Reuse `PropClassTag::class_tags()` to seed tag list.
   - Parse `PropAISoundTags` string into key/value pairs (confirm delimiter).
   - Include voice-level defaults from `VoiceRegistry` (if voice archetypes specify tags).

3. **Dynamic Context Hooks**
   - Expose functions to retrieve:
     - Alert level / concept transitions.
     - `investigate` flag or search state.
     - Latest sensed stimulus (sight/sound).
     - Carried body state (inventory checks).
     - Nearby friendly distance (requires radius search or existing sensors).
     - Weapon tags (via weapon component or currently equipped inventory).
   - Where data missing, add TODO logging or placeholders for future instrumentation.

4. **Tag Builder API**
   - Implement function `build_speech_tags(entity_id, SpeechContext)` returning canonical lowercased tuples.
   - Ensure consistent ordering (sort by key) to ease testing.
   - Allow optional overrides/injections (for scripted events).

5. **Testing & Validation**
   - Unit tests with mocked components verifying tag output for specific scenarios (e.g. guard carrying body sets `carrybody true`).
   - Integration test hooking into a simple scripted AI to confirm tags passed to speech selection.

## Open Questions / Research
- Determine best source for alert level (existing AI state or new tracking).
- Confirm how `NearbyFriends` distance is encoded (int or float). Inspect Dark Engine code (`sqrt(...)`).
- Are boolean tags encoded as `"true"` strings? Validate via `voices.spew`.

## Acceptance Criteria
- Tag builder returns expected set for at least three states (idle, alert, combat).
- Missing data paths emit trace logs guiding future work.
- Tests covering parsing of `PropAISoundTags` and voice defaults pass.

## Handoff Checklist
- Provide mapping table (tag → data source) to share with subsequent steps.
- Identify any required engine changes (physics queries, inventory lookups) not yet implemented.
