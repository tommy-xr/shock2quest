# Camera Awareness Implementation Plan

## Scope
- Deliver Shock 2–faithful camera behaviour: idle scan (green model), warning when the player is in view (yellow), alarm escalation (red) with automatic reset after contact is lost.
- Limit work to a single mission run: no global security propagation or turret integration yet (note future hooks).
- Reuse existing scripting framework in `shock2vr/src/scripts/ai/camera_ai.rs`, plus property/link ingestion already in the data pipeline.

## Canonical References
- Behaviour logic: `references/darkengine/src/ai/aicamera.cpp`, `aiprdev.cpp`, `aiprcore.cpp`, `aiaware.h`.
- Camera parameters: `AI_Device`, `AI_Camera`.
- Awareness tuning: `AI_AlertCap`, `AI_AwrDel2`.
- Model assets: template defaults to `PropModelName("camgrn")`; no explicit yellow/red entries exist, so derive variants (`camyel`, `camred`) via naming convention with hardcoded mapping for now. TweqModelConfig only lists the damaged mesh (`camdam`), so do not rely on it for colour changes.

## Property & Link Support
1. **Add readers in `dark/src/properties/`**:
   - `AI_Device` → joint ids (`m_jointActivate`, `m_jointRotate`), inactive/active positions, activation speed, facing epsilon, rotate-on-activate flag.
   - `AI_Camera` → scan angle limits and scan speed (deg/msec).
   - `AI_AlertCap` → min/max alert levels and minimum post-peak level.
 - `AI_AwrDel2` (AIAwareDelay) → delays for reaching level two/three, reuse windows, ignore range.
  - No Tweq-driven model swapping is available (config only points to `camdam`); scripts must map the base model to its yellow/red variants explicitly.
2. Register the new readers in `dark/src/properties/mod.rs` with accumulator `latest`.
3. **Populate entity components** in `mission_entity_populator`:
   - When `PropAI` == `camera`, attach new components or stash parsed params on an `InternalCameraConfig` struct.
   - Record base model name from `PropModelName` so the script can flip between colour variants.
   - (Optional) capture `AI_VisJoint`/`AI_VisDesc` later if we need per-camera FOV overrides.

## Runtime Data Model
- Introduce a `CameraConfig` struct (new module under `camera_ai` or shared AI config) holding:
  - Joint indices/positions (from `AI_Device`).
  - Scan angles & speed (from `AI_Camera`).
  - Alert caps & delays.
- Model asset ids (`green`, `yellow`, `red`) derived from `PropModelName` plus a hardcoded suffix map (default: `camgrn`, `camyel`, `camred`). Store the damaged mesh (`camdam`) separately for future damage handling.
- Add a `CameraAwarenessState` struct on the script:
  - Current level (`GreenIdle`, `YellowTracking`, `RedAlarm`, `Cooldown`).
  - Timers: `visible_accumulator`, `lost_timer`, `reuse_block`.
  - Last known LOS result to avoid jitter.
  - Flags for “alarm broadcast sent” and “current model”.

## Behaviour Flow
1. **Initialization**
   - Cache config and baseline model.
   - Set joint 1 rotation to inactive pose from `AI_Device`.
   - Reset awareness to idle (green model).
2. **Idle (Green)**
   - Drive joint scan using `AI_Device::m_jointRotate` and the two scan angles/speed.
   - If FOV+LOS detects player, transition to Yellow, change model to `camyel.bin`, start `visible_accumulator`.
3. **Warning (Yellow)**
   - Suspend idle sweep; rotate towards player using measured yaw (respect facing epsilon).
   - Accumulate visible time; if LOS drops, start `lost_timer`; if `lost_timer` exceeds the `AI_AwrDel2.ignoreRange`, revert to Green and resume scanning/model swap.
   - Once `visible_accumulator` exceeds the `AI_AwrDel2.toThree` (or explicit threshold derived from original timings), escalate to Red.
4. **Alarm (Red)**
   - Swap model to `camred.bin`; emit alarm sound schema (`AlertToThree`) and queue SwitchLink broadcast (future integration; for now, log/stub).
   - Maintain red while LOS persists; if lost, begin cooldown using `reuse` values so repeated short glimpses don’t immediately re-trigger.
   - When cooldown elapsed and LOS absent, downgrade to Yellow or Green per `AI_AlertCap.minRelax`.
5. **Reset**
   - Return to idle scan (green model) once awareness decays to `kAIAL_Lowest`.
   - Ensure we obey `AI_AlertCap.maxLevel` so designer caps are honoured.

## Supporting Utility Updates
- Extend `ai_util::is_player_visible` to accept:
  - `vision_joint` transform for cameras.
  - Horizontal FOV derived from scan angles.
  - Returns both LOS boolean and facing angle to reuse for tracking yaw.
- Add helper to step `CameraAwarenessState` each tick (apply discharge using `AI_AwrDel2` values).
- Provide a `set_camera_model(entity_id, CameraColor)` wrapper that issues `Effect::ChangeModel` only when the target model differs from current.

## Audio & Effects
- On transitions:
  - Green ↔ Yellow: request `kAISC_AlertToOne` / `kAISC_AlertDownToZero` if available.
  - Yellow → Red: trigger `kAISC_AlertToThree`, schedule (future) SwitchLink broadcast.
- Ensure sounds respect existing positional playback helper (`ai_util::play_positional_sound`).

## Testing & Validation
- Create a debug mission or place the base game camera template in a sandbox level; verify scan idle behaviour, LOS detection, timers, and reset.
- Add tracing spans (behind `cfg!(debug_assertions)`) for state transitions, LOS toggles, timer values.
- Regression checklist:
  - Player steps briefly into cone: camera flashes yellow, returns to green within delay.
  - Player remains visible past dwell time: camera reaches red and stays until player hides; then decays.
  - Rapid peek after alarm before reuse window: stays red (or yellow) per capacitor rules.

## Out-of-Scope Follow-Ups
- Hooking `AICamera` relation data to fire actual SwitchLink messages.
- Sharing awareness with turrets/security system.
- Respecting per-camera quest bit overrides or scripted responses.
