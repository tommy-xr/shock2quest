// Input context is an abstraction layer over the motion controllers that the runtime will provide.
// Because this project is VR focused, this abstraction is geared towards VR.
// For Oculus / VR, this is a fairly direct mapping from the standard motion controllers.
// For desktop / PC runtime, the mapping is a bit more interesting..

use cgmath::{Quaternion, Vector2, Vector3, Zero};

#[derive(Debug, Clone)]
pub struct InputContext {
    // Information about the head position
    pub head: Head,

    // Information about each of the hands
    pub left_hand: Hand,
    pub right_hand: Hand,

    // 2D screen pointer (mouse) for flatscreen UI. `None` in VR, where
    // interaction is hand/pointer-ray based. Populated by flat runtimes when a
    // scene wants a cursor (e.g. menus).
    pub pointer: Option<Pointer2D>,

    // Runtime crouch request (flat key/debug channel, or physical VR head
    // height). The *actual* crouch state can lag this: standing up is refused
    // while there is no headroom.
    pub crouch: bool,

    // Ordinary locomotion jump request. Runtimes provide this as a held
    // button; the physics controller edge-detects it so holding the button
    // cannot repeatedly add upward velocity.
    pub jump: bool,
}

impl InputContext {
    pub fn default() -> InputContext {
        InputContext {
            head: Head::default(),

            left_hand: Hand::default(),
            right_hand: Hand::default(),
            pointer: None,
            crouch: false,
            jump: false,
        }
    }

    /// A copy with both hands raised by `offset_meters`, the head untouched.
    ///
    /// Hand positions are in pawn space, whose +Y is world up (the pawn's own
    /// rotation is yaw-only), so a comfort offset is a plain addition on `y`
    /// once converted out of meters - no rotation involved, and it therefore
    /// cannot depend on which way the player is facing or how the hand is
    /// turned.
    ///
    /// Pure and total so the arm-height knob is testable without a runtime;
    /// [`crate::dev_params::ARM_HEIGHT_OFFSET`] supplies the live value.
    pub fn with_arm_height_offset(&self, offset_meters: f32) -> InputContext {
        let mut adjusted = self.clone();
        if offset_meters == 0.0 {
            return adjusted;
        }
        let offset = offset_meters / crate::METERS_PER_WORLD_UNIT;
        adjusted.left_hand.position.y += offset;
        adjusted.right_hand.position.y += offset;
        adjusted
    }
}

#[cfg(test)]
mod arm_height_tests {
    use super::*;

    fn sample() -> InputContext {
        let mut input = InputContext::default();
        input.left_hand.position = Vector3::new(-0.3, 1.0, -0.5);
        input.right_hand.position = Vector3::new(0.3, 1.0, -0.5);
        input.head.position = Vector3::new(0.0, 1.4, 0.0);
        input
    }

    /// The knob is in meters of real space but hand positions are world units,
    /// so the conversion - not the raw number - is what must land. A version
    /// that forgot to divide would move the hands by 0.762x too much.
    #[test]
    fn the_offset_is_converted_from_meters_into_world_units() {
        let raised = sample().with_arm_height_offset(0.1);
        let expected = 1.0 + 0.1 / crate::METERS_PER_WORLD_UNIT;
        assert!((raised.right_hand.position.y - expected).abs() < 1e-5);
        assert!((raised.left_hand.position.y - expected).abs() < 1e-5);
    }

    /// The whole point of an ARM-height knob (as opposed to #1028's stage-wide
    /// eye offset) is that the head stays put - otherwise it would just be a
    /// second, redundant eye offset.
    #[test]
    fn the_head_does_not_move_with_the_arms() {
        let raised = sample().with_arm_height_offset(0.25);
        assert_eq!(raised.head.position, sample().head.position);
    }

    /// Only height changes: an offset that leaked into the horizontal axes
    /// would push the hands away from the body as well as up.
    #[test]
    fn only_the_vertical_axis_moves() {
        let raised = sample().with_arm_height_offset(-0.2);
        assert_eq!(raised.right_hand.position.x, 0.3);
        assert_eq!(raised.right_hand.position.z, -0.5);
        assert!(
            raised.right_hand.position.y < 1.0,
            "a negative offset lowers"
        );
    }

    /// The default must be bit-exactly inert, or every player who never opens
    /// the Developer screen pays for the knob existing.
    #[test]
    fn the_default_offset_changes_nothing() {
        let raised = sample().with_arm_height_offset(0.0);
        assert_eq!(raised.right_hand.position, sample().right_hand.position);
        assert_eq!(raised.left_hand.position, sample().left_hand.position);
    }
}

/// A 2D screen-space pointer for flatscreen UI. Position is normalized to
/// `[0, 1]` on each axis with the origin at the top-left, so it is
/// resolution-independent; scenes scale it to their own canvas.
#[derive(Debug, Clone, Copy)]
pub struct Pointer2D {
    pub position: Vector2<f32>,
    pub pressed: bool,
}

#[derive(Debug, Clone)]
pub struct Head {
    /// Where the head is, in pawn space (the same space the hands use), so a
    /// world-anchored panel can be placed from the player's actual eye rather
    /// than from a fixed height on the pawn origin.
    pub position: Vector3<f32>,
    pub rotation: Quaternion<f32>,
}

/// The seated/standing eye height every runtime falls back to when no tracked
/// head position is available - the fixed offset the render camera has always
/// applied on top of the pawn origin.
pub const DEFAULT_HEAD_HEIGHT: f32 = crate::PLAYER_EYE_HEIGHT / dark::SCALE_FACTOR;

impl Head {
    pub fn default() -> Head {
        Head {
            position: Vector3::new(0.0, DEFAULT_HEAD_HEIGHT, 0.0),
            rotation: Quaternion {
                v: Vector3::zero(),
                s: 1.0,
            },
        }
    }
}

// Context for an individual hand (motion controller)
#[derive(Debug, Clone)]
pub struct Hand {
    pub position: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub thumbstick: Vector2<f32>,
    pub trigger_value: f32,
    pub squeeze_value: f32,
    pub a_value: f32,
}

impl Hand {
    pub fn default() -> Hand {
        Hand {
            position: Vector3::zero(),
            rotation: Quaternion {
                v: Vector3::zero(),
                s: 1.0,
            },
            thumbstick: Vector2::zero(),
            trigger_value: 0.0,
            squeeze_value: 0.0,
            a_value: 0.0,
        }
    }
}
