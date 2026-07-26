use std::collections::HashMap;

use dark::{
    properties::{Link, Links, PropGunState, PropStackCount},
    ss2_entity_info::{self, SystemShock2EntityInfo},
};
use shipyard::{EntityId, Get, Unique, UniqueView, View, ViewMut, World};

/// Projectile archetype -> compatible ammo-clip archetypes, authored by the
/// Dark engine's `Clip` relation.
#[derive(Unique, Clone, Default)]
pub(crate) struct GlobalProjectileClips(pub HashMap<i32, Vec<i32>>);

impl GlobalProjectileClips {
    pub fn from_entity_info(entity_info: &SystemShock2EntityInfo) -> Self {
        let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
        let mut projectile_clips = HashMap::new();

        // Relations inherit just like properties. Build the effective Clip
        // relation for every archetype so a concrete projectile child can use a
        // relation authored on its family archetype.
        for template_id in entity_info.entity_to_properties.keys() {
            let mut ancestors = ss2_entity_info::get_ancestors(hierarchy, template_id);
            ancestors.push(*template_id);
            let mut compatible_clips = Vec::new();
            for ancestor in ancestors {
                let Some(links) = entity_info.template_to_links.get(&ancestor) else {
                    continue;
                };
                for clip in links
                    .to_links
                    .iter()
                    .filter(|link| matches!(link.link, Link::Clip))
                {
                    if !compatible_clips.contains(&clip.to_template_id) {
                        compatible_clips.push(clip.to_template_id);
                    }
                }
            }
            if !compatible_clips.is_empty() {
                projectile_clips.insert(*template_id, compatible_clips);
            }
        }

        Self(projectile_clips)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ReloadOutcome {
    pub rounds_loaded: i32,
    pub depleted_items: Vec<EntityId>,
}

/// Move matching reserve rounds from the backpack into `weapon`.
///
pub(crate) fn load_from_reserve(world: &World, weapon: EntityId, capacity: i32) -> ReloadOutcome {
    let current_ammo = world
        .borrow::<View<PropGunState>>()
        .ok()
        .and_then(|states| states.get(weapon).ok().map(|state| state.ammo));
    let Some(current_ammo) = current_ammo else {
        return ReloadOutcome::default();
    };
    let mut rounds_needed = (capacity.max(0) - current_ammo.max(0)).max(0);
    if rounds_needed == 0 {
        return ReloadOutcome::default();
    }

    let projectiles = crate::scripts::script_util::ordered_projectile_links(world, weapon);
    if projectiles.is_empty() {
        return ReloadOutcome::default();
    }
    let selected = world
        .borrow::<View<crate::runtime_props::RuntimePropSelectedAmmo>>()
        .ok()
        .and_then(|selected| selected.get(weapon).ok().map(|selected| selected.0))
        .unwrap_or(0);
    let projectile_template = projectiles[selected % projectiles.len()].0;
    let compatible_clip_templates = world
        .borrow::<UniqueView<GlobalProjectileClips>>()
        .ok()
        .and_then(|clips| clips.0.get(&projectile_template).cloned());
    let Some(compatible_clip_templates) = compatible_clip_templates else {
        return ReloadOutcome::default();
    };

    let inventory = match world.borrow::<UniqueView<crate::mission::PlayerInfo>>() {
        Ok(player) => player.inventory_entity_id,
        Err(_) => return ReloadOutcome::default(),
    };
    let reserve_items: Vec<EntityId> = {
        let Ok(links) = world.borrow::<View<Links>>() else {
            return ReloadOutcome::default();
        };
        let Ok(inventory_links) = links.get(inventory) else {
            return ReloadOutcome::default();
        };
        inventory_links
            .to_links
            .iter()
            .filter(|link| matches!(link.link, Link::Contains(_)))
            .filter_map(|link| link.to_entity_id.map(|id| id.0))
            .collect()
    };

    let matching_reserve: Vec<EntityId> = {
        let Ok((stacks, hierarchy)) = world.borrow::<(
            View<PropStackCount>,
            UniqueView<crate::mission::GlobalTemplateHierarchy>,
        )>() else {
            return ReloadOutcome::default();
        };
        reserve_items
            .into_iter()
            .filter(|item| {
                let Some(class_template_id) =
                    crate::scripts::script_util::entity_class_template_id(world, *item)
                else {
                    return false;
                };
                let compatible = compatible_clip_templates.iter().any(|clip_template| {
                    hierarchy.is_or_descends_from(class_template_id, *clip_template)
                });
                compatible && stacks.get(*item).is_ok_and(|stack| stack.0 > 0)
            })
            .collect()
    };

    let mut outcome = ReloadOutcome::default();
    {
        let Ok(mut stacks) = world.borrow::<ViewMut<PropStackCount>>() else {
            return outcome;
        };
        for item in matching_reserve {
            if rounds_needed == 0 {
                break;
            }
            let Ok(stack) = (&mut stacks).get(item) else {
                continue;
            };
            let consumed = stack.0.min(rounds_needed).max(0);
            stack.0 -= consumed;
            rounds_needed -= consumed;
            outcome.rounds_loaded += consumed;
            if stack.0 == 0 {
                outcome.depleted_items.push(item);
            }
        }
    }

    if outcome.rounds_loaded > 0 {
        let mut states = world.borrow::<ViewMut<PropGunState>>().unwrap();
        if let Ok(state) = (&mut states).get(weapon) {
            state.ammo = (state.ammo.max(0) + outcome.rounds_loaded).min(capacity.max(0));
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mission::{GlobalTemplateHierarchy, PlayerInfo},
        runtime_props::{RuntimePropCanonicalTemplateId, RuntimePropSelectedAmmo},
    };
    use cgmath::{Quaternion, Vector3};
    use dark::properties::{
        Link, Links, ProjectileOptions, PropStackCount, PropTemplateId, ToLink, WrappedEntityId,
    };
    use shipyard::View;

    const PISTOL: i32 = -17;
    const STD_PROJECTILE: i32 = -362;
    const ASSAULT_STD_PROJECTILE: i32 = -2253;
    const HE_PROJECTILE: i32 = -33;
    const PRISM_PROJECTILE: i32 = -232;
    const STD_CLIP: i32 = -31;
    const SMALL_STD_CLIP: i32 = -1358;
    const EARTH_SMALL_STD_CLIP: i32 = 247;
    const HE_CLIP: i32 = -32;
    const SMALL_PRISM: i32 = -41;
    const LARGE_PRISM: i32 = -44;

    struct Fixture {
        world: World,
        weapon: EntityId,
        inventory: EntityId,
    }

    impl Fixture {
        fn new(ammo: i32, selected: usize) -> Self {
            let mut world = World::new();
            let player = world.add_entity(());
            let inventory = world.add_entity((Links::empty(),));
            let weapon = world.add_entity((
                PropTemplateId {
                    template_id: PISTOL,
                },
                PropGunState {
                    ammo,
                    condition: 1.0,
                    setting: 0,
                    modification: 0,
                    silence_value: 0.0,
                },
                RuntimePropSelectedAmmo(selected),
                Links {
                    to_links: vec![
                        ToLink {
                            link: Link::Projectile(ProjectileOptions {
                                order: 0,
                                setting: 0,
                            }),
                            to_entity_id: None,
                            to_template_id: STD_PROJECTILE,
                        },
                        ToLink {
                            link: Link::Projectile(ProjectileOptions {
                                order: 1,
                                setting: 0,
                            }),
                            to_entity_id: None,
                            to_template_id: HE_PROJECTILE,
                        },
                    ],
                },
            ));
            world.add_unique(PlayerInfo {
                pos: Vector3::new(0.0, 0.0, 0.0),
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                entity_id: player,
                left_hand_entity_id: Some(weapon),
                right_hand_entity_id: None,
                inventory_entity_id: inventory,
            });
            world.add_unique(GlobalProjectileClips(HashMap::from([
                (STD_PROJECTILE, vec![STD_CLIP, SMALL_STD_CLIP]),
                (ASSAULT_STD_PROJECTILE, vec![SMALL_STD_CLIP, STD_CLIP]),
                (HE_PROJECTILE, vec![HE_CLIP]),
                (PRISM_PROJECTILE, vec![SMALL_PRISM, LARGE_PRISM]),
            ])));
            world.add_unique(GlobalTemplateHierarchy(HashMap::from([
                (SMALL_STD_CLIP, vec![STD_CLIP]),
                (EARTH_SMALL_STD_CLIP, vec![SMALL_STD_CLIP]),
            ])));
            Self {
                world,
                weapon,
                inventory,
            }
        }

        fn reserve(&mut self, template_id: i32, rounds: i32) -> EntityId {
            let item = self
                .world
                .add_entity((PropTemplateId { template_id }, PropStackCount(rounds)));
            let mut links = self.world.borrow::<ViewMut<Links>>().unwrap();
            (&mut links)
                .get(self.inventory)
                .unwrap()
                .to_links
                .push(ToLink {
                    link: Link::Contains(0),
                    to_entity_id: Some(WrappedEntityId(item)),
                    to_template_id: template_id,
                });
            item
        }

        fn reserve_with_canonical_template(
            &mut self,
            concrete_template_id: i32,
            canonical_template_id: i32,
            rounds: i32,
        ) -> EntityId {
            let item = self.world.add_entity((
                PropTemplateId {
                    template_id: concrete_template_id,
                },
                RuntimePropCanonicalTemplateId(canonical_template_id),
                PropStackCount(rounds),
            ));
            let mut links = self.world.borrow::<ViewMut<Links>>().unwrap();
            (&mut links)
                .get(self.inventory)
                .unwrap()
                .to_links
                .push(ToLink {
                    link: Link::Contains(0),
                    to_entity_id: Some(WrappedEntityId(item)),
                    to_template_id: concrete_template_id,
                });
            item
        }

        fn set_standard_projectile(&mut self, template_id: i32) {
            let mut links = self.world.borrow::<ViewMut<Links>>().unwrap();
            let weapon_links = (&mut links).get(self.weapon).unwrap();
            weapon_links
                .to_links
                .iter_mut()
                .find(|link| {
                    matches!(
                        link.link,
                        Link::Projectile(ProjectileOptions { order: 0, .. })
                    )
                })
                .unwrap()
                .to_template_id = template_id;
        }

        fn ammo(&self) -> i32 {
            self.world
                .borrow::<View<PropGunState>>()
                .unwrap()
                .get(self.weapon)
                .unwrap()
                .ammo
        }

        fn rounds(&self, item: EntityId) -> i32 {
            self.world
                .borrow::<View<PropStackCount>>()
                .unwrap()
                .get(item)
                .unwrap()
                .0
        }
    }

    #[test]
    fn reload_with_no_reserve_does_not_mint_rounds() {
        let fixture = Fixture::new(0, 0);

        let outcome = load_from_reserve(&fixture.world, fixture.weapon, 12);

        assert_eq!(fixture.ammo(), 0);
        assert_eq!(outcome, ReloadOutcome::default());
    }

    #[test]
    fn reload_consumes_exact_reserve_across_real_small_clip_stacks() {
        let mut fixture = Fixture::new(0, 0);
        let first = fixture.reserve(EARTH_SMALL_STD_CLIP, 6);
        let second = fixture.reserve(EARTH_SMALL_STD_CLIP, 6);

        let outcome = load_from_reserve(&fixture.world, fixture.weapon, 12);

        assert_eq!(fixture.ammo(), 12);
        assert_eq!(fixture.rounds(first), 0);
        assert_eq!(fixture.rounds(second), 0);
        assert_eq!(outcome.rounds_loaded, 12);
        assert_eq!(outcome.depleted_items, vec![first, second]);
    }

    #[test]
    fn reload_uses_preserved_archetype_when_concrete_id_collides_across_missions() {
        const SOURCE_SMALL_CLIP_OBJECT: i32 = 1496;
        const DESTINATION_PELLET_BOX: i32 = -42;

        let mut fixture = Fixture::new(0, 0);
        // The destination reuses positive object 1496 for an unrelated pellet
        // box. The carried source object retains 1496 as its concrete identity,
        // while its stable eng1 archetype provenance remains Small Std Clip.
        fixture
            .world
            .add_unique(GlobalTemplateHierarchy(HashMap::from([
                (SMALL_STD_CLIP, vec![STD_CLIP]),
                (SOURCE_SMALL_CLIP_OBJECT, vec![DESTINATION_PELLET_BOX]),
            ])));
        let reserve =
            fixture.reserve_with_canonical_template(SOURCE_SMALL_CLIP_OBJECT, SMALL_STD_CLIP, 6);

        let outcome = load_from_reserve(&fixture.world, fixture.weapon, 12);

        assert_eq!(fixture.ammo(), 6);
        assert_eq!(fixture.rounds(reserve), 0);
        assert_eq!(outcome.rounds_loaded, 6);
        assert_eq!(outcome.depleted_items, vec![reserve]);
    }

    #[test]
    fn reload_migrates_legacy_carried_clip_from_unique_saved_sym_name() {
        use crate::save_load::{EntitySaveData, backfill_legacy_canonical_template_ids};

        const SOURCE_SMALL_CLIP_OBJECT: i32 = 1496;
        const DESTINATION_PELLET_BOX: i32 = -42;

        let mut fixture = Fixture::new(0, 0);
        fixture
            .world
            .add_unique(GlobalTemplateHierarchy(HashMap::from([
                (SMALL_STD_CLIP, vec![STD_CLIP]),
                (SOURCE_SMALL_CLIP_OBJECT, vec![DESTINATION_PELLET_BOX]),
            ])));

        let old_clip = EntityId::new_from_index_and_gen(1496, 0);
        let mut legacy = EntitySaveData::empty();
        legacy.all_entities.push(old_clip.inner());
        legacy.properties.insert(
            "__P$InternalTemplateId".to_owned(),
            HashMap::from([(
                old_clip.inner(),
                serde_json::json!({"template_id": SOURCE_SMALL_CLIP_OBJECT}),
            )]),
        );
        legacy.properties.insert(
            "P$SymName".to_owned(),
            HashMap::from([(old_clip.inner(), serde_json::json!("Small Standard Clip"))]),
        );
        legacy.properties.insert(
            "P$StackCoun".to_owned(),
            HashMap::from([(old_clip.inner(), serde_json::json!(6))]),
        );
        assert!(legacy.canonical_template_ids.is_empty());

        backfill_legacy_canonical_template_ids(
            &mut legacy,
            &HashMap::from([
                ("Pistol".to_owned(), PISTOL),
                ("Small Standard Clip".to_owned(), SMALL_STD_CLIP),
            ]),
        );
        let (_, remapped) = legacy.instantiate(&mut fixture.world);
        let reserve = remapped[&old_clip];
        let mut links = fixture.world.borrow::<ViewMut<Links>>().unwrap();
        (&mut links)
            .get(fixture.inventory)
            .unwrap()
            .to_links
            .push(ToLink {
                link: Link::Contains(0),
                to_entity_id: Some(WrappedEntityId(reserve)),
                to_template_id: SOURCE_SMALL_CLIP_OBJECT,
            });
        drop(links);

        let outcome = load_from_reserve(&fixture.world, fixture.weapon, 12);

        assert_eq!(fixture.ammo(), 6);
        assert_eq!(fixture.rounds(reserve), 0);
        assert_eq!(outcome.rounds_loaded, 6);
        assert_eq!(outcome.depleted_items, vec![reserve]);
    }

    #[test]
    fn reload_accepts_every_authored_assault_standard_clip_target() {
        let mut fixture = Fixture::new(0, 0);
        fixture.set_standard_projectile(ASSAULT_STD_PROJECTILE);
        let small = fixture.reserve(EARTH_SMALL_STD_CLIP, 6);
        let regular = fixture.reserve(STD_CLIP, 6);

        let outcome = load_from_reserve(&fixture.world, fixture.weapon, 12);

        assert_eq!(fixture.ammo(), 12);
        assert_eq!(fixture.rounds(small), 0);
        assert_eq!(fixture.rounds(regular), 0);
        assert_eq!(outcome.rounds_loaded, 12);
        assert_eq!(outcome.depleted_items, vec![small, regular]);
    }

    #[test]
    fn reload_accepts_sibling_small_and_large_prism_targets() {
        let mut fixture = Fixture::new(0, 0);
        fixture.set_standard_projectile(PRISM_PROJECTILE);
        let small = fixture.reserve(SMALL_PRISM, 10);
        let large = fixture.reserve(LARGE_PRISM, 20);

        let outcome = load_from_reserve(&fixture.world, fixture.weapon, 30);

        assert_eq!(fixture.ammo(), 30);
        assert_eq!(fixture.rounds(small), 0);
        assert_eq!(fixture.rounds(large), 0);
        assert_eq!(outcome.rounds_loaded, 30);
        assert_eq!(outcome.depleted_items, vec![small, large]);
    }

    #[test]
    fn partial_reload_consumes_only_missing_rounds() {
        let mut fixture = Fixture::new(7, 0);
        let reserve = fixture.reserve(EARTH_SMALL_STD_CLIP, 6);

        let outcome = load_from_reserve(&fixture.world, fixture.weapon, 12);

        assert_eq!(fixture.ammo(), 12);
        assert_eq!(fixture.rounds(reserve), 1);
        assert_eq!(outcome.rounds_loaded, 5);
        assert!(outcome.depleted_items.is_empty());
    }

    #[test]
    fn mismatched_selected_ammo_does_not_consume_reserve() {
        let mut fixture = Fixture::new(0, 1);
        let standard = fixture.reserve(EARTH_SMALL_STD_CLIP, 6);

        let outcome = load_from_reserve(&fixture.world, fixture.weapon, 12);

        assert_eq!(fixture.ammo(), 0);
        assert_eq!(fixture.rounds(standard), 6);
        assert_eq!(outcome, ReloadOutcome::default());
    }

    #[test]
    fn repeated_reload_cannot_reuse_depleted_reserve() {
        let mut fixture = Fixture::new(0, 0);
        fixture.reserve(EARTH_SMALL_STD_CLIP, 6);

        let first = load_from_reserve(&fixture.world, fixture.weapon, 12);
        let second = load_from_reserve(&fixture.world, fixture.weapon, 12);

        assert_eq!(fixture.ammo(), 6);
        assert_eq!(first.rounds_loaded, 6);
        assert_eq!(second, ReloadOutcome::default());
    }
}
