# Speech Step 2 – Expose Speech Query APIs

## Objective
Provide a reusable API to translate voice + concept + tags into weighted schema selections, including schema → sample resolution.

## Prerequisites
- Step 1 `VoiceRegistry` complete and accessible.
- Familiarity with `dark/src/tag_database.rs` and `dark/src/gamesys/sound_schema.rs`.
- Access to `references/voices.spew` for validation.

## Deliverables
- Updated `TagDatabase` supporting retrieval of `(data, weight)` pairs while keeping existing callers functional.
- `Gamesys::select_speech_schema` (or equivalent module) returning a weighted choice structure (e.g. `Vec<(i32, f32)>`).
- Helper to map schema id → concrete audio sample path using both `SoundSchema` and its sample weights.
- Unit tests validating selection logic with canned tag queries.

## Detailed Tasks
1. **TagDatabase Enhancements**
   - Add method (e.g. `query_weighted`) that traverses branches and collects `TagDatabaseData` entries with weights.
   - Preserve current `query_match_all` behaviour by delegating to new method but returning ints only.
   - Ensure optional tags still handled (existing recursion logic).

2. **Concept Lookup Flow**
   - Define a struct (`SpeechSelectionInput { voice_index, concept_id, tags: Vec<(u32, u8)> }`).
   - Implement new `Gamesys` method:
     - Resolve concept name → id via `SpeechDB.concept_map`.
     - Get `Voice` data by index, fetch concept-specific `TagDatabase`.
     - Convert string tags to numeric IDs (reuse `NameMap::get_index`).
     - Call `query_weighted`, returning weighted schema ids.
   - Handle empty results gracefully with logging.

3. **Schema Resolution**
   - Extend `SoundSchema` with a method `schema_samples(schema_id)` returning per-sample frequencies.
   - Combine speech weight and sample frequency to compute final probability when choosing audio clip.
   - Provide helper returning `Option<ResolvedSample { schema_id, sample: SchemaSample }>` after random selection.

4. **API Surface**
   - Expose lightweight wrapper for consumers (later steps) returning both deterministic data (for logging) and final sample path.
   - Consider caching concept/tag indices to avoid repeated string lookups (optional micro-optimization).

5. **Testing**
   - Write unit tests using simplified `TagDatabase` fixtures to verify weighting.
   - Add regression test loading `references/voices.spew` snippet (or actual speech DB) to confirm key concepts produce expected schema ids.

## Open Questions / Research
- Confirm all voices share identical concept ordering; otherwise store mapping per voice.
- Determine whether schema ids map directly to `SoundSchema.id_to_samples` keys or require additional translation.
- Validate tag keys requiring int ranges vs enum values (see `investigate` tag in `voices.spew`).

## Acceptance Criteria
- API returns weighted choices for at least one known scenario (e.g. Rumbler `comattack` with `investigate=true` yields 13+ entries).
- Existing env sound and motion code paths unaffected (no regressions).
- Tests compile and pass.

## Handoff Checklist
- Document new public functions and sample usage.
- Provide snippets showing expected log output for a sample query.
