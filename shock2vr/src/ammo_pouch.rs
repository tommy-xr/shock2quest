//! The ammo pouch: the right hip hands out a clip for the gun you are holding.
//!
//! Like the belt card this is **presentation** - there is no pouch object and
//! nothing extra to save. What is drawn is the top clip of the reserve stack
//! the pouch would actually hand over, resolved from the inventory every frame,
//! so the hip is occupied exactly when a grip there would produce something.
//! An empty pouch simply is not drawn, and the glove's amber light is what says
//! the gun is held but the reserve has nothing for it.

use cgmath::Matrix4;

use crate::body_frame::BodyFrame;

/// The clip sitting in the pouch this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PouchClip {
    /// The clip archetype the withdrawal would hand over.
    pub template_id: i32,
    /// Its model, so the hip shows the ammo the player actually carries.
    pub model: String,
    /// Rounds in that clip.
    pub rounds: i32,
}

/// The clip riding the right hip, upright and facing the way the body does. Its
/// size comes from the same `vr_grips` profile the hand uses, so hip and hand
/// can never disagree about how big a clip is.
pub fn pouch_transform(frame: &BodyFrame, model: &str) -> Matrix4<f32> {
    Matrix4::from_translation(frame.pouch())
        * Matrix4::from(frame.rotation())
        * Matrix4::from_scale(crate::vr_config::held_geometry_scale(model))
}
