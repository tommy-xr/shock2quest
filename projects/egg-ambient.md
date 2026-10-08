# Egg ambient sound

Retail eggs have an engine-owned ambient emitter, separate from their hatch
script. The shipped `shock2.gam` records show:

- `Eggs` (-1474): `P$AmbientHa`, schema `eggloop`, radius 25 Dark units,
  volume 0, flags 0, no auxiliary schemas. Goo, grub and swarmer pods inherit it.
- `eggloop` (-2748): samples `egglp1`, `egglp2`, `egglp3`, equal weights;
  `P$SchLoopPa` has max_samples=1, zero intervals and no count limit
  (one selected sample loops seamlessly). Schema volume is -500 millibels.
- The samples are shipped as `snd/GRUB/EGGLP1.WAV` through `EGGLP3.WAV`.

Reproduce the property inspection with `cargo dq templates 1476` and
`cargo dq templates 2748`. The sample mapping is also in
`references/env_sound.spew` under `eggloop`.

[Telliamed's BaseEgg reference](https://thiefmissions.com/telliamed/allscripts.html)
lists `TurnOn`, activating tweqs, and the `pod_exp` hatch sound; its GooEgg,
GrubEgg and SwarmerEgg subclasses supply their respective payloads. It does
not describe a script-owned hum or stopping ambience on hatch. The inherited
ambient property remains on the opened shell in this port.

The original engine's `src/sound/ambient.c` starts object schemas with
`SCH_SET_OBJ | SCH_SET_CALLBACK | SCH_RADIUS_VOLUME`, adds `SCH_SHARP_ATTEN`
unless disabled, and halts them outside their radius. This establishes that
this sound belongs to ambient playback, not a new BaseEgg timer.

## Reproduced omission and fix

The existing mission ambient path already plays `egglp[123].wav` beside
hydro1 object 326. Both common debug scene wrappers instead inherited the
GameScene default `ambient_audio_state() -> None`, so debug_annelid was silent
even though its pods had the correct properties. Both wrappers now forward
to MissionCore, using the same sound path as a normal mission.

`tools/shock2-sdk/test/egg-ambient.e2e.test.ts` checks live audio sinks in
flat and VR for debug_annelid and hydro1: an egg sample is looping, hatching
keeps the same sink/sample, leaving range stops it, and returning starts a
new loop. The debug_annelid flat case fails before the forwarding fix.

This restores the missing hum; it does not claim complete retail acoustic
parity. The existing shared ambient mixer uses fixed gain 0.5, generic rodio
attenuation, an eight-emitter cap, and compares unscaled authored radii to
world distances. Retail uses schema volume and radius-based sharp attenuation.
Those broader mixer differences are separate from this scene omission.
