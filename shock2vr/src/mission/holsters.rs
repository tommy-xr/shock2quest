//! Thigh slots own distinct weapon entities, independently of backpack capacity.
use super::body_inventory::{RetainedRelease, followed_yaw};
use crate::{
    input_context::InputContext, runtime_props::RuntimePropHolstered, vr_support::GripPose,
};
use cgmath::{InnerSpace, Matrix4, Quaternion, Rotation, Vector3, vec3};
use shipyard::{EntityId, IntoIter, IntoWithId, UniqueView, View, World};

const SCALE: f32 = crate::METERS_PER_WORLD_UNIT;
pub(super) const RADIUS: f32 = 0.14 / SCALE;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Store { entity: EntityId, slot: usize },
    Retrieve { entity: EntityId, slot: usize },
    Refuse { entity: EntityId },
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Holsters, InputContext, [EntityId; 2]) {
        let mut world = World::new();
        let ids = [world.add_entity(()), world.add_entity(())];
        let mut h = Holsters::default();
        let input = InputContext::default();
        h.update(
            &input, [None; 2], [true; 2], [None; 2], 1, [false; 2], true, 0.016,
        );
        (h, input, ids)
    }

    #[test]
    fn pack_rat_stat_unlocks_a_second_slot() {
        let world = World::new();
        world.add_unique(crate::quest_info::QuestInfo::new());
        assert_eq!(slot_count(&world), 1);
        world
            .borrow::<shipyard::UniqueViewMut<crate::quest_info::QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .add_os_trait(crate::scripts::gui::TRAIT_PACK_RAT);
        assert_eq!(slot_count(&world), 2);
    }

    #[test]
    fn stow_is_a_release_and_draw_is_a_fresh_squeeze_for_either_hand() {
        for hand in 0..2 {
            let (mut h, mut input, ids) = setup();
            let mut held = [None; 2];
            held[hand] = Some(ids[0]);
            let target = h.centers.unwrap()[0];
            let controller = if hand == 0 {
                &mut input.left_hand
            } else {
                &mut input.right_hand
            };
            controller.position = target;
            controller.squeeze_value = 1.0;
            assert_eq!(
                h.update(
                    &input, held, [false; 2], [None; 2], 1, [true; 2], true, 0.016
                ),
                [None; 2]
            );
            let controller = if hand == 0 {
                &mut input.left_hand
            } else {
                &mut input.right_hand
            };
            controller.squeeze_value = 0.0;
            assert_eq!(
                h.update(
                    &input, held, [false; 2], [None; 2], 1, [true; 2], true, 0.016
                )[hand],
                Some(Action::Store {
                    entity: ids[0],
                    slot: 0
                })
            );
            let controller = if hand == 0 {
                &mut input.left_hand
            } else {
                &mut input.right_hand
            };
            controller.squeeze_value = 1.0;
            assert_eq!(
                h.update(
                    &input,
                    [None; 2],
                    [true; 2],
                    [Some(ids[0]), None],
                    1,
                    [false; 2],
                    true,
                    0.016
                )[hand],
                Some(Action::Retrieve {
                    entity: ids[0],
                    slot: 0
                })
            );
            assert_eq!(
                h.update(
                    &input,
                    [None; 2],
                    [true; 2],
                    [Some(ids[0]), None],
                    1,
                    [false; 2],
                    true,
                    0.016
                ),
                [None; 2]
            );
        }
    }

    #[test]
    fn simultaneous_hands_cannot_store_or_draw_the_same_slot_twice() {
        let (mut h, mut input, ids) = setup();
        for hand in [&mut input.left_hand, &mut input.right_hand] {
            hand.position = h.centers.unwrap()[0];
            hand.squeeze_value = 1.0;
        }
        let held = ids.map(Some);
        h.update(
            &input, held, [false; 2], [None; 2], 2, [true; 2], true, 0.016,
        );
        input.left_hand.squeeze_value = 0.0;
        input.right_hand.squeeze_value = 0.0;
        let actions = h.update(
            &input, held, [false; 2], [None; 2], 2, [true; 2], true, 0.016,
        );
        assert_eq!(
            actions,
            [
                Some(Action::Store {
                    entity: ids[0],
                    slot: 0
                }),
                Some(Action::Refuse { entity: ids[1] })
            ]
        );
        assert!(h.retained.keep_grip(1));
        input.left_hand.squeeze_value = 1.0;
        input.right_hand.squeeze_value = 1.0;
        let actions = h.update(
            &input,
            [None; 2],
            [true; 2],
            [Some(ids[0]), None],
            2,
            [false; 2],
            true,
            0.016,
        );
        assert_eq!(
            actions,
            [
                Some(Action::Retrieve {
                    entity: ids[0],
                    slot: 0
                }),
                None
            ]
        );
    }

    #[test]
    fn pack_rat_unlocks_left_slot_and_nonweapons_do_not_claim_releases() {
        for count in [1, 2] {
            let (mut h, mut input, ids) = setup();
            input.right_hand.position = h.centers.unwrap()[1];
            input.right_hand.squeeze_value = 1.0;
            h.update(
                &input,
                [None, Some(ids[0])],
                [false; 2],
                [None; 2],
                count,
                [true; 2],
                true,
                0.016,
            );
            input.right_hand.squeeze_value = 0.0;
            let action = h.update(
                &input,
                [None, Some(ids[0])],
                [false; 2],
                [None; 2],
                count,
                [true; 2],
                true,
                0.016,
            )[1];
            assert_eq!(action.is_some(), count == 2);
            input.right_hand.position = h.centers.unwrap()[0];
            input.right_hand.squeeze_value = 1.0;
            h.update(
                &input,
                [None, Some(ids[0])],
                [false; 2],
                [None; 2],
                count,
                [false; 2],
                true,
                0.016,
            );
            input.right_hand.squeeze_value = 0.0;
            assert_eq!(
                h.update(
                    &input,
                    [None, Some(ids[0])],
                    [false; 2],
                    [None; 2],
                    count,
                    [false; 2],
                    true,
                    0.016
                ),
                [None; 2]
            );
        }
    }

    #[test]
    fn tracking_recovery_and_disabled_input_cannot_invent_a_draw() {
        let (mut h, mut input, ids) = setup();
        input.right_hand.position = h.centers.unwrap()[0];
        input.right_hand.squeeze_value = 1.0;
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [true, false],
        });
        assert_eq!(
            h.update(
                &input,
                [None; 2],
                [true; 2],
                [Some(ids[0]), None],
                1,
                [false; 2],
                true,
                0.016
            ),
            [None; 2]
        );
        input.pose_tracking = None;
        assert_eq!(
            h.update(
                &input,
                [None; 2],
                [true; 2],
                [Some(ids[0]), None],
                1,
                [false; 2],
                true,
                0.016
            ),
            [None; 2]
        );
        h.update(
            &input,
            [None; 2],
            [true; 2],
            [Some(ids[0]), None],
            1,
            [false; 2],
            false,
            0.016,
        );
        assert!(
            h.centers.is_some(),
            "opening UI preserves the worn slot visuals"
        );
        assert_eq!(
            h.update(
                &input,
                [None; 2],
                [true; 2],
                [Some(ids[0]), None],
                1,
                [false; 2],
                true,
                0.016
            ),
            [None; 2]
        );
    }
}

