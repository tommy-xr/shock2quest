use dark::properties::PropKeySrc;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{mission::PlayerInfo, physics::PhysicsWorld};

use super::{Effect, MessagePayload, Script, script_util::set_quest_bit_effect};

pub struct FrobQB {}
impl FrobQB {
    pub fn new() -> FrobQB {
        FrobQB {}
    }
}
impl Script for FrobQB {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => match set_quest_bit_effect(world, entity_id) {
                Some(quest_bit_effect) => {
                    // PropKeySrc entities always receive `internal_keycard`.
                    // That script owns both access acquisition and the card's
                    // physical transfer/consumption, so FrobQB must contribute
                    // only the authored quest bit. Otherwise the two scripts
                    // would both reparent (or destroy) the same entity.
                    let is_keycard = world
                        .borrow::<View<PropKeySrc>>()
                        .map(|keycards| keycards.get(entity_id).is_ok())
                        .unwrap_or(false);
                    if is_keycard {
                        return quest_bit_effect;
                    }

                    // What happens to the object after the quest bit is awarded
                    // is decided by its `PropFrobInfo`, not by this script: a
                    // pickup item (`world_action` with MOVE/USE_AMMO - the
                    // Engineering circuit board, the access key cards, the Big
                    // Bomb) is *taken*, so it goes into the player's backpack
                    // through the same `DropEntityInfo` transfer the grab and
                    // loot-container paths use. Everything else (buttons,
                    // corpses, computers) is used in place and consumed, as
                    // before. `can_grab_item` is the shared eligibility rule -
                    // reparenting anything else would corrupt it.
                    //
                    // A take-able object only goes to the backpack if there is
                    // actually one to put it in. With no `PlayerInfo` (a debug
                    // scene) we must still consume it: the award is one-shot,
                    // and leaving the object frobbable would let a second frob
                    // re-fire the other scripts attached to it - `BaseButton`
                    // resends every SwitchLink - after the bit is already set.
                    let backpack = if crate::virtual_hand::can_grab_item(world, entity_id) {
                        world
                            .borrow::<UniqueView<PlayerInfo>>()
                            .map(|player| player.inventory_entity_id)
                            .ok()
                    } else {
                        None
                    };

                    match backpack {
                        Some(parent_entity_id) => Effect::combine(vec![
                            quest_bit_effect,
                            Effect::DropEntityInfo {
                                parent_entity_id,
                                dropped_entity_id: entity_id,
                            },
                        ]),
                        None => Effect::combine(vec![
                            quest_bit_effect,
                            Effect::DestroyEntity { entity_id },
                        ]),
                    }
                }
                None => Effect::NoEffect,
            },
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{
        FrobFlag, Links, PropFrobInfo, PropQuestBitName, PropQuestBitValue, QuestBitValue,
    };
    use shipyard::World;

    use crate::{
        mission::PlayerInfo,
        physics::PhysicsWorld,
        scripts::{Effect, MessagePayload, Script},
    };

    use super::FrobQB;

    /// A world with one `FrobQB` object carrying a quest bit, plus the player
    /// info the inventory transfer resolves the backpack through.
    fn quest_object_world(
        world_action: FrobFlag,
    ) -> (World, shipyard::EntityId, shipyard::EntityId) {
        let mut world = World::new();
        let object = world.add_entity((
            PropQuestBitName("Note_1_10".to_owned()),
            PropQuestBitValue(QuestBitValue::COMPLETE),
            PropFrobInfo {
                world_action,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
        ));
        let player = world.add_entity(());
        let inventory = world.add_entity(Links::empty());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        (world, object, inventory)
    }

    /// The Engineering circuit board (eng1 obj 705) is `MOVE | SCRIPT`: frobbing
    /// it must award its quest bit *and* hand it to the player. Destroying it
    /// strands the eng2 receptor (#560).
    #[test]
    fn frobbing_a_takeable_object_awards_its_quest_bit_and_stores_it() {
        let (world, object, inventory) = quest_object_world(FrobFlag::MOVE | FrobFlag::SCRIPT);

        let effects = Effect::flatten(vec![FrobQB::new().handle_message(
            object,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )]);

        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::SetQuestBit { quest_bit_name, .. } if quest_bit_name == "Note_1_10"
            )),
            "the quest bit must still be awarded, got {effects:?}"
        );
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::DropEntityInfo { parent_entity_id, dropped_entity_id }
                    if *parent_entity_id == inventory && *dropped_entity_id == object
            )),
            "a take-able object must be transferred to the backpack, got {effects:?}"
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::DestroyEntity { .. })),
            "a take-able object must not be destroyed, got {effects:?}"
        );
    }

    #[test]
    fn keycard_frob_qb_leaves_physical_ownership_to_the_keycard_script() {
        use dark::properties::{KeyCard, PropKeySrc};

        let (mut world, object, _inventory) = quest_object_world(FrobFlag::MOVE | FrobFlag::SCRIPT);
        world.add_component(
            object,
            PropKeySrc(KeyCard {
                is_master: false,
                region_id: 32,
                lock_id: 0,
            }),
        );

        let effects = Effect::flatten(vec![FrobQB::new().handle_message(
            object,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )]);

        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::SetQuestBit { quest_bit_name, .. } if quest_bit_name == "Note_1_10"
            )),
            "FrobQB must still award the keycard's authored quest bit, got {effects:?}"
        );
        assert!(
            !effects.iter().any(|effect| matches!(
                effect,
                Effect::DropEntityInfo { .. } | Effect::DestroyEntity { .. }
            )),
            "the internal keycard script must be the sole physical owner, got {effects:?}"
        );
    }

    /// With no `PlayerInfo` (a debug scene) there is no backpack to transfer
    /// into, so a take-able object falls back to consume-on-frob. Leaving it
    /// alive would let a second frob re-fire the other scripts attached to it
    /// (`BaseButton` resends every SwitchLink) after its one-shot award.
    #[test]
    fn a_takeable_object_is_consumed_when_there_is_no_backpack() {
        let mut world = World::new();
        let object = world.add_entity((
            PropQuestBitName("Note_1_10".to_owned()),
            PropQuestBitValue(QuestBitValue::COMPLETE),
            PropFrobInfo {
                world_action: FrobFlag::MOVE | FrobFlag::SCRIPT,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
        ));

        let effects = Effect::flatten(vec![FrobQB::new().handle_message(
            object,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )]);

        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::SetQuestBit { quest_bit_name, .. } if quest_bit_name == "Note_1_10"
            )),
            "the quest bit must be awarded, got {effects:?}"
        );
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::DestroyEntity { entity_id } if *entity_id == object
            )),
            "with no backpack the object must not be left frobbable, got {effects:?}"
        );
    }

    /// Use-in-place objects (corpses, the Shield Computer, plot buttons) have no
    /// MOVE in their world action and keep the consume-on-frob behavior.
    #[test]
    fn frobbing_a_use_only_object_still_consumes_it() {
        let (world, object, _inventory) = quest_object_world(FrobFlag::SCRIPT);

        let effects = Effect::flatten(vec![FrobQB::new().handle_message(
            object,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )]);

        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::SetQuestBit { quest_bit_name, .. } if quest_bit_name == "Note_1_10"
            )),
            "the quest bit must be awarded, got {effects:?}"
        );
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::DestroyEntity { entity_id } if *entity_id == object
            )),
            "a use-only object must still be consumed, got {effects:?}"
        );
    }
}
