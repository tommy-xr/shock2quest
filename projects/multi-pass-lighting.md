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

### Step 1: Core Light Data Structure ✅ COMPLETED
- ✅ Add `Light` trait with common properties (position, color, intensity)
- ✅ Implement `SpotLight` struct with direction, inner/outer cone angles
- ✅ Design for future extensibility: `PointLight`, `DirectionalLight`
- ✅ Add light management to `Scene` type

**Implementation Details:**
- **Files Created:**
  - `engine/src/scene/light.rs` - Core Light trait and SpotLight implementation
  - `engine/src/scene/light_system.rs` - LightSystem container for managing multiple lights
- **Files Modified:**
  - `engine/src/scene/scene.rs` - Enhanced Scene struct with integrated LightSystem
  - `engine/src/scene/mod.rs` - Module exports for new lighting components
- **Features Delivered:**
  - `Light` trait with position, color/intensity, and light type identification
  - `SpotLight` implementation with cone angles, range, and attenuation calculations
  - `LightSystem` container with light management, culling, and querying capabilities
  - Enhanced `Scene` struct with backwards compatibility via Deref trait
  - Comprehensive test coverage (8 passing tests)
  - Foundation for portal-based light culling integration

### Step 2: Multi-Pass Rendering Foundation ✅ COMPLETED
- ✅ Modify `gl_engine.rs` render loop to support multiple light passes
- ✅ Implement additive blending for light accumulation after base pass
- ✅ Add depth buffer management for light passes (read-only depth testing)
- ✅ Structure for future shadow map integration

**Implementation Details:**
- **Files Modified:**
  - `engine/src/gl_engine.rs` - Enhanced render loop with 3-pass system (base → lighting → transparent)
  - `engine/src/scene/material.rs` - Added `draw_light_pass()` method to Material trait
  - `engine/src/scene/scene_object.rs` - Added `draw_light_pass()` and `get_world_position()` methods
- **Features Delivered:**
  - Three-pass rendering system: base opaque pass, per-light additive passes, transparent pass
  - Proper OpenGL state management: additive blending (GL_ONE, GL_ONE) for light accumulation
  - Read-only depth testing (GL_EQUAL) prevents overdraw during light passes
  - Light culling: only render objects affected by each light using `affects_position()`
  - Future-ready shadow map parameter in `draw_light_pass()` interface
  - Backwards compatibility: default `draw_light_pass()` implementation returns false (no-op)

### Step 3: Extended Material System ✅ COMPLETED
- ✅ Implement lighting in each material type:
  - ✅ `BasicMaterial`: Standard Phong/Blinn-Phong lighting with spotlight support
  - ✅ `SkinnedMaterial`: Phong lighting with bone transformations and spotlight support
  - ✅ `LightmapMaterial`: Combine dynamic lights with existing lightmaps
  - ✅ `BillboardMaterial`: Skip light passes (appropriate for UI/particle effects)

**Implementation Details:**
- **Files Modified:**
  - `engine/src/scene/basic_material.rs` - Added lighting vertex/fragment shaders and draw_light_pass implementation
  - `engine/src/scene/skinned_material.rs` - Added lighting shaders with bone transformation support
  - `engine/src/materials/lightmap_material.rs` - Added lighting support that combines with existing lightmaps
  - `engine/src/scene/billboard_material.rs` - Added stub implementation (no lighting support by design)
  - `engine/src/scene/light.rs` - Enhanced Light trait with spotlight_params() method for shader uniforms
- **Features Delivered:**
  - **Spotlight-only Support**: All materials support spotlight lighting with proper cone attenuation
  - **Shader Compilation**: Each material initializes both base and lighting shader programs
  - **Uniform Management**: Proper OpenGL uniform setup for light parameters (position, color, direction, cone angles, range)
  - **Backwards Compatibility**: Existing draw_opaque/draw_transparent methods unchanged
  - **Performance Optimization**: Early discard in shaders for fragments outside light influence
  - **Bone Animation Support**: SkinnedMaterial lighting respects bone transformations
  - **Lightmap Integration**: LightmapMaterial adds dynamic lights on top of baked lighting

### Step 4: Normal Vector Support (CRITICAL FOR LIGHTING QUALITY)
**Status**: 🔴 **REQUIRED** - Current lighting uses hardcoded upward normals causing flat, unrealistic lighting

**Problem Identified**: All materials use `vec3(0.0, 1.0, 0.0)` placeholder normals, causing:
- Blocky, unrealistic lighting appearance
- No surface detail in lighting calculations
- Spotlight effects appear flat and wrong