pub(super) struct Holsters {
    yaw: Option<f32>,
    pub centers: Option<[Vector3<f32>; 2]>,
    pub near: [Option<usize>; 2],
    pressed: [bool; 2],
    pressed_item: [Option<EntityId>; 2],
    pub retained: RetainedRelease,
}

impl Default for Holsters {
    fn default() -> Self {
        Self {
            yaw: None,
            centers: None,
            near: [None; 2],
            pressed: [true; 2],
            pressed_item: [None; 2],
            retained: RetainedRelease::default(),
        }
    }
}

/// Slots are right (default), then left (Pack Rat). Hand arrays are left/right.
pub(super) fn occupants(world: &World) -> [Option<EntityId>; 2] {
    let mut slots = [None; 2];
    let holstered = world.borrow::<View<RuntimePropHolstered>>().unwrap();
    for (id, slot) in holstered.iter().with_id() {
        if let Some(cell) = slots.get_mut(slot.slot as usize) {
            *cell = Some(id);
        }
    }
    slots
}

pub(super) fn slot_count(world: &World) -> usize {
    1 + usize::from(
        world
            .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
            .is_ok_and(|quests| {
                quests
                    .player_stats()
                    .has_os_trait(crate::scripts::gui::TRAIT_PACK_RAT)
            }),
    )
}

