//! Shared body-slot gestures for thigh weapons and backpack-backed chest items.
use super::body_inventory::{RetainedRelease, followed_yaw};
use crate::{
    input_context::InputContext, runtime_props::RuntimePropHolstered, vr_support::GripPose,
};
use cgmath::{InnerSpace, Matrix4, Quaternion, Rotation, Vector3, vec3};
use shipyard::{EntityId, IntoIter, IntoWithId, View, World};

const SCALE: f32 = crate::METERS_PER_WORLD_UNIT;
fn radius() -> f32 {
    crate::dev_params::get(crate::dev_params::VR_HOLSTER_RADIUS) / SCALE
}

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
            &input, [None; 2], [true; 2], [None; 2], 1, [false; 2], [false; 2], true, 0.016,
        );
        (h, input, ids)
    }

    #[test]
    fn chest_tracking_or_modal_recovery_never_invents_a_release() {
        for invalid in ["head", "hand", "modal"] {
            let (_, mut input, ids) = setup();
            let mut slots = Holsters::chest();
            slots.update(
                &input, [None; 2], [true; 2], [None; 2], 2, [false; 2], [false; 2], true, 0.016,
            );
            input.right_hand.position = slots.centers.unwrap()[0];
            input.right_hand.squeeze_value = 1.0;
            let held = [None, Some(ids[0])];
            slots.update(
                &input,
                held,
                [true; 2],
                [None; 2],
                2,
                [false, true],
                [false, true],
                true,
                0.016,
            );
            input.pose_tracking = Some(crate::input_context::PoseTracking {
                head: invalid != "head",
                hands: [true, invalid != "hand"],
            });
            if invalid == "modal" {
                slots.disarm_input();
            }
            slots.update(
                &input,
                held,
                [true; 2],
                [None; 2],
                2,
                [false, true],
                [false, true],
                invalid != "modal",
                0.016,
            );
            input.pose_tracking = None;
            input.right_hand.squeeze_value = 0.0;
            assert_eq!(
                slots.update(
                    &input,
                    held,
                    [true; 2],
                    [None; 2],
                    2,
                    [false, true],
                    [false, true],
                    true,
                    0.016
                ),
                [None; 2],
                "{invalid}"
            );
            if invalid != "modal" {
                assert!(
                    slots.retained.keep_grip(1),
                    "tracking loss retains the item"
                );
            }
            input.right_hand.squeeze_value = 1.0;
            slots.update(
                &input,
                held,
                [true; 2],
                [None; 2],
                2,
                [false, true],
                [false, true],
                true,
                0.016,
            );
            input.right_hand.squeeze_value = 0.0;
            assert_eq!(
                slots.update(
                    &input,
                    held,
                    [true; 2],
                    [None; 2],
                    2,
                    [false, true],
                    [false, true],
                    true,
                    0.016
                ),
                [
                    None,
                    Some(Action::Store {
                        entity: ids[0],
                        slot: 0
                    })
                ]
            );
        }
    }

    #[test]
    fn chest_mounts_share_body_heading_and_ignore_head_pitch() {
        let mut slots = Holsters::chest();
        let mut input = InputContext::default();
        slots.body_pose = Some(super::super::body_inventory::BodyPose {
            head: input.head.position,
            yaw: 0.4,
        });
        slots.update(
            &input, [None; 2], [true; 2], [None; 2], 2, [false; 2], [false; 2], true, 0.016,
        );
        let original = slots.centers;
        use cgmath::Rotation3;
        input.head.rotation = Quaternion::from_angle_x(cgmath::Deg(75.0));
        slots.update(
            &input, [None; 2], [true; 2], [None; 2], 2, [false; 2], [false; 2], true, 0.016,
        );
        assert_eq!(slots.centers, original);
    }

    #[test]
    fn an_authored_non_weapon_can_use_the_same_release_gesture() {
        let (mut holsters, mut input, ids) = setup();
        input.right_hand.position = holsters.centers.unwrap()[0];
        input.right_hand.squeeze_value = 1.0;
        let held = [None, Some(ids[0])];
        holsters.update(
            &input,
            held,
            [true; 2],
            [None; 2],
            2,
            [false; 2],
            [false, true],
            true,
            0.016,
        );
        input.right_hand.squeeze_value = 0.0;
        assert_eq!(
            holsters.update(
                &input,
                held,
                [true; 2],
                [None; 2],
                2,
                [false; 2],
                [false, true],
                true,
                0.016
            )[1],
            Some(Action::Store {
                entity: ids[0],
                slot: 0
            })
        );
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
                    &input, held, [false; 2], [None; 2], 1, [true; 2], [true; 2], true, 0.016
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
                    &input, held, [false; 2], [None; 2], 1, [true; 2], [true; 2], true, 0.016
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
            &input, held, [false; 2], [None; 2], 2, [true; 2], [true; 2], true, 0.016,
        );
        input.left_hand.squeeze_value = 0.0;
        input.right_hand.squeeze_value = 0.0;
        let actions = h.update(
            &input, held, [false; 2], [None; 2], 2, [true; 2], [true; 2], true, 0.016,
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
    fn capacity_limits_slots_and_nonweapons_do_not_claim_releases() {
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
                [false; 2],
                true,
                0.016
            ),
            [None; 2]
        );
    }
}

