# Player Death

When the player 'dies' in the VR game, we want to make them _feel_ it. The idea is to take control of the camera and have the camera 'collapse' to the floor.

If there is a revive chamber (a ResurrectionStation / ResStation - based on template -1677) and the following conditions are met:
1. The user has _activated_ the revive station
2. The user has 5 nanites

If the revive station is available, 5 nanites should be deducted from the player balance and the player should be teleported to the revive station.

Some example entities in medsci1.mis (can be queried with dark_query):
- 600: the resurrection state
- 909: the res station button

It looks like the relevant script is 'ResurrectMachine' - but I'm not sure how/if there is a link between the button and the actually resurrection station. It _might_ be that there is a limit of one per level, or, that the same teleport trap used for the button is used for the player (I think the latter is most likely)

## Death Animation

When the player dies, we should lerp the camera position to a position on the floor, close to straight downward, with a random vector facing front or straightforward.

## Engine Enhancements

The _biggest_ change to support this is the ability to have the game 'override' the VR head position. I'd propose adding a new API like 'CameraOverride { transition: 0.0, position, forward, up }`. 

Then, we'd add a 'CameraOverride' to the 'SceneContext' as an Option. If not specified (Default), we use the VR position, rotation as today. If it _is_ specified, we'll override the camera position / forward / up vectors, with a transition parameter (how _much_ we are overriding).

## Other enhancements

A 'static' or 'blood' effect when the player dies would be pretty cool but not on the critical path.
