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
        let unique_clips: std::collections::HashSet<i32> =
            projectile_clips.values().flatten().copied().collect();
        let mut clip_sizes = HashMap::new();
        for clip in unique_clips {
            if let Some(stack) = crate::scripts::script_util::hydrate_template_component::<
                PropStackCount,
            >(clip, entity_info)
            {
                clip_sizes.insert(clip, stack.0);
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
pub(crate) fn selected_clip_templates(world: &World, weapon: EntityId) -> Option<Vec<i32>> {
    let projectiles = crate::scripts::script_util::ordered_projectile_links(world, weapon);
    if projectiles.is_empty() {
        return None;
    }
    clip_templates_for_projectile(world, projectiles[selected_ammo_index(world, weapon)].0)
}

/// The clip archetypes authored for one projectile archetype.
fn clip_templates_for_projectile(world: &World, projectile_template: i32) -> Option<Vec<i32>> {
    world
        .borrow::<UniqueView<GlobalProjectileClips>>()
        .ok()
        .and_then(|clips| clips.clips.get(&projectile_template).cloned())
        .filter(|clips| !clips.is_empty())
}

/// Which of `weapon`'s projectile links is currently selected, already wrapped
/// into range. A weapon with no `RuntimePropSelectedAmmo` is on its first.
pub(crate) fn selected_ammo_index(world: &World, weapon: EntityId) -> usize {
    let count = crate::scripts::script_util::ordered_projectile_links(world, weapon).len();
    if count == 0 {
        return 0;
    }
    world
        .borrow::<View<crate::runtime_props::RuntimePropSelectedAmmo>>()
        .ok()
        .and_then(|selected| selected.get(weapon).ok().map(|selected| selected.0))
        .unwrap_or(0)
        % count
}

/// Whether `item`'s class descends from any clip archetype the data authors a
/// `Clip` relation to - "this is ammo", regardless of which gun it fits.
///
/// The physical insert gesture needs this separately from
/// [`clip_projectile_index`]: an item that is not ammo at all must pass through
/// the magazine zone in silence, while ammo the gun cannot take earns a
/// refusal.
pub(crate) fn is_ammo_clip(world: &World, item: EntityId) -> bool {
    let Some(class_template_id) =
        crate::scripts::script_util::entity_class_template_id(world, item)
    else {
        return false;
    };
    let (Ok(projectile_clips), Ok(hierarchy)) = (
        world.borrow::<UniqueView<GlobalProjectileClips>>(),
        world.borrow::<UniqueView<crate::mission::GlobalTemplateHierarchy>>(),
    ) else {
        return false;
    };
    projectile_clips
        .clips
        .values()
        .flatten()
        .any(|clip_template| hierarchy.is_or_descends_from(class_template_id, *clip_template))
}

/// Which of `weapon`'s ammo types the clip entity `clip` loads, as an index
/// into its ordered projectile links. `None` when the weapon takes no such
/// clip at all.
///
/// The CURRENTLY selected type is tested first, so a clip that fits more than
/// one of a weapon's projectiles (a family archetype two ammo types both
/// inherit) tops the loaded type off rather than silently swapping it.
pub(crate) fn clip_projectile_index(
    world: &World,
    weapon: EntityId,
    clip: EntityId,
) -> Option<usize> {
    let projectiles = crate::scripts::script_util::ordered_projectile_links(world, weapon);
    if projectiles.is_empty() {
        return None;
    }
    let class_template_id = crate::scripts::script_util::entity_class_template_id(world, clip)?;
    let hierarchy = world
        .borrow::<UniqueView<crate::mission::GlobalTemplateHierarchy>>()
        .ok()?;
    let selected = selected_ammo_index(world, weapon);
    let order = std::iter::once(selected).chain((0..projectiles.len()).filter(|i| *i != selected));
    order.into_iter().find(|index| {
        clip_templates_for_projectile(world, projectiles[*index].0).is_some_and(|clip_templates| {
            clip_templates.iter().any(|clip_template| {
                hierarchy.is_or_descends_from(class_template_id, *clip_template)
            })
        })
    })
}

/// How many rounds a clip entity still carries (0 for anything with no stack).
pub(crate) fn clip_rounds(world: &World, clip: EntityId) -> i32 {
    world
        .borrow::<View<PropStackCount>>()
        .ok()
        .and_then(|stacks| stacks.get(clip).ok().map(|stack| stack.0))
        .unwrap_or(0)
}

/// Move rounds from ONE clip entity the player is physically holding into
/// `weapon`'s magazine - the physical VR reload's counterpart to
/// [`load_from_reserve`], which draws from the backpack instead.
///
/// Only as many rounds as the magazine is missing move, so a fuller clip keeps
/// its remainder and stays in the hand. A clip drained to zero is reported in
/// `depleted_items` for the caller to destroy, exactly as a reserve stack is.
pub(crate) fn load_from_held_clip(
    world: &World,
    weapon: EntityId,
    clip: EntityId,
    capacity: i32,
) -> ReloadOutcome {
    let current_ammo = world
        .borrow::<View<PropGunState>>()
        .ok()
        .and_then(|states| states.get(weapon).ok().map(|state| state.ammo));
    let Some(current_ammo) = current_ammo else {
        return ReloadOutcome::default();
    };
    let rounds_needed = (capacity.max(0) - current_ammo.max(0)).max(0);
    if rounds_needed == 0 {
        return ReloadOutcome::default();
    }

    let mut outcome = ReloadOutcome::default();
    {
        let Ok(mut stacks) = world.borrow::<ViewMut<PropStackCount>>() else {
            return outcome;
        };
        let Ok(stack) = (&mut stacks).get(clip) else {
            return outcome;
        };
        let consumed = stack.0.min(rounds_needed).max(0);
        if consumed == 0 {
            return outcome;
        }
        stack.0 -= consumed;
        outcome.rounds_loaded = consumed;
        if stack.0 == 0 {
            outcome.depleted_items.push(clip);
        }
    }

    let mut states = world.borrow::<ViewMut<PropGunState>>().unwrap();
    if let Ok(state) = (&mut states).get(weapon) {
        state.ammo = (state.ammo.max(0) + outcome.rounds_loaded).min(capacity.max(0));
    }
    outcome
}

/// Enter/exit radii of a weapon's magazine zone at the view model's authored
/// size - the volume a held clip is inserted by entering, around the weapon's
/// magazine anchor (`vr_config::magazine_anchor_from_entity`).
///
/// Expressed against the model, not against the room: they scale with the gun
/// (`clip_insert_radii`). At the authored size one world unit is ~0.76 m, so
/// the enter radius is a generous ~19 cm; a gun drawn at life size shrinks the
/// zone with it (~7.6 cm at `gun_scale` 0.4), which is what keeps the zone
/// meaning "the magwell" rather than "somewhere near the gun" - a life-size
/// pistol is only ~0.26 units long, so an unscaled zone would swallow the whole
/// weapon and the per-model anchors with it.
pub(crate) const CLIP_INSERT_ENTER_RADIUS: f32 = 0.25;
pub(crate) const CLIP_INSERT_EXIT_RADIUS: f32 = 0.35;

/// The zone's (enter, exit) radii for a weapon drawn at `scale`.
pub(crate) fn clip_insert_radii(scale: f32) -> (f32, f32) {
    (
        scale * CLIP_INSERT_ENTER_RADIUS,
        scale * CLIP_INSERT_EXIT_RADIUS,
    )
}

/// Whether the magazine zone is engaged this frame, given whether it was
/// engaged last frame. The two radii are deliberately different: a single
/// threshold flickers on hand jitter right at the boundary, and every flicker
/// is another insert attempt.
pub(crate) fn clip_insert_zone_engaged(was_engaged: bool, distance: f32, scale: f32) -> bool {
    let (enter, exit) = clip_insert_radii(scale);
    if was_engaged {
        distance <= exit
    } else {
        distance <= enter
    }
}

/// The backpack's stackable items whose class descends from one of
/// `clip_templates` - the reserve a reload draws from and an unload merges into.
pub(crate) fn compatible_reserve_items(world: &World, clip_templates: &[i32]) -> Vec<EntityId> {
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

/// One clip's worth of reserve ammo, ready to be handed out of the belt pouch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PouchWithdrawal {
    /// The reserve stack the rounds come out of.
    pub item: EntityId,
    /// That stack's own archetype - the clip the player actually carries, so a
    /// pouch full of small clips never hands out a large one.
    pub template_id: i32,
    /// Rounds in the withdrawn clip: the archetype's authored stack size, or
    /// the whole remaining stack when it holds less.
    pub rounds: i32,
    /// Whether `rounds` empties `item`, so the stack itself can be handed over
    /// rather than split. Splitting is what needs a fresh entity.
    pub takes_whole_stack: bool,
}

/// The clip the ammo pouch would hand out for `weapon`'s CURRENTLY selected
/// ammo, or `None` when nothing compatible is in reserve - which is exactly
/// when the pouch is empty and the glove pre-lights amber.
///
/// Rounds are never fabricated: everything here is measured off a stack the
/// player is already carrying.
pub(crate) fn pouch_withdrawal(world: &World, weapon: EntityId) -> Option<PouchWithdrawal> {
    let clip_templates = selected_clip_templates(world, weapon)?;
    let item = compatible_reserve_items(world, &clip_templates)
        .into_iter()
        .next()?;
    let template_id = crate::scripts::script_util::entity_class_template_id(world, item)?;
    let stack = world
        .borrow::<View<PropStackCount>>()
        .ok()
        .and_then(|stacks| stacks.get(item).ok().map(|stack| stack.0))?;
    if stack <= 0 {
        return None;
    }
    // Reserve stacks are counted in ROUNDS, so a clip's worth is the
    // archetype's authored stack size. An archetype with no authored size has
    // no defined clip, and the whole remaining stack comes out in one go.
    let clip_size = world
        .borrow::<UniqueView<GlobalProjectileClips>>()
        .ok()
        .and_then(|clips| clip_size_of(world, &clips, &clip_templates, template_id))
        .unwrap_or(stack);
    let rounds = stack.min(clip_size.max(1));
    Some(PouchWithdrawal {
        item,
        template_id,
        rounds,
        takes_whole_stack: rounds >= stack,
    })
}

/// The authored stack size for the clip archetype `class_template_id` descends
/// from. A reserve item's own class may be a mission-local child of the
/// archetype the `Clip` relation names, so the size is looked up through the
/// hierarchy rather than on the class directly.
fn clip_size_of(
    world: &World,
    clips: &GlobalProjectileClips,
    clip_templates: &[i32],
    class_template_id: i32,
) -> Option<i32> {
    if let Some(size) = clips.clip_sizes.get(&class_template_id) {
        return Some(*size);
    }
    let hierarchy = world
        .borrow::<UniqueView<crate::mission::GlobalTemplateHierarchy>>()
        .ok()?;
    clip_templates
        .iter()
        .find(|clip| hierarchy.is_or_descends_from(class_template_id, **clip))
        .and_then(|clip| clips.clip_sizes.get(clip))
        .copied()
}

/// Take `rounds` out of a reserve stack, for a pouch withdrawal that splits it.
/// The caller then mints the clip that carries them, so the rounds exist in
/// exactly one place at every instant. `false` leaves the stack untouched.
pub(crate) fn take_rounds_from_reserve(world: &World, item: EntityId, rounds: i32) -> bool {
    let Ok(mut stacks) = world.borrow::<ViewMut<PropStackCount>>() else {
        return false;
    };
    let Ok(stack) = (&mut stacks).get(item) else {
        return false;
    };
    if rounds <= 0 || stack.0 < rounds {
        return false;
    }
    stack.0 -= rounds;
    true
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
                    condition: 100.0,
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

        /// A clip the player is physically holding: an ordinary stackable
        /// entity with NO `Contains` link, since a grabbed item has left the
        /// backpack.
        fn held_clip(&mut self, template_id: i32, rounds: i32) -> EntityId {
            self.world
                .add_entity((PropTemplateId { template_id }, PropStackCount(rounds)))
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
        entity_info
            .entity_to_properties
            .insert(SMALL_STD_CLIP, vec![Arc::new(Box::new(PropStackCount(6)))]);
        entity_info.template_to_links.insert(
            STD_PROJECTILE,
            dark::properties::TemplateLinks {
                to_links: vec![
                    ToTemplateLink {
                        link: Link::Clip,
                        to_template_id: SMALL_STD_CLIP,
                    },
                    ToTemplateLink {
                        link: Link::Clip,
                        to_template_id: STD_CLIP,
                    },
                ],
            },
        );

        let clips = GlobalProjectileClips::from_entity_info(&entity_info);

        assert_eq!(
            clips.clips.get(&STD_PROJECTILE),
            Some(&vec![SMALL_STD_CLIP, STD_CLIP]),
            "the authored order is preserved - the mint's tie-break and fallback"
        );
        assert_eq!(clips.clip_sizes.get(&STD_CLIP), Some(&12));
        assert_eq!(clips.clip_sizes.get(&SMALL_STD_CLIP), Some(&6));
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

    // --- the physical VR insert gesture (a clip carried to the magazine) ---

    #[test]
    fn a_held_clip_of_the_selected_type_loads_that_type() {
        let mut fixture = Fixture::new(0, 0);
        let clip = fixture.held_clip(EARTH_SMALL_STD_CLIP, 6);

        assert_eq!(
            clip_projectile_index(&fixture.world, fixture.weapon, clip),
            Some(0)
        );
    }

    #[test]
    fn a_held_clip_of_another_carried_type_names_that_types_projectile() {
        // Standard is selected and HE is what the hand brought: the gesture has
        // to name index 1 so the caller ejects and swaps to it.
        let mut fixture = Fixture::new(0, 0);
        let clip = fixture.held_clip(HE_CLIP, 4);

        assert_eq!(
            clip_projectile_index(&fixture.world, fixture.weapon, clip),
            Some(1)
        );
    }

    #[test]
    fn a_clip_this_weapon_does_not_take_has_no_projectile_index() {
        // Real ammo (a prism), but for a gun the pistol's projectiles never
        // link - the refusal case, as distinct from carrying a medkit past.
        let mut fixture = Fixture::new(0, 0);
        let clip = fixture.held_clip(SMALL_PRISM, 10);

        assert!(is_ammo_clip(&fixture.world, clip));
        assert_eq!(
            clip_projectile_index(&fixture.world, fixture.weapon, clip),
            None
        );
    }

    #[test]
    fn an_item_that_is_not_ammo_at_all_is_not_a_clip() {
        const A_MEDKIT: i32 = -9999;
        let mut fixture = Fixture::new(0, 0);
        let item = fixture.held_clip(A_MEDKIT, 1);

        assert!(!is_ammo_clip(&fixture.world, item));
    }

    #[test]
    fn inserting_a_held_clip_fills_the_magazine_and_empties_the_clip() {
        let mut fixture = Fixture::new(0, 0);
        let clip = fixture.held_clip(EARTH_SMALL_STD_CLIP, 12);

        let outcome = load_from_held_clip(&fixture.world, fixture.weapon, clip, 12);

        assert_eq!(fixture.ammo(), 12);
        assert_eq!(fixture.rounds(clip), 0);
        assert_eq!(outcome.rounds_loaded, 12);
        assert_eq!(
            outcome.depleted_items,
            vec![clip],
            "a spent clip is the caller's to destroy - it leaves the hand with it"
        );
    }

    #[test]
    fn a_fuller_clip_keeps_its_remainder_in_the_hand() {
        let mut fixture = Fixture::new(0, 0);
        let clip = fixture.held_clip(STD_CLIP, 20);

        let outcome = load_from_held_clip(&fixture.world, fixture.weapon, clip, 12);

        assert_eq!(fixture.ammo(), 12);
        assert_eq!(fixture.rounds(clip), 8, "the remainder stays on the clip");
        assert_eq!(outcome.rounds_loaded, 12);
        assert!(
            outcome.depleted_items.is_empty(),
            "a clip with rounds left is not destroyed"
        );
    }

    #[test]
    fn a_partly_loaded_magazine_is_topped_off_from_the_held_clip() {
        let mut fixture = Fixture::new(7, 0);
        let clip = fixture.held_clip(EARTH_SMALL_STD_CLIP, 6);

        let outcome = load_from_held_clip(&fixture.world, fixture.weapon, clip, 12);

        assert_eq!(fixture.ammo(), 12);
        assert_eq!(fixture.rounds(clip), 1);
        assert_eq!(outcome.rounds_loaded, 5);
    }

    #[test]
    fn inserting_into_a_full_magazine_moves_nothing() {
        // The rounds must not be silently burned: a full gun simply refuses the
        // clip, and the player still carries it.
        let mut fixture = Fixture::new(12, 0);
        let clip = fixture.held_clip(EARTH_SMALL_STD_CLIP, 6);

        let outcome = load_from_held_clip(&fixture.world, fixture.weapon, clip, 12);

        assert_eq!(outcome, ReloadOutcome::default());
        assert_eq!(fixture.ammo(), 12);
        assert_eq!(fixture.rounds(clip), 6);
    }

    #[test]
    fn the_magazine_zone_needs_a_closer_approach_than_it_needs_to_hold() {
        // Hysteresis: without it, hand jitter right at the boundary re-enters
        // the zone every few frames, and every entry is another insert.
        // At both the authored size and a life-size wield, so the hysteresis
        // survives the scaling rather than only holding at 1.0.
        for scale in [1.0f32, 0.4] {
            let (enter, exit) = clip_insert_radii(scale);
            let between = (enter + exit) / 2.0;

            assert!(!clip_insert_zone_engaged(false, between, scale));
            assert!(clip_insert_zone_engaged(true, between, scale));
            assert!(clip_insert_zone_engaged(false, enter - 0.01 * scale, scale));
            assert!(!clip_insert_zone_engaged(true, exit + 0.01 * scale, scale));
        }
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
