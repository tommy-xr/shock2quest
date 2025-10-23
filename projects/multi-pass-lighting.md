# Multi-Pass Lighting Implementation Plan

## Overview
Implement Doom 3-style multi-pass lighting system starting with spotlight support for in-game flashlight functionality. The implementation will extend the current SceneObject architecture and work across both desktop and Oculus VR runtimes, with extensibility for future light types and shadow mapping.

> **2024-?? Update:** The engine now renders opaque geometry in a single pass using a per-object `LightingBatch`. Materials receive up to two spotlights directly through `Material::draw_opaque`, so the multi-pass sections below are retained for historical context and shadow-map planning.

## Current Architecture Analysis
- **SceneObject** structure: Contains material, geometry, transform, and skinning data
- **Material trait**: Has `draw_opaque()` (now accepts a `LightingBatch`) and `draw_transparent()` methods
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

### Step 2: Single-Pass Spotlight Batching ✅ COMPLETED
- ✅ Replace multi-pass loop in `gl_engine.rs` with per-object spotlight batching
- ✅ Introduce `LightingBatch` (two spotlights) and feed it into `SceneObject::draw_opaque`
- ✅ Maintain standard blend/depth state for opaque pass, reserve transparent phase for alpha objects
- ✅ Keep hooks in place for future shadow-map integration

**Implementation Details:**
- **Files Modified:**
  - `engine/src/gl_engine.rs` - Simplified render loop with single-pass opacity + lighting batch
  - `engine/src/scene/material.rs` - Replaced `draw_light_pass()` with a `LightingBatch` parameter on `draw_opaque`
  - `engine/src/scene/scene_object.rs` - Updated `draw_opaque()` wiring, removed multi-pass helper
- **Features Delivered:**
  - Three-pass rendering system: base opaque pass, per-light additive passes, transparent pass
  - Proper OpenGL state management: additive blending (GL_ONE, GL_ONE) for light accumulation
  - Read-only depth testing (GL_EQUAL) prevents overdraw during light passes
  - Light culling: only render objects affected by each light using `affects_position()`
  - Future-ready shadow map parameter in `draw_light_pass()` interface
  - Backwards compatibility: default `draw_light_pass()` implementation returns false (no-op)

### Step 3: Extended Material System ✅ COMPLETED
- ✅ Implement lighting in each material type:
  - ✅ `BasicMaterial`: Standard Phong/Blinn-Phong lighting with spotlight support (single shader)
  - ✅ `SkinnedMaterial`: Phong lighting with bone transformations and spotlight batching
  - ✅ `LightmapMaterial`: Combine dynamic lights with existing lightmaps and dynamic spotlights in one pass
  - ✅ `BillboardMaterial`: Remains unlit (ignores `LightingBatch`)

**Implementation Details:**
- **Files Modified:**
  - `engine/src/scene/basic_material.rs` - Unified shader handles base + dynamic lighting arrays
  - `engine/src/scene/skinned_material.rs` - Same as basic, with bone matrix support
  - `engine/src/materials/lightmap_material.rs` - Single shader blending baked lightmaps with batched spotlights
  - `engine/src/scene/billboard_material.rs` - Continues to skip lighting
  - `engine/src/scene/light.rs` - Enhanced `Light` trait with `influence_at` for batching heuristics
- **Features Delivered:**
  - **Spotlight-only Support**: All materials support spotlight lighting with proper cone attenuation
  - **Shader Compilation**: Each material initializes both base and lighting shader programs
  - **Uniform Management**: Proper OpenGL uniform setup for light parameters (position, color, direction, cone angles, range)
  - **Backwards Compatibility**: Existing draw_opaque/draw_transparent methods unchanged
  - **Performance Optimization**: Early discard in shaders for fragments outside light influence
  - **Bone Animation Support**: SkinnedMaterial lighting respects bone transformations
  - **Lightmap Integration**: LightmapMaterial adds dynamic lights on top of baked lighting

