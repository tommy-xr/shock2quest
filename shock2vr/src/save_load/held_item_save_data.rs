use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

use super::EntitySaveData;

#[derive(Debug, Clone, PartialEq)]
pub enum HeldItemError {
    InvalidEntityId(u64),
}

impl std::fmt::Display for HeldItemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeldItemError::InvalidEntityId(id) => write!(f, "Invalid entity ID: {}", id),
        }
    }
}

impl std::error::Error for HeldItemError {}

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
    ) -> Result<(Option<EntityId>, Option<EntityId>, Option<EntityId>), HeldItemError> {
        let (_, entity_id_map) = self.held_entities.instantiate(world);

        let mut left_hand_entity_id = None;
        let mut right_hand_entity_id = None;
        let mut inventory_entity_id = None;

        if let Some(ent) = self.entity_in_left_hand {
            let entity_id = EntityId::from_inner(ent).ok_or(HeldItemError::InvalidEntityId(ent))?;
            if let Some(new_entity_id) = entity_id_map.get(&entity_id) {
                left_hand_entity_id = Some(*new_entity_id);
            }
        }

        if let Some(ent) = self.entity_in_right_hand {
            let entity_id = EntityId::from_inner(ent).ok_or(HeldItemError::InvalidEntityId(ent))?;
            if let Some(new_entity_id) = entity_id_map.get(&entity_id) {
                right_hand_entity_id = Some(*new_entity_id);
            }
        }

        if let Some(ent) = self.inventory_entity {
            let entity_id = EntityId::from_inner(ent).ok_or(HeldItemError::InvalidEntityId(ent))?;
            if let Some(new_entity_id) = entity_id_map.get(&entity_id) {
                inventory_entity_id = Some(*new_entity_id);
            }
        }

        Ok((
            left_hand_entity_id,
            right_hand_entity_id,
            inventory_entity_id,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shipyard::World;

    #[test]
    fn test_instantiate_with_valid_entities() {
        let mut world = World::new();

        // Create a valid entity first
        let valid_entity = world.add_entity(());
        let valid_entity_inner = valid_entity.inner();

        // Create save data with valid entity IDs
        let save_data = HeldItemSaveData {
            held_entities: EntitySaveData::empty(),
            entity_in_left_hand: Some(valid_entity_inner),
            entity_in_right_hand: None,
            inventory_entity: None,
        };

        // This should succeed (though entity mapping may not exist)
        let result = save_data.instantiate(&mut world);
        assert!(result.is_ok());

        let (left, right, inventory) = result.unwrap();
        // Since we don't have proper entity mapping, these should be None
        assert_eq!(left, None);
        assert_eq!(right, None);
        assert_eq!(inventory, None);
    }

    #[test]
    fn test_instantiate_with_invalid_entity_fails() {
        let mut world = World::new();

        // Create save data with entity ID 0, which EntityId::from_inner treats as invalid
        let save_data = HeldItemSaveData {
            held_entities: EntitySaveData::empty(),
            entity_in_left_hand: Some(0), // EntityId::from_inner(0) returns None
            entity_in_right_hand: None,
            inventory_entity: None,
        };

        // This should fail with our custom error
        let result = save_data.instantiate(&mut world);
        assert!(result.is_err());

        match result.unwrap_err() {
            HeldItemError::InvalidEntityId(id) => assert_eq!(id, 0),
        }
    }

    #[test]
    fn test_instantiate_with_no_entities() {
        let mut world = World::new();

        let save_data = HeldItemSaveData::empty();

        let result = save_data.instantiate(&mut world);
        assert!(result.is_ok());

        let (left, right, inventory) = result.unwrap();
        assert_eq!(left, None);
        assert_eq!(right, None);
        assert_eq!(inventory, None);
    }
}
