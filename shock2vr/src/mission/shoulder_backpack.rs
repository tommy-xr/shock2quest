//! Release-to-stow behind either shoulder. Coordinates are in tracked pawn space.
//! Uses the head's slowly followed yaw, never its pitch/roll, for body direction.
use cgmath::{InnerSpace, Matrix4, Quaternion, Rotation, Vector3, vec3};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};

use crate::{input_context::InputContext, vr_support::GripPose};

const SCALE: f32 = crate::METERS_PER_WORLD_UNIT;
const SIDE: f32 = 0.18 / SCALE;
const DROP: f32 = 0.20 / SCALE;
const BEHIND: f32 = 0.18 / SCALE;
pub(super) const RADIUS: f32 = 0.18 / SCALE;

pub(super) struct ShoulderBackpack {
    yaw: Option<f32>,
    pressed_item: [Option<EntityId>; 2],
    retained: super::body_inventory::RetainedRelease,
    pub centers: Option<[Vector3<f32>; 2]>,
    pub near: [bool; 2],
    pub near_slot: [Option<usize>; 2],
    pub draws: [Option<usize>; 2],
    pub blocks_grab: [bool; 2],
    draw_pressed: [bool; 2],
    consumed: [bool; 2],
}

impl Default for ShoulderBackpack {
    fn default() -> Self {
        Self {
            yaw: None,
            pressed_item: [None; 2],
            retained: Default::default(),
            centers: None,
            near: [false; 2],
            near_slot: [None; 2],
            draws: [None; 2],
            blocks_grab: [false; 2],
            draw_pressed: [true; 2],
            consumed: [false; 2],
        }
    }
}

/// Only live inventory membership can supply a shoulder recall. A marker never
/// creates ownership or allows a dropped/destroyed/transferred weapon to return.
pub(super) fn weapons(world: &World, inventory: EntityId) -> [Option<EntityId>; 2] {
    let mut slots = [None; 2];
    let contents = crate::inventory::Inventory::from_container(
        world,
        inventory,
        crate::inventory::grid_for(world, inventory),
    );
    let marked = world
        .borrow::<View<crate::runtime_props::RuntimePropShoulderWeapon>>()
        .unwrap();
    for item in contents.all_items() {
        if let Ok(slot) = marked.get(item.entity) {
            if let Some(out) = slots.get_mut(slot.0 as usize) {
                *out = Some(item.entity);
            }
        }
    }
    slots
}

pub(super) fn remember(world: &mut World, entity: EntityId, slot: usize) {
    let old: Vec<_> = world
        .borrow::<View<crate::runtime_props::RuntimePropShoulderWeapon>>()
        .unwrap()
        .iter()
        .with_id()
        .filter(|(_, marker)| marker.0 as usize == slot)
        .map(|(id, _)| id)
        .collect();
    for id in old {
        world.remove::<crate::runtime_props::RuntimePropShoulderWeapon>(id);
    }
    world.add_component(
        entity,
        crate::runtime_props::RuntimePropShoulderWeapon(slot as u8),
    );
}

