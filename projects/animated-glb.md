# Animated GLB Support Project

## Overview

This project adds support for animated GLB files to the shock2quest engine, enabling the dark_viewer tool to load and display skeletal animations stored within GLB files. Unlike the current animation system which expects separate files for each animation clip, GLB files can contain multiple animations in a single file.

## Current Status

- ✅ Animated GLB import path converts glTF channels into `AnimationClip`s, including inverse-bind support.
- ✅ `dark_viewer` can preview animated GLBs via the new `GlbAnimatedViewerScene`, with CLI filtering.
- ✅ Skinned meshes render with GPU skinning (`SkinnedMaterial`) and read diffuse textures (incl. `KHR_materials_pbrSpecularGlossiness`).
- ⚠️ Multi-weight skinning is still simplified to a dominant joint per vertex; keep as follow-up.
- ✅ Documentation and tooling updated; noisy debug logging removed.

## Goals

- Support GLB files with skeletal animations in dark_viewer
- Enable syntax like `dark_viewer shark.glb swimming` or `dark_viewer robot.glb walk,run,idle`
- Bridge GLB's keyframe-based animation system with shock2quest's matrix-per-frame format
- Reuse proven animation loading logic from the functor project

## Key Findings

### Current System (shock2quest)
- Uses `AnimationClip` with `HashMap<JointId, Vec<Matrix4<f32>>>` for joint transforms per frame
- Skeleton made of `Bone` objects with `joint_id`, `parent_id`, and `local_transform`
- Animation player handles blending and sequencing of clips
- Expects separate `.mc` files for each animation clip

### GLB Format (from functor & glTF spec)
- Multiple animations stored in single file
- Each animation has channels targeting node properties (translation, rotation, scale)
- Keyframe-based with interpolation support
- Node-based skeleton with hierarchical transforms

### Mapping Strategy
Port the functor implementation and adapt it to shock2quest data structures. Convert GLB's keyframe-based channel animations into shock2quest's matrix-per-frame format through interpolation.

## Implementation Notes (Highlights)

- New GLB animation IR lives in `dark/src/motion/glb_animation.rs`.
- `GLB_ANIMATION_IMPORTER` loads animations and skeletons, converts channels to per-frame matrices (30 FPS) with proper joint lookup.
- Skeletons now cache node→joint mapping, rest locals, and inverse-bind matrices so animation clips produce correct deltas.
- `dark_viewer` gained `GlbAnimatedViewerScene`; CLI accepts comma-separated animation names (or default to all).
- `GlbViewerScene` still handles static GLBs; animated path now uses `SkinnedMaterial`.
- Added fallback solid-color textures for skinned meshes without images.
- Enabled `KHR_materials_pbrSpecularGlossiness` to read diffuse textures for assets such as the shark.

## Key Technical Challenges & Solutions

### 1. Node Index to Joint ID Mapping
- **Challenge**: GLB uses node indices, shock2quest uses JointId
- **Solution**: Create mapping during skeleton creation or use node index directly as JointId

### 2. Keyframe Interpolation
- **Challenge**: GLB stores sparse keyframes, shock2quest expects dense frame arrays
- **Solution**: Implement linear/cubic interpolation functions for each property type, use glTF's interpolation mode (LINEAR, STEP, CUBICSPLINE)

### 3. Skeleton Compatibility
- **Challenge**: GLB nodes vs shock2quest Bones
- **Solution**: Port functor's skeleton building logic and adapt to shock2quest's Skeleton structure

### 4. Multiple Animations in Single File
- **Challenge**: Current system expects one animation per file
- **Solution**: GLB_ANIMATION_IMPORTER returns `Vec<AnimationClip>` and handles name-based selection

### 5. Transform Composition
- **Challenge**: GLB stores separate T/R/S channels, shock2quest expects final matrices
- **Solution**: Compose TRS matrices during conversion, following glTF transform order

### 6. Skinning and Vertex Weights (Deferred)
- **Challenge**: GLB supports multiple bone weights per vertex; rendering currently uses single-dominant joints.
- **Solution**: For now, continue selecting the highest-weight bone per vertex. Track follow-up work to implement full multi-weight skinning in both importer and shader.

## File Structure

```
dark/src/motion/
├── glb_animation.rs          # New GLB animation data structures
├── animation_clip.rs         # Existing
└── mod.rs                    # Updated exports

dark/src/importers/
├── glb_animation_importer.rs # New GLB animation importer
├── glb_model_importer.rs     # Existing
└── mod.rs                    # Updated exports

tools/dark_viewer/src/scenes/
├── glb_animated_viewer.rs    # New animated GLB viewer
├── glb_viewer.rs             # Existing static GLB viewer
└── mod.rs                    # Updated exports
```

## Success Criteria

- [x] dark_viewer can load and display animated GLB files
- [x] Multiple animations can be specified and cycled through
- [x] Smooth animation playback with proper interpolation
- [x] Integration with existing animation player system
- [x] Support for common glTF animation features (translation, rotation, scale)
- [x] Simplified skinning works with single dominant bone per vertex
- [x] Clear TODO markers for future multi-bone skinning implementation
- [x] Available animations are listed when loading GLB files
- [x] Helpful error messages when requested animations are not found
- [ ] Full multi-weight GPU skinning (follow-up)

## Example Usage

```bash
# List all available animations
dark_viewer robot.glb

# Output:
# Available animations in robot.glb:
#   1. Walk (duration: 2.50s, frames: 75)
#   2. Run (duration: 1.80s, frames: 54)
#   3. Idle (duration: 5.00s, frames: 150)
#   4. Jump (duration: 1.20s, frames: 36)
# Playing animations: Walk

# Play specific animations
dark_viewer robot.glb walk,jump

# Output:
# Available animations in robot.glb:
#   1. Walk (duration: 2.50s, frames: 75)
#   2. Run (duration: 1.80s, frames: 54)
#   3. Idle (duration: 5.00s, frames: 150)
#   4. Jump (duration: 1.20s, frames: 36)
# Playing animations: Walk, Jump

# Error case
dark_viewer robot.glb dance

# Output:
# Available animations in robot.glb:
#   1. Walk (duration: 2.50s, frames: 75)
#   2. Run (duration: 1.80s, frames: 54)
#   3. Idle (duration: 5.00s, frames: 150)
#   4. Jump (duration: 1.20s, frames: 36)
# Error: No matching animations found for: dance
```

## References

- Functor implementation: `/Users/bryphe/functor/runtime/functor-runtime-common/src/asset/pipelines/model_pipeline.rs`
- Functor animation structures: `/Users/bryphe/functor/runtime/functor-runtime-common/src/animation.rs`
- glTF 2.0 Animation Specification: https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#animations
- Existing shock2quest animation system: `dark/src/motion/animation_clip.rs`

## Notes

This approach leverages the proven functor implementation while adapting it to shock2quest's existing animation infrastructure. The key innovation is the keyframe-to-matrix conversion that bridges the two animation paradigms.
