# Quest 3: 90/120 Hz exploration and OpenXR recovery checks

The six-hybrid Rec1 workload misses reliable 90 Hz. Requesting 120 Hz succeeds,
but the application delivers only 92.506 FPS and the compositor reports 865 stale
frames over 30 seconds. These exploratory samples do not justify changing the
shipping 90 Hz target. They identify a workload for the next optimization pass.

## Workload and provenance

Collected October 6, 2026 on Quest 3 / Android 14, with release APKs verified as
non-debuggable. OpenXR advertised **72, 80, 90, and 120 Hz**; 100 and 110 Hz were
not advertised. Both eyes rendered at **1680×1760**. The active rate matched the
requested rate throughout each accepted sample.

All four samples used `rec1-six-hybrids`: a fixed camera over the court, six
animated pipe hybrids, the authored Rumbler removed, and sixteen authored lamps
on. Every sample passed the runner's focus, refresh, mesh-count, lamp-state, and
animation checks. Each run restarted the app, warmed up for 10 seconds, and
collected 30 one-second buckets. The fixtures compensate the resting headset
pose; physics, scripts, and animation continue running.

| Build | Source | APK SHA-256 |
| --- | --- | --- |
| Parent | `30d92fdb27f08a5f43a640d3a18af6adf9d10769` | `5861008ab560ef5ee4547bc23fdd437047c363fb496dd5d0c02e9d3b0666aa16` |
| PR #2068 head | `5dec9748f4620c7ba8d5afba056384319cda9246` | `e5719fee87fe29ee94ff7559ffa0c8b2f05dfca06ad34bfef0369a9cf70c4d89` |
| Head, experimental 120 Hz | Head plus [one-line target patch](2026-10-06-quest3-refresh/target-120.patch) | `c4ef5fa8845d284503593726c42a31e862bc1c66f1992c42a4474cf91a4fd2b1` |

[Results JSON](2026-10-06-quest3-refresh/results.json) records the OS fingerprint,
remaster archive hashes, full metric distributions, battery snapshots, fixtures,
and screenshot URLs. The device archives matched the host archives. Device
serials and host-specific paths are omitted from the published evidence.

## Frame delivery

Rows below follow collection order. FPS minimum is the lowest **one-second
compositor reading**, not a per-frame percentile. Stale, torn, and skipped counts
are totals over each 30-second interval. Compositor FPS alone does not establish
fresh application-frame delivery.

| Build / lighting | Active Hz | App FPS mean | Compositor FPS mean/min | Stale | Torn | Skipped |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Head / on | 90 | 86.348 | 87.167 / 82 | 160 | 2 | 0 |
| Parent / off¹ | 90 | 84.737 | 85.400 / 81 | 265 | 0 | 0 |
| Parent / on | 90 | 84.479 | 84.967 / 80 | 244 | 1 | 0 |
| Head + target patch / on | 120 | 92.506 | 93.267 / 86 | 865 | 0 | 0 |

¹ Visual review rejected lighting-off as an acceptable quality baseline:
creatures and props render as black silhouettes. Its timings remain useful for
diagnosis. This observation is on the parent build and is not attributed to
PR #2068.

## Timings, startup, and memory

Timings are means in milliseconds. **Eyes includes swapchain synchronization**
and CPU submission for both eyes; it is not pure GPU time. `App` is the VrApi
field. GPU load is VrApi's reported fraction expressed as a percentage.

| Build / lighting | Update | Scene | Eyes | Finish | App | GPU load | PSS MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Head / on | 4.779 | 1.170 | 3.161 | 1.295 | 4.473 | 47.2% | 666.2 |
| Parent / off | 4.796 | 1.124 | 3.141 | 1.332 | 3.420 | 37.3% | 667.2 |
| Parent / on | 4.857 | 1.119 | 3.234 | 1.332 | 4.525 | 47.3% | 668.5 |
| Head + target patch / on | 4.893 | 0.989 | 3.081 | 1.311 | 4.386 | 52.8% | 668.8 |

| Build / lighting | Game init ms | Launch-to-focused ms | Battery temperature before → after |
| --- | ---: | ---: | --- |
| Head / on | 6043.792 | 7161 | 32 → 34 °C |
| Parent / off | 6116.899 | 7200 | 33 → 35 °C |
| Parent / on | 5380.513 | 6612 | 35 → 36 °C |
| Head + target patch / on | 5732.035 | 6973 | 36 → 37 °C |

Android reported AC powered and USB powered false throughout. Battery level
fell from 96% to 93% across these runs. Clocks were not locked. Different
temperatures, restart timing, and a single sample per condition prevent a
performance improvement/regression claim about PR #2068.

On the parent, enabling object lighting increased App time by 1.105 ms and
reported GPU load by 10 percentage points. Repeat paired runs before treating
that as a stable cost estimate. The device's hardware-counter catalog was
queried, but **no hardware-counter sample was collected in this pass**; these
measurements do not establish GPU saturation.

## Device images

These are unchanged Quest stereo captures from the measured runs. Stills show
the workload and visual output, not achieved frame rate or animation cadence.