pub(super) struct Holsters {
    /// Chest mounts share the grip state machine, but use the belt's heading
    /// and inventory-backed ownership instead of independent thigh capacity.
    pub chest: bool,
    pub can_store: [bool; 2],
    pub matching_slots: [[bool; 2]; 2],
    pub refusal_flash: [f32; 2],
    pub poses: crate::vr_holster::HolsterLibrary,
    poses_loaded: bool,
    pub body_pose: Option<super::body_inventory::BodyPose>,
    pub freeze_heading: bool,
    pub pouch_priority: [bool; 2],
    pub shoulder_priority: [bool; 2],
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
            chest: false,
            can_store: [false; 2],
            matching_slots: [[false; 2]; 2],
            refusal_flash: [0.0; 2],
            poses: Default::default(),
            poses_loaded: false,
            body_pose: None,
            freeze_heading: false,
            pouch_priority: [false; 2],
            shoulder_priority: [false; 2],
            yaw: None,
            centers: None,
            near: [None; 2],
            pressed: [true; 2],
            pressed_item: [None; 2],
            retained: RetainedRelease::default(),
        }
    }
}

/// Slots are right/left; hand arrays are left/right.
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

/// Both thigh holsters are standard equipment, independent of O/S upgrades.
pub(super) const SLOT_COUNT: usize = 2;

impl Holsters {
    pub fn disarm_input(&mut self) {
        self.pressed = [true; 2];
        self.pressed_item = [None; 2];
        self.near = [None; 2];
        self.can_store = [false; 2];
    }

    pub fn chest() -> Self {
        Self {
            chest: true,
            ..Self::default()
        }
    }

    fn radius(&self) -> f32 {
        if self.chest { 0.075 / SCALE } else { radius() }
    }

    pub fn occupants(&self, world: &World) -> [Option<EntityId>; 2] {
        if !self.chest {
            return occupants(world);
        }
        let mut slots = [None; 2];
        let inventory = world
            .borrow::<shipyard::UniqueView<super::PlayerInfo>>()
            .unwrap()
            .inventory_entity_id;
        let contents = crate::inventory::Inventory::from_container(
            world,
            inventory,
            crate::inventory::grid_for(world, inventory),
        );
        let markers = world
            .borrow::<View<crate::runtime_props::RuntimePropChestSlot>>()
            .unwrap();
        use shipyard::Get;
        for item in contents.all_items() {
            if let Ok(marker) = markers.get(item.entity) {
                for (slot, occupant) in slots.iter_mut().enumerate() {
                    if marker.0 & (1 << slot) != 0 {
                        *occupant = Some(item.entity);
                    }
                }
            }
        }
        slots
    }

    pub fn same_chest_kind(world: &World, a: EntityId, b: EntityId) -> bool {
        crate::scripts::script_util::entity_class_template_id(world, a)
            .zip(crate::scripts::script_util::entity_class_template_id(
                world, b,
            ))
            .is_some_and(|(a, b)| a == b)
    }

    /// Live backpack stock only: items already in either hand are unavailable.
    pub fn chest_stock(world: &World, source: EntityId) -> Vec<(EntityId, u32)> {
        use shipyard::{Get, UniqueView};
        let player = world.borrow::<UniqueView<super::PlayerInfo>>().unwrap();
        let inventory = player.inventory_entity_id;
        let held = [player.left_hand_entity_id, player.right_hand_entity_id];
        let contents = crate::inventory::Inventory::from_container(
            world,
            inventory,
            crate::inventory::grid_for(world, inventory),
        );
        let stacks = world
            .borrow::<View<dark::properties::PropStackCount>>()
            .unwrap();
        contents
            .all_items()
            .filter_map(|item| {
                let count = stacks.get(item.entity).map_or(1, |stack| stack.0).max(0) as u32;
                (count > 0
                    && !held.contains(&Some(item.entity))
                    && Self::same_chest_kind(world, source, item.entity))
                .then_some((item.entity, count))
            })
            .collect()
    }

