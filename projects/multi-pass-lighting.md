# Multi-Pass Lighting Implementation Plan

## Overview
Implement Doom 3-style multi-pass lighting system starting with spotlight support for in-game flashlight functionality. The implementation will extend the current SceneObject architecture and work across both desktop and Oculus VR runtimes, with extensibility for future light types and shadow mapping.

## Current Architecture Analysis
- **SceneObject** structure: Contains material, geometry, transform, and skinning data
- **Material trait**: Has `draw_opaque()` and `draw_transparent()` methods
- **Rendering pipeline**: Two-pass system (opaque → transparent) in `gl_engine.rs:125-137`
- **Portal system**: Exists for culling optimization
- **Existing lighting**: Basic lightmap system already implemented

## Implementation Plan (Incremental Steps)

### Step 1: Core Light Data Structure
- Add `Light` trait with common properties (position, color, intensity)
- Implement `SpotLight` struct with direction, inner/outer cone angles
- Design for future extensibility: `PointLight`, `DirectionalLight`
- Add light management to `Scene` type

### Step 2: Multi-Pass Rendering Foundation
- Modify `gl_engine.rs` render loop to support multiple light passes
- Implement additive blending for light accumulation after base pass
- Add depth buffer management for light passes (read-only depth testing)
- Structure for future shadow map integration

### Step 3: Extended Material System
- Add `draw_light_pass()` method to base `Material` trait
- Signature supports future features: `draw_light_pass(context, matrices, light, shadow_map: Option<&ShadowMap>)`
- Implement lighting in each material type:
  - `BasicMaterial`: Standard Phong/Blinn-Phong lighting
  - `SkinnedMaterial`: Phong lighting with bone transformations
  - `LightmapMaterial`: Combine dynamic lights with existing lightmaps
  - `BillboardMaterial`: Simple diffuse or skip light passes
- Default implementation returns `false` (no-op) for backwards compatibility

### Step 4: Portal-Based Light Culling
- Integrate with existing `PortalVisibilityEngine`
- Implement light frustum culling against portal system
- Add light influence bounds checking for performance
- Prepare culling system for shadow map optimization

### Step 5: Cross-Platform Integration
- Ensure OpenGL ES compatibility for Oculus runtime
- Test light passes on both desktop and VR platforms
- Add flashlight attachment to player/hand position in VR
- Validate performance on Quest hardware

### Step 6: Documentation & Future Roadmap
- Create comprehensive documentation in `projects/multi-pass-lighting/`
- Document material lighting implementation patterns
- Outline shadow mapping integration points
- Add performance benchmarks and optimization notes
- Test with existing System Shock 2 levels

## Future Extension Points
- **Shadow Mapping**: Light passes already accept optional shadow map parameter
- **Multiple Light Types**: `Light` trait system supports point lights, directional lights
- **Deferred Rendering**: Current multi-pass structure can evolve to deferred pipeline
- **Light Volumes**: Portal culling system ready for light volume optimization

Each step builds incrementally and can be tested independently, following the project's philosophy of small, manageable changes while maintaining extensibility for advanced lighting features.