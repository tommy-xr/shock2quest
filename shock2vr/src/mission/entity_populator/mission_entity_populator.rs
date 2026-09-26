use cgmath::{Deg, Quaternion, Rotation3};
use dark::gamesys::Difficulty;
use dark::properties::WrappedEntityId;
use dark::properties::{Link, Links, PropDifficultyDestroy, PropDifficultyPermit};
///
/// mission_entity_populator.rs
///
/// An implementation of EntityPopulator that creates entities based on the entity data
/// in a mission file
use shipyard::{Get, IntoIter, View, ViewMut, World};
use std::collections::{HashMap, HashSet};

use dark::ss2_entity_info::SystemShock2EntityInfo;

use crate::mission::entity_creator;

use super::{EntityPopulation, EntityPopulator};

pub struct MissionEntityPopulator {
    difficulty: Difficulty,
}

impl MissionEntityPopulator {
    pub fn create(difficulty: Difficulty) -> MissionEntityPopulator {
        MissionEntityPopulator { difficulty }
    }
}

impl EntityPopulator for MissionEntityPopulator {
    fn populate(
        &self,
        gamesys_entity_info: &SystemShock2EntityInfo,
        level_entity_info: &SystemShock2EntityInfo,

        obj_name_map: &HashMap<i32, String>, // name override map
        world: &mut World,
    ) -> EntityPopulation {
        let mut template_to_entity_id = HashMap::new();
        let mut all_entities = Vec::new();
        for (template_id, _props) in &level_entity_info.entity_to_properties {
            // Create the entity
            let entity = world.add_entity(());
            template_to_entity_id.insert(*template_id, WrappedEntityId(entity));

            all_entities.push((*template_id, entity))
        }

        // Second pass - hydrate properties
        for (template_id, _props) in &level_entity_info.entity_to_properties {
            let entity = template_to_entity_id.get(template_id).unwrap();
            entity_creator::initialize_entity_with_props(
                *template_id,
                gamesys_entity_info,
                world,
                entity.0,
                &obj_name_map,
            );
        }

        // HACK: If the entity is an 'AI' entity, it is rotated 90 degrees to the left.
        // This is a hack to fix that. Probably a bug somewhere else in the pipeline...
        // hopefully we can find/fix the root cause and remove this hack.
        hack_rotate_ai_entities(world);

        // Third pass - initialize links for entity
        for (template_id, entity_id) in all_entities {
            entity_creator::initialize_links_for_entity(
                template_id,
                entity_id,
                gamesys_entity_info,
                &template_to_entity_id,
                world,
            );
        }

        // Properties and containment are inherited before evaluation, but no
        // scripts or physics have been instantiated yet. Saves use a separate
        // populator, so their already-filtered object set is never reconsidered.
        filter_difficulty(world, &mut template_to_entity_id, self.difficulty);

        EntityPopulation {
            template_to_entity_id,
            entity_id_map: HashMap::new(),
            script_states: Vec::new(),
        }
    }
}

fn filter_difficulty(
    world: &mut World,
    objects: &mut HashMap<i32, WrappedEntityId>,
    difficulty: Difficulty,
) {
    let bit = 1u32 << difficulty.retail_index();
    let mut excluded = HashSet::new();
    {
        let destroy = world.borrow::<View<PropDifficultyDestroy>>().unwrap();
        let permit = world.borrow::<View<PropDifficultyPermit>>().unwrap();
        for (&id, entity) in objects.iter().filter(|(id, _)| **id > 0) {
            if destroy.get(entity.0).is_ok_and(|p| p.0 & bit != 0)
                || permit.get(entity.0).is_ok_and(|p| p.0 & bit == 0)
            {
                excluded.insert(id);
            }
        }
    }
    // Destroying a container also destroys its contents, transitively. Include
    // cycle protection for malformed containment graphs.
    {
        let links = world.borrow::<View<Links>>().unwrap();
        let mut pending: Vec<_> = excluded.iter().copied().collect();
        while let Some(id) = pending.pop() {
            if let Some(entity) = objects.get(&id) {
                if let Ok(links) = links.get(entity.0) {
                    for link in &links.to_links {
                        if matches!(link.link, Link::Contains(_))
                            && link.to_template_id > 0
                            && excluded.insert(link.to_template_id)
                        {
                            pending.push(link.to_template_id);
                        }
                    }
                }
            }
        }
    }
    for id in &excluded {
        if let Some(entity) = objects.remove(id) {
            world.delete_entity(entity.0);
        }
    }
    // Remove incoming links as well: a removed concrete target must not become
    // a deferred spawn/reference. Abstract template links remain available.
    let mut links = world.borrow::<ViewMut<Links>>().unwrap();
    for links in (&mut links).iter() {
        links
            .to_links
            .retain(|link| !excluded.contains(&link.to_template_id));
    }
}

