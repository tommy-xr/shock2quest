# Climbing

## Goals

- Implement 'jumping' in both the desktop_runtime and vr runtime
- Implement climbing ladders in the desktop runtime
  - 'Ladder-type' - when touching the ladder, should be able to move up and down the ladder
- Implement 'mantling' in the desktop_runtime
  - When looking at an object that can be mantled, and holding 'jump', the player gets pushed up on top of the object
- Implement climbing ladders / mantling in the vr runtime
  - For both interactions, we can use the vr-climbing mechanic

## Climbing Mechanics

### Jumping

### VR Climbing Loop

1. Detect a valid grid
   a. Hand collider overlaps a climbable collider (layer/tag?)
   b. Player is squeezing past a trigger threshold and has an empty hand
2. Lock a hand anchor
   a. On grip start, store handAnchor = handPoseWorld
   b. While gripping, compute delta = currentHandPose - handAnchor
   c. Move the player root by -delta (kinematic move?)
   d. Update handAnchor = currentHandPose each frame
3. Release + momentum
   a. On release, apply a body velocity derived from controller velocity
4. Gravity
   a. While any hand is gripping -> reduce gravity to zero (or near zero) and rely on pull
   b. When no hands are gripping: restore gravity

## VR Desktop Climbing Mechanic

For ladders, behave like half-life / half-life 2
For mantling

## Potential Prepatory Refactors

- Upgrade to latest rapier ahead of time to make sure we have the latest code
- Factor out different 'movement modes' for player movement in physics_world
  - Create common movement mode trait
  - WalkMode
  - DesktopLadderMovementMode
  - DesktopMantleMovementMode
  - VRClimbMovementMode
- Add helper for is_touching_ground, is_touching_climbable