impl ShoulderBackpack {
    /// A prior tracked squeeze with this same item is required. Crossing a zone
    /// while still holding never stows; tracking recovery never invents a release.
    pub fn update(
        &mut self,
        input: &InputContext,
        held: [Option<EntityId>; 2],
        enabled: bool,
        dt: f32,
    ) -> [bool; 2] {
        let hands = [&input.left_hand, &input.right_hand];
        self.retained.update(held, hands.map(|h| h.squeeze_value));
        self.draws = [None; 2];
        self.near_slot = [None; 2];
        let tracked = std::array::from_fn::<_, 2, _>(|i| {
            let hand = hands[i];
            (GripPose {
                position: hand.position,
                rotation: hand.rotation,
            })
            .is_tracked()
                && hand.squeeze_value.is_finite()
                && input.pose_tracking.is_none_or(|p| p.hands[i])
        });
        for i in 0..2 {
            if tracked[i] && hands[i].squeeze_value < 0.5 {
                self.consumed[i] = false;
            }
            self.blocks_grab[i] = held[i].is_none() && self.consumed[i];
        }
        let head = GripPose {
            position: input.head.position,
            rotation: input.head.rotation,
        };
        if !enabled || !head.is_tracked() || input.pose_tracking.is_some_and(|p| !p.head) {
            self.centers = None;
            self.near = [false; 2];
            self.pressed_item = [None; 2];
            self.yaw = None;
            self.draw_pressed = [true; 2];
            return [false; 2];
        }
        let forward = crate::ui::PanelPlacement::from_head(head.position, head.rotation).forward;
        let observed = forward.x.atan2(-forward.z);
        let yaw = super::body_inventory::followed_yaw(
            self.yaw,
            observed,
            self.near.iter().any(|near| *near),
            dt,
        );
        self.yaw = Some(yaw);
        let forward = vec3(yaw.sin(), 0.0, -yaw.cos());
        let right = forward.cross(Vector3::unit_y());
        let center = head.position - forward * BEHIND - Vector3::unit_y() * DROP;
        let centers = [center - right * SIDE, center + right * SIDE];
        self.centers = Some(centers);
        let mut released = [false; 2];
        for i in 0..2 {
            let hand = hands[i];
            let tracked = tracked[i];
            // Require the hand genuinely behind the head, not beside the face.
            self.near_slot[i] = (tracked
                && (hand.position - head.position).dot(forward) < -0.03 / SCALE)
                .then(|| {
                    centers
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| (hand.position - **c).magnitude2() <= RADIUS * RADIUS)
                        .min_by(|(_, a), (_, b)| {
                            (hand.position - **a)
                                .magnitude2()
                                .total_cmp(&(hand.position - **b).magnitude2())
                        })
                        .map(|(slot, _)| slot)
                })
                .flatten();
            self.near[i] = self.near_slot[i].is_some();
            let pressed = hand.squeeze_value >= 0.5;
            if held[i].is_none() && self.near[i] {
                self.blocks_grab[i] = true;
                if pressed {
                    self.consumed[i] = true;
                    if !self.draw_pressed[i] {
                        self.draws[i] = self.near_slot[i];
                    }
                }
            }
            self.draw_pressed[i] = !tracked || pressed;
            released[i] = self.near[i]
                && hand.squeeze_value < 0.5
                && held[i].is_some()
                && self.pressed_item[i] == held[i];
            self.pressed_item[i] = if tracked && hand.squeeze_value >= 0.5 {
                held[i]
            } else {
                None
            };
        }
        released
    }

    /// A rejected deposit stays held until a real re-grip, even if the hand
    /// leaves the zone or the cyber interface opens. No invisible floor drop.
    pub fn retain(&mut self, slot: usize, entity: EntityId) {
        self.retained.retain(slot, entity);
    }
    pub fn keep_grip(&self, slot: usize) -> bool {
        self.retained.keep_grip(slot)
    }

    pub fn render(
        &self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> Vec<engine::scene::SceneObject> {
        use engine::scene::{SceneObject, color_material, lines_mesh};
        let Some(centers) = self.centers else {
            return Vec::new();
        };
        let root = Matrix4::from_translation(position) * Matrix4::from(rotation);
        let mut vertices = Vec::new();
        for center in centers {
            dark::hit_box::append_capsule_lines(&mut vertices, &root, center, center, RADIUS);
        }
        let color = if self.retained.0.iter().any(Option::is_some) {
            vec3(1.0, 0.3, 0.1)
        } else if self.near.iter().any(|near| *near) {
            vec3(0.1, 1.0, 0.2)
        } else {
            vec3(0.1, 0.8, 1.0)
        };
        let mut objects = vec![SceneObject::new(
            color_material::create(color),
            Box::new(lines_mesh::create(vertices)),
        )];
        crate::util::tag_render_source(&mut objects, crate::util::render_source::PLAYER_HANDS);
        objects
    }

    pub fn diagnostics(
        &self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> serde_json::Value {
        serde_json::json!({
            "centers": self.centers.map(|cs| cs.map(|c| { let p = position + rotation.rotate_vector(c); [p.x,p.y,p.z] })),
            "radius": RADIUS, "near": self.near, "near_slot": self.near_slot,
            "retained": self.retained.0.map(|e| e.is_some()),
        })
    }
}

/// Reserve real grid cells for both simultaneous deposits. A full backpack
/// refuses rather than creating a hidden overflow item. No world mutation here.
pub(super) fn reserve_slots(
    world: &World,
    inventory: EntityId,
    requests: [Option<EntityId>; 2],
) -> [Option<(usize, usize)>; 2] {
    if requests.iter().all(Option::is_none) {
        return [None; 2];
    }
    let grid = crate::inventory::grid_for(world, inventory);
    let mut occupied = crate::inventory::Inventory::from_container(world, inventory, grid);
    let dimensions = world
        .borrow::<View<dark::properties::PropInventoryDimensions>>()
        .unwrap();
    requests.map(|request| {
        let entity = request?;
        let (w, h) = dimensions
            .get(entity)
            .map(|d| (d.width as usize, d.height as usize))
            .unwrap_or((1, 1));
        if w == 0 || h == 0 {
            return None;
        }
        let slot = occupied.first_free_slot(w, h)?;
        let cell = occupied.cell_of(slot)?;
        occupied
            .insert_if_fits(entity, cell.0, cell.1, w, h)
            .then_some(cell)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, Rotation3, Zero};
    use dark::properties::{Links, PropContainDimensions, PropInventoryDimensions};

    fn fixture() -> (ShoulderBackpack, InputContext, EntityId) {
        let mut world = World::new();
        let item = world.add_entity(());
        let mut input = InputContext::default();
        input.left_hand.position = vec3(-0.3, 1.0, -0.4);
        input.right_hand.position = vec3(0.3, 1.0, -0.4);
        (ShoulderBackpack::default(), input, item)
    }

    #[test]
    fn either_hand_can_release_at_either_shoulder_but_crossing_while_held_does_not_stow() {
        for hand in 0..2 {
            for shoulder in 0..2 {
                let (mut tracker, mut input, item) = fixture();
                let mut held = [None; 2];
                held[hand] = Some(item);
                tracker.update(&input, held, true, 0.016);
                let center = tracker.centers.unwrap()[shoulder];
                let h = if hand == 0 {
                    &mut input.left_hand
                } else {
                    &mut input.right_hand
                };
                h.position = center;
                h.squeeze_value = 1.0;
                assert_eq!(tracker.update(&input, held, true, 0.016), [false; 2]);
                let h = if hand == 0 {
                    &mut input.left_hand
                } else {
                    &mut input.right_hand
                };
                h.squeeze_value = 0.0;
                let result = tracker.update(&input, held, true, 0.016);
                assert!(result[hand]);
                assert!(!result[1 - hand]);
                assert_eq!(tracker.update(&input, held, true, 0.016), [false; 2]);
            }
        }
    }

    #[test]
    fn recall_requires_fresh_tracked_press_and_consumes_it_until_release() {
        let (mut tracker, mut input, _) = fixture();
        tracker.update(&input, [None; 2], true, 0.016);
        input.left_hand.position = tracker.centers.unwrap()[1];
        tracker.update(&input, [None; 2], true, 0.016);
        assert_eq!(tracker.draws, [None; 2]);
        input.left_hand.squeeze_value = 1.0;
        tracker.update(&input, [None; 2], true, 0.016);
        assert_eq!(tracker.draws, [Some(1), None]);
        input.left_hand.position = vec3(-1.0, 1.0, -1.0);
        tracker.update(&input, [None; 2], true, 0.016);
        assert_eq!(tracker.draws, [None; 2]);
        assert!(tracker.blocks_grab[0]);
        tracker.update(&input, [None; 2], false, 0.016);
        input.left_hand.position = input.head.position + vec3(-SIDE, -DROP, BEHIND);
        tracker.update(&input, [None; 2], true, 0.016);
        assert_eq!(tracker.draws, [None; 2]);
        input.left_hand.squeeze_value = 0.0;
        tracker.update(&input, [None; 2], true, 0.016);
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [false, true],
        });
        input.left_hand.squeeze_value = 1.0;
        tracker.update(&input, [None; 2], true, 0.016);
        input.pose_tracking = None;
        tracker.update(&input, [None; 2], true, 0.016);
        assert_eq!(tracker.draws, [None; 2]);
    }

    #[test]
    fn face_level_front_and_low_releases_stay_world_drops() {
        for offset in [
            vec3(0.0, 0.0, -0.2),
            vec3(0.0, 0.0, 0.0),
            vec3(0.18, -0.9, 0.18),
        ] {
            let (mut tracker, mut input, item) = fixture();
            input.right_hand.position = input.head.position + offset / SCALE;
            input.right_hand.squeeze_value = 1.0;
            tracker.update(&input, [None, Some(item)], true, 0.016);
            input.right_hand.squeeze_value = 0.0;
            assert_eq!(
                tracker.update(&input, [None, Some(item)], true, 0.016),
                [false; 2]
            );
        }
    }

    #[test]
    fn untracked_or_disabled_frames_disarm_the_release() {
        for bad_head in [true, false] {
            let (mut tracker, mut input, item) = fixture();
            let held = [None, Some(item)];
            tracker.update(&input, held, true, 0.016);
            input.right_hand.position = tracker.centers.unwrap()[1];
            input.right_hand.squeeze_value = 1.0;
            tracker.update(&input, held, true, 0.016);
            if bad_head {
                input.head.rotation = Quaternion::zero();
            } else {
                input.right_hand.rotation = Quaternion::zero();
            }
            assert_eq!(tracker.update(&input, held, true, 0.016), [false; 2]);
            input.head.rotation = Quaternion::from_angle_y(Deg(0.0));
            input.right_hand.rotation = input.head.rotation;
            input.right_hand.squeeze_value = 0.0;
            assert_eq!(tracker.update(&input, held, true, 0.016), [false; 2]);
            input.right_hand.squeeze_value = 1.0;
            tracker.update(&input, held, true, 0.016);
            tracker.update(&input, held, false, 0.016);
            input.right_hand.squeeze_value = 0.0;
            assert_eq!(tracker.update(&input, held, true, 0.016), [false; 2]);
        }
    }

    #[test]
    fn stale_valid_pose_values_do_not_override_runtime_tracking_loss() {
        for lost in 0..3 {
            let (mut tracker, mut input, item) = fixture();
            let hand = if lost == 1 { 0 } else { 1 };
            let mut held = [None; 2];
            held[hand] = Some(item);
            tracker.update(&input, held, true, 0.016);
            let center = tracker.centers.unwrap()[hand];
            for h in [&mut input.left_hand, &mut input.right_hand] {
                h.position = center;
                h.squeeze_value = 1.0;
            }
            tracker.update(&input, held, true, 0.016);
            input.pose_tracking = Some(crate::input_context::PoseTracking {
                head: lost != 0,
                hands: [lost != 1, lost != 2],
            });
            input.left_hand.squeeze_value = 0.0;
            input.right_hand.squeeze_value = 0.0;
            assert_eq!(tracker.update(&input, held, true, 0.016), [false; 2]);
            input.pose_tracking = None;
            assert_eq!(tracker.update(&input, held, true, 0.016), [false; 2]);
        }
    }

    #[test]
    fn body_zones_follow_head_translation_and_yaw_but_ignore_pitch_and_roll() {
        let (mut tracker, mut input, _) = fixture();
        tracker.update(&input, [None; 2], true, 0.016);
        let original = tracker.centers.unwrap();
        input.head.position += vec3(2.0, -0.5, 1.0);
        input.head.rotation =
            Quaternion::from_angle_x(Deg(-40.0)) * Quaternion::from_angle_z(Deg(25.0));
        tracker.update(&input, [None; 2], true, 0.016);
        for i in 0..2 {
            assert!(
                (tracker.centers.unwrap()[i] - original[i] - vec3(2.0, -0.5, 1.0)).magnitude()
                    < 0.0001
            );
        }
        let mut turned = ShoulderBackpack::default();
        input.head.rotation = Quaternion::from_angle_y(Deg(90.0));
        turned.update(&input, [None; 2], true, 0.016);
        for i in 0..2 {
            let expected = input.head.position
                + input
                    .head
                    .rotation
                    .rotate_vector(original[i] - InputContext::default().head.position);
            assert!((turned.centers.unwrap()[i] - expected).magnitude() < 0.0001);
        }
    }

    #[test]
    fn failed_deposit_retains_item_until_a_real_regrip() {
        let (mut tracker, mut input, item) = fixture();
        tracker.retain(1, item);
        tracker.update(&input, [None, Some(item)], false, 0.016);
        assert!(tracker.keep_grip(1));
        input.right_hand.squeeze_value = 1.0;
        tracker.update(&input, [None, Some(item)], true, 0.016);
        assert!(!tracker.keep_grip(1));
        tracker.retain(1, item);
        tracker.update(&input, [None; 2], true, 0.016);
        assert!(!tracker.keep_grip(1));
    }

    #[test]
    fn simultaneous_deposits_reserve_different_cells_and_refuse_overflow() {
        let mut world = World::new();
        let inventory = world.add_entity((
            Links { to_links: vec![] },
            PropContainDimensions {
                width: 2,
                height: 1,
            },
        ));
        let left = world.add_entity((PropInventoryDimensions {
            width: 1,
            height: 1,
        },));
        let right = world.add_entity((PropInventoryDimensions {
            width: 2,
            height: 1,
        },));
        assert_eq!(
            reserve_slots(&world, inventory, [Some(left), Some(right)]),
            [Some((0, 0)), None]
        );
        world.add_component(
            right,
            PropInventoryDimensions {
                width: 1,
                height: 1,
            },
        );
        assert_eq!(
            reserve_slots(&world, inventory, [Some(left), Some(right)]),
            [Some((0, 0)), Some((1, 0))]
        );
    }
}