**Root Cause**: Normal data exists in source files but is being discarded during loading:
- **Object files (.bin)**: Normal indices read but thrown away (`ss2_bin_obj_loader.rs:444`)
- **AI meshes**: Packed vertex normals ignored (`ss2_bin_ai_loader.rs:412`)
- **World geometry**: No normal processing in mission loading

**Implementation Required**:
1. **Vertex Structure Updates** (`engine/src/scene/vertex.rs`):
   - Add `VertexPositionTextureNormal` for basic models
   - Add `VertexPositionTextureLightmapAtlasNormal` for world geometry
   - Add `VertexPositionTextureSkinnedNormal` for character models

2. **Shader Updates** (all material files):
   - Add normal input: `layout (location = N) in vec3 inNormal;`
   - Transform normals to world space: `worldNormal = normalize(mat3(world) * inNormal);`
   - Replace hardcoded `vec3(0.0, 1.0, 0.0)` with actual vertex normals

3. **Data Loading Updates**:
   - **Object models**: Unignore normal indices and implement normal loading
   - **AI meshes**: Implement packed normal decoding (reference SystemShock2VR project)
   - **World geometry**: Add normal calculation from geometry if not in files

4. **Normal Visualization Debug Mode**:
   - Add debug shader that renders normals as RGB colors (`normal.xyz * 0.5 + 0.5`)
   - Include `debug_normals` experimental feature flag
   - Add debug material that replaces lighting calculations with normal visualization
   - Essential for validating normal data correctness before lighting implementation
   - Red=X, Green=Y, Blue=Z components of normal vectors
   - Should show smooth color gradients on curved surfaces, distinct colors on edges

**Files to Modify**:
- `engine/src/scene/vertex.rs` - New vertex structures with normals
- `engine/src/scene/basic_material.rs` - Normal support in basic material
- `engine/src/materials/lightmap_material.rs` - Normal support in lightmap material
- `engine/src/scene/skinned_material.rs` - Normal support in skinned material
- `dark/src/ss2_bin_obj_loader.rs` - Unignore and load normal data
- `dark/src/ss2_bin_ai_loader.rs` - Implement packed normal decoding

### Step 5: Improve Light-Object Culling (CRITICAL FOR WORLD GEOMETRY)
**Status**: 🔴 **MAJOR ISSUE** - Current `affects_position` fails for large world geometry

**Problem Identified**: Current culling in `gl_engine.rs:144` uses single point check:
```rust
let world_pos = scene_object.get_world_position(); // Just the transform position!
if light.affects_position(world_pos) {
    // Only lights objects whose pivot point is within light range
}
```

**Impact**:
- Large world geometry (walls, floors, rooms) gets no lighting
- Only small objects near their pivot points receive light
- Spotlights appear to "miss" most of the environment

**Solutions** (in order of complexity):

1. **Quick Fix - Bounding Sphere Check**:
   ```rust
   // Check if light affects object's bounding sphere
   let (center, radius) = scene_object.get_bounding_sphere();
   if light.affects_bounding_sphere(center, radius) {
   ```

2. **Better - Bounding Box Check**:
   ```rust
   // Check if light affects object's axis-aligned bounding box
   let (min_bounds, max_bounds) = scene_object.get_bounding_box();
   if light.affects_bounding_box(min_bounds, max_bounds) {
   ```

3. **Best - Use Existing Portal Culling**:
   - Leverage `LightSystem::cull_lights_for_bounds()` (already implemented!)
   - Use object bounding boxes with portal system integration
   - Most accurate for complex world geometry

**Implementation Required**:
- Add bounding volume methods to `SceneObject`
- Add bounding volume checks to `Light` trait
- Update culling logic in `gl_engine.rs` render loop
- Consider integration with existing portal visibility system

**Files to Modify**:
- `engine/src/scene/scene_object.rs` - Add bounding volume methods
- `engine/src/scene/light.rs` - Add bounding volume intersection methods
- `engine/src/gl_engine.rs` - Update culling logic from single point to bounding volume

### Step 6: Portal-Based Light Culling
- Integrate with existing `PortalVisibilityEngine`
- Implement light frustum culling against portal system
- Add light influence bounds checking for performance
- Prepare culling system for shadow map optimization

### Step 7: Cross-Platform Integration
- Ensure OpenGL ES compatibility for Oculus runtime
- Test light passes on both desktop and VR platforms
- Add flashlight attachment to player/hand position in VR
- Validate performance on Quest hardware

### Step 8: Documentation & Future Roadmap
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