    pub fn counts(&self, world: &World) -> [u32; 2] {
        self.occupants(world).map(|source| {
            source.map_or(0, |source| {
                Self::chest_stock(world, source)
                    .iter()
                    .fold(0_u32, |total, (_, count)| total.saturating_add(*count))
            })
        })
    }

    /// A reserve can supply both slots without duplicating its inventory owner.
    pub fn assign_chest_slot(world: &mut World, source: EntityId, slot: usize) {
        use crate::runtime_props::RuntimePropChestSlot;
        let bit = 1_u8 << slot;
        let markers: Vec<_> = world
            .borrow::<View<RuntimePropChestSlot>>()
            .unwrap()
            .iter()
            .with_id()
            .map(|(id, marker)| (id, marker.0))
            .collect();
        let previous = markers
            .iter()
            .find(|(id, _)| *id == source)
            .map_or(0, |(_, bits)| *bits);
        for (id, bits) in markers {
            if id != source && bits & bit != 0 {
                if bits & !bit == 0 {
                    world.remove::<RuntimePropChestSlot>(id);
                } else {
                    world.add_component(id, RuntimePropChestSlot(bits & !bit));
                }
            }
        }
        world.add_component(source, RuntimePropChestSlot(previous | bit));
    }

    pub fn flash_refusal(&mut self, hand: usize) {
        if let Some(slot) = self.near[hand] {
            self.refusal_flash[slot] = 0.35;
        }
    }

    pub fn accepts_chest_item(world: &World, item: EntityId) -> bool {
        crate::vr_holster::model_for_entity(world, item).is_some_and(|model| {
            matches!(
                model.as_str(),
                "medpatch"
                    | "psipatch"
                    | "radpatch"
                    | "toxpatch"
                    | "portbatt"
                    | "battery"
                    | "batteryb"
            )
        })
    }

    pub fn load_poses(&mut self, assets: &mut engine::assets::asset_cache::AssetCache) {
        if self.poses_loaded {
            return;
        }
        self.poses_loaded = true;
        if let Some(text) = assets.get_opt(
            &engine::assets::text_importer::TEXT_IMPORTER,
            crate::vr_holster::RESOURCE,
        ) {
            match crate::vr_holster::HolsterLibrary::parse(&text) {
                Ok(poses) => self.poses = poses,
                Err(error) => {
                    eprintln!("Invalid holster definitions; holstering disabled: {error}")
                }
            }
        }
    }

