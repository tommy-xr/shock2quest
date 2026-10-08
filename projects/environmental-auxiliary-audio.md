# Environmental auxiliary audio

Retail `P$AmbientHa` environmental regions can specify a main schema and two
auxiliary schemas. `src/sound/ambient.c` in the original Dark Engine chooses
the nearest environmental region and starts both nonempty auxiliaries at the
listener. Region changes halt the old auxiliaries even when the main schema
is unchanged.

Confirmed authored regions in the installed game data:

| Mission / object | Main schema | Auxiliary |
| --- | --- | --- |
| eng1 / 138 | eng_pump1 | eng_hor1 |
| medsci1 / 1299 | ms_ambhum | ms_stress |
| rec3 / 442 | rec_lowtone | rec_ssim |
| rick1 / 231, 1380, 2090 | rck_start | rck_stress |
| shodan / 832 | shd_black | shd_horror |

The port previously discarded both auxiliary fields. Environmental selection
now carries the region identity and all three schemas. The shared game update
owns auxiliary playback, so desktop, Quest, and debug runtime use the same
path. Main-bed identity is tracked by schema rather than the randomly resolved
sample, avoiding per-frame restarts for multi-sample main schemas.

The auxiliary controller reuses listener-relative playback, authored gain/pan,
pause handling, and audio diagnostics. It stops its handles and schedule on
region exit or scene replacement. Each auxiliary is independent: an empty or
unavailable schema does not suppress another layer or the main bed.

`eng_hor1`, `rec_ssim`, and `shd_horror` repeat seamlessly. `ms_stress` randomly
selects among seven `stres_m` samples every 6–10 seconds; `rck_stress` uses
10–20 seconds. Both interval schemas are polyphonic (flag bit 0), allowing two
samples. The controller uses the simulation clock for deterministic stepping
and pause behavior, selects a fresh weighted sample/pan per play, and bounds
its retained voices by the authored maximum. Monophonic timing includes the
clip duration; count-limited schedules stop after the authored count.

This is not a global schema scheduler: positional emitters and main beds keep
their existing playback policy. Ambient radius scaling, sound propagation,
main-bed gain, and other ambient flags remain separate work.

The Engineering marker names `eng_pump1`, which is absent from the installed
gamesys/sample assets (`eng_pump` exists). Its auxiliary must still play; this
change does not invent a replacement for the missing main schema.

Validation covers authored Engineering loops in flat/VR (auxiliary live despite the missing main bed,
stable handles, pause/resume, range re-entry, and scene cleanup), Med/Sci
interval cadence and cancellation, and unit tests for loop timing/count flags.
