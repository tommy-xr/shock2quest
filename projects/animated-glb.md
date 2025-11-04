# Animated GLB Support Project

## Overview

This project adds support for animated GLB files to the shock2quest engine, enabling the dark_viewer tool to load and display skeletal animations stored within GLB files. Unlike the current animation system which expects separate files for each animation clip, GLB files can contain multiple animations in a single file.

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

## Implementation Plan

### Phase 1: Create GLB Animation Data Structures

Create new data structures in `dark/src/motion/glb_animation.rs`:

```rust
pub struct GlbAnimation {
    pub name: String,
    pub channels: Vec<GlbAnimationChannel>,
    pub duration: f32,
}

pub struct GlbAnimationChannel {
    pub target_node_index: usize,
    pub target_property: GlbAnimationProperty,
    pub keyframes: Vec<GlbKeyframe>,
}

pub enum GlbAnimationProperty {
    Translation,
    Rotation,
    Scale,
}

pub struct GlbKeyframe {
    pub time: f32,
    pub value: GlbAnimationValue,
}

pub enum GlbAnimationValue {
    Translation(Vector3<f32>),
    Rotation(Quaternion<f32>),
    Scale(Vector3<f32>),
}
```

### Phase 2: GLB Animation Importer

Create `dark/src/importers/glb_animation_importer.rs`:

```rust
pub struct GlbAnimationCollection {
    pub animations: Vec<GlbAnimation>,
    pub skeleton: Option<Skeleton>,
}

pub static GLB_ANIMATION_IMPORTER: Lazy<AssetImporter<GlbAnimationCollection, Vec<AnimationClip>, ()>>;

fn load_glb_animations(
    name: String,
    reader: &mut Box<dyn ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> GlbAnimationCollection {
    // Parse GLB file (similar to model importer)
    // Extract all animations and skeleton
    // Port logic from functor's process_animations()
}

fn process_glb_animations(
    glb_data: GlbAnimationCollection,
    _asset_cache: &mut AssetCache,
    _config: &(),
) -> Vec<AnimationClip> {
    // Convert GLB animations to shock2quest AnimationClips
    // Key challenge: interpolate keyframes to create frame-by-frame matrices
}
```

### Phase 3: Keyframe-to-Matrix Conversion

The critical piece - converting GLB's keyframe-based animations to shock2quest's frame-based matrix arrays:

```rust
fn convert_glb_to_animation_clip(
    glb_animation: &GlbAnimation,
    skeleton: &Skeleton,
    target_fps: f32, // e.g., 30 FPS
) -> AnimationClip {
    let frame_count = (glb_animation.duration * target_fps).ceil() as u32;
    let time_per_frame = Duration::from_secs_f32(1.0 / target_fps);

    let mut joint_to_frame: HashMap<JointId, Vec<Matrix4<f32>>> = HashMap::new();

    // Group channels by target node (joint)
    let channels_by_node = group_channels_by_node(&glb_animation.channels);

    for (node_index, channels) in channels_by_node {
        let joint_id = node_index as JointId; // May need mapping
        let mut frame_transforms = Vec::new();

        for frame in 0..frame_count {
            let time = frame as f32 / target_fps;

            // Interpolate translation, rotation, scale at this time
            let translation = interpolate_translation(&channels, time);
            let rotation = interpolate_rotation(&channels, time);
            let scale = interpolate_scale(&channels, time);

            // Compose TRS matrix
            let transform = Matrix4::from_translation(translation)
                * Matrix4::from(rotation)
                * Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z);

            frame_transforms.push(transform);
        }

        joint_to_frame.insert(joint_id, frame_transforms);
    }

    AnimationClip {
        num_frames: frame_count,
        time_per_frame,
        duration: Duration::from_secs_f32(glb_animation.duration),
        joint_to_frame,
        // ... other fields
    }
}
```

### Phase 4: dark_viewer Integration

Extend dark_viewer to handle GLB animation syntax:

```bash
# New syntax:
dark_viewer shark.glb swimming
dark_viewer robot.glb walk,run,idle
```

Modify `tools/dark_viewer/src/main.rs` in `create_scene()`:

```rust
fn create_scene(
    filename: &str,
    animations: &[String],
    asset_cache: &mut AssetCache,
    data_resolver: fn(&str) -> String,
) -> Result<Box<dyn ToolScene>, Box<dyn std::error::Error>> {
    let lower = filename.to_ascii_lowercase();

    if lower.ends_with(".glb") {
        if animations.is_empty() {
            // Static GLB viewing (existing GlbViewerScene)
            let scene = GlbViewerScene::from_model(filename.to_string(), asset_cache)?;
            Ok(Box::new(scene))
        } else {
            // Animated GLB viewing (new GlbAnimatedViewerScene)
            let scene = GlbAnimatedViewerScene::from_glb_animations(
                filename.to_string(),
                animations.to_vec(),
                asset_cache,
            )?;
            Ok(Box::new(scene))
        }
    }
    // ... existing code
}
```

