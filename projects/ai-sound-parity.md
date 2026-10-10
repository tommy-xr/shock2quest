# AI sound parity audit

Checked against the shipped voice database and sound schemas, October 2026.
This is a trigger and playback audit, not a claim of complete audible parity.

## Restored in this change

Animated creatures (including spiders, hybrids, monkeys, midwives and robots),
grubs and swarms now request `atlevelzero` through `atlevelthree` according to
their current alertness. Each uses its own voice. A missing concept stays silent;
it does not borrow a different creature's voice or repeatedly warn in the log.

The selected schema's repeat interval and decoded clip length determine the
next call. Initial requests are staggered over two seconds. The countdown is
saved in the AI's script state. Dead, stunned and currently scripted animated
actors do not request ambient calls; distant actors do not allocate inaudible
voices. The global broadcast-disable flag in `P$AI_BcstSe` is honored.

Example authored intervals (milliseconds after clip end for these monophonic
schemas; values are from `P$SchLoopPa`, not a generic creature timer):

| Schema | Creature/state | Minimum | Maximum |
| --- | --- | ---: | ---: |
| `sp1at0` | Spider, calm | 100 | 15000 |
| `sp1at3` | Spider, combat | 100 | 1000 |
| `pd1at0` | Protocol droid, calm | 100 | 10000 |
| `sd1at0` | Security droid, calm | 100 | 10000 |
| `md1at0` | Maintenance droid, calm | 100 | 10000 |
| `mw1at0` | Midwife, calm | 100 | 2000 |
| `og1at0` | Hybrid, calm | 100 | 100 |

Speech now observes the concept priorities stored in `Speech_DB`: equal/lower
priority requests wait while a clip is playing; higher priority requests stop
the previous voice. This prevents ambient speech from talking over an attack
or death line. Playback and interruption remain visible in `/v1/audio/recent`.

## Other triggers

| Sound family | Current coverage | Remaining work |
| --- | --- | --- |
| Footsteps | Animation foot plants query the creature's material and the ground surface. Spider samples resolve in `debug_melee`. | No new gap established by this audit. |
| Attack voices | Animated attacks request `comattack`, including protocol droid self-destruct. | Requests are tied to animation selection rather than a complete combat-sound policy. |
| Enemy weapon shots | Weapon/projectile sound queries are present. | Voice concepts such as `ogshoot` need a separate comparison against weapon sounds. |
| Death voices | Animated organic deaths request loud/soft concepts. Linked debris/explosion deaths use a separate path. | Loud/soft selection is random; custom grub/swarm and linked-death voice coverage is incomplete. |
| Alert transitions | Cameras have transition calls. | Mobile creatures lack `tolevelone`, `toleveltwo`, `tolevelthree`, `spotplayer`, `backtozero` and `lostcontact`. Ambient state changes are not substitutes for these lines. |
| Pain | Physical damage reactions exist. | No mobile-AI requests for `comhithigh`, `comhitlow`, `comhitamb` or `comhitnodam`. |
| Melee feedback | Physical parry/impact effects exist. | Voice reactions such as `comsucchit`, `comsuccblock`, `comblocked` and `comdetblock` are not wired. |
| Tactical/discovery calls | Some related gameplay behaviors exist. | Charge, fleeing, ally warnings, body discovery and similar speech triggers remain incomplete. |
| Cameras/turrets | Camera speech and camera motor sound; turret activation/deactivation and firing. | Device sustain loops and turret rotation audio need their own pass. |

## Playback limitations to follow up

- Per-concept broadcast overrides and extra AI sound tags are not applied; only
  the global mute flag is read from broadcast customization.
- The existing speech resolver collects all candidate schemas when no tags are
  supplied. Context-sensitive variants need explicit tags and matching tests.
- Ambient speech deliberately allows only one spoken sample per actor. Schemas
  authored for overlapping samples (some swarm/SHODAN voices) are not reproduced
  polyphonically. Continuous loops are re-requested at clip boundaries rather
  than held in a seamless audio sink.
- Cameras retain their existing one-line-per-alert-level sustain policy; this
  change adds recurring calls to mobile AI, not every device AI.

## Verification

Unit coverage checks speech priorities, countdown restoration, mute and missing
voice behavior, alert-state changes, and suppression during death, stun and
scripted sequences. SDK regression tests exercise actual spider samples,
authored repeat gaps, silence after death, and protocol droid ambient speech.
