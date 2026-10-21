# Quest 120 Hz: an 8.33 ms frame budget

Status: FFR implemented and measured in [PR #2124](https://github.com/tommy-xr/shock2quest/pull/2124),
with low selected as the default after wearer review. The [16-run sweep](https://github.com/tommy-xr/shock2quest/blob/709d2cba1649aea726cc32921eb163caf30287d3/benchmarks/results/2026-10-09-quest3-ffr.md)
shows App-time savings but a warm-run freshness regression requiring isolation.
High blurred peripheral text; low remained readable with a minor quality loss. [CPU/clock diagnostics](https://github.com/tommy-xr/shock2quest/pull/2126)
confirm frame-driven solver drift on-device. [Opt-in clock correction](https://github.com/tommy-xr/shock2quest/pull/2129)
passes paired Quest timing and pause/resume checks; [results and evidence](../benchmarks/results/2026-10-09-quest3-fixed-clock.md)
show solver/active time corrected from about 1.35 to 1.00 and update time reduced
from 6.44 to 4.85 ms, with little FPS change. Normal gameplay retains legacy
scheduling pending interaction/presentation validation. Refresh remains 90 Hz.

## Goal and constraints

Deliver fresh stereo frames at 120 Hz on Quest 3 with an **8.33 ms budget** and
headroom for spikes, while retaining the upgraded 25AE terrain and normal
object lighting. The measured headset advertises 72/80/90/120 Hz, not 100 Hz.
Stable 90 Hz is an intermediate milestone; average presentation FPS alone is
not evidence of success.

Physics and gameplay clocks must advance according to elapsed real time,
independently of display frequency. Preserve solver step size and gameplay
units; do not change gravity, velocities, timers, or animation speeds to make
a faster refresh rate appear correct. Keep tracked head presentation responsive
at every display frame. Any scheduling correction that changes existing
frame-rate-dependent behavior must be documented and regression-tested.

Implement one independently reviewable experiment per PR. Keep experiments
opt-in, preserve a known-working fallback, and retain negative results. Choose
later optimizations from measured bottlenecks rather than implementing every
candidate automatically.

## Starting evidence

Use the [Many terrain benchmark](../benchmarks/results/2026-10-09-quest3-many-terrain.md)
and [refresh-rate exploration](../benchmarks/results/2026-10-06-quest3-refresh.md),
not the old stationary mission-spawn baseline alone.

| Many brain crowd, upgraded terrain | Measured mean |
| --- | ---: |
| CPU update | 6.275 ms |
| CPU scene preparation | 0.703 ms |
| Both eyes, including swapchain synchronization | 2.278 ms |
| Finish/visibility preparation | 1.525 ms |
| VrApi App time | 8.599 ms |
| Presentation FPS at 90 Hz | 89.0; minimum one-second bucket 84 |
| GPU fragment share, separate device-wide counter intervals | 89.9% |

These are different timing domains: do not add VrApi App time to CPU stages,
interpret eye wall time as pure GPU execution, or infer per-frame tail latency
from one-second means. The classic repeat-2 GPU counter interval was invalid
and is excluded; all six separately measured timing intervals remain valid.

The CPU update is the largest measured CPU component. FFR is first because it
provides an isolated experiment against the substantial fragment workload,
without changing simulation scheduling. Wetness adds cost and is a separate
axis; its fixed-camera highlight is barely visible in this scene.

## Ordered milestones

### 1. Fixed foveated rendering: done

Shipped in #2124; low became the default after headset review (#2127). See the
[sweep results](../benchmarks/results/2026-10-09-quest3-ffr.md). The warm-run
freshness regression it showed still needs isolating.

### 2. Simulation clock audit and fixed-step scheduling

This is a **prerequisite for raising the measured refresh rate**, regardless of
FFR's outcome. Initial code inspection found Quest passing wall-clock elapsed
time into `Game::update`, while `PhysicsWorld::update_player_movement` calls
Rapier with its fixed internal integration step once per non-paused update.
The paired Quest audit measured solver/active ratios of 1.339–1.355 in legacy
mode and 0.999633–1.000117 with the opt-in fixed scheduler. The existing solver
timestep is unchanged. See [clock results](../benchmarks/results/2026-10-09-quest3-fixed-clock.md).

- [ ] Instrument elapsed real time, simulation time, physics step count and
  solver dt. Reproduce under synthetic 60/72/90/120 Hz render schedules,
  irregular frame intervals, pauses, and stalls before altering behavior.
- [x] Establish one shared simulation clock with a fixed-step accumulator (opt-in).
  Advance only complete steps; retain the remainder and document a bounded
  catch-up policy. Do not silently discard simulation time or claim that a
  catch-up cap preserves real-time simulation during sustained overload.
- [ ] Validate equal simulated time for equal elapsed time, with movement,
  falling/jumping, doors/platforms, held-item contacts, projectiles,
  cooldowns, script timers, animation/root motion, and save round-trips.
- [x] Latch sampled discrete input edges until consumed by a simulation tick; zero or
  multiple ticks in one rendered frame must not drop or duplicate actions.
- [ ] Add rendering interpolation where needed. Preserve authoritative physics
  state; keep presentation transforms separate. Previous/current-state
  interpolation normally adds one simulation tick of world presentation delay,
  so do not apply that delay to tracked head pose. Explicitly verify visual
  hands, physical held items, and contact/haptic latency.
- [ ] Verify pause/resume and focus loss do not create a catch-up burst. Keep
  deterministic debug stepping and replay semantics explicit and tested.

Use [Fix Your Timestep!](https://gafferongames.com/post/fix_your_timestep/) as the
scheduling reference. Fixed-step scheduling does not require a worker thread,
and reducing tick frequency does not guarantee every render deadline is met:
a frame that performs a full expensive simulation tick can still exceed budget.

Deliverable: clock evidence and focused scheduling/interpolation PRs, with
unchanged intended gameplay time across presentation rates. Choose the world
simulation rate from correctness and interaction requirements; 60 Hz is a
candidate, not an already-approved global rate change.

### 3. CPU update and visibility costs

- [ ] Add low-overhead aggregated timing for physics/contact queries,
  animation/pose work, AI/scripts, effects/entity synchronization, and visibility
  preparation. Attribute the 6.3 ms update and 1.5 ms finish costs before choosing
  an algorithm or changing update frequency.
- [x] Add a one-shot host update-only benchmark using the complete game update,
  exact 60 Hz simulation intervals, excluded warmup, individual latency samples,
  and existing inclusive subsystem timers. See [benchmark usage](../benchmarks/README.md#host-update-only-benchmark).
  Host attribution complements, rather than replaces, rendered Quest measurements.
- [ ] Separate per-eye scene work, draw submission, swapchain waits, and frame
  submission. Keep logging outside hot loops; collect per-frame histograms or
  bounded samples rather than logging each frame.
- [ ] Remove or cache demonstrated redundant work, with invalidation tests and
  gameplay coverage. AI/pathfinding already has a worker for route queries;
  verify which remaining work dominates rather than moving it a second time.
- [ ] Measure each focused change against the same scene and prior best
  configuration. Preserve creature behavior, collision, visibility, and
  animation correctness instead of disabling the workload to improve numbers.

Deliverable: subsystem attribution followed by small PRs for the measured
hotspots. Reassess GPU headroom after CPU work is reduced.

### 4. Multiview experiment

- [ ] Measure how much of the eye stage is duplicated CPU submission/geometry
  work rather than synchronization or fragment execution.
- [ ] Prototype a texture-array swapchain, multiview framebuffer/shaders, and
  eye-indexed view/projection data behind a flag, with the sequential path as
  fallback. Share eye-independent preparation while preserving eye-dependent
  rendering and stereo offsets.
- [ ] Validate both eyes, transparent/material passes, HUD/hands, and runtime
  lifecycle. Test FFR off/on compatibility as a separate paired comparison.
- [ ] Keep it only if the measured saving justifies complexity and parity risk.

Multiview may reduce duplicated submission and some geometry overhead. It
still shades pixels for both eyes; a roughly 90% fragment share does not imply
that multiview can halve GPU time. The measured 2.3 ms eye stage is not a
promise of 2.3 ms recoverable work.

### 5. Selective parallel work, then optional simulation/render overlap

- [ ] If remaining CPU time justifies it, identify independent work that can
  run as jobs and apply results at a deterministic synchronization point.
- [ ] Consider full simulation/render overlap only after the fixed-step clock
  and rendering-state boundaries are established. Use immutable render
  snapshots with explicit ownership/publication; keep graphics resources and
  graphics API calls on their owning thread. The first candidate is one worker
  owning physics, scripts/AI, animation controllers, and effect application in
  their existing order. Scripts have mutable-state exceptions: returned effects
  alone do not make the live world safe to share with rendering.
- [ ] Interpolate between two **completed** immutable world snapshots. Never
  read a worker's in-progress frame. Sample the latest head and controller poses
  on the render path; do not interpolate tracking with the world snapshots.
  Specify the bridge for physical hands, held objects, release velocity, and
  haptics so current visual poses remain consistent with simulation contacts.
- [ ] Measure snapshot construction/copying, synchronization, queue age, input
  latency and worst-case frame time. A global lock around the existing mutable
  game state would serialize the two threads and defeat the intended benefit.
- [ ] Avoid queuing old simulation states or delaying head tracking to increase
  apparent throughput. Validate effect ordering, input edges, transitions,
  saves, physics/hand contacts and haptics.

This is conditional work, not a prerequisite for FFR or a guaranteed gain.
Overlap shortens a critical path only when independent work and CPU capacity
exist; it does not remove GPU cost or necessarily reduce total CPU work.

## Measurement and completion gates

Reuse [the benchmark runner](../benchmarks/README.md) and the repository
`oculus-profiling`, `vr-device-loop`, and `pr-visuals` workflows. Retain release
APK/source/asset hashes, effective features, refresh and eye dimensions,
per-run fixtures, raw telemetry, valid GPU counter mapping, and screenshots.
Never compare different refresh rates as a single-variable optimization A/B.

After the simulation clock gate passes, add an explicit 120 Hz benchmark
configuration and verify the actual active rate. Repeat both 90 and 120 Hz
baselines to distinguish feature savings from clock/governor changes.

Proposed completion criteria:

- [ ] Correct real-time simulation and interaction behavior at every supported
  presentation rate, including dropped frames and focus transitions.
- [ ] At 120 Hz, measured CPU critical-path and GPU frame work fit **8.33 ms**
  at p99, with a working headroom target of **7.5 ms at p95**. Measure those
  percentiles from individual frames, not the existing one-second aggregates.
  If GPU per-frame timing is unavailable, report the limitation and use
  compositor deadline/freshness evidence; do not invent GPU tail percentiles.
- [ ] At least 99.9% fresh application frames in each controlled 10-minute
  thermally settled run, with no sustained missed-deadline bursts, no torn
  frames, and no unexpected skipped application frames. Count actual display
  opportunities; exclude explicitly labeled startup/focus transitions rather
  than hiding them inside the average.
- [ ] Pass Many crowd, Rec1 crowd, and representative MedSci movement/combat/UI
  sessions with accepted stereo and peripheral image quality. Track PSS and
  thermal drift, and repeat regressions on the shipping 90 Hz configuration.
- [ ] Human headset review accepts text readability, held-item behavior and
  latency. If performance or quality fails, retain the prior default and
  document the remaining bottleneck instead of declaring completion from mean
  FPS alone.

## Tracking

- Completed foundation: [terrain benchmark PR #2120](https://github.com/tommy-xr/shock2quest/pull/2120).
- Current: opt-in clock correction measured at 90 Hz; broaden interaction, lifecycle and presentation checks before promotion.
- Before any 120 Hz experiment: finish milestone 2 interaction/presentation gates and remeasure full-tick frame tails.
- Subsequent CPU, multiview, and threading PRs: select from updated measurements.

## Default-off merge review

Independent Claude review recommends merging the instrumentation, opt-in clock,
and update-only benchmark with fixed simulation disabled by default. Review
follow-ups before enabling it:

- Bound or explicitly discard catch-up debt from loading/quickload stalls; the
  current eight-tick per-frame limit retains an unbounded backlog. Verify scene
  transitions and sustained overload without fast-forwarding the new mission.
- Bound/coalesce buffered input transitions, including analog noise crossing
  zero, so a render rate above 60 Hz cannot create increasing control latency.
- Exercise pause/focus, scene swaps, replay start, and zero/multiple-tick input
  delivery end to end. Presentation interpolation remains a separate task.

The review's default-path focus-time reset is now gated to fixed mode. CPU
profiling totals are discarded alongside frame-profiler resets, and the runner
requires matching CPU/PERF sample counts to reject mixed measurement windows.
