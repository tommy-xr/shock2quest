use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

use super::EntitySaveData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeldItemSlot {
    LeftHand,
    RightHand,
    Inventory,
}

impl std::fmt::Display for HeldItemSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            HeldItemSlot::LeftHand => "left hand",
            HeldItemSlot::RightHand => "right hand",
            HeldItemSlot::Inventory => "inventory",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HeldItemError {
    InvalidEntityId { slot: HeldItemSlot, raw_id: u64 },
    MissingEntityMapping { slot: HeldItemSlot, raw_id: u64 },
}

impl std::fmt::Display for HeldItemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeldItemError::InvalidEntityId { slot, raw_id } => {
                write!(f, "Invalid entity ID ({slot}): {raw_id}")
            }
            HeldItemError::MissingEntityMapping { slot, raw_id } => write!(
                f,
                "Missing remapped entity for {slot} (original id: {raw_id})"
            ),
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

        let left_hand_entity_id = Self::remap_entity(
            self.entity_in_left_hand,
            HeldItemSlot::LeftHand,
            &entity_id_map,
        )?;

        let right_hand_entity_id = Self::remap_entity(
            self.entity_in_right_hand,
            HeldItemSlot::RightHand,
            &entity_id_map,
        )?;

        let inventory_entity_id = Self::remap_entity(
            self.inventory_entity,
            HeldItemSlot::Inventory,
            &entity_id_map,
        )?;

        Ok((
            left_hand_entity_id,
            right_hand_entity_id,
            inventory_entity_id,
        ))
    }

    fn remap_entity(
        entity: Option<u64>,
        slot: HeldItemSlot,
        entity_id_map: &std::collections::HashMap<EntityId, EntityId>,
    ) -> Result<Option<EntityId>, HeldItemError> {
        let raw_id = match entity {
            Some(id) => id,
            None => return Ok(None),
        };

        let entity_id =
            EntityId::from_inner(raw_id).ok_or(HeldItemError::InvalidEntityId { slot, raw_id })?;

        let remapped = entity_id_map
            .get(&entity_id)
            .copied()
            .ok_or(HeldItemError::MissingEntityMapping { slot, raw_id })?;

        Ok(Some(remapped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shipyard::World;

    #[test]
    fn test_instantiate_with_valid_entities() {
        let mut world = World::new();

        let left_item = world.add_entity(());
        let right_item = world.add_entity(());
        let inventory_item = world.add_entity(());

        let mut held_entities = EntitySaveData::empty();
        held_entities.all_entities = vec![
            left_item.inner(),
            right_item.inner(),
            inventory_item.inner(),
        ];

        let save_data = HeldItemSaveData {
            held_entities,
            entity_in_left_hand: Some(left_item.inner()),
            entity_in_right_hand: Some(right_item.inner()),
            inventory_entity: Some(inventory_item.inner()),
        };

        let (left, right, inventory) = save_data.instantiate(&mut world).unwrap();

        assert!(left.is_some());
        assert!(right.is_some());
        assert!(inventory.is_some());
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
            HeldItemError::InvalidEntityId { slot, raw_id } => {
                assert_eq!(slot, HeldItemSlot::LeftHand);
                assert_eq!(raw_id, 0);
            }
            other => panic!("Unexpected error variant: {:?}", other),
        }
    }

    #[test]
    fn test_instantiate_returns_error_when_mapping_missing() {
        let mut world = World::new();
        let dangling_entity = world.add_entity(());
        let entity_id = dangling_entity.inner();

        let save_data = HeldItemSaveData {
            held_entities: EntitySaveData::empty(),
            entity_in_left_hand: Some(entity_id),
            entity_in_right_hand: None,
            inventory_entity: None,
        };

        let result = save_data.instantiate(&mut world);
        assert!(result.is_err());

        match result.unwrap_err() {
            HeldItemError::MissingEntityMapping { slot, raw_id } => {
                assert_eq!(slot, HeldItemSlot::LeftHand);
                assert_eq!(raw_id, entity_id);
            }
            other => panic!("Unexpected error variant: {:?}", other),
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