### Phase 5: GlbAnimatedViewerScene

Create `tools/dark_viewer/src/scenes/glb_animated_viewer.rs`:

```rust
pub struct GlbAnimatedViewerScene {
    model: Rc<dark::model::Model>,
    animation_player: AnimationPlayer,
    animation_controller: Option<AnimationController>,
}

impl GlbAnimatedViewerScene {
    pub fn from_glb_animations(
        glb_file_path: String,
        animation_names: Vec<String>,
        asset_cache: &mut AssetCache,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Load model from GLB_MODELS_IMPORTER
        let model = asset_cache.get(&GLB_MODELS_IMPORTER, &glb_file_path);

        // Load animations from GLB_ANIMATION_IMPORTER
        let all_clips = asset_cache.get(&GLB_ANIMATION_IMPORTER, &glb_file_path);

        // Filter clips by requested animation names
        let selected_clips = filter_clips_by_name(all_clips, animation_names)?;

        // Create animation controller and player
        let mut controller = AnimationController::new(selected_clips);
        let mut animation_player = AnimationPlayer::empty();

        if let Some(first_clip) = controller.take_next() {
            animation_player = AnimationPlayer::queue_animation(&animation_player, first_clip);
        }

        Ok(GlbAnimatedViewerScene {
            model,
            animation_player,
            animation_controller: Some(controller),
        })
    }
}
```

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
- **Challenge**: GLB supports multiple bone weights per vertex, current shader only uses `boneweight.x`
- **Solution**: For initial implementation, use highest weight bone only. Add proper multi-bone skinning in subsequent change
- **Implementation**: When processing vertices, find the bone with highest weight and assign it to `joint_indices.x` and `weights.x = 1.0`

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

## Implementation Order

1. **Create GLB animation data structures** - Define the intermediate representation
2. **Implement keyframe interpolation utilities** - Core animation conversion logic
3. **Create GLB_ANIMATION_IMPORTER** - Main importer with conversion logic
4. **Update GLB model importer for simplified skinning** - Use highest weight bone only
5. **Add GlbAnimatedViewerScene** - Viewer scene for animated GLBs
6. **Integrate with dark_viewer CLI** - Command line argument handling
7. **Test with animated GLB files** - Validation and debugging

### Skinning Implementation Notes

For the initial implementation, we'll simplify vertex skinning:

```rust
// In GLB model importer - process_primitive() function
// When creating VertexPositionTextureSkinned vertices:

// TODO: Implement proper multi-bone skinning in future PR
// For now, use only the highest weight bone per vertex
fn find_dominant_bone(joint_indices: [u16; 4], weights: [f32; 4]) -> (u16, f32) {
    let mut max_weight = 0.0;
    let mut dominant_joint = 0;

    for i in 0..4 {
        if weights[i] > max_weight {
            max_weight = weights[i];
            dominant_joint = joint_indices[i];
        }
    }

    (dominant_joint, max_weight)
}

// Then assign to vertex:
let (dominant_joint, _weight) = find_dominant_bone(joint_indices, weights);
VertexPositionTextureSkinned {
    position: transformed_pos,
    uv: tex,
    joint_indices: vec4(dominant_joint as f32, 0.0, 0.0, 0.0),
    weights: vec4(1.0, 0.0, 0.0, 0.0), // Full weight to dominant bone
}
```

## Success Criteria

- [ ] dark_viewer can load and display animated GLB files
- [ ] Multiple animations can be specified and cycled through
- [ ] Smooth animation playback with proper interpolation
- [ ] Integration with existing animation player system
- [ ] Support for common glTF animation features (translation, rotation, scale)
- [ ] Simplified skinning works with single dominant bone per vertex
- [ ] Clear TODO markers for future multi-bone skinning implementation

## References

- Functor implementation: `/Users/bryphe/functor/runtime/functor-runtime-common/src/asset/pipelines/model_pipeline.rs`
- Functor animation structures: `/Users/bryphe/functor/runtime/functor-runtime-common/src/animation.rs`
- glTF 2.0 Animation Specification: https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#animations
- Existing shock2quest animation system: `dark/src/motion/animation_clip.rs`

## Notes

This approach leverages the proven functor implementation while adapting it to shock2quest's existing animation infrastructure. The key innovation is the keyframe-to-matrix conversion that bridges the two animation paradigms.