### Step 4: Normal Vector Support (CRITICAL FOR LIGHTING QUALITY) 🔄 **IN PROGRESS**
**Status**: 🔄 **PARTIAL IMPLEMENTATION** - Shaders updated for normal support, data loading in progress

**Problem Identified**: All materials use `vec3(0.0, 1.0, 0.0)` placeholder normals, causing:
- Blocky, unrealistic lighting appearance
- No surface detail in lighting calculations
- Spotlight effects appear flat and wrong

**Root Cause**: Normal data exists in source files but is being discarded during loading:
- **Object files (.bin)**: Normal indices read but thrown away (`ss2_bin_obj_loader.rs:444`)
- **AI meshes**: Packed vertex normals ignored (`ss2_bin_ai_loader.rs:412`)
- **World geometry**: No normal processing in mission loading

**Implementation Status**:

**Completed ✅**:
1. **Vertex Structure Updates** (`engine/src/scene/vertex.rs`):
   - ✅ Added `VertexPositionTextureNormal` for basic models with location 0,1,2 layout
   - ✅ Added `VertexPositionTextureLightmapAtlasNormal` for world geometry with location 0,1,2,3,4 layout
   - ✅ Added `VertexPositionTextureSkinnedNormal` for character models with location 0,1,2,3 layout
   - ✅ Full Vertex trait implementations with proper attribute layouts

**Completed ✅**:
2. **Debug Normal Visualization** (`engine/src/scene/debug_normal_material.rs`):
   - ✅ Debug material that renders normals as RGB colors (Red=X, Green=Y, Blue=Z)
   - ✅ Proper Material trait implementation with initialization and draw methods
   - ✅ Expects normals at location 2 for basic models
   - ✅ Can be used to validate normal data correctness: smooth gradients = good normals

**In Progress 🔄**:
3. **Shader Updates** (Updated to use real normals):
   - ✅ `BasicMaterial` lighting shaders now expect normals at location 2 and use actual vertex normals
   - ✅ `SkinnedMaterial` lighting shaders expect normals at location 3 and transform them through bone matrices
   - ✅ `LightmapMaterial` lighting shaders expect normals at location 4 and combine with dynamic lighting
   - 🔄 **COMPATIBILITY ISSUE**: Current vertex data lacks normals, causing runtime shader attribute binding failures

**Still Required 🔴**:
4. **Data Loading Updates** (Convert existing vertex data to include normals):
   - **Object models**: Unignore normal indices and implement normal loading
   - **AI meshes**: Implement packed normal decoding (reference SystemShock2VR project)
   - **World geometry**: Add normal calculation from geometry if not in files

**Immediate Next Steps**:

5. **Solve Compatibility Issue** (Choose one approach):
   - **Option A**: Create backward-compatible shaders that use computed normals when vertex normals unavailable
   - **Option B**: Update data loaders to generate normals from existing geometry (triangle face normals)
   - **Option C**: Implement full normal data loading from .bin/.mis files (larger scope)

6. **Object File Normal Loading** (`dark/src/ss2_bin_obj_loader.rs`):
   - ✅ **Started**: Normal indices no longer discarded, added to `SystemShock2ObjectPolygon` struct
   - 🔄 **In Progress**: Need to update vertex generation to use normal indices
   - 🔴 **Blocked**: Requires converting from non-normal vertex structures to normal-enabled ones

**Current Compatibility Issue**:
The materials now expect vertex attributes with normals, but existing data loaders create vertices without normals:
- `VertexPositionTexture` → should become `VertexPositionTextureNormal`
- `VertexPositionTextureSkinned` → should become `VertexPositionTextureSkinnedNormal`
- `VertexPositionTextureLightmapAtlas` → should become `VertexPositionTextureLightmapAtlasNormal`

**Debug Normal Visualization Ready**:
Once compatibility is resolved, `debug_normal_material::create()` can be used to validate normals.

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
