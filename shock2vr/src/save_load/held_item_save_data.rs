use std::collections::HashMap;

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

    /// Instantiate held entities while conservatively migrating saves written
    /// before canonical archetype provenance was persisted.
    ///
    /// Old saves still contain each entity's effective `P$SymName`. Only an
    /// exact name with one unique negative gamesys template is accepted;
    /// ambiguous and mission-specific names remain unresolved.
    pub fn instantiate_with_legacy_template_names(
        &self,
        world: &mut World,
        unique_gamesys_templates: &HashMap<String, i32>,
    ) -> (Option<EntityId>, Option<EntityId>, Option<EntityId>) {
        let mut migrated = self.clone();
        backfill_legacy_canonical_template_ids(
            &mut migrated.held_entities,
            unique_gamesys_templates,
        );
        migrated.instantiate(world)
    }
}

pub(crate) fn backfill_legacy_canonical_template_ids(
    entities: &mut EntitySaveData,
    unique_gamesys_templates: &HashMap<String, i32>,
) {
    let Some(saved_names) = entities.properties.get("P$SymName") else {
        return;
    };
    for (entity_id, saved_name) in saved_names {
        if entities.canonical_template_ids.contains_key(entity_id) {
            continue;
        }
        let Some(name) = saved_name.as_str() else {
            continue;
        };
        if let Some(template_id) = unique_gamesys_templates.get(name) {
            entities
                .canonical_template_ids
                .insert(*entity_id, *template_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_entity(name: &str) -> EntitySaveData {
        let old_entity = EntityId::new_from_index_and_gen(12, 1);
        let mut entities = EntitySaveData::empty();
        entities.all_entities.push(old_entity.inner());
        entities.properties.insert(
            "P$SymName".to_owned(),
            HashMap::from([(old_entity.inner(), serde_json::json!(name))]),
        );
        entities
    }

    #[test]
    fn legacy_backfill_requires_an_exact_unique_gamesys_name() {
        let mut exact = legacy_entity("Pistol");
        backfill_legacy_canonical_template_ids(
            &mut exact,
            &HashMap::from([("Pistol".to_owned(), -17)]),
        );
        assert_eq!(
            exact
                .canonical_template_ids
                .values()
                .copied()
                .collect::<Vec<_>>(),
            vec![-17]
        );

        let mut different_case = legacy_entity("pistol");
        backfill_legacy_canonical_template_ids(
            &mut different_case,
            &HashMap::from([("Pistol".to_owned(), -17)]),
        );
        assert!(different_case.canonical_template_ids.is_empty());

        let mut unmatched = legacy_entity("Mission Specific Object");
        backfill_legacy_canonical_template_ids(&mut unmatched, &HashMap::new());
        assert!(unmatched.canonical_template_ids.is_empty());
    }
}
