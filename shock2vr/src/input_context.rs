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

    // Flat-runtime crouch request (desktop key / debug-runtime channel). The
    // *actual* crouch state can lag this: standing up is refused while there
    // is no headroom. VR leaves this false - physically crouching moves the
    // HMD instead.
    pub crouch: bool,

    // Held jump/swim-up request. The physics layer edge-detects dry jumps;
    // underwater this remains continuous, matching Dark's swim control.
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
    pub rotation: Quaternion<f32>,
}

impl Head {
    pub fn default() -> Head {
        Head {
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
