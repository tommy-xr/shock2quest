//! `<hand>_hand.world_target`: hold a VR hand on a world point while the body
//! moves. Re-resolved into the hand's pawn-space controller pose before every
//! update, so a climbing or turning pawn does not drag the hand off the point.
//! It solves against the pose the pawn ENDED the last frame at, so while the
//! pawn moves the hand trails the target by that frame's motion; at rest it is
//! exact. The debug runtime never marks a hand untracked, so the glove
//! calibration the solve inverts always applies.

use cgmath::{Vector3, vec3};
use serde_json::Value;
use shock2vr::{
    Game, Handedness, dev_params,
    input::hand_target::{hand_world_point, resolve_world_target},
    input_context::{Hand, InputContext},
};

/// World targets for (left, right), the scene they were set in, and whether
/// hands are tracked at all.
pub struct HandTargets {
    vr: bool,
    targets: [Option<Vector3<f32>>; 2],
    scene: String,
}

const SIDES: [Handedness; 2] = [Handedness::Left, Handedness::Right];

fn side_of(channel: &str, field: &str) -> Option<usize> {
    let (side, rest) = channel.split_once("_hand.")?;
    (rest == field).then_some(())?;
    match side {
        "left" => Some(0),
        "right" => Some(1),
        _ => None,
    }
}

fn hand(input: &InputContext, index: usize) -> &Hand {
    if index == 0 {
        &input.left_hand
    } else {
        &input.right_hand
    }
}

fn hand_mut(input: &mut InputContext, index: usize) -> &mut Hand {
    if index == 0 {
        &mut input.left_hand
    } else {
        &mut input.right_hand
    }
}

fn resolve(
    game: &Game,
    input: &InputContext,
    index: usize,
    target: Vector3<f32>,
) -> Result<Vector3<f32>, String> {
    let (pawn_position, pawn_rotation) = game
        .player_pose()
        .ok_or_else(|| "no player in this scene".to_owned())?;
    resolve_world_target(
        SIDES[index],
        target,
        hand(input, index),
        input.head.position,
        input.head.rotation,
        game.player_eye_cap_above_center(),
        pawn_position,
        pawn_rotation,
        dev_params::get(dev_params::GLOVE_FORWARD_CM),
    )
}

impl HandTargets {
    pub fn new(vr: bool) -> Self {
        Self {
            vr,
            targets: [None; 2],
            scene: String::new(),
        }
    }

    /// Apply a `world_target` patch (`[x,y,z]`, or `null` to clear), or `None`
    /// for any other channel. An out-of-reach target is rejected and the hand
    /// is left where it was.
    pub fn patch(
        &mut self,
        game: &Game,
        input: &mut InputContext,
        channel: &str,
        value: &Value,
    ) -> Option<Result<(), String>> {
        let index = side_of(channel, "world_target")?;
        if value.is_null() {
            self.targets[index] = None;
            return Some(Ok(()));
        }
        Some((|| {
            if !self.vr {
                return Err(format!("channel '{channel}' needs --vr"));
            }
            let target = value
                .as_array()
                .filter(|a| a.len() == 3)
                .and_then(|a| {
                    a.iter()
                        .map(|v| v.as_f64().map(|f| f as f32))
                        .collect::<Option<Vec<_>>>()
                })
                .filter(|p| p.iter().all(|f| f.is_finite()))
                .map(|p| vec3(p[0], p[1], p[2]))
                .ok_or_else(|| {
                    format!("channel '{channel}' expects [x,y,z] world units or null, got {value}")
                })?;
            let local = resolve(game, input, index, target)
                .map_err(|reason| format!("channel '{channel}' rejected: {reason}"))?;
            hand_mut(input, index).position = local;
            self.targets[index] = Some(target);
            self.scene = game.scene_name().to_owned();
            Ok(())
        })())
    }

    /// Drop every target (a replay takes the hands over).
    pub fn clear(&mut self) {
        self.targets = [None; 2];
    }

    /// A `<hand>_hand.position` patch takes the hand back from its target.
    pub fn release_on_position(&mut self, channel: &str) {
        if let Some(index) = side_of(channel, "position") {
            self.targets[index] = None;
        }
    }

    /// Re-solve every target against the current pawn pose. While a target is
    /// out of reach (the body moved away) the hand stays where it was, and it
    /// follows again once back in reach. A level change drops the targets:
    /// their coordinates belong to the old level.
    pub fn apply(&mut self, game: &Game, input: &mut InputContext) {
        if game.scene_name() != self.scene {
            self.clear();
        }
        for (index, target) in self.targets.iter().enumerate() {
            if let Some(Ok(local)) = target.map(|target| resolve(game, input, index, target)) {
                hand_mut(input, index).position = local;
            }
        }
    }

    /// `(world_target, world_position)` of each hand, for GET /v1/control/input.
    pub fn report(
        &self,
        game: &Game,
        input: &InputContext,
    ) -> [(Option<[f32; 3]>, Option<[f32; 3]>); 2] {
        let pose = game.player_pose().filter(|_| self.vr);
        let world = |hand: &Hand| {
            pose.map(|(position, rotation)| {
                let p = hand_world_point(
                    hand,
                    position,
                    rotation,
                    dev_params::get(dev_params::GLOVE_FORWARD_CM),
                );
                [p.x, p.y, p.z]
            })
        };
        let target = |index: usize| self.targets[index].map(|t| [t.x, t.y, t.z]);
        [
            (target(0), world(&input.left_hand)),
            (target(1), world(&input.right_hand)),
        ]
    }
}
