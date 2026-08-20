use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

use super::EntitySaveData;
use crate::scripts::SavedScriptState;

pub struct HeldItemInstantiation {
    pub left_hand_entity_id: Option<EntityId>,
    pub right_hand_entity_id: Option<EntityId>,
    pub inventory_entity_id: Option<EntityId>,
    pub entity_id_map: HashMap<EntityId, EntityId>,
    pub script_states: Vec<SavedScriptState>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct HeldItemSaveData {
    pub held_entities: EntitySaveData,
    pub entity_in_left_hand: Option<u64>,
    pub entity_in_right_hand: Option<u64>,
    pub inventory_entity: Option<u64>,
    /// Width used to encode backpack `Contains` ordinals in this save.
    /// Saves written before #948 omit it and always used the old fixed width
    /// of 15 columns.
    #[serde(default)]
    pub backpack_width: Option<usize>,
}

impl HeldItemSaveData {
    pub fn empty() -> HeldItemSaveData {
        HeldItemSaveData {
            held_entities: EntitySaveData::empty(),
            entity_in_left_hand: None,
            entity_in_right_hand: None,
            inventory_entity: None,
            backpack_width: None,
        }
    }

    /// Re-encode an instantiated backpack from the width stored by the save to
    /// the character sheet's current usable width. Missing or invalid metadata
    /// is a legacy shock2quest save and therefore used the former fixed 15.
    pub fn remap_instantiated_backpack(
        &self,
        world: &mut World,
        inventory_entity: EntityId,
        current_width: usize,
    ) -> crate::inventory::WidthRemapOutcome {
        let saved_width = self
            .backpack_width
            .filter(|width| {
                (crate::inventory::BACKPACK_MIN_WIDTH..=crate::inventory::BACKPACK_GRID.0)
                    .contains(width)
            })
            .unwrap_or(crate::inventory::BACKPACK_GRID.0);
        crate::inventory::remap_container_width(
            world,
            inventory_entity,
            (saved_width, crate::inventory::BACKPACK_GRID.1),
            (current_width, crate::inventory::BACKPACK_GRID.1),
        )
    }

    pub fn instantiate(
        &self,
        world: &mut World,
    ) -> (Option<EntityId>, Option<EntityId>, Option<EntityId>) {
        let instantiated = self.instantiate_with_script_state(world);
        (
            instantiated.left_hand_entity_id,
            instantiated.right_hand_entity_id,
            instantiated.inventory_entity_id,
        )
    }

    fn instantiate_with_script_state(&self, world: &mut World) -> HeldItemInstantiation {
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

        HeldItemInstantiation {
            left_hand_entity_id,
            right_hand_entity_id,
            inventory_entity_id,
            entity_id_map,
            script_states: self.held_entities.script_states.clone(),
        }
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
    ) -> HeldItemInstantiation {
        let mut migrated = self.clone();
        backfill_legacy_canonical_template_ids(
            &mut migrated.held_entities,
            unique_gamesys_templates,
        );
        migrated.instantiate_with_script_state(world)
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
    use dark::properties::{Link, Links, ToLink, WrappedEntityId};
    use shipyard::{Get, View};

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

    #[test]
    fn legacy_fixed_width_backpack_fixture_migrates_row_ordinals() {
        let mut source = World::new();
        let old_inventory = source.add_entity(());
        let old_items: Vec<_> = (0..3).map(|_| source.add_entity(())).collect();
        let mut held_entities = EntitySaveData::empty();
        held_entities.all_entities = std::iter::once(old_inventory)
            .chain(old_items.iter().copied())
            .map(EntityId::inner)
            .collect();
        held_entities.links.insert(
            old_inventory.inner(),
            serde_json::to_value(Links {
                to_links: old_items
                    .iter()
                    .zip([0, 15, 30])
                    .map(|(item, ordinal)| ToLink {
                        to_template_id: 0,
                        to_entity_id: Some(WrappedEntityId(*item)),
                        link: Link::Contains(ordinal),
                    })
                    .collect(),
            })
            .unwrap(),
        );
        let mut fixture = HeldItemSaveData::empty();
        fixture.held_entities = held_entities;
        fixture.inventory_entity = Some(old_inventory.inner());

        // This JSON shape is exactly what pre-#948 shock2quest wrote: no
        // backpack-width metadata, and row ordinals encoded against width 15.
        let mut legacy_json = serde_json::to_value(fixture).unwrap();
        legacy_json
            .as_object_mut()
            .unwrap()
            .remove("backpack_width");
        let legacy: HeldItemSaveData = serde_json::from_value(legacy_json).unwrap();

        let mut loaded = World::new();
        let instantiated = legacy.instantiate_with_script_state(&mut loaded);
        let inventory = instantiated.inventory_entity_id.unwrap();
        let outcome = legacy.remap_instantiated_backpack(&mut loaded, inventory, 10);
        assert!(outcome.overflow.is_empty());

        let links = loaded.borrow::<View<Links>>().unwrap();
        let mut ordinals: Vec<_> = links
            .get(inventory)
            .unwrap()
            .to_links
            .iter()
            .filter_map(|link| match link.link {
                Link::Contains(ordinal) => Some(ordinal),
                _ => None,
            })
            .collect();
        ordinals.sort_unstable();
        assert_eq!(
            ordinals,
            vec![0, 10, 20],
            "legacy row coordinates survive the 15-column to 10-column migration"
        );
    }
}
