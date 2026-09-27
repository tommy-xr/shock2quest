# Quantum Relocation

`Teleport` (-1018, power 35) marks the player's current position on its first
activation. A later activation returns to that marker and destroys it. Each
activation costs the authored five psi points. Changing powers or putting the
amp away does not clear the marker. Alt+T or **CLEAR MARKER** in the cyber
interface's psi MFD removes it for free, without moving the player.
The psi trainer now sells the implemented power at its authored cost.

This corrects #1306's speculative aimed-floor, free-arm and cycle-to-cancel
sketch. The installed 25AE `sshock2.kpf:data/res/strings/psihelp.str` entry
`Psi35` explicitly specifies the current position and Alt+T. The original
[manual, printed page 20](https://retrogamer.biz/wp-content/uploads/2016/06/System-Shock-2-Manual.pdf)
also describes marking, recall and explicit deletion.

The original [weapon trigger path](https://github.com/infernuslord/DarkEngine/blob/c8542d03825bc650bfd6944dc03da5b793c92c19/cam/src/shock/shkplgun.cpp#L1215)
passes through `PullTrigger`, `ReleaseTrigger`, `StartFiringSequence` and
`Fire` to `PlayerPsi::Activate`. In
[`shkpsi.cpp`](https://github.com/infernuslord/DarkEngine/blob/c8542d03825bc650bfd6944dc03da5b793c92c19/cam/src/shock/shkpsi.cpp#L231),
that activation pays `m_startCost` before replacing an already-active
metaproperty. It does not bypass payment on the second use. Type 3 is named
`kPsiTypeSustained` in `shkpsibs.h`; it is not an aimed-destination type.
The port retains its existing non-overloadable classification for Teleport;
this change adds no overload bonus or invented range.

The power's authored `Teleport` link points to `Teleport Marker` (-1109),
which owns five `ParticleAttachment` links. The port creates that real entity
and its particles; its existing position and entity serialization retain the
marker across a same-build save/load. Its cosmetic riders are excluded from
saves; loading the marker recreates them through the existing rider routine. The single-player marker is shared by
all amps. [Telliamed](https://thiefmissions.com/telliamed/allscripts.html)
identifies `TeleportMarker`'s `EndLevel` message. The mission removes the marker
and attached particles before caching an outgoing level, so returning to that
level cannot resurrect it. Ordinary saves and pause/resume do not end a level.

A stored location can become obstructed. Recall tests the current player
capsule with the existing player-placement collision filter. A blocked return
shows a message and preserves both marker and psi points; it does not invent a
nearby destination. A clear return uses the normal player relocation effect,
including held-item relocation, movement reset and locomotion tripwire handling.
This clearance refusal is the port's safety policy, not a claimed retail rule.

`psi-teleport.e2e.test.ts` covers current-position marking/recall, power swaps,
flat/VR clear input, blocked return and real-mission save/transition lifecycle.
All runtime verification uses the 25AE asset root explicitly.
