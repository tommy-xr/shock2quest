use std::collections::HashMap;

use dark::{
    properties::{Link, Links, PropGunState, PropStackCount},
    ss2_entity_info::{self, SystemShock2EntityInfo},
};
use shipyard::{EntityId, Get, Unique, UniqueView, View, ViewMut, World};

/// Projectile archetype -> compatible ammo-clip archetypes, authored by the
/// Dark engine's `Clip` relation, plus each clip archetype's authored stack
/// size (its effective `P$StackCount`) so an eject can mint the box that
/// actually fits the rounds.
#[derive(Unique, Clone, Default)]
pub(crate) struct GlobalProjectileClips {
    pub clips: HashMap<i32, Vec<i32>>,
    pub clip_sizes: HashMap<i32, i32>,
}

impl GlobalProjectileClips {
    pub fn from_entity_info(entity_info: &SystemShock2EntityInfo) -> Self {
        let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
        let mut projectile_clips = HashMap::new();

        // Relations inherit just like properties. Build the effective Clip
        // relation for every archetype so a concrete projectile child can use a
        // relation authored on its family archetype.
        //
        // MOST-DERIVED FIRST, and that order is load-bearing: it is an unload's
        // tie-break and its fallback when no clip has an authored size (see
        // [`mint_clip_template`]), so a projectile's OWN authored clip must
        // outrank anything it merely inherits, and the data's own preference
        // must be preserved within a template. `get_ancestors` is root-first,
        // so it is reversed here. Compatibility checks elsewhere only test
        // membership and are indifferent to the order.
        for template_id in entity_info.entity_to_properties.keys() {
            let mut lineage = vec![*template_id];
            lineage.extend(
                ss2_entity_info::get_ancestors(hierarchy, template_id)
                    .into_iter()
                    .rev(),
            );
            let mut compatible_clips = Vec::new();
            for ancestor in lineage {
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

        // Resolve each clip archetype's authored stack size - "Small Standard
        // Clip" holds 6, "Standard Clip" 12 - which is what an eject sizes its
        // minted clip against.
        let mut clip_sizes = HashMap::new();
        for clip in projectile_clips.values().flatten() {
            if !clip_sizes.contains_key(clip) {
                if let Some(stack) = crate::scripts::script_util::hydrate_template_component::<
                    PropStackCount,
                >(*clip, entity_info)
                {
                    clip_sizes.insert(*clip, stack.0);
                }
            }
        }

        Self {
            clips: projectile_clips,
            clip_sizes,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ReloadOutcome {
    pub rounds_loaded: i32,
    pub depleted_items: Vec<EntityId>,
}

/// The clip archetypes that hold `weapon`'s CURRENTLY selected ammo, in the
/// order the data's `Clip` relation authors them. `None` when the weapon has no
/// projectile links at all, or the selected projectile has no clip archetype -
/// which is also what makes its rounds unreturnable (see [`can_unload`]).
fn selected_clip_templates(world: &World, weapon: EntityId) -> Option<Vec<i32>> {
    let projectiles = crate::scripts::script_util::ordered_projectile_links(world, weapon);
    if projectiles.is_empty() {
        return None;
    }
    let selected = world
        .borrow::<View<crate::runtime_props::RuntimePropSelectedAmmo>>()
        .ok()
        .and_then(|selected| selected.get(weapon).ok().map(|selected| selected.0))
        .unwrap_or(0);
    let projectile_template = projectiles[selected % projectiles.len()].0;
    world
        .borrow::<UniqueView<GlobalProjectileClips>>()
        .ok()
        .and_then(|clips| clips.clips.get(&projectile_template).cloned())
        .filter(|clips| !clips.is_empty())
}

/// The backpack's stackable items whose class descends from one of
/// `clip_templates` - the reserve a reload draws from and an unload merges into.
fn compatible_reserve_items(world: &World, clip_templates: &[i32]) -> Vec<EntityId> {
    let Ok(inventory) = world
        .borrow::<UniqueView<crate::mission::PlayerInfo>>()
        .map(|player| player.inventory_entity_id)
    else {
        return Vec::new();
    };
    let reserve_items: Vec<EntityId> = {
        let Ok(links) = world.borrow::<View<Links>>() else {
            return Vec::new();
        };
        let Ok(inventory_links) = links.get(inventory) else {
            return Vec::new();
        };
        inventory_links
            .to_links
            .iter()
            .filter(|link| matches!(link.link, Link::Contains(_)))
            .filter_map(|link| link.to_entity_id.map(|id| id.0))
            .collect()
    };

    let Ok((stacks, hierarchy)) = world.borrow::<(
        View<PropStackCount>,
        UniqueView<crate::mission::GlobalTemplateHierarchy>,
    )>() else {
        return Vec::new();
    };
    reserve_items
        .into_iter()
        .filter(|item| {
            let Some(class_template_id) =
                crate::scripts::script_util::entity_class_template_id(world, *item)
            else {
                return false;
            };
            // A zero stack is neither a source to load from nor a target to
            // merge into: it is destroyed on depletion, and rounds merged into
            // one that somehow survived would go with it.
            stacks.get(*item).is_ok_and(|stack| stack.0 > 0)
                && clip_templates.iter().any(|clip_template| {
                    hierarchy.is_or_descends_from(class_template_id, *clip_template)
                })
        })
        .collect()
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

    let Some(compatible_clip_templates) = selected_clip_templates(world, weapon) else {
        return ReloadOutcome::default();
    };
    let matching_reserve = compatible_reserve_items(world, &compatible_clip_templates);

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

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct UnloadOutcome {
    /// Rounds that have ALREADY moved out of the magazine (0 when nothing was
    /// unloaded, and 0 alongside a `spawn_clip` request, which has not happened
    /// yet).
    pub rounds_unloaded: i32,
    /// The clip archetype the caller must instantiate into the backpack, and how
    /// many rounds it must carry. The magazine still holds them until the caller
    /// has placed it and called [`empty_magazine`]. `None` when the rounds
    /// merged into a stack the player already had.
    pub spawn_clip: Option<(i32, i32)>,
}

/// Whether `weapon`'s loaded rounds could be returned to the backpack. A
/// projectile with no authored clip archetype has no reserve representation, so
/// its rounds can only be fired.
pub(crate) fn can_unload(world: &World, weapon: EntityId) -> bool {
    selected_clip_templates(world, weapon).is_some()
}

/// Return `weapon`'s loaded rounds to the backpack as clips of the ammo type
/// they actually are - the reverse of [`load_from_reserve`], and what lets a
/// player swap ammo type without the loaded rounds either vanishing or being
/// silently converted.
///
/// Rounds merge into the first compatible reserve stack, which cannot fail, so
/// that case empties the magazine here. With no such stack the outcome instead
/// asks the CALLER to mint the clip archetype - instantiating an entity needs
/// the mission's entity creator, which this module has no access to - and the
/// magazine is deliberately left LOADED until that placement succeeds. Nothing
/// then has to be undone: rounds exist in exactly one place at every instant.
///
/// So `rounds_unloaded` counts rounds that have already moved; a `spawn_clip`
/// outcome is a request, not a completed unload.
///
/// Reserve stacks have no capacity ceiling in this port - `load_from_reserve`
/// drains them without one and the game never caps them on pickup - so the
/// merge is unconditional rather than spilling into a second stack.
pub(crate) fn unload_to_reserve(world: &World, weapon: EntityId) -> UnloadOutcome {
    let loaded = world
        .borrow::<View<PropGunState>>()
        .ok()
        .and_then(|states| states.get(weapon).ok().map(|state| state.ammo))
        .unwrap_or(0);
    if loaded <= 0 {
        return UnloadOutcome::default();
    }
    let Some(compatible_clip_templates) = selected_clip_templates(world, weapon) else {
        return UnloadOutcome::default();
    };

    let destination = compatible_reserve_items(world, &compatible_clip_templates)
        .into_iter()
        .next();

    let Some(item) = destination else {
        let template = world
            .borrow::<UniqueView<GlobalProjectileClips>>()
            .map(|clips| mint_clip_template(&compatible_clip_templates, &clips.clip_sizes, loaded))
            .unwrap_or(compatible_clip_templates[0]);
        return UnloadOutcome {
            rounds_unloaded: 0,
            spawn_clip: Some((template, loaded)),
        };
    };

    {
        let Ok(mut stacks) = world.borrow::<ViewMut<PropStackCount>>() else {
            return UnloadOutcome::default();
        };
        let Ok(stack) = (&mut stacks).get(item) else {
            return UnloadOutcome::default();
        };
        stack.0 += loaded;
    }
    empty_magazine(world, weapon);
    UnloadOutcome {
        rounds_unloaded: loaded,
        spawn_clip: None,
    }
}

/// The clip archetype to mint for `rounds` ejected rounds: the smallest
/// authored box that holds them all, so the label matches the contents - an
/// assault rifle's 30 rounds must not come back as a "Small Standard Clip"
/// (issue #1171). When even the largest box cannot hold them, the largest is
/// minted holding them all anyway: reserve stacks have no capacity ceiling in
/// this port. The data's authored order breaks ties and stands in for
/// archetypes with no authored size.
fn mint_clip_template(clip_templates: &[i32], sizes: &HashMap<i32, i32>, rounds: i32) -> i32 {
    let mut fitting: Option<(i32, i32)> = None;
    let mut largest: Option<(i32, i32)> = None;
    for &clip in clip_templates {
        let Some(&size) = sizes.get(&clip) else {
            continue;
        };
        if size >= rounds && fitting.is_none_or(|(_, best)| size < best) {
            fitting = Some((clip, size));
        }
        if largest.is_none_or(|(_, best)| size > best) {
            largest = Some((clip, size));
        }
    }
    fitting
        .or(largest)
        .map(|(clip, _)| clip)
        .unwrap_or(clip_templates[0])
}

/// Zero `weapon`'s magazine, once its rounds are safely somewhere else.
pub(crate) fn empty_magazine(world: &World, weapon: EntityId) {
    let Ok(mut states) = world.borrow::<ViewMut<PropGunState>>() else {
        return;
    };
    if let Ok(state) = (&mut states).get(weapon) {
        state.ammo = 0;
    }
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
            world.add_unique(GlobalProjectileClips {
                clips: HashMap::from([
                    (STD_PROJECTILE, vec![STD_CLIP, SMALL_STD_CLIP]),
                    (ASSAULT_STD_PROJECTILE, vec![SMALL_STD_CLIP, STD_CLIP]),
                    (HE_PROJECTILE, vec![HE_CLIP]),
                    (PRISM_PROJECTILE, vec![SMALL_PRISM, LARGE_PRISM]),
                ]),
                // The shipped archetypes' authored sizes.
                clip_sizes: HashMap::from([
                    (STD_CLIP, 12),
                    (SMALL_STD_CLIP, 6),
                    (HE_CLIP, 12),
                    (SMALL_PRISM, 10),
                    (LARGE_PRISM, 20),
                ]),
            });
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

    /// A projectile the data authors no `Clip` relation for: its rounds have no
    /// reserve representation, so they can never be ejected.
    const CLIPLESS_PROJECTILE: i32 = -9999;

    #[test]
    fn ejecting_an_empty_magazine_does_nothing() {
        let fixture = Fixture::new(0, 0);

        let outcome = unload_to_reserve(&fixture.world, fixture.weapon);

        assert_eq!(outcome, UnloadOutcome::default());
        assert_eq!(fixture.ammo(), 0);
    }

    #[test]
    fn ejecting_merges_into_a_matching_reserve_stack() {
        let mut fixture = Fixture::new(5, 0);
        let reserve = fixture.reserve(EARTH_SMALL_STD_CLIP, 6);

        let outcome = unload_to_reserve(&fixture.world, fixture.weapon);

        assert_eq!(
            outcome,
            UnloadOutcome {
                rounds_unloaded: 5,
                spawn_clip: None,
            }
        );
        assert_eq!(fixture.ammo(), 0);
        assert_eq!(
            fixture.rounds(reserve),
            11,
            "the stack absorbs the magazine"
        );
    }

    #[test]
    fn ejecting_without_a_matching_stack_asks_for_a_fresh_clip() {
        let mut fixture = Fixture::new(5, 0);
        // Carrying only HE, while standard is loaded and selected.
        let mismatched = fixture.reserve(HE_CLIP, 6);

        let outcome = unload_to_reserve(&fixture.world, fixture.weapon);

        assert_eq!(
            outcome,
            UnloadOutcome {
                // Nothing has moved yet - this is a request, not a completed
                // unload, so the rounds are still counted in the magazine.
                // 5 rounds fit the small box, so that is the one requested.
                rounds_unloaded: 0,
                spawn_clip: Some((SMALL_STD_CLIP, 5)),
            }
        );
        assert_eq!(fixture.rounds(mismatched), 6, "HE reserve is untouched");
    }

    #[test]
    fn a_requested_clip_leaves_the_magazine_loaded_until_it_is_placed() {
        // The rounds must exist in exactly one place at every instant: if the
        // caller cannot carry the fresh clip, the weapon is simply still loaded
        // and the swap is unearned. Emptying up front would need an undo, and a
        // missed undo is a free ammo-type conversion.
        let fixture = Fixture::new(5, 0);

        let outcome = unload_to_reserve(&fixture.world, fixture.weapon);

        assert_eq!(outcome.spawn_clip, Some((SMALL_STD_CLIP, 5)));
        assert_eq!(fixture.ammo(), 5, "still loaded until the clip is carried");

        empty_magazine(&fixture.world, fixture.weapon);
        assert_eq!(fixture.ammo(), 0);
    }

    #[test]
    fn ejecting_more_rounds_than_any_box_holds_mints_the_largest_archetype() {
        // Issue #1171's headline case: the assault rifle's data prefers the
        // SMALL standard clip, but 30 rounds fit neither the 6- nor the
        // 12-round box, so the largest must absorb them rather than labeling
        // 30 rounds a "Small Standard Clip".
        let mut fixture = Fixture::new(30, 0);
        fixture.set_standard_projectile(ASSAULT_STD_PROJECTILE);

        let outcome = unload_to_reserve(&fixture.world, fixture.weapon);

        assert_eq!(outcome.spawn_clip, Some((STD_CLIP, 30)));
    }

    #[test]
    fn ejecting_exactly_a_full_box_mints_that_box() {
        // 12 rounds are exactly a full Standard Clip, even for the assault
        // rifle whose authored order prefers the small one.
        let mut fixture = Fixture::new(12, 0);
        fixture.set_standard_projectile(ASSAULT_STD_PROJECTILE);

        let outcome = unload_to_reserve(&fixture.world, fixture.weapon);

        assert_eq!(outcome.spawn_clip, Some((STD_CLIP, 12)));
    }

    #[test]
    fn ejecting_fewer_rounds_than_the_smallest_box_mints_the_smallest() {
        // 6 rounds fit the Small Standard Clip exactly, even for the pistol
        // whose authored order prefers the full-size one.
        let fixture = Fixture::new(6, 0);

        let outcome = unload_to_reserve(&fixture.world, fixture.weapon);

        assert_eq!(outcome.spawn_clip, Some((SMALL_STD_CLIP, 6)));
    }

    #[test]
    fn minting_without_authored_sizes_falls_back_to_the_authored_order() {
        assert_eq!(
            mint_clip_template(&[SMALL_STD_CLIP, STD_CLIP], &HashMap::new(), 7),
            SMALL_STD_CLIP
        );
    }

    #[test]
    fn from_entity_info_records_each_clip_archetypes_authored_size() {
        use dark::properties::ToTemplateLink;
        use std::sync::Arc;

        let mut entity_info = SystemShock2EntityInfo::empty();
        entity_info
            .entity_to_properties
            .insert(STD_PROJECTILE, vec![]);
        entity_info
            .entity_to_properties
            .insert(STD_CLIP, vec![Arc::new(Box::new(PropStackCount(12)))]);
        entity_info.template_to_links.insert(
            STD_PROJECTILE,
            dark::properties::TemplateLinks {
                to_links: vec![ToTemplateLink {
                    link: Link::Clip,
                    to_template_id: STD_CLIP,
                }],
            },
        );

        let clips = GlobalProjectileClips::from_entity_info(&entity_info);

        assert_eq!(clips.clips.get(&STD_PROJECTILE), Some(&vec![STD_CLIP]));
        assert_eq!(clips.clip_sizes.get(&STD_CLIP), Some(&12));
    }

    #[test]
    fn ejecting_returns_the_selected_ammo_type_not_the_first() {
        // HE selected, HE loaded: the rounds must come back as HE even though
        // standard is the weapon's first projectile link.
        let mut fixture = Fixture::new(4, 1);
        let he = fixture.reserve(HE_CLIP, 2);

        let outcome = unload_to_reserve(&fixture.world, fixture.weapon);

        assert_eq!(outcome.rounds_unloaded, 4);
        assert_eq!(outcome.spawn_clip, None);
        assert_eq!(fixture.rounds(he), 6);
    }

    #[test]
    fn a_magazine_with_no_clip_archetype_is_not_ejected() {
        let mut fixture = Fixture::new(5, 0);
        fixture.set_standard_projectile(CLIPLESS_PROJECTILE);

        let outcome = unload_to_reserve(&fixture.world, fixture.weapon);

        assert_eq!(outcome, UnloadOutcome::default());
        assert_eq!(fixture.ammo(), 5, "unreturnable rounds stay loaded");
    }

    // `can_cycle_ammo` lives in `script_util`, but its contract is this
    // module's: cycling is allowed exactly when a loaded magazine could be
    // ejected. Tested here because the eject fixture is what it needs.
    use crate::scripts::script_util::can_cycle_ammo;

    #[test]
    fn a_loaded_multi_ammo_weapon_may_cycle_because_it_can_eject() {
        let fixture = Fixture::new(5, 0);

        assert!(can_cycle_ammo(&fixture.world, fixture.weapon));
    }

    #[test]
    fn a_loaded_weapon_whose_rounds_cannot_be_returned_still_refuses_to_cycle() {
        let mut fixture = Fixture::new(5, 0);
        fixture.set_standard_projectile(CLIPLESS_PROJECTILE);

        assert!(!can_cycle_ammo(&fixture.world, fixture.weapon));
    }

    #[test]
    fn a_single_ammo_weapon_still_refuses_to_cycle() {
        let fixture = Fixture::new(0, 0);
        {
            let mut links = fixture.world.borrow::<ViewMut<Links>>().unwrap();
            (&mut links)
                .get(fixture.weapon)
                .unwrap()
                .to_links
                .retain(|link| {
                    !matches!(
                        link.link,
                        Link::Projectile(ProjectileOptions { order: 1, .. })
                    )
                });
        }

        assert!(!can_cycle_ammo(&fixture.world, fixture.weapon));
    }
}
