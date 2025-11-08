use std::collections::HashMap;
use std::io::Cursor;
use std::time::Duration;

use cgmath::{Deg, Matrix4, Quaternion, SquareMatrix, Vector3, vec3};
use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::{
    motion::{
        AnimationClip, GlbAnimation, GlbAnimationChannel, GlbAnimationProperty, GlbAnimationValue,
        GlbKeyframe, JointId,
    },
    ss2_skeleton::{Bone, JointRestTransform, Skeleton},
};

/// Collection of GLB animations and skeleton data from a single GLB file
pub struct GlbAnimationCollection {
    pub animations: Vec<GlbAnimation>,
    pub skeleton: Option<Skeleton>,
}

/// Load GLB animations from file
fn load_glb_animations(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> GlbAnimationCollection {
    // Read the entire GLB file into memory
    let mut buffer = Vec::new();
    let _ = std::io::copy(reader, &mut buffer);

    // Parse the GLTF document
    let cursor = Cursor::new(buffer);
    let gltf = gltf::Gltf::from_slice(cursor.get_ref()).expect("Failed to parse GLB file");

    let document = gltf.document;
    let blob = gltf.blob;

    // Process buffers (similar to model importer)
    let mut buffers_data = Vec::new();
    for buffer in document.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => blob.as_ref().expect("No binary blob in GLB file").clone(),
            gltf::buffer::Source::Uri(uri) => {
                panic!("External buffer not supported: {}", uri);
            }
        };
        buffers_data.push(gltf::buffer::Data(data));
    }

    // Extract skeleton from first scene (if any)
    let skeleton = extract_skeleton_from_document(&document, &buffers_data);

    // Process animations from the GLB file
    let animations = extract_glb_animations(&document, &buffers_data);

    GlbAnimationCollection {
        animations,
        skeleton,
    }
}