impl Holsters {
    pub fn update(
        &mut self,
        input: &InputContext,
        held: [Option<EntityId>; 2],
        available: [bool; 2],
        mut slots: [Option<EntityId>; 2],
        count: usize,
        weapons: [bool; 2],
        enabled: bool,
        dt: f32,
    ) -> [Option<Action>; 2] {
        let hands = [&input.left_hand, &input.right_hand];
        self.retained.update(held, hands.map(|h| h.squeeze_value));
        let head = GripPose {
            position: input.head.position,
            rotation: input.head.rotation,
        };
        if !head.is_tracked() || input.pose_tracking.is_some_and(|p| !p.head) {
            self.centers = None;
            self.near = [None; 2];
            self.pressed = [true; 2];
            self.pressed_item = [None; 2];
            self.yaw = None;
            return [None; 2];
        }
        let direction = crate::ui::PanelPlacement::from_head(head.position, head.rotation).forward;
        let yaw = followed_yaw(
            self.yaw,
            direction.x.atan2(-direction.z),
            self.near.iter().any(Option::is_some),
            dt,
        );
        self.yaw = Some(yaw);
        let forward = vec3(yaw.sin(), 0.0, -yaw.cos());
        let right = forward.cross(Vector3::unit_y());
        let base = head.position
            - Vector3::unit_y()
                * (crate::dev_params::get(crate::dev_params::VR_HOLSTER_DROP) / SCALE)
            + forward * (0.04 / SCALE);
        let side = crate::dev_params::get(crate::dev_params::VR_HOLSTER_SIDE) / SCALE;
        let centers = [base + right * side, base - right * side];
        self.centers = Some(centers);
        if !enabled {
            self.near = [None; 2];
            self.pressed = [true; 2];
            self.pressed_item = [None; 2];
            return [None; 2];
        }
        let mut actions = [None; 2];
        let mut claimed = [false; 2];
        // Reserve a slot/entity in this snapshot before considering the other
        // hand, so two simultaneous deposits or draws cannot claim it twice.
        for i in 0..2 {
            let hand = hands[i];
            let tracked = GripPose {
                position: hand.position,
                rotation: hand.rotation,
            }
            .is_tracked()
                && hand.squeeze_value.is_finite()
                && input.pose_tracking.is_none_or(|p| p.hands[i]);
            let pressed = hand.squeeze_value >= 0.5;
            self.near[i] = (tracked && (held[i].is_none() || weapons[i])).then(|| (0..2).find(|s|
                // A stored item remains retrievable if its perk is removed.
                (*s < count || slots[*s].is_some()) && (hand.position - centers[*s]).magnitude2() <= RADIUS * RADIUS
            )).flatten();
            if let Some(slot) = self.near[i] {
                if let Some(entity) = held[i] {
                    if !pressed && self.pressed_item[i] == Some(entity) {
                        if weapons[i] && slot < count && slots[slot].is_none() && !claimed[slot] {
                            claimed[slot] = true;
                            slots[slot] = Some(entity);
                            actions[i] = Some(Action::Store { entity, slot });
                        } else {
                            self.retained.retain(i, entity);
                            actions[i] = Some(Action::Refuse { entity });
                        }
                    }
                } else if available[i] && pressed && !self.pressed[i] && !claimed[slot] {
                    if let Some(entity) = slots[slot].take() {
                        claimed[slot] = true;
                        actions[i] = Some(Action::Retrieve { entity, slot });
                    }
                }
            }
            self.pressed[i] = if tracked { pressed } else { true };
            self.pressed_item[i] = if tracked && pressed { held[i] } else { None };
        }
        actions
    }

    pub fn world_rotation(&self, rotation: Quaternion<f32>) -> Matrix4<f32> {
        // The heading convention is clockwise from -Z; cgmath yaw is opposite.
        Matrix4::from(rotation) * Matrix4::from_angle_y(cgmath::Rad(-self.yaw.unwrap_or(0.0)))
    }

    pub fn world_centers(
        &self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> Option<[Vector3<f32>; 2]> {
        self.centers
            .map(|cs| cs.map(|c| position + rotation.rotate_vector(c)))
    }

    pub fn diagnostics(
        &self,
        world: &World,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> serde_json::Value {
        serde_json::json!({"centers": self.world_centers(position, rotation).map(|cs| cs.map(|c| [c.x,c.y,c.z])),
            "radius":RADIUS, "enabled_slots":slot_count(world), "near":self.near,
            "items":occupants(world).map(|e| e.map(|id| id.inner() as i32)),
            "retained":self.retained.0.map(|e| e.is_some())})
    }

    pub fn render(
        &self,
        world: &World,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> Vec<engine::scene::SceneObject> {
        use engine::scene::{SceneObject, color_material, lines_mesh};
        let Some(centers) = self.world_centers(position, rotation) else {
            return vec![];
        };
        let slots = occupants(world);
        let count = slot_count(world);
        let debug = crate::dev_params::get_bool(crate::dev_params::VR_HOLSTER_ZONES);
        let mut objects = Vec::new();
        for slot in 0..2 {
            if slot >= count && slots[slot].is_none() {
                continue;
            }
            let reached = self.near.contains(&Some(slot));
            let refused = (0..2).any(|h| self.near[h] == Some(slot) && self.retained.keep_grip(h));
            let color = if refused {
                vec3(1.0, 0.2, 0.1)
            } else if reached {
                vec3(0.1, 1.0, 0.35)
            } else {
                vec3(0.1, 0.55, 0.7)
            };
            let radius = if debug { RADIUS } else { 0.04 / SCALE };
            let mut points = Vec::new();
            dark::hit_box::append_capsule_lines(
                &mut points,
                &Matrix4::from_scale(1.0),
                centers[slot],
                centers[slot],
                radius,
            );
            objects.push(SceneObject::new(
                color_material::create(color),
                Box::new(lines_mesh::create(points)),
            ));
        }
        crate::util::tag_render_source(&mut objects, crate::util::render_source::PLAYER_HANDS);
        objects
    }
}