#[cfg(test)]
mod difficulty_tests {
    use super::*;
    use dark::properties::ToLink;
    #[test]
    fn difficulty_masks_remove_nested_contents_and_incoming_links() {
        for difficulty in Difficulty::ALL {
            let mut world = World::new();
            let container = world.add_entity((PropDifficultyDestroy(0x10),));
            let child = world.add_entity(());
            let grandchild = world.add_entity(());
            let donor = world.add_entity((PropDifficultyDestroy(0x1e),));
            let hard_only = world.add_entity((PropDifficultyPermit(0x38),));
            let playtest_only = world.add_entity((PropDifficultyPermit(1),));
            let survivor = world.add_entity(());
            let link = |id, entity, kind| ToLink {
                to_template_id: id,
                to_entity_id: Some(WrappedEntityId(entity)),
                link: kind,
            };
            world.add_component(
                container,
                (Links {
                    to_links: vec![link(2, child, Link::Contains(0))],
                },),
            );
            world.add_component(
                child,
                (Links {
                    to_links: vec![link(3, grandchild, Link::Contains(0))],
                },),
            );
            world.add_component(
                survivor,
                (Links {
                    to_links: vec![
                        link(1, container, Link::Contains(0)),
                        link(-1, donor, Link::Contains(1)),
                    ],
                },),
            );
            let mut objects = HashMap::from([
                (1, WrappedEntityId(container)),
                (2, WrappedEntityId(child)),
                (3, WrappedEntityId(grandchild)),
                (-1, WrappedEntityId(donor)),
                (4, WrappedEntityId(hard_only)),
                (5, WrappedEntityId(playtest_only)),
                (6, WrappedEntityId(survivor)),
            ]);
            filter_difficulty(&mut world, &mut objects, difficulty);
            for id in [1, 2, 3] {
                assert_eq!(
                    objects.contains_key(&id),
                    difficulty != Difficulty::Impossible
                );
            }
            assert!(objects.contains_key(&-1));
            assert!(!objects.contains_key(&5));
            assert_eq!(
                objects.contains_key(&4),
                matches!(difficulty, Difficulty::Hard | Difficulty::Impossible)
            );
            let links = world.borrow::<View<Links>>().unwrap();
            assert_eq!(
                links.get(survivor).unwrap().to_links.len(),
                if difficulty == Difficulty::Impossible {
                    1
                } else {
                    2
                }
            );
        }
    }
}

fn hack_rotate_ai_entities(world: &mut World) {
    let mut v_prop_pos = world
        .borrow::<ViewMut<dark::properties::PropPosition>>()
        .unwrap();
    let v_prop_ai = world.borrow::<View<dark::properties::PropAI>>().unwrap();

    for (prop_pos, prop_ai) in (&mut v_prop_pos, &v_prop_ai).iter() {
        // Ignore some ai...
        if prop_ai.0.eq_ignore_ascii_case("turret") || prop_ai.0.eq_ignore_ascii_case("camera") {
            continue;
        }
        let rotation = prop_pos.rotation;
        prop_pos.rotation = rotation * Quaternion::from_angle_y(Deg(-90.0))
    }
}
