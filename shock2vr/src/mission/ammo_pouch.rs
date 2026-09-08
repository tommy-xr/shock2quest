//! A shared belt pouch serves the selected ammo of the opposite hand's gun.
use super::{body_inventory::BodyPose, reload::PouchClip};
use crate::{input_context::InputContext, vr_support::GripPose};
use cgmath::{InnerSpace, Matrix4, Quaternion, Rotation, Vector3};
use shipyard::EntityId;

pub(super) const RADIUS: f32 = 0.10 / crate::METERS_PER_WORLD_UNIT;
const BELOW: f32 = 0.55;
const FORWARD: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Withdraw { weapon: EntityId },
    Return { entity: EntityId },
    Empty,
}

pub(super) struct AmmoPouch {
    pub enabled: bool,
    pub center: Option<Vector3<f32>>,
    pub near: [bool; 2],
    pub blocks_grab: [bool; 2],
    pub offers: [Option<PouchClip>; 2],
    pub refused: [bool; 2],
    consumed: [bool; 2],
    pressed: [bool; 2],
    pressed_item: [Option<EntityId>; 2],
}

impl Default for AmmoPouch {
    fn default() -> Self {
        Self {
            enabled: false,
            center: None,
            near: [false; 2],
            blocks_grab: [false; 2],
            offers: [None; 2],
            refused: [false; 2],
            consumed: [false; 2],
            pressed: [true; 2],
            pressed_item: [None; 2],
        }
    }
}

impl AmmoPouch {
    pub fn update(
        &mut self,
        input: &InputContext,
        body: Option<BodyPose>,
        held: [Option<EntityId>; 2],
        available: [bool; 2],
        weapons: [Option<EntityId>; 2],
        ammo: [bool; 2],
        offers: [Option<PouchClip>; 2],
        enabled: bool,
    ) -> [Option<Action>; 2] {
        self.enabled = enabled && body.is_some();
        self.center = body.map(|body| body.front(BELOW, FORWARD));
        self.offers = offers;
        self.near = [false; 2];
        self.blocks_grab = [false; 2];
        let active_center = self.center.filter(|_| enabled);
        let mut actions = [None; 2];
        for (i, hand) in [&input.left_hand, &input.right_hand]
            .into_iter()
            .enumerate()
        {
            let tracked = GripPose {
                position: hand.position,
                rotation: hand.rotation,
            }
            .is_tracked()
                && hand.squeeze_value.is_finite()
                && input.pose_tracking.is_none_or(|p| p.hands[i]);
            let pressed = hand.squeeze_value >= 0.5;
            self.near[i] = tracked
                && active_center
                    .is_some_and(|center| (hand.position - center).magnitude2() <= RADIUS * RADIUS);
            if tracked && !pressed {
                self.consumed[i] = false;
            }
            let can_draw =
                self.near[i] && held[i].is_none() && available[i] && weapons[i].is_some();
            self.blocks_grab[i] = held[i].is_none() && (can_draw || self.consumed[i]);
            if self.near[i] {
                if ammo[i] && !pressed && self.pressed_item[i] == held[i] {
                    if let Some(entity) = held[i] {
                        actions[i] = Some(Action::Return { entity });
                    }
                } else if can_draw && pressed && !self.pressed[i] {
                    self.consumed[i] = true;
                    actions[i] = Some(match offers[i] {
                        Some(_) => Action::Withdraw {
                            weapon: weapons[i].unwrap(),
                        },
                        None => Action::Empty,
                    });
                }
            }
            self.pressed[i] = active_center.is_none() || !tracked || pressed;
            self.pressed_item[i] = if active_center.is_some() && tracked && pressed {
                held[i]
            } else {
                None
            };
        }
        actions
    }

