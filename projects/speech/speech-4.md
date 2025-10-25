# Speech Step 4 – Speech Playback Pipeline

## Objective
Create the runtime plumbing to request, play, and manage speech audio clips with priority and pause logic.

## Prerequisites
- Steps 1–3 implemented (`VoiceRegistry`, speech selection API, tag builder).
- Familiarity with existing audio effect handling in `shock2vr/src/mission/mod.rs`.
- Understanding of `engine::audio` spatial playback APIs.

## Deliverables
- New effect enum variant (e.g. `Effect::PlaySpeech`) and handling code in mission loop.
- Speech manager storing active speech handles, last-play timestamps, and concept priority per entity.
- Integration with speech selection API to choose sample and play spatial audio.
- Respect for pause min/max and concept priority rules mirroring Dark Engine behaviour.

## Detailed Tasks
1. **Effect & Message Wiring**
   - Extend `Effect` enum with `PlaySpeech { entity_id, voice_id?, concept, context_tags, priority_hint }`.
   - Update script utilities to enqueue speech effects.
   - Adjust save/load or serialization if effects persisted (confirm not needed).

2. **Speech Manager**
   - Create struct (e.g. `SpeechController`) within mission state to track:
     - Currently playing schema handle per entity.
     - Last concept played with timestamp.
     - Derived pause windows from `VoiceRegistry`.
   - Provide methods `can_play(concept, now)` and `on_played(...)`.

3. **Playback Logic**
   - On `PlaySpeech` effect:
     - Gather tags via Step 3 builder unless already provided.
     - Call `Gamesys::select_speech_schema` to get weighted schema/sample.
     - Apply priority rules (if existing speech has higher/equal priority, skip).
     - Stop previous speech if necessary (`engine::audio::stop_audio` by handle).
     - Play new sample spatially (similar to env sounds) and store handle.

4. **Priority & Pause Handling**
   - Mirror Dark Engine priority: use concept priority ordering from `SpeechDB` (requires exposing priority map).
   - Enforce min/max delay between core concepts using current time and `m_ConceptTimes` logic from reference.
   - Manage `reacquire` timer for `spotplayer` (set flag, start countdown).

5. **Lifecycle Management**
   - Hook into audio callbacks or poll to detect speech end and clear active handle.
   - Ensure entity removal or slay stops active speech to avoid orphaned audio.

6. **Testing**
   - Unit tests for `SpeechController` priority/pause decisions.
   - Integration test scenario: trigger two speech effects rapidly and confirm only higher-priority plays.
   - Manual sandbox test verifying audio plays at entity position.

## Open Questions / Research
- Need schema priority map from speech DB (is it stored with concept map?). Investigate `priority_size` blob.
- Determine if we have audio callback infrastructure to detect clip completion; if not, store duration or rely on handle query.
- Clarify how to handle player speech (if separate) vs AI speech.

## Acceptance Criteria
- Mission loop can play speech for at least one AI archetype with correct spatialization.
- Priority/pause rules prevent rapid repeated `spotplayer` or overlapping `comattack` clips.
- No regressions in existing `Effect::PlaySound` or environmental audio.

## Handoff Checklist
- Document `SpeechController` API and state expectations.
- Provide logging sample showing decision process (skip vs play) for debugging.