/// Extract skeleton data from GLB document
pub fn extract_skeleton_from_document(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> Option<Skeleton> {
    // Look for the first skin in any scene
    for scene in document.scenes() {
        for node in scene.nodes() {
            if let Some(skeleton) = extract_skeleton_from_node(&node, buffers) {
                return Some(skeleton);
            }
        }
    }
    None
}

/// Recursively extract skeleton from a node
fn extract_skeleton_from_node(
    node: &gltf::Node,
    buffers: &[gltf::buffer::Data],
) -> Option<Skeleton> {
    // Check if this node has a skin
    if let Some(skin) = node.skin() {
        println!("Found skin in GLB, extracting skeleton...");

        let joint_nodes: Vec<gltf::Node> = skin.joints().collect();
        let inverse_bind_matrices = extract_inverse_bind_matrices(&skin, buffers);

        let mut node_to_joint = HashMap::new();
        for (joint_index, joint_node) in joint_nodes.iter().enumerate() {
            node_to_joint.insert(joint_node.index(), joint_index as JointId);
        }

        let mut parent_map: HashMap<JointId, JointId> = HashMap::new();
        for (joint_index, joint_node) in joint_nodes.iter().enumerate() {
            let parent_joint = joint_index as JointId;
            for child in joint_node.children() {
                if let Some(child_joint) = node_to_joint.get(&child.index()) {
                    parent_map.insert(*child_joint, parent_joint);
                }
            }
        }

        let mut bones = Vec::new();
        let mut rest_transforms = HashMap::new();

        for (joint_index, joint_node) in joint_nodes.iter().enumerate() {
            let joint_id = joint_index as JointId;
            let parent_id = parent_map.get(&joint_id).copied();

            let transform_array = joint_node.transform().matrix();
            let local_transform = Matrix4::from(transform_array);

            let (translation_arr, rotation_arr, scale_arr) = joint_node.transform().decomposed();
            let translation =
                Vector3::new(translation_arr[0], translation_arr[1], translation_arr[2]);
            let rotation = Quaternion::new(
                rotation_arr[3],
                rotation_arr[0],
                rotation_arr[1],
                rotation_arr[2],
            );
            let scale = Vector3::new(scale_arr[0], scale_arr[1], scale_arr[2]);

            let local_inverse = local_transform.invert().unwrap_or_else(Matrix4::identity);
            let bind_inverse = inverse_bind_matrices
                .get(joint_index)
                .copied()
                .unwrap_or_else(Matrix4::identity);

            rest_transforms.insert(
                joint_id,
                JointRestTransform {
                    translation,
                    rotation,
                    scale,
                    local_matrix: local_transform,
                    local_inverse,
                    inverse_bind: bind_inverse,
                },
            );

            bones.push(Bone {
                joint_id,
                parent_id,
                local_transform,
            });

            println!(
                "  Joint {}: {} (parent: {:?})",
                joint_id,
                joint_node.name().unwrap_or("unnamed"),
                parent_id
            );
        }

        return Some(Skeleton::create_from_bones_with_mapping(
            bones,
            node_to_joint,
            rest_transforms,
        ));
    }

    // Check child nodes
    for child in node.children() {
        if let Some(skeleton) = extract_skeleton_from_node(&child, buffers) {
            return Some(skeleton);
        }
    }

    None
}

fn extract_inverse_bind_matrices(
    skin: &gltf::Skin,
    buffers: &[gltf::buffer::Data],
) -> Vec<Matrix4<f32>> {
    let accessor = match skin.inverse_bind_matrices() {
        Some(accessor) => accessor,
        None => return Vec::new(),
    };

    let view = match accessor.view() {
        Some(view) => view,
        None => return Vec::new(),
    };

    let buffer = match buffers.get(view.buffer().index()) {
        Some(data) => data,
        None => return Vec::new(),
    };

    let stride = view.stride().unwrap_or_else(|| accessor.size());

    let start = view.offset() + accessor.offset();
    let count = accessor.count();

    let mut matrices = Vec::with_capacity(count);

    for i in 0..count {
        let base_offset = start + i * stride;
        let mut values = [0f32; 16];

        for j in 0..16 {
            let byte_index = base_offset + j * 4;
            if byte_index + 3 >= buffer.0.len() {
                values[j] = 0.0;
            } else {
                values[j] = f32::from_le_bytes([
                    buffer.0[byte_index],
                    buffer.0[byte_index + 1],
                    buffer.0[byte_index + 2],
                    buffer.0[byte_index + 3],
                ]);
            }
        }

        let matrix = Matrix4::new(
            values[0], values[1], values[2], values[3], values[4], values[5], values[6], values[7],
            values[8], values[9], values[10], values[11], values[12], values[13], values[14],
            values[15],
        );

        matrices.push(matrix);
    }

    matrices
}

/// Process all animations from the GLB document (ported from functor)
fn extract_glb_animations(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> Vec<GlbAnimation> {
    let mut animations = Vec::new();

    println!(
        "Processing {} GLB animations...",
        document.animations().count()
    );

    for animation in document.animations() {
        let animation_name = animation.name().unwrap_or("Unnamed Animation").to_owned();

        let mut glb_animation = GlbAnimation::new(animation_name.clone());

        println!("  Processing animation: {}", animation_name);

        for channel in animation.channels() {
            let target = channel.target();
            let node_index = target.node().index();

            let property = match target.property() {
                gltf::animation::Property::Translation => GlbAnimationProperty::Translation,
                gltf::animation::Property::Rotation => GlbAnimationProperty::Rotation,
                gltf::animation::Property::Scale => GlbAnimationProperty::Scale,
                gltf::animation::Property::MorphTargetWeights => {
                    println!("    WARN: Skipping morph target weights (not yet supported)");
                    continue;
                }
            };

            let reader = channel.reader(|buffer| Some(&buffers[buffer.index()]));

            let input_times: Vec<f32> = reader
                .read_inputs()
                .expect("Failed to read animation input times")
                .collect();

            let output_values = reader
                .read_outputs()
                .expect("Failed to read animation output values");

            let mut glb_channel = GlbAnimationChannel::new(node_index, property.clone());

            // Process keyframes based on output type
            match output_values {
                gltf::animation::util::ReadOutputs::Translations(translations) => {
                    for (i, translation) in translations.enumerate() {
                        let time = input_times[i];
                        glb_channel.add_keyframe(GlbKeyframe {
                            time,
                            value: GlbAnimationValue::Translation(vec3(
                                translation[0],
                                translation[1],
                                translation[2],
                            )),
                        });
                    }
                }
                gltf::animation::util::ReadOutputs::Rotations(rotations) => {
                    for (i, rotation) in rotations.into_f32().enumerate() {
                        let time = input_times[i];
                        glb_channel.add_keyframe(GlbKeyframe {
                            time,
                            // glTF quaternions are [x, y, z, w], cgmath uses w-first internally
                            value: GlbAnimationValue::Rotation(Quaternion {
                                v: vec3(rotation[0], rotation[1], rotation[2]),
                                s: rotation[3],
                            }),
                        });
                    }
                }
                gltf::animation::util::ReadOutputs::Scales(scales) => {
                    for (i, scale) in scales.enumerate() {
                        let time = input_times[i];
                        glb_channel.add_keyframe(GlbKeyframe {
                            time,
                            value: GlbAnimationValue::Scale(vec3(scale[0], scale[1], scale[2])),
                        });
                    }
                }
                gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => {
                    println!("    WARN: Morph target weights not yet supported");
                    continue;
                }
            }

            println!(
                "    Channel: node {} {:?} ({} keyframes)",
                node_index,
                property,
                glb_channel.keyframes.len()
            );

            glb_animation.add_channel(glb_channel);
        }

        animations.push(glb_animation);
    }

    animations
}

/// Convert GLB animations to shock2quest AnimationClips
fn process_glb_animations(
    glb_data: GlbAnimationCollection,
    _asset_cache: &mut AssetCache,
    _config: &(),
) -> Vec<AnimationClip> {
    let mut animation_clips = Vec::new();

    println!(
        "Converting {} GLB animations to shock2quest format...",
        glb_data.animations.len()
    );

    for glb_animation in glb_data.animations {
        match convert_glb_to_animation_clip(&glb_animation, &glb_data.skeleton) {
            Ok(clip) => {
                println!(
                    "  Converted: {} -> {} frames",
                    clip.name.as_deref().unwrap_or("Unnamed"),
                    clip.num_frames
                );
                animation_clips.push(clip);
            }
            Err(err) => {
                println!("  ERROR converting {}: {}", glb_animation.name, err);
            }
        }
    }

    animation_clips
}

/// Convert a single GLB animation to shock2quest AnimationClip format
fn convert_glb_to_animation_clip(
    glb_animation: &GlbAnimation,
    skeleton: &Option<Skeleton>, // Use for joint mapping
) -> Result<AnimationClip, String> {
    const TARGET_FPS: f32 = 30.0; // Convert to 30 FPS for shock2quest

    if glb_animation.duration <= 0.0 {
        return Err("Animation has zero or negative duration".to_string());
    }

    let frame_count = (glb_animation.duration * TARGET_FPS).ceil() as u32;
    let time_per_frame = Duration::from_secs_f32(1.0 / TARGET_FPS);
    let duration = Duration::from_secs_f32(glb_animation.duration);

    println!(
        "    Converting {} -> {} frames @ {}fps (duration: {:.2}s)",
        glb_animation.name, frame_count, TARGET_FPS, glb_animation.duration
    );

    // Group channels by target node
    let mut channels_by_node: HashMap<usize, Vec<&GlbAnimationChannel>> = HashMap::new();
    for channel in &glb_animation.channels {
        channels_by_node
            .entry(channel.target_node_index)
            .or_insert_with(Vec::new)
            .push(channel);
    }

    let mut joint_to_frame: HashMap<JointId, Vec<Matrix4<f32>>> = HashMap::new();

    // Process each animated node
    for (node_index, channels) in channels_by_node {
        let (joint_id, used_mapping) = match skeleton.as_ref() {
            Some(sk) => {
                if let Some(mapped_joint) = sk.joint_for_node(node_index) {
                    (mapped_joint, true)
                } else {
                    let fallback_joint = node_index as JointId;
                    println!(
                        "    WARN: No joint mapping for node {}. Using fallback joint {}",
                        node_index, fallback_joint
                    );
                    (fallback_joint, false)
                }
            }
            None => (node_index as JointId, false),
        };
        let mut frame_transforms = Vec::new();

        let rest_data = skeleton
            .as_ref()
            .and_then(|sk| sk.rest_transform(joint_id).cloned());

        let rest_translation = rest_data
            .as_ref()
            .map(|rest| rest.translation)
            .unwrap_or(Vector3::new(0.0, 0.0, 0.0));

        let rest_rotation = rest_data
            .as_ref()
            .map(|rest| rest.rotation)
            .unwrap_or(Quaternion::new(1.0, 0.0, 0.0, 0.0));

        let rest_scale = rest_data
            .as_ref()
            .map(|rest| rest.scale)
            .unwrap_or(Vector3::new(1.0, 1.0, 1.0));

        let rest_local_inverse = rest_data
            .as_ref()
            .map(|rest| rest.local_inverse)
            .unwrap_or_else(Matrix4::identity);

        if used_mapping {
            println!(
                "    Processing node {} -> joint {} ({} channels)",
                node_index,
                joint_id,
                channels.len()
            );
        } else {
            println!(
                "    Processing node {} -> joint {} ({} channels, fallback mapping)",
                node_index,
                joint_id,
                channels.len()
            );
        }

        // Generate transforms for each frame
        for frame in 0..frame_count {
            let time = frame as f32 / TARGET_FPS;

            // Get T, R, S at this time by interpolating channels
            let translation =
                interpolate_property_at_time(&channels, GlbAnimationProperty::Translation, time)
                    .unwrap_or(rest_translation);

            let rotation = interpolate_rotation_at_time(&channels, time).unwrap_or(rest_rotation);

            let scale = interpolate_property_at_time(&channels, GlbAnimationProperty::Scale, time)
                .unwrap_or(rest_scale);

            // Compose TRS matrix (T * R * S order as per glTF spec)
            let animated_matrix = Matrix4::from_translation(translation)
                * Matrix4::from(rotation)
                * Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z);

            if frame < 3 {
                println!(
                    "      Frame {}: raw translation ({:.3}, {:.3}, {:.3})",
                    frame, translation.x, translation.y, translation.z
                );
            }

            let transform = rest_local_inverse * animated_matrix;

            frame_transforms.push(transform);
        }

        joint_to_frame.insert(joint_id, frame_transforms);
    }

    Ok(AnimationClip {
        num_frames: frame_count,
        time_per_frame,
        duration,
        blend_length: Duration::from_millis(250), // Default blend
        end_rotation: Deg(0.0),                   // TODO: Calculate from animation data
        sliding_velocity: Vector3::new(0.0, 0.0, 0.0), // TODO: Calculate from root motion
        translation: Vector3::new(0.0, 0.0, 0.0), // TODO: Calculate from root motion
        joint_to_frame,
        motion_flags: Vec::new(), // TODO: Add motion flags if needed
        name: Some(glb_animation.name.clone()),
    })
}

/// Interpolate a specific property type at a given time from channels
fn interpolate_property_at_time(
    channels: &[&GlbAnimationChannel],
    property: GlbAnimationProperty,
    time: f32,
) -> Option<Vector3<f32>> {
    // Find the channel for this property
    let channel = channels.iter().find(|ch| ch.target_property == property)?;

    // Interpolate the value at the given time
    if let Some(value) = channel.interpolate_at_time(time) {
        match value {
            GlbAnimationValue::Translation(v) | GlbAnimationValue::Scale(v) => Some(v),
            _ => None,
        }
    } else {
        None
    }
}

/// Interpolate rotation at a given time from channels
fn interpolate_rotation_at_time(
    channels: &[&GlbAnimationChannel],
    time: f32,
) -> Option<Quaternion<f32>> {
    // Find the rotation channel
    let channel = channels
        .iter()
        .find(|ch| ch.target_property == GlbAnimationProperty::Rotation)?;

    // Interpolate the value at the given time
    if let Some(GlbAnimationValue::Rotation(quat)) = channel.interpolate_at_time(time) {
        Some(quat)
    } else {
        None
    }
}

pub static GLB_ANIMATION_IMPORTER: Lazy<
    AssetImporter<GlbAnimationCollection, Vec<AnimationClip>, ()>,
> = Lazy::new(|| AssetImporter::define(load_glb_animations, process_glb_animations));