    pub fn update(
        &mut self,
        input: &InputContext,
        held: [Option<EntityId>; 2],
        available: [bool; 2],
        mut slots: [Option<EntityId>; 2],
        count: usize,
        weapons: [bool; 2],
        eligible: [bool; 2],
        enabled: bool,
        dt: f32,
    ) -> [Option<Action>; 2] {
        for remaining in &mut self.refusal_flash {
            *remaining = (*remaining - dt.max(0.0)).max(0.0);
        }
        let hands = [&input.left_hand, &input.right_hand];
        let head = GripPose {
            position: input.head.position,
            rotation: input.head.rotation,
        };
        let head_tracked = head.is_tracked() && input.pose_tracking.is_none_or(|p| p.head);
        let tracked_hands: [bool; 2] = std::array::from_fn(|i| {
            head_tracked
                && GripPose {
                    position: hands[i].position,
                    rotation: hands[i].rotation,
                }
                .is_tracked()
                && hands[i].squeeze_value.is_finite()
                && input.pose_tracking.is_none_or(|p| p.hands[i])
        });
        self.retained.update(
            held,
            std::array::from_fn(|i| {
                if tracked_hands[i] {
                    hands[i].squeeze_value
                } else {
                    0.0
                }
            }),
        );
        // Losing tracking during a chest gesture disarms both storage and the
        // ordinary world release. A tracked re-grip explicitly unlocks it.
        if self.chest {
            for i in 0..2 {
                if !tracked_hands[i] && self.near[i].is_some() {
                    if let Some(item) = held[i] {
                        self.retained.retain(i, item);
                    }
                }
            }
        }
        if !head_tracked {
            self.centers = None;
            self.body_pose = None;
            self.near = [None; 2];
            self.pressed = [true; 2];
            self.pressed_item = [None; 2];
            self.yaw = None;
            return [None; 2];
        }
        let direction = crate::ui::PanelPlacement::from_head(head.position, head.rotation).forward;
        let yaw = if self.chest && self.body_pose.is_some() {
            self.body_pose.unwrap().yaw
        } else {
            followed_yaw(
                self.yaw,
                direction.x.atan2(-direction.z),
                self.freeze_heading || self.near.iter().any(Option::is_some),
                dt,
            )
        };
        self.yaw = Some(yaw);
        self.body_pose = Some(super::body_inventory::BodyPose {
            head: head.position,
            yaw,
        });
        let forward = vec3(yaw.sin(), 0.0, -yaw.cos());
        let right = forward.cross(Vector3::unit_y());
        let (below, forward_distance, side) = if self.chest {
            (
                crate::dev_params::get(crate::dev_params::VR_CHEST_DROP),
                crate::dev_params::get(crate::dev_params::VR_CHEST_FORWARD),
                crate::dev_params::get(crate::dev_params::VR_CHEST_SIDE),
            )
        } else {
            (
                crate::dev_params::get(crate::dev_params::VR_HOLSTER_DROP),
                crate::dev_params::get(crate::dev_params::VR_HOLSTER_FORWARD),
                crate::dev_params::get(crate::dev_params::VR_HOLSTER_SIDE),
            )
        };
        let base = head.position - Vector3::unit_y() * (below / SCALE)
            + forward * (forward_distance / SCALE);
        let side = side / SCALE;
        let centers = [base + right * side, base - right * side];
        self.centers = Some(centers);
        if !enabled {
            self.near = [None; 2];
            self.pressed = [true; 2];
            self.pressed_item = [None; 2];
            return [None; 2];
        }
        self.can_store = [false; 2];
        let mut actions = [None; 2];
        let mut claimed = [false; 2];
        // Reserve a slot/entity in this snapshot before considering the other
        // hand, so two simultaneous deposits or draws cannot claim it twice.
        for i in 0..2 {
            let hand = hands[i];
            let tracked = tracked_hands[i];
            let pressed = hand.squeeze_value >= 0.5;
            // Calibration can put a thigh target over the ammo pouch. An empty
            // hand with an opposite gun uses the pouch there, never both slots.
            let at_pouch = self.pouch_priority[i]
                && held[i].is_none()
                && (hand.position - super::ammo_pouch::center_for(self.body_pose.unwrap()))
                    .magnitude2()
                    <= (super::ammo_pouch::RADIUS + super::body_inventory::hand_radius()).powi(2);
            self.near[i] = (tracked
                && !at_pouch
                && !self.shoulder_priority[i]
                && (held[i].is_none() || weapons[i] || eligible[i]))
                .then(|| {
                    (0..2)
                        // Stored items remain retrievable even when a slot is disabled.
                        .filter(|s| {
                            (*s < count || slots[*s].is_some())
                                && (hand.position - centers[*s]).magnitude2()
                                    <= (self.radius() + super::body_inventory::hand_radius())
                                        .powi(2)
                        })
                        .min_by(|a, b| {
                            (hand.position - centers[*a])
                                .magnitude2()
                                .total_cmp(&(hand.position - centers[*b]).magnitude2())
                        })
                })
                .flatten();
            if let Some(slot) = self.near[i] {
                self.can_store[i] = eligible[i]
                    && slot < count
                    && (slots[slot].is_none() || self.matching_slots[i][slot])
                    && !claimed[slot];
                if let Some(entity) = held[i] {
                    if !pressed && self.pressed_item[i] == Some(entity) {
                        if eligible[i]
                            && slot < count
                            && (slots[slot].is_none() || self.matching_slots[i][slot])
                            && !claimed[slot]
                        {
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
            "radius":self.radius(), "enabled_slots":SLOT_COUNT, "near":self.near, "can_store":self.can_store, "refusal_flash":self.refusal_flash,
            "counts":if self.chest { self.counts(world) } else { [0; 2] },
            "items":self.occupants(world).map(|e| e.map(|id| id.inner() as i32)),
            "retained":self.retained.0.map(|e| e.is_some())})
    }

    pub fn render(
        &self,
        world: &World,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> Vec<engine::scene::SceneObject> {
        use engine::scene::{SceneObject, color_material, lines_mesh};
        let toggle = if self.chest {
            crate::dev_params::VR_BODY_INVENTORY_ZONES
        } else {
            crate::dev_params::VR_HOLSTER_ZONES
        };
        if !crate::dev_params::get_bool(toggle) {
            return vec![];
        }
        let Some(centers) = self.world_centers(position, rotation) else {
            return vec![];
        };
        let slots = self.occupants(world);
        let count = SLOT_COUNT;
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
            let mut points = Vec::new();
            dark::hit_box::append_capsule_lines(
                &mut points,
                &Matrix4::from_scale(1.0),
                centers[slot],
                centers[slot],
                self.radius()
                    + if self.chest {
                        super::body_inventory::hand_radius()
                    } else {
                        0.0
                    },
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