    pub fn world_center(
        &self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> Option<Vector3<f32>> {
        self.center
            .map(|center| position + rotation.rotate_vector(center))
    }

    pub fn diagnostics(
        &self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> serde_json::Value {
        serde_json::json!({
            "center": self.world_center(position, rotation).map(|p| [p.x,p.y,p.z]),
            "radius": RADIUS, "near": self.near, "refused": self.refused,
            "offers": self.offers.map(|o| o.map(|o| serde_json::json!({
                "reserve": o.reserve.inner() as i32, "template": o.template, "rounds": o.rounds, "stock": o.stock
            })))
        })
    }

    pub fn render_zone(
        &self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> Vec<engine::scene::SceneObject> {
        use engine::scene::{SceneObject, color_material, lines_mesh};
        let Some(center) = self.world_center(position, rotation) else {
            return vec![];
        };
        let color = if self.refused.iter().any(|v| *v) {
            cgmath::vec3(1.0, 0.2, 0.1)
        } else if (0..2).any(|i| self.near[i] && self.offers[i].is_some()) {
            cgmath::vec3(0.1, 1.0, 0.3)
        } else if self.blocks_grab.iter().any(|v| *v) {
            cgmath::vec3(1.0, 0.65, 0.1)
        } else {
            cgmath::vec3(0.1, 0.55, 0.7)
        };
        let mut points = Vec::new();
        dark::hit_box::append_capsule_lines(
            &mut points,
            &Matrix4::from_scale(1.0),
            center,
            center,
            RADIUS,
        );
        let mut objects = vec![SceneObject::new(
            color_material::create(color),
            Box::new(lines_mesh::create(points)),
        )];
        crate::util::tag_render_source(&mut objects, crate::util::render_source::PLAYER_HANDS);
        objects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shipyard::World;

    #[test]
    fn draw_requires_fresh_squeeze_tracking_free_hand_and_other_gun() {
        let mut world = World::new();
        let gun = world.add_entity(());
        let reserve = world.add_entity(());
        let offer = PouchClip {
            reserve,
            template: -31,
            rounds: 12,
            stock: 24,
        };
        let mut input = InputContext::default();
        let body = BodyPose {
            head: input.head.position,
            yaw: 0.0,
        };
        input.left_hand.position = body.front(BELOW, FORWARD);
        let mut pouch = AmmoPouch::default();
        let update = |pouch: &mut AmmoPouch, input: &InputContext, available: bool, enabled| {
            pouch.update(
                input,
                Some(body),
                [None, Some(gun)],
                [available, false],
                [Some(gun), None],
                [false; 2],
                [Some(offer), None],
                enabled,
            )[0]
        };
        assert_eq!(update(&mut pouch, &input, true, true), None);
        input.left_hand.squeeze_value = 1.0;
        assert_eq!(
            update(&mut pouch, &input, true, true),
            Some(Action::Withdraw { weapon: gun })
        );
        assert_eq!(update(&mut pouch, &input, true, true), None);
        update(&mut pouch, &input, true, false);
        assert_eq!(update(&mut pouch, &input, true, true), None);
        input.left_hand.squeeze_value = 0.0;
        update(&mut pouch, &input, true, true);
        input.left_hand.squeeze_value = 1.0;
        assert_eq!(update(&mut pouch, &input, false, true), None);
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [false, true],
        });
        assert_eq!(update(&mut pouch, &input, true, true), None);
        input.pose_tracking = None;
        assert_eq!(update(&mut pouch, &input, true, true), None);
    }

    #[test]
    fn threshold_release_and_empty_squeeze_do_not_leak() {
        let mut world = World::new();
        let clip = world.add_entity(());
        let mut input = InputContext::default();
        let body = BodyPose {
            head: input.head.position,
            yaw: 0.0,
        };
        input.left_hand.position = body.front(BELOW, FORWARD);
        let mut pouch = AmmoPouch::default();
        for squeeze in [1.0, 0.5] {
            input.left_hand.squeeze_value = squeeze;
            assert_eq!(
                pouch.update(
                    &input,
                    Some(body),
                    [Some(clip), None],
                    [false; 2],
                    [None; 2],
                    [true, false],
                    [None; 2],
                    true
                )[0],
                None
            );
        }
        input.left_hand.squeeze_value = 0.4;
        assert_eq!(
            pouch.update(
                &input,
                Some(body),
                [Some(clip), None],
                [false; 2],
                [None; 2],
                [true, false],
                [None; 2],
                true
            )[0],
            Some(Action::Return { entity: clip })
        );
        input.left_hand.squeeze_value = 1.0;
        assert_eq!(
            pouch.update(
                &input,
                Some(body),
                [None; 2],
                [true; 2],
                [Some(clip), None],
                [false; 2],
                [None; 2],
                true
            )[0],
            Some(Action::Empty)
        );
        input.left_hand.position.x += 5.0;
        pouch.update(
            &input,
            Some(body),
            [None; 2],
            [true; 2],
            [Some(clip), None],
            [false; 2],
            [None; 2],
            true,
        );
        assert!(
            pouch.blocks_grab[0],
            "consumed squeeze stays blocked outside pouch"
        );
        pouch.update(
            &input,
            None,
            [None; 2],
            [true; 2],
            [Some(clip), None],
            [false; 2],
            [None; 2],
            false,
        );
        assert!(
            pouch.blocks_grab[0],
            "disabled mode must keep consumed input masked"
        );
        input.left_hand.squeeze_value = 0.0;
        pouch.update(
            &input,
            Some(body),
            [None; 2],
            [true; 2],
            [Some(clip), None],
            [false; 2],
            [None; 2],
            true,
        );
        assert!(!pouch.blocks_grab[0]);
    }

    #[test]
    fn pouch_is_separate_from_thighs_across_all_calibration_extremes() {
        let body = BodyPose {
            head: cgmath::vec3(0.0, 2.0, 0.0),
            yaw: 0.0,
        };
        let center = body.front(BELOW, FORWARD);
        for drop in [0.55, 1.10] {
            for side in [-0.40, -0.16, 0.16, 0.40] {
                let thigh = body.front(drop, 0.04)
                    + cgmath::vec3(side / crate::METERS_PER_WORLD_UNIT, 0.0, 0.0);
                assert!((thigh - center).magnitude() > RADIUS + super::super::holsters::RADIUS);
            }
        }
        assert!(
            (BELOW - 0.20) / crate::METERS_PER_WORLD_UNIT
                > RADIUS + super::super::shoulder_backpack::RADIUS
        );
    }
}
