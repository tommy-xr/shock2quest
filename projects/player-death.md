# Player Death

When the player 'dies' in the VR game, we want to make them _feel_ it. The idea is to take control of the camera and have the camera 'collapse' to the floor.

If there is a revive chamber (a ResurrectionStation / ResStation - based on template -1677) and the following conditions are met:
1. The user has _activated_ the revive station
2. The user has 10 nanites (the retail Normal-or-higher cost)

If the revive station is available, 10 nanites should be deducted from the player balance and the player should be teleported to the revive station.

## Death Animation

When the player dies, we should lerp the camera position to a position on the floor, close to straight downward, with a random vector facing front or straightforward.

## Engine Enhancements

The _biggest_ change to support this is the ability to have the game 'override' the VR head position. I'd propose adding a new API like 'CameraOverride { transition: 0.0, position, forward, up }`.

Then, we'd add a 'CameraOverride' to the 'SceneContext' as an Option. If not specified (Default), we use the VR position, rotation as today. If it _is_ specified, we'll override the camera position / forward / up vectors, with a transition parameter (how _much_ we are overriding).

## Other enhancements

A 'static' or 'blood' effect when the player dies would be pretty cool but not on the critical path.

---

## Gameplay implementation

Player health is tracked by `PropHitPoints`/`PropMaxHitPoints`. All HP effects
flow through `MissionCore::handle_effects`; lethal player damage is clamped at
zero and enters `PlayerLifeState` exactly once.

The resurrection scanner uses the mission's existing authored flow:

- `Res_Station_Button` (`ResurrectMachine` + `Tweqable`) changes from `res_pad`
  to `res_pad2` when activated.
- The switched `PropModelName` persists in normal mission save data and is the
  durable activation marker.
- Its `SwitchLink` points to the authored `TrapTeleport` at the reconstruction
  position. Death never guesses a station coordinate or uses a runtime ID.
- With an active station and 10 carried nanites, the cost is debited atomically,
  input is suppressed for the retail five-second delay, then the player is
  teleported to that trap and restored to half maximum health.
- Without an activated and affordable station, the player remains terminally
  dead until an explicit load/restart. Dead games cannot be saved.

The debug runtime exposes `player.life_state` as `alive`, `dead`, or
`respawning`, giving play-through automation an explicit loss signal.

## Presentation follow-up

The functional lifecycle does not yet force the VR camera into a collapse pose
or draw a dedicated death overlay. A future presentation pass can add those
effects without changing the authoritative HP/QBR state machine above.
