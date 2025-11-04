// glb_animation.rs
// Data structures for GLB/glTF animations, based on functor implementation
// These structures serve as an intermediate representation before conversion to shock2quest's AnimationClip format

use cgmath::{Quaternion, Vector3};

/// A complete animation from a GLB file
#[derive(Clone, Debug)]
pub struct GlbAnimation {
    pub name: String,
    pub channels: Vec<GlbAnimationChannel>,
    pub duration: f32,
}

/// An animation channel targets a specific property of a specific node
#[derive(Clone, Debug)]
pub struct GlbAnimationChannel {
    pub target_node_index: usize,
    pub target_property: GlbAnimationProperty,
    pub keyframes: Vec<GlbKeyframe>,
    // TODO: Add interpolation mode support (LINEAR, STEP, CUBICSPLINE)
    // pub interpolation: GlbInterpolationMode,
}

/// The property being animated on the target node
#[derive(Clone, Debug, PartialEq)]
pub enum GlbAnimationProperty {
    Translation,
    Rotation,
    Scale,
    // TODO: Add morph target weights support in future
    // Weights,
}

/// A keyframe containing a time and the animated value
#[derive(Clone, Debug)]
pub struct GlbKeyframe {
    pub time: f32,
    pub value: GlbAnimationValue,
}

/// The value stored in a keyframe, typed by the property being animated
#[derive(Clone, Debug)]
pub enum GlbAnimationValue {
    Translation(Vector3<f32>),
    Rotation(Quaternion<f32>),
    Scale(Vector3<f32>),
    // TODO: Add morph target weights support in future
    // Weights(Vec<f32>),
}

impl GlbAnimation {
    /// Create a new GLB animation with the given name
    pub fn new(name: String) -> Self {
        Self {
            name,
            channels: Vec::new(),
            duration: 0.0,
        }
    }

    /// Add a channel to this animation
    pub fn add_channel(&mut self, channel: GlbAnimationChannel) {
        // Update duration based on the latest keyframe in any channel
        if let Some(last_keyframe) = channel.keyframes.last() {
            self.duration = self.duration.max(last_keyframe.time);
        }
        self.channels.push(channel);
    }

    /// Get all channels targeting a specific node
    pub fn channels_for_node(&self, node_index: usize) -> Vec<&GlbAnimationChannel> {
        self.channels
            .iter()
            .filter(|channel| channel.target_node_index == node_index)
            .collect()
    }

    /// Get all unique node indices targeted by this animation
    pub fn get_animated_nodes(&self) -> Vec<usize> {
        let mut nodes: Vec<usize> = self
            .channels
            .iter()
            .map(|channel| channel.target_node_index)
            .collect();
        nodes.sort_unstable();
        nodes.dedup();
        nodes
    }
}

impl GlbAnimationChannel {
    /// Create a new animation channel
    pub fn new(target_node_index: usize, target_property: GlbAnimationProperty) -> Self {
        Self {
            target_node_index,
            target_property,
            keyframes: Vec::new(),
        }
    }

    /// Add a keyframe to this channel
    pub fn add_keyframe(&mut self, keyframe: GlbKeyframe) {
        self.keyframes.push(keyframe);
    }

    /// Get the value at a specific time using linear interpolation
    /// TODO: Support other interpolation modes (STEP, CUBICSPLINE)
    pub fn interpolate_at_time(&self, time: f32) -> Option<GlbAnimationValue> {
        if self.keyframes.is_empty() {
            return None;
        }

        // Find the keyframes to interpolate between
        let mut before_idx = None;
        let mut after_idx = None;

        for (i, keyframe) in self.keyframes.iter().enumerate() {
            if keyframe.time <= time {
                before_idx = Some(i);
            }
            if keyframe.time >= time && after_idx.is_none() {
                after_idx = Some(i);
                break;
            }
        }

        match (before_idx, after_idx) {
            // Exact match or single keyframe
            (Some(i), None) | (None, Some(i)) => Some(self.keyframes[i].value.clone()),
            (Some(before), Some(after)) if before == after => {
                Some(self.keyframes[before].value.clone())
            }
            // Interpolate between two keyframes
            (Some(before), Some(after)) => {
                let before_frame = &self.keyframes[before];
                let after_frame = &self.keyframes[after];

                let t = (time - before_frame.time) / (after_frame.time - before_frame.time);
                Some(interpolate_values(
                    &before_frame.value,
                    &after_frame.value,
                    t,
                ))
            }
            // No valid keyframes found
            _ => None,
        }
    }
}

/// Linear interpolation between two animation values
fn interpolate_values(a: &GlbAnimationValue, b: &GlbAnimationValue, t: f32) -> GlbAnimationValue {
    use GlbAnimationValue::*;

    match (a, b) {
        (Translation(a), Translation(b)) => Translation(a + (b - a) * t),
        (Rotation(a), Rotation(b)) => {
            // Use spherical linear interpolation for quaternions
            Rotation(a.slerp(*b, t))
        }
        (Scale(a), Scale(b)) => Scale(a + (b - a) * t),
        // Mismatched types - return the first value
        _ => a.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec3;

    #[test]
    fn test_glb_animation_creation() {
        let mut animation = GlbAnimation::new("test_walk".to_string());
        assert_eq!(animation.name, "test_walk");
        assert_eq!(animation.duration, 0.0);
        assert!(animation.channels.is_empty());

        let mut channel = GlbAnimationChannel::new(0, GlbAnimationProperty::Translation);
        channel.add_keyframe(GlbKeyframe {
            time: 1.0,
            value: GlbAnimationValue::Translation(vec3(1.0, 0.0, 0.0)),
        });

        animation.add_channel(channel);
        assert_eq!(animation.duration, 1.0);
        assert_eq!(animation.channels.len(), 1);
    }

    #[test]
    fn test_interpolation() {
        let mut channel = GlbAnimationChannel::new(0, GlbAnimationProperty::Translation);

        channel.add_keyframe(GlbKeyframe {
            time: 0.0,
            value: GlbAnimationValue::Translation(vec3(0.0, 0.0, 0.0)),
        });

        channel.add_keyframe(GlbKeyframe {
            time: 2.0,
            value: GlbAnimationValue::Translation(vec3(4.0, 0.0, 0.0)),
        });

        // Test interpolation at halfway point
        if let Some(GlbAnimationValue::Translation(pos)) = channel.interpolate_at_time(1.0) {
            assert_eq!(pos, vec3(2.0, 0.0, 0.0));
        } else {
            panic!("Expected translation value");
        }
    }

    #[test]
    fn test_animated_nodes() {
        let mut animation = GlbAnimation::new("test".to_string());

        let channel1 = GlbAnimationChannel::new(5, GlbAnimationProperty::Translation);
        let channel2 = GlbAnimationChannel::new(3, GlbAnimationProperty::Rotation);
        let channel3 = GlbAnimationChannel::new(5, GlbAnimationProperty::Scale);

        animation.add_channel(channel1);
        animation.add_channel(channel2);
        animation.add_channel(channel3);

        let animated_nodes = animation.get_animated_nodes();
        assert_eq!(animated_nodes, vec![3, 5]); // Should be sorted and deduplicated
    }
}
