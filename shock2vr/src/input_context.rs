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

    /// Conversion used by a tracked runtime; lets a stance change rebase all poses together.
    pub tracking: Option<crate::vr_tracking::TrackingTransform>,
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
            tracking: None,
        }
    }

    /// Preserve tracked poses and existing VR grip ownership while scripted
    /// gameplay controls are locked.
    ///
    /// A held object is owned by a continuous squeeze in VR. Replacing the
    /// complete input with `default()` therefore looks exactly like the player
    /// opened their hand and emits a world drop. Keep that ownership latch for
    /// hands which were already holding an object, but leave an empty hand's
    /// squeeze neutral so the lock cannot acquire anything new.
    pub(crate) fn with_player_controls_suppressed(
        &self,
        left_hand_holds_item: bool,
        right_hand_holds_item: bool,
    ) -> InputContext {
        let mut suppressed = self.clone();
        suppress_hand_controls(&mut suppressed.left_hand, left_hand_holds_item);
        suppress_hand_controls(&mut suppressed.right_hand, right_hand_holds_item);
        suppressed.pointer = None;
        suppressed.crouch = false;
        suppressed.jump = false;
        suppressed
    }
}

fn suppress_hand_controls(hand: &mut Hand, holds_item: bool) {
    hand.thumbstick = Vector2::zero();
    hand.trigger_value = 0.0;
    hand.squeeze_value = if holds_item { 1.0 } else { 0.0 };
    hand.a_value = 0.0;
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

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, Vector2, Vector3};

    use super::{InputContext, Pointer2D};

    #[test]
    fn scripted_control_lock_preserves_only_existing_grip_ownership_and_poses() {
        let mut input = InputContext::default();
        input.head.position = Vector3::new(1.0, 2.0, 3.0);
        input.head.rotation = Quaternion::new(0.5, 0.1, 0.2, 0.3);
        input.left_hand.position = Vector3::new(4.0, 5.0, 6.0);
        input.left_hand.rotation = Quaternion::new(0.6, 0.2, 0.3, 0.4);
        input.left_hand.thumbstick = Vector2::new(1.0, -1.0);
        input.left_hand.trigger_value = 1.0;
        input.left_hand.squeeze_value = 0.0;
        input.left_hand.a_value = 1.0;
        input.right_hand.position = Vector3::new(7.0, 8.0, 9.0);
        input.right_hand.rotation = Quaternion::new(0.7, 0.3, 0.4, 0.5);
        input.right_hand.thumbstick = Vector2::new(-1.0, 1.0);
        input.right_hand.trigger_value = 1.0;
        input.right_hand.squeeze_value = 1.0;
        input.right_hand.a_value = 1.0;
        input.pointer = Some(Pointer2D {
            position: Vector2::new(0.25, 0.75),
            pressed: true,
        });
        input.crouch = true;
        input.jump = true;

        let suppressed = input.with_player_controls_suppressed(false, true);

        assert_eq!(suppressed.head.position, input.head.position);
        assert_eq!(suppressed.head.rotation, input.head.rotation);
        assert_eq!(suppressed.left_hand.position, input.left_hand.position);
        assert_eq!(suppressed.left_hand.rotation, input.left_hand.rotation);
        assert_eq!(suppressed.right_hand.position, input.right_hand.position);
        assert_eq!(suppressed.right_hand.rotation, input.right_hand.rotation);
        assert_eq!(suppressed.left_hand.squeeze_value, 0.0);
        assert_eq!(suppressed.right_hand.squeeze_value, 1.0);
        assert_eq!(suppressed.left_hand.thumbstick, Vector2::new(0.0, 0.0));
        assert_eq!(suppressed.right_hand.thumbstick, Vector2::new(0.0, 0.0));
        assert_eq!(suppressed.left_hand.trigger_value, 0.0);
        assert_eq!(suppressed.right_hand.trigger_value, 0.0);
        assert_eq!(suppressed.left_hand.a_value, 0.0);
        assert_eq!(suppressed.right_hand.a_value, 0.0);
        assert!(suppressed.pointer.is_none());
        assert!(!suppressed.crouch);
        assert!(!suppressed.jump);
    }
}
