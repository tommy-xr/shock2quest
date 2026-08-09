use shipyard::{EntityId, UniqueView, World};

use crate::{mission::PlayerInfo, physics::PhysicsWorld};

use super::{Effect, MessagePayload, Script, script_util, script_util::set_quest_bit_effect};

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
                    // What happens to the object after the quest bit is awarded
                    // is decided by its `PropFrobInfo`, not by this script. A
                    // pickup item (`world_action` with MOVE/USE_AMMO - the
                    // Engineering circuit board, the access key cards, the Big
                    // Bomb) is *taken*, so it goes into the player's backpack
                    // through the same `DropEntityInfo` transfer the grab and
                    // loot-container paths use. A use-in-place host (buttons,
                    // corpses, computers) remains alive: its sibling scripts
                    // own the host lifetime and may need it after this effect
                    // batch, notably `ContainerScript`'s open loot panel.
                    // `can_grab_item` is the shared eligibility rule -
                    // reparenting anything else would corrupt it.
                    //
                    // A take-able object only goes to the backpack if there is
                    // actually one to put it in. With no `PlayerInfo` (a debug
                    // scene) we must still consume it: the award is one-shot,
                    // and leaving the object frobbable would let a second frob
                    // re-fire the other scripts attached to it - `BaseButton`
                    // resends every SwitchLink - after the bit is already set.
                    //
                    // A key source (the MedSci2 R&D card, obj 772: `MOVE |
                    // SCRIPT` + `FrobQB` + a derived `internal_keycard`) is the
                    // exception: its sibling keycard script registers it on the
                    // keyring and consumes it, so awarding the bit here is all
                    // this script does. Transferring it too would emit both a
                    // backpack transfer and a destroy for one Frob.
                    if crate::virtual_hand::can_grab_item(world, entity_id)
                        && !script_util::is_key_source(world, entity_id)
                    {
                        match world.borrow::<UniqueView<PlayerInfo>>() {
                            Ok(player) => Effect::combine(vec![
                                quest_bit_effect,
                                Effect::DropEntityInfo {
                                    parent_entity_id: player.inventory_entity_id,
                                    dropped_entity_id: entity_id,
                                },
                            ]),
                            Err(_) => Effect::combine(vec![
                                quest_bit_effect,
                                Effect::DestroyEntity { entity_id },
                            ]),
                        }
                    } else {
                        quest_bit_effect
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
    use shipyard::{EntitiesView, World};

    use crate::{
        gui::gui_script,
        mission::PlayerInfo,
        physics::PhysicsWorld,
        scripts::{CompositeScript, ContainerGui, Effect, MessagePayload, Script, script_util},
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

    /// The MedSci2 R&D card (obj 772) is `MOVE | SCRIPT` with `FrobQB` *and* a
    /// derived `internal_keycard`. One Frob must award `rdgrab` once and leave
    /// the card's fate to the keycard script (register-and-vanish) - emitting a
    /// backpack transfer here would stash and destroy the same card.
    #[test]
    fn frobbing_a_quest_key_card_awards_its_bit_and_leaves_its_fate_to_the_keycard_script() {
        let (mut world, object, _inventory) = quest_object_world(FrobFlag::MOVE | FrobFlag::SCRIPT);
        world.add_component(
            object,
            dark::properties::PropKeySrc(dark::properties::KeyCard {
                is_master: false,
                region_id: 4,
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
            "the quest bit must still be awarded, got {effects:?}"
        );
        assert!(
            !effects.iter().any(|effect| matches!(
                effect,
                Effect::DropEntityInfo { .. } | Effect::DestroyEntity { .. }
            )),
            "a key source's fate belongs to internal_keycard, got {effects:?}"
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
    /// MOVE in their world action. `FrobQB` contributes the quest transition but
    /// leaves their lifetime to their sibling scripts.
    #[test]
    fn frobbing_a_use_only_object_leaves_its_lifetime_to_sibling_scripts() {
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
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::DestroyEntity { entity_id } if *entity_id == object)),
            "FrobQB must not consume a sibling-owned use-only host, got {effects:?}"
        );
    }

    /// Rickenbacker 1 corpse 1636 is an authored composite host: its
    /// `ContainerScript` owns the loot panel while `FrobQB` awards Note_7_8.
    /// One Frob must not tear down that shared host (and its Contains links)
    /// after opening the panel.
    #[test]
    fn frobbing_a_quest_container_opens_it_without_destroying_its_contents() {
        let mut world = World::new();
        let card = world.add_entity(());
        let corpse = world.add_entity((
            PropQuestBitName("Note_7_8".to_owned()),
            PropQuestBitValue(QuestBitValue::COMPLETE),
            PropFrobInfo {
                world_action: FrobFlag::SCRIPT,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
            Links {
                to_links: vec![dark::properties::ToLink {
                    to_template_id: 1779,
                    to_entity_id: Some(dark::properties::WrappedEntityId(card)),
                    link: dark::properties::Link::Contains(0),
                }],
            },
        ));
        let mut scripts = CompositeScript::new(vec![
            gui_script(Box::new(ContainerGui::loot_container())),
            Box::new(FrobQB::new()),
        ]);

        let effects = Effect::flatten(vec![scripts.handle_message(
            corpse,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )]);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::OpenPanel { entity } if *entity == corpse)),
            "the container sibling must open its panel, got {effects:?}"
        );
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::SetQuestBit { quest_bit_name, .. } if quest_bit_name == "Note_7_8"
            )),
            "FrobQB must still award the corpse's quest bit, got {effects:?}"
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::DestroyEntity { entity_id } if *entity_id == corpse)),
            "FrobQB must not destroy a use-in-place host owned by sibling scripts, got {effects:?}"
        );

        // Apply the only lifetime-changing effect relevant to this regression.
        // Production handles OpenPanel/SetQuestBit without modifying the host;
        // DestroyEntity is what removed the corpse and orphaned its children.
        for effect in &effects {
            if matches!(effect, Effect::DestroyEntity { entity_id } if *entity_id == corpse) {
                world.delete_entity(corpse);
            }
        }

        assert!(
            world.borrow::<EntitiesView>().unwrap().is_alive(corpse),
            "the shared quest/container host must survive the effect batch"
        );
        let contains = script_util::get_all_links_with_data(&world, corpse, |link| match link {
            dark::properties::Link::Contains(_) => Some(()),
            _ => None,
        })
        .into_iter()
        .map(|(entity, ())| entity)
        .collect::<Vec<_>>();
        assert_eq!(
            contains,
            vec![card],
            "the corpse must retain its authored Contains link"
        );

        // Setting the same Dark quest value is idempotent. A later Frob may
        // reopen a sibling-owned panel, but FrobQB must never start owning the
        // persistent host's lifetime on a repeat interaction.
        let repeat = Effect::flatten(vec![scripts.handle_message(
            corpse,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )]);
        assert!(repeat.iter().any(|effect| matches!(
            effect,
            Effect::SetQuestBit { quest_bit_name, .. } if quest_bit_name == "Note_7_8"
        )));
        assert!(
            !repeat
                .iter()
                .any(|effect| matches!(effect, Effect::DestroyEntity { entity_id } if *entity_id == corpse)),
            "repeat Frob must remain an idempotent quest transition, got {repeat:?}"
        );
        assert_eq!(
            script_util::get_all_links_with_data(&world, corpse, |link| match link {
                dark::properties::Link::Contains(_) => Some(()),
                _ => None,
            })
            .len(),
            1,
            "repeat Frob must leave the quest container's contents intact"
        );
    }
}
