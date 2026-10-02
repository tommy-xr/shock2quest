//! Physical access to the existing powered equipment slots. Installed items
//! retain their backpack ownership, energy, script state and save identity.
use super::body_inventory::RetainedRelease;
use crate::input_context::InputContext;
use cgmath::{InnerSpace, Matrix4, Vector3};
use shipyard::{EntityId, World};

#[derive(Clone, Copy)]
pub(super) enum Action {
    Install { entity: EntityId, slot: usize },
    Remove { entity: EntityId },
    Refuse { reason: &'static str },
}

pub(super) struct Sockets {
    pressed: [bool; 2],
    pressed_item: [Option<EntityId>; 2],
    pub retained: RetainedRelease,
    pub near: [bool; 2],
}

impl Default for Sockets {
    fn default() -> Self {
        Self {
            pressed: [true; 2],
            pressed_item: [None; 2],
            retained: Default::default(),
            near: [false; 2],
        }
    }
}

impl Sockets {
    pub fn update(
        &mut self,
        world: &World,
        input: &InputContext,
        held: [Option<EntityId>; 2],
        palms: [Vector3<f32>; 2],
        frames: [Option<Matrix4<f32>>; 2],
        enabled: bool,
    ) -> [Option<Action>; 2] {
        let hands = [&input.left_hand, &input.right_hand];
        self.retained.update(held, hands.map(|h| h.squeeze_value));
        let occupants = crate::implants::equipped(world);
        let mut actions = [None; 2];
        for i in 0..2 {
            let slot = 1 - i;
            let pressed = hands[i].squeeze_value > 0.5;
            let tracked = enabled
                && input
                    .pose_tracking
                    .is_none_or(|p| p.head && p.hands[0] && p.hands[1])
                && hands.iter().all(|hand| {
                    crate::vr_support::GripPose {
                        position: hand.position,
                        rotation: hand.rotation,
                    }
                    .is_tracked()
                });
            self.near[i] = tracked
                && frames[slot].is_some_and(|frame| {
                    (palms[i] - frame.w.truncate()).magnitude2() < 0.09_f32.powi(2)
                });
            if self.near[i] {
                if let Some(entity) =
                    held[i].filter(|id| crate::implants::kind(world, *id).is_some())
                {
                    if !pressed && self.pressed_item[i] == Some(entity) {
                        let duplicate = actions.iter().any(|action| matches!(action,
                            Some(Action::Install { entity: other, .. })
                                if crate::implants::kind(world, *other) == crate::implants::kind(world, entity)));
                        let validation = if duplicate {
                            Err("Two implants of the same type cannot be equipped together.")
                        } else {
                            crate::implants::validate_socket(world, entity, slot)
                        };
                        actions[i] = Some(match validation {
                            Ok(()) => Action::Install { entity, slot },
                            Err(reason) => {
                                self.retained.retain(i, entity);
                                Action::Refuse { reason }
                            }
                        });
                    }
                } else if held[i].is_none() && pressed && !self.pressed[i] {
                    actions[i] = occupants[slot].map(|entity| Action::Remove { entity });
                }
            }
            self.pressed[i] = !tracked || pressed;
            self.pressed_item[i] = if tracked && pressed { held[i] } else { None };
        }
        actions
    }

    pub fn render(
        &self,
        asset_cache: &mut engine::assets::asset_cache::AssetCache,
        world: &World,
        frames: [Option<Matrix4<f32>>; 2],
    ) -> Vec<engine::scene::SceneObject> {
        use crate::hud::virtual_arms::{BIO_BAND_RADIUS, IMPLANT_SLOT_WIDTH};
        let mut objects = Vec::new();
        for (slot, frame) in frames.into_iter().enumerate() {
            let Some(frame) = frame else {
                continue;
            };
            let canvas = crate::hud::implant_slot::canvas(world, slot);
            objects.extend(canvas.render_world_space_bent(
                asset_cache,
                frame * Matrix4::from_scale(IMPLANT_SLOT_WIDTH),
                BIO_BAND_RADIUS / IMPLANT_SLOT_WIDTH,
                0.0003 / IMPLANT_SLOT_WIDTH,
            ));
        }
        crate::util::tag_render_source(&mut objects, crate::util::render_source::PLAYER_HANDS);
        objects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mission::PlayerInfo, quest_info::QuestInfo, runtime_props::RuntimePropImplantSlot,
    };
    use cgmath::{Quaternion, vec3};
    use dark::properties::{Link, Links, PropEnergy, PropImplantDesc, ToLink, WrappedEntityId};

    fn fixture() -> (World, EntityId, InputContext) {
        let mut world = World::new();
        let implant = world.add_entity((PropImplantDesc(0), PropEnergy(100.0)));
        let player = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(implant)),
                link: Link::Contains(0),
            }],
        });
        world.add_unique(PlayerInfo {
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: Some(implant),
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
        });
        world.add_unique(QuestInfo::new());
        let mut input = InputContext::default();
        input.left_hand.rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        input.right_hand.rotation = input.left_hand.rotation;
        (world, implant, input)
    }

    fn update(
        s: &mut Sockets,
        world: &World,
        input: &InputContext,
        held: [Option<EntityId>; 2],
    ) -> [Option<Action>; 2] {
        s.update(
            world,
            input,
            held,
            [vec3(0.0, 0.0, 0.0); 2],
            [Some(Matrix4::from_scale(1.0)); 2],
            true,
        )
    }

    #[test]
    fn requires_release_to_install_and_fresh_opposite_grip_to_remove() {
        let (mut world, implant, mut input) = fixture();
        let mut s = Sockets::default();
        let held = [None, Some(implant)];
        assert!(update(&mut s, &world, &input, held)[1].is_none());
        input.right_hand.squeeze_value = 1.0;
        assert!(update(&mut s, &world, &input, held)[1].is_none());
        input.right_hand.squeeze_value = 0.0;
        assert!(
            matches!(update(&mut s, &world, &input, held)[1], Some(Action::Install { entity, slot:0 }) if entity == implant)
        );
        world.add_component(implant, RuntimePropImplantSlot(0));
        update(&mut s, &world, &input, [None; 2]);
        input.right_hand.squeeze_value = 1.0;
        assert!(
            matches!(update(&mut s, &world, &input, [None;2])[1], Some(Action::Remove { entity }) if entity == implant)
        );
        assert!(update(&mut s, &world, &input, [None; 2])[1].is_none());
    }

    #[test]
    fn locked_socket_refuses_and_tracking_recovery_cannot_invent_a_release() {
        let (world, implant, mut input) = fixture();
        let mut s = Sockets::default();
        let held = [Some(implant), None];
        input.left_hand.squeeze_value = 1.0;
        update(&mut s, &world, &input, held);
        input.left_hand.squeeze_value = 0.0;
        assert!(matches!(
            update(&mut s, &world, &input, held)[0],
            Some(Action::Refuse { .. })
        ));
        assert!(s.retained.keep_grip(0));
        input.left_hand.squeeze_value = 1.0;
        update(&mut s, &world, &input, held);
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [false, true],
        });
        update(&mut s, &world, &input, held);
        input.left_hand.squeeze_value = 0.0;
        input.pose_tracking = None;
        assert!(update(&mut s, &world, &input, held)[0].is_none());
    }
}
