# Speech System Implementation Plan

## Goal
Implement Dark Engine–style creature speech so AI actors in `shock2vr` can emit context-aware VO lines, while documenting the final architecture in `references/speech-system.md`.

## Workstreams & Tasks

### 1. Surface Speech Metadata
- Parse speech-related properties (`SpchVoice`, `SpchVoiceIndex`, `SpchNextPlay`, `AI_SndTags`, pause settings) inside `dark/src/properties`.
- Build a `VoiceRegistry` during gamesys load that maps template names to voice indices, pause windows, and default tag strings.
- Add validation/logging to detect missing or duplicate voice assignments.

### 2. Expose Speech Query APIs
- Extend `TagDatabase` to surface `(schema_id, weight)` pairs for concept lookups without breaking existing callers.
- Add `Gamesys::select_speech_schema(voice_id, concept, tags)` that converts tags to ids, runs the concept-specific tag database, and returns weighted schema choices.
- Provide helpers to resolve schema ids to concrete sample paths via `SoundSchema`.

### 3. Runtime Tag Assembly
- Collect baseline tags from `PropClassTag`, `AI_SndTags`, and voice metadata.
- Expose AI state (alert level, investigation flag, senses, nearby allies, weapon info, carrying body) so scripts can append dynamic tags.
- Create a small utility to transform high-level AI context into the tag tuples expected by `Gamesys::select_speech_schema`.

### 4. Speech Playback Pipeline
- Introduce an `Effect::PlaySpeech` (or extend `PlaySound`) that carries `entity_id`, concept, runtime tags, and priority metadata.
- Implement a speech player in the mission loop that queries the speech API, selects samples with weights, plays spatial audio, and records active speech handles.
- Respect pause windows and concept priorities (stop lower-priority clips, throttle repeats).

### 5. AI Concept Triggers
- Audit AI behaviors and state machines to emit speech concepts at key beats (spot player, alert transitions, combat, pain, death, idle chatter).
- Provide configuration hooks for scripts to request speech manually (signal responses, scripted sequences).
- Ensure death/kill flows reconcile with existing environmental sound playback to avoid duplicates.

### 6. Tooling & Debug Support
- Add targeted logging or a debug console command to trace speech resolution (voice, concept, tags, selected schema/sample).
- Build regression tests for tag conversion and voice selection using captured data from `references/voices.spew`.

### 7. Final Documentation
- Summarize the completed architecture, data flow, and extension points in `references/speech-system.md` once implementation stabilizes.

## Tracking Notes
- Reuse existing spew files as goldens while bringing up the system.
- Align naming with Dark Engine where sensible to ease cross-referencing.
