use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

use super::EntitySaveData;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct HeldItemSaveData {
    pub held_entities: EntitySaveData,
    pub entity_in_left_hand: Option<u64>,
    pub entity_in_right_hand: Option<u64>,
    pub inventory_entity: Option<u64>,
}

impl HeldItemSaveData {
    pub fn empty() -> HeldItemSaveData {
        HeldItemSaveData {
            held_entities: EntitySaveData::empty(),
            entity_in_left_hand: None,
            entity_in_right_hand: None,
            inventory_entity: None,
        }
    }

    pub fn instantiate(
        &self,
        world: &mut World,
    ) -> (Option<EntityId>, Option<EntityId>, Option<EntityId>) {
        let (_, entity_id_map) = self.held_entities.instantiate(world);

        let mut left_hand_entity_id = None;
        let mut right_hand_entity_id = None;
        let mut inventory_entity_id = None;

        if let Some(ent) = self.entity_in_left_hand {
            if let Some(new_entity_id) = entity_id_map.get(&EntityId::from_inner(ent).unwrap()) {
                left_hand_entity_id = Some(*new_entity_id);
            }
        }

        if let Some(ent) = self.entity_in_right_hand {
            if let Some(new_entity_id) = entity_id_map.get(&EntityId::from_inner(ent).unwrap()) {
                right_hand_entity_id = Some(*new_entity_id);
            }
        }

        if let Some(ent) = self.inventory_entity {
            if let Some(new_entity_id) = entity_id_map.get(&EntityId::from_inner(ent).unwrap()) {
                inventory_entity_id = Some(*new_entity_id);
            }
        }

        (
            left_hand_entity_id,
            right_hand_entity_id,
            inventory_entity_id,
        )
    }
}
