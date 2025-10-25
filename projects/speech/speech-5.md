# Speech Step 5 – AI Concept Triggers

## Objective
Hook speech playback into AI behaviours so appropriate concepts fire during gameplay (spotting player, combat, death, etc.).

## Prerequisites
- Speech playback pipeline operational (Step 4).
- Awareness of AI behaviour code paths (`shock2vr/src/scripts/ai/*`).
- Understanding of existing environmental sound triggers to avoid duplicates.

## Deliverables
- Concept trigger map (behaviour/state → speech concept).
- Instrumented AI scripts emitting `Effect::PlaySpeech` at correct times with necessary context overrides.
- Debounce logic preventing redundant triggers (leveraging Step 4’s controller).
- Updated documentation referencing concept trigger points.

## Detailed Tasks
1. **Concept Mapping**
   - Review Dark Engine concept table (`references/darkengine/src/ai/aisound.cpp`) and align with our behaviours.
   - Decide minimum subset for initial implementation (spot player, alert transitions, combat attack, hit reactions, death, idle chatter).
   - Document mapping in this file and share with QA.

2. **Behaviour Instrumentation**
   - For each behaviour (idle, chase, combat, ranged, melee, death), identify entry points where speech should trigger.
   - Insert calls to a helper (e.g. `request_speech(entity_id, concept, overrides)`) which packages context and yields `Effect::PlaySpeech`.
   - Ensure triggers fire once per transition (use local state or Step 4 controller).

3. **Global Signals & Scripts**
   - Update signal handling (e.g. `PropAISignalResponse`) to allow script messages to request concept-specific speech when appropriate.
   - Provide optional convenience functions for designers to trigger speech from scripts without deep knowledge of tagging.

4. **Coordination with Environmental Audio**
   - Audit existing `effect.rs` or death handling that already plays env sounds (e.g. death grunt).
   - Decide on precedence: speech may replace env sound or they may coexist (document choice).

5. **Testing**
   - Write behavior-level tests (integration) ensuring specific triggers fire once (e.g., spawn AI, simulate spotting player, confirm speech).
   - Manual playtest scenarios: stealth detection, combat, AI death verifying correct lines.

## Open Questions / Research
- Do we need random idle barks / scheduler similar to Dark Engine? Might require timers.
- How to handle non-creature entities (turrets, cameras) – do they map to same system?
- Determine fallback for missing voice data (should log but not spam).

## Acceptance Criteria
- At least three concept triggers active in-game (spot player, combat attack, death).
- Speech does not spam or overlap due to repeated triggers; Step 4 throttles confirm.
- Environmental audio regression risk assessed/documented.

## Handoff Checklist
- Provide list of modified behaviours and rationale.
- Supply debug commands/steps for QA to verify triggers.
