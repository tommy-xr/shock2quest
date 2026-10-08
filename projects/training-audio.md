# Training briefing cancellation

Earth's basic-to-advanced transition exposed two independent concerns:

- `TrapSound` plays at its object; `TrapSoundAmb` plays at the listener. Remote
  Xerxes announcements must remain audible even when their marker is far away.
- Stopping a sound trap must also invalidate starts already queued by older
  activations. Making a stale briefing quieter is not cancellation.

#870 addressed overlap by spatializing sound traps. #1685 restored ambient
announcements, with a positional exception for objects running both sound
scripts. Earth's montage and lobby traps author `TrapSoundAmb` with script
inheritance disabled, so that exception never applied to them. The old
position regression used trap 449, which does inherit `TrapSound`.

## Runtime contract

The script dispatcher assigns a monotonically ordered activation ID to each
root message. Synchronous relays, inverters, and delayed switch messages keep
that ID independently of the sender's entity ID. Reusing a button or entering
the same tripwire again is a new activation.

A sound trap remembers the newest activation that stopped it:

- Older starts are ignored, including ones that arrive later through timers.
- Equal-ID starts are allowed: one trigger commonly fans out to a silence
  inverter and a delayed start. The requested new briefing must survive its
  own silence operation.
- Newer activations can play normally. An old delayed stop cannot kill them.
- Pending delays and cancellation IDs survive save/load. Restoring IDs advances
  the allocator so new requests cannot be mistaken for pre-save work.

This is a port behavior improvement, not a claim that retail `TrapDelay`
cancels its timers. Timers still relay both switch edges and retain all other
effects (doors, destruction, progression). Only a sound trap rejects an
obsolete activation. Placement, gain, ambient looping, and AI speech mixing
are unchanged.

## Regression contract

Keep the following tests together when changing training or announcement audio:

- `earth-briefing-transition.e2e.test.ts`: enter and leave basic training,
  cross advanced training's real sensor, then observe the remaining 45-second
  playback window. Cover VR/flat and immediate/one-second lobby transitions.
  Assert no stale basic start, no overlapping playback intervals, and no early
  interruption of the advanced briefing. The immediate case stages between
  sensors to stress the pending exit voice; it is not a full walked route.
- `trap-sound-amb.e2e.test.ts`: the remotely triggered Xerxes announcement
  stays listener-relative, while authored positional traps stay positional.
- `audio-message-diagnostics.e2e.test.ts` and
  `earth-training-narration.e2e.test.ts`: preserve the existing diagnostics and
  single-entry behavior. These alone do not cover a transition.
- Asset-free Rust tests in `message_origin.rs` and `trap_sound.rs` (CI now
  explicitly selects the `shock2vr` package): delay/inverter
  fanout, a reused sender, save/load, a stale stop, and intentional restart.

Do not resolve failures by globally changing ambient sounds to spatial, by
making every new voice interrupt the current one, or by shortening the test
window before the late timer fires. Each can hide the overlap while losing
the intended narration. Tests check simulation playback intervals and actual
resolved playback settings; they are not a recording of wall-clock audio.