![PR head at 90 Hz, lighting enabled](https://gist.githubusercontent.com/tommy-xr/9de5bcd0f203cf2bdae4af09a26c4a4f/raw/4c4b8f98d8b9b428db9ef9aeda7666f32daba10a/pr2068-head-90hz-six-hybrids.png)

![PR head with experimental 120 Hz request](https://gist.githubusercontent.com/tommy-xr/9de5bcd0f203cf2bdae4af09a26c4a4f/raw/4c4b8f98d8b9b428db9ef9aeda7666f32daba10a/pr2068-head-target120hz-six-hybrids.png)

![Parent with lighting disabled: black silhouettes](https://gist.githubusercontent.com/tommy-xr/9de5bcd0f203cf2bdae4af09a26c4a4f/raw/4c4b8f98d8b9b428db9ef9aeda7666f32daba10a/base-90hz-lighting-off.png)

[Parent with lighting enabled](https://gist.githubusercontent.com/tommy-xr/9de5bcd0f203cf2bdae4af09a26c4a4f/raw/4c4b8f98d8b9b428db9ef9aeda7666f32daba10a/base-90hz-lighting-on.png)
provides the corresponding lit view. The [media gist](https://gist.github.com/tommy-xr/9de5bcd0f203cf2bdae4af09a26c4a4f)
contains all five images with pinned raw URLs.

## PR #2068 device validation

The exact [PR #2068](https://github.com/tommy-xr/shock2quest/pull/2068) head built
as a release APK and passed its five host recovery-bookkeeping tests.
In `debug_minimal`, two Home/return cycles followed by two sleep/wake cycles
each traversed `STOPPING → IDLE → READY → FOCUSED` and resumed fresh focused
performance samples in the same process. Each cycle waited three seconds away,
resumed the existing activity without force-stop, then waited seven seconds
before checking fresh samples. The four checks observed 6, 7, 7, and 7 fresh
focused buckets, respectively.

[Lifecycle results](2026-10-06-quest3-refresh/lifecycle.json) and
[structured runtime log](2026-10-06-quest3-refresh/lifecycle.txt) preserve the
observations. The final [stereo grid capture](https://gist.githubusercontent.com/tommy-xr/9de5bcd0f203cf2bdae4af09a26c4a4f/raw/4c4b8f98d8b9b428db9ef9aeda7666f32daba10a/pr2068-lifecycle-cycle4.png)
was inspected. No panic, XR-call failure, or ANR was found in the captured
lifecycle logs; broad Android logs were inspected locally and are not included
in this public evidence bundle.

No blocking PR-specific failure was observed. These checks cover ordinary
rendering and session restart. **No transient OpenXR call failure was observed
or injected on device.** Retry branches, session/instance loss, longer ANR
soaks, and passthrough-specific recovery remain unverified.

After the experiment, the exact 90 Hz PR-head APK was restored and stopped.
Guardian/proximity automation, mission selection, benchmark configuration, and
log-buffer settings were restored. The shipping refresh target was not changed.

## Reproduction and retained evidence

Build/install the recorded source revision as a release APK, then run:

```sh
node tools/quest-bench/run.mjs --scene rec1-six-hybrids --lighting on \
  --repeats 1 --warmup 10 --seconds 30 --output /tmp/quest-refresh-sample
```

For the parent's lighting comparison, use `--lighting both`. To reproduce the
120 Hz experiment, apply the retained target patch with `git apply --unidiff-zero`
to the recorded PR head,
rebuild, and reinstall first. Restore the unmodified APK after testing. Source
revisions are recorded because the runner does not build or install APKs itself.

The [evidence directory](2026-10-06-quest3-refresh/) contains a fixture and filtered
telemetry log for each row. Logs retain the original ordering of engine timing,
paired workload, and compositor records. This OS emitted the single `Fov=0`
series; the parser uses it when the paired `Fov=0D` series is absent. Logs may
include buffered warmup records; use the parsers' **last 30 buckets**, as the
original runner does. The retained logs were re-parsed and workload-validated
against the recorded results before publication.

## Limits and next measurements

- An initial MedSci1 spawn sample faced a nearby wall and is excluded from
  rendering-performance conclusions. This pass is not a survey of worst-case
  levels or a thermally settled play session.
- Alcohol distortion, translucent particles, large item piles, denser crowds,
  and other mission views were not measured. The alcohol implementation adds
  vertex shear and a translucent per-eye pattern; its cost needs an on/off test.
- First investigate the ~4.9 ms game update and ~1.3 ms visibility-finish work.
  Separate swapchain waits from draw submission in eye timings before choosing
  a CPU or GPU optimization. Stable 90 Hz needs an 11.11 ms frame budget;
  120 Hz needs 8.33 ms, with headroom for frame-time tails.
- Add repeat pairs and longer runs, then broaden the workload set. One-second
  means cannot establish per-frame p95/p99 latency or reliable fresh delivery.

The profiling pass also exposed a host-tool cleanup bug: `adb exec-out cat` can
return missing-file error text with a successful host exit. An absent mission
selector was consequently recreated with that text. It was manually removed
during cleanup; the accompanying helper fix probes absence explicitly, reads
through non-PTY `adb shell`, and propagates connection/read failures.
