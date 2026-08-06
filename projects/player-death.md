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
- Without an activated and affordable station, death is terminal: the authored
  player death vocalization (`PlayerDeath0..4`, the gamesys SPEECH_TRIGGERS
  schemas) plays, continuous input stays suppressed (discrete actions such as
  quick-load deliberately keep working) for a three-second death sequence, and
  the mission is then replaced by the game-over screen. Dead games cannot be
  saved.

## Game over

Retail death without reconstruction ends the run at the Tri-Optimum archive
database - the load-game screen - so that is where this port lands too.
`scenes::GameOverScene` draws the original `GAMELOD.PCX` backdrop on the shared
`UiCanvas`, positioned by the authored `GAMELODR.BIN` widget rects (header,
archive list, and the two right-hand buttons), and shows:

- "YOU HAVE DIED" in the archive header,
- the most recent save in `<data_root>/saves` (or `GAMELOD.STR`'s `< EMPTY >`),
- "LOAD", which emits `GlobalEffect::Load` for that save, and "QUIT".

"LOAD" is inert and dimmed when no save exists, so the screen never offers a
recovery it cannot perform. Quicksaves resolve through `save_file_path`, so they
land in the same directory and are offered like any other save.

A full save browser (selecting among slots, the retail list widget) is deferred;
the screen offers the most recent save, which is the port's existing quick-load
idiom. Like the main menu, its buttons are pointer-driven, so clicking is a
flatscreen path until VR gets a pointer; the screen also forwards discrete input
actions (quick-load, quick-save), so every runtime keeps a way out.

The debug runtime exposes `player.life_state` as `alive`, `dead`, `game_over`,
or `respawning`, and `mission` becomes `game_over` on the screen, giving
play-through automation an explicit loss signal.

## Presentation follow-up

The functional lifecycle does not yet force the VR camera into a collapse pose
during the death sequence. A future presentation pass can add that without
changing the authoritative HP/QBR state machine above.
