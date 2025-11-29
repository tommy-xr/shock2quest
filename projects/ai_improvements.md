# AI Improvements

## Current Findings

### Blocker Bugs
- `shock2vr/src/scripts/ai/behavior/scripted_sequence_behavior.rs:36` panics if a scripted action list is empty; any misconfigured AIWatchObj instantly crashes the mission.
- `shock2vr/src/scripts/ai/behavior/scripted_sequence_behavior.rs:394` misspells the null animation tag (`"__NULL_ACTION__"`), preventing scripted sequences from idling cleanly.
- `shock2vr/src/scripts/ai/steering/collision_avoidance_steering_strategy.rs:63` ignores the preconfigured whisker lengths and always uses hard-coded distances, so conservative/comprehensive modes behave the same.
- `shock2vr/src/scripts/ai/ai_util.rs:176` hard-codes a 90° quaternion when spawning projectiles; weapons that are already oriented correctly still fire sideways.

### Animation Pipeline Issues
- Animation playback pulls clip velocity and end rotation but discards blend lengths and duration scaling, causing jerky locomotion when clips loop or switch.
- `AnimationEvent::VelocityChanged` is emitted at the first frame of a clip (`dark/src/motion/animation_player.rs:162`) but the mission loop ignores it, so Rapier updates velocity one frame late.
- Direction-change handling halves the requested yaw before applying it (`shock2vr/src/mission/mod.rs:553`), forcing steering to oscillate.
- `AnimationPlayer::queue_animation` restarts the clip queue from frame 0 with no cross-fade, which exaggerates gait pops when MotionDB returns similar clips sequentially.
- Clip metadata comes from `MotionStuff` (blend length, duration, end direction, translation) and `MpsMotion` (frame rate, flags). Current playback only uses `sliding_velocity` and `end_rotation`, ignoring blend and timing hints.
- Root motion is applied to physics by remapping `(x, z)` → `(-z, x)` and multiplying by the entity transform (`shock2vr/src/mission/mod.rs:535`). The skeleton importer already rotates the rig 90° (`dark/src/ss2_skeleton.rs:130`), so the extra swap can introduce lateral drift.
- The render skeleton only receives vertical translation (`dark/src/motion/animation_player.rs:225`), while physics owns horizontal motion. A mismatch between the two causes visible pops when Rapier corrects the capsule.
- Missing cross-fade support means every clip switch snaps to its first frame, skipping the blend window defined in `blend_length`.

### Not Yet Implemented
- Pathfinding: AI cannot navigate around obstacles, only beelines toward targets.
- Security propagation: Cameras don't broadcast alarms through SwitchLink/SecuritySystem.
- Turret coordination: Turrets don't respect global alert state before arming.

## Objectives
1. Resolve blocker bugs and typos that destabilize AI scripts and steering utilities.
2. Integrate additional `MotionStuff` metadata (blend length, duration, end direction) into animation playback to smooth locomotion and honour clip semantics.
3. Add a lightweight cross-fade path inside `AnimationPlayer` so clip transitions respect `blend_length` and share velocity between outgoing and incoming motions.
4. Build a nav-graph from mission cell data and implement A* pathfinding.
5. Implement security alarm propagation via SwitchLink and coordinate turret arming with global alert state.

## Execution Plan

### Phase 0 – Blocker Fixes
- Add defensive handling for empty scripted-action queues and emit structured warnings instead of panicking.
- Correct the null-animation tag in `FaceScriptedAction`.
- Honour `extra_whisker_distance` and `main_whisker_distance` members in collision avoidance.
- Replace the hard-coded projectile quaternion with data-driven orientation (read muzzle joint/joint-frame orientation or weapon link data).
- **Acceptance**: scripted sequences with empty arrays no longer crash, Face scripted actions idle cleanly, conservative vs comprehensive avoidance differs, and ranged enemies fire using their weapon orientation.

### Phase 1 – Motion Metadata Integration
- Extend `AnimationClip` to expose `blend_length` and any duration-scaling flags derived from `MotionStuff`.
- Modify `AnimationPlayer::update` to:
  - Apply duration scaling when `MotionStuff` requests it.
  - Emit and handle `VelocityChanged` by updating Rapier velocity immediately in `Mission::update_animations`.
  - Apply `end_direction` exactly (no arbitrary scaling) when processing `DirectionChanged`.
- Update the physics velocity mapping so the sliding vector rotates purely by the entity's orientation, avoiding redundant axis swaps.
- **Acceptance**: looping walks no longer hitch, yaw changes match the clip, and a quick animation swap does not produce a frame of stale velocity.

### Phase 2 – Cross-Fade Support
- Introduce internal state to `AnimationPlayer` to keep both the current and next clip active for `blend_length` milliseconds.
- Blend joint transforms and sliding velocities during the fade window before handing results to rendering and physics.
- Ensure queuing a new clip preserves residual time so walk cycles resume smoothly after interruptions.
- **Acceptance**: switching between locomotion variants (eg, `locomote` → `locourgent`) shows a smooth blend with no visible pop or speed drop.

### Phase 3 – Pathfinding
- Build a nav-graph from mission cell data and implement A* pathfinding.
- Expose path-following steering that feeds into existing collision avoidance.
- Parse `AIPath`, `AIGoalLoc`, and `PathObstruction` links to respect authored patrols and blocked passages.
- **Acceptance**: AI can navigate around obstacles instead of beelining toward targets.

### Phase 4 – Security Propagation
- Parse `SwitchLink` / `SecurityCamera` relations so cameras can broadcast alarms.
- Capture any global security quest bits (`AI_Security`, `SecurityState`) required to coordinate turrets.
- Link turret open/close logic to the global alert state so devices stay dormant until the system is raised.
- Ensure alarms propagate through SwitchLink/SecuritySystem entities and surface actionable signals to scripts.
- **Acceptance**: cameras raise alarms after sustained sight, turrets arm only under high alert, and other systems can subscribe to the propagated security state.

## Data Support Work
- Motion metadata is already read from `MotionStuff`; expose `blend_length`, duration-scaling flags, and exact `end_direction` through `AnimationClip`.
- Load mission navigation aids:
  - Reuse existing `mission/cell.rs` data to build a path node graph.
  - Parse `AIPath`, `AIGoalLoc`, and `PathObstruction` links to respect authored patrols and blocked passages.
- Expand link/property support for security devices:
  - `SwitchLink`, `SecurityCamera`, and `SecuritySystem` relations to propagate alarms and drive quest bits.
  - Optional `AI_Team`/`AI_Ecology` props so turrets and cameras can filter friend/foe targets.
