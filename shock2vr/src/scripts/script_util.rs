use crate::mission::entity_creator::initialize_entity_with_props;
use crate::mission::mission_core::GlobalTemplateHierarchy;
use crate::{
    runtime_props::RuntimePropTransform,
    util::{point3_to_vec3, resolve_proxy_entity},
};
use cgmath::{Transform, Vector3, point3};
use dark::{
    EnvSoundQuery,
    properties::{
        GunSettingDesc, Link, Links, ProjectileOptions, PropBaseGunDesc, PropClassTag,
        PropGunSettingHeader1, PropGunSettingHeader2, PropGunSettingText1, PropGunSettingText2,
        PropGunState, PropMaterial, PropObjShortName, PropSymName, PropTemplateId,
        PropTweqModelConfig, ToLink,
    },
    ss2_entity_info::SystemShock2EntityInfo,
};
use engine::audio::AudioHandle;
use shipyard::{
    Component, EntityId, Get, IntoIter, IntoWithId, UniqueView, UniqueViewMut, View, ViewMut, World,
};
use std::collections::HashMap;

use super::{Effect, Message, MessagePayload};

/// Stable class identity for hierarchy/tag lookups. Concrete objects retain a
/// positive mission-local `PropTemplateId`, while carried objects preserve
/// their source gamesys archetype separately across level transitions.
pub(crate) fn entity_class_template_id(world: &World, entity: EntityId) -> Option<i32> {
    world
        .borrow::<View<crate::runtime_props::RuntimePropCanonicalTemplateId>>()
        .ok()
        .and_then(|canonical| canonical.get(entity).ok().map(|id| id.0))
        .or_else(|| {
            world
                .borrow::<View<PropTemplateId>>()
                .ok()
                .and_then(|templates| templates.get(entity).ok().map(|id| id.template_id))
        })
}

/// Resolve contact damage and freeze against the owning receiver. Damage goes
/// through the struck hitbox so limb scaling and ragdoll metadata survive.
/// Non-damage stims and immune receivers do not emit a zero-damage AI alert.
pub(crate) fn projectile_contact_effects(
    world: &World,
    projectile: EntityId,
    struck: EntityId,
    impact: Option<super::DamageImpact>,
) -> Effect {
    let Some(template) = entity_class_template_id(world, projectile) else {
        return Effect::NoEffect;
    };
    let receiver = resolve_proxy_entity(world, struck);
    let amount = crate::mission::stim_response::contact_stim_damage_scaled(
        world,
        template,
        receiver,
        crate::runtime_props::RuntimePropShotModifiers::of(world, projectile).stim,
    );
    let damage = if amount > 0.0 {
        Effect::Send {
            msg: Message {
                to: struck,
                payload: MessagePayload::Damage { amount, impact },
            },
        }
    } else {
        Effect::NoEffect
    };
    if let Some(duration_seconds) = crate::mission::stim_response::contact_stim_freeze(
        world,
        template,
        receiver,
        crate::runtime_props::RuntimePropShotModifiers::of(world, projectile).stim,
    ) {
        Effect::combine(vec![
            damage,
            Effect::Send {
                msg: Message {
                    to: receiver,
                    payload: MessagePayload::Freeze { duration_seconds },
                },
            },
        ])
    } else {
        damage
    }
}

/// Base gamesys templates for the three nanite pile sizes (Small/Medium/Big
/// Nanite Pile). Deliberately narrower than their shared ultimate ancestor
/// `Nanites` (-85): that ancestor also roots `FakeNanites` (-1271), a
/// bomb-trap payload dressed up to look like a nanite pickup, which must keep
/// going through the ordinary generic-frob/inventory path rather than being
/// auto-collected as currency. The pile templates are the narrowest common
/// ancestor of every real spawnable pickup (`1/5/10/20 Nanites` and any
/// authored pile placed directly) while excluding that decoy.
pub(crate) const NANITE_PILE_TEMPLATE_IDS: [i32; 3] = [-1589, -1590, -1591];

/// Whether `template_id` is one of [`NANITE_PILE_TEMPLATE_IDS`] or descends
/// from one, given a template-inheritance hierarchy (MetaProp parent map).
/// The single predicate both call sites consult so "is this a nanite pile"
/// can't silently drift between them: `mission::entity_creator` (script
/// attachment, from the load-time `SystemShock2EntityInfo` hierarchy) and
/// [`is_nanite_pickup`] below (from the runtime `GlobalTemplateHierarchy`
/// unique, which is that same hierarchy cloned into the `World`). The
/// ancestry check itself is shared with `GlobalTemplateHierarchy::
/// is_or_descends_from` via `mission_core::template_is_or_descends_from`.
pub(crate) fn is_nanite_pile_template(
    hierarchy: &HashMap<i32, Vec<i32>>,
    template_id: i32,
) -> bool {
    NANITE_PILE_TEMPLATE_IDS.iter().any(|class_id| {
        crate::mission::mission_core::template_is_or_descends_from(
            hierarchy,
            template_id,
            *class_id,
        )
    })
}

/// Whether `entity` is a real world nanite pickup, identified via the runtime
/// template hierarchy rather than the legacy `nan_ic` icon check below (which
/// also matches the `FakeNanites` decoy). Used to route VR squeeze / flat
/// pickup through Frob instead of a physical grab - see
/// `virtual_hand::uses_scripted_world_frob` and its flat call site.
pub(crate) fn is_nanite_pickup(world: &World, entity: EntityId) -> bool {
    let Ok(hierarchy) = world.borrow::<UniqueView<GlobalTemplateHierarchy>>() else {
        return false;
    };
    let Some(template_id) = entity_class_template_id(world, entity) else {
        return false;
    };
    is_nanite_pile_template(&hierarchy.0, template_id)
}

/// Whether `entity`'s object scripts include `script`. Case-insensitive: the
/// data authors these names in mixed case (`ExpCookie`, `LogDiscScript`) while
/// the port's own derived scripts are lowercase.
pub(crate) fn entity_has_script(world: &World, entity: EntityId, script: &str) -> bool {
    world
        .borrow::<View<dark::properties::PropScripts>>()
        .ok()
        .is_some_and(|scripts| {
            scripts.get(entity).is_ok_and(|entity_scripts| {
                entity_scripts
                    .scripts
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(script))
            })
        })
}

/// The cyber-module pickup script (`scripts::exp_cookie`), authored on the
/// `EXP Cookies` archetype and inherited by every module pile.
const EXP_COOKIE_SCRIPT: &str = "expcookie";

/// Whether `entity` is a cyber-module pickup, identified by the pickup script
/// that actually awards the modules rather than by template ancestry - the
/// script is the thing whose Frob does the collecting.
pub(crate) fn is_cyber_module(world: &World, entity: EntityId) -> bool {
    entity_has_script(world, entity, EXP_COOKIE_SCRIPT)
}

/// Whether `entity` belongs to a category the game *collects* rather than
/// carries: keycards, nanite piles, cyber modules and audio/data logs.
///
/// These four never become a physically-held prop and never occupy an inventory
/// slot - their value goes straight into a player stat, the credential list or
/// the PDA, and their scripts' side effects (SwitchLinks, quest bits, awards)
/// only fire on Frob. So *every* acquisition gesture, on every path - world
/// frob, world squeeze, a loot panel's take arm or squeeze, a click on the
/// inventory strip - must route through Frob instead of a grab or a transfer.
///
/// The single predicate all of those sites consult, so the category cannot
/// drift between them. Each arm delegates to the per-type predicate that owns
/// that type's identity rather than re-deriving it here.
pub(crate) fn is_always_collected(world: &World, entity: EntityId) -> bool {
    crate::virtual_hand::is_key_source(world, entity)
        || is_nanite_pickup(world, entity)
        || is_cyber_module(world, entity)
        || crate::scripts::gui::is_collectable_log(world, entity)
}

pub fn is_message_turnon_or_turnoff(msg: &MessagePayload) -> bool {
    match msg {
        MessagePayload::TurnOn { from: _ } => true,
        MessagePayload::TurnOff { from: _ } => true,
        _ => false,
    }
}

/// Whether an entity is currently locked against the player: it carries
/// `PropLocked(true)` and either has no key destination or a key/quest gate
/// the player hasn't satisfied. Shared by buttons and (door-opening) AIs.
pub fn is_entity_locked(world: &World, entity_id: EntityId) -> bool {
    let v_locked = world
        .borrow::<View<dark::properties::PropLocked>>()
        .unwrap();
    let Ok(locked) = v_locked.get(entity_id) else {
        return false;
    };
    if !locked.0 {
        return false;
    }
    let v_key_dst = world
        .borrow::<View<dark::properties::PropKeyDst>>()
        .unwrap();
    let quest = world
        .borrow::<shipyard::UniqueView<crate::quest_info::QuestInfo>>()
        .unwrap();
    match v_key_dst.get(entity_id) {
        // Locked, and the player lacks the key/quest state to open it
        Ok(key_dst) => !quest.can_unlock(&key_dst.0),
        // Locked with no key destination - nothing can unlock it
        Err(_) => true,
    }
}

/// Set the lock state of one live entity, writing Dark's own `P$Locked`
/// property - the same state [`is_entity_locked`] reads. This is the
/// application of `Effect::SetLocked`; see `scripts::trap_lock` for why the
/// lock lives in the property rather than in runtime state.
pub fn set_entity_locked(world: &mut World, entity_id: EntityId, locked: bool) {
    let is_alive = world
        .borrow::<shipyard::EntitiesView>()
        .map(|entities| entities.is_alive(entity_id))
        .unwrap_or(false);
    if is_alive {
        world.add_component(entity_id, dark::properties::PropLocked(locked));
    }
}

/// Whether a translating door is closed (i.e. worth opening): normally that
/// means it is nearer its closed endpoint than its open one, but a door with
/// no travel - whose endpoints coincide, so position says nothing - answers
/// from its authored state. `None` for entities that aren't translating doors.
pub fn door_is_closed(world: &World, entity_id: EntityId) -> Option<bool> {
    use cgmath::InnerSpace;
    // A door with no travel can never leave its authored pose, and both
    // endpoints sit on top of each other - the distance test below is
    // degenerate there (equal distances always read as "closed"). Answer from
    // the authored state instead, so a permanently open doorway isn't
    // reported shut (#602).
    let no_travel_state = {
        let v_door = world
            .borrow::<View<dark::properties::PropTranslatingDoor>>()
            .unwrap();
        let door = v_door.get(entity_id).ok()?;
        (!door.has_travel()).then(|| !door.is_permanently_open())
    };
    if let Some(closed) = no_travel_state {
        return Some(closed);
    }
    let (from_closed, travel) = door_travel(world, entity_id)?;
    let to_open = (from_closed - travel).magnitude2();
    Some(from_closed.magnitude2() <= to_open)
}

/// Where a travelling door's leaf currently sits, as `(from_closed, travel)`:
/// the offset of the live leaf from its closed pose, and the full closed ->
/// open vector. `None` for entities that are not translating doors and for a
/// door with no travel (both endpoints coincide, so neither vector says
/// anything).
///
/// StdDoor drives the live transform via SetPosition each frame, so the
/// transform - not the property - is where the leaf actually is.
fn door_travel(world: &World, entity_id: EntityId) -> Option<(Vector3<f32>, Vector3<f32>)> {
    let v_door = world
        .borrow::<View<dark::properties::PropTranslatingDoor>>()
        .unwrap();
    let door = v_door.get(entity_id).ok()?;
    if !door.has_travel() {
        return None;
    }
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let current = v_transform
        .get(entity_id)
        .ok()
        .map(|t| point3_to_vec3(t.0.transform_point(point3(0.0, 0.0, 0.0))))?;
    Some((
        current - door.base_closed_location,
        door.base_open_location - door.base_closed_location,
    ))
}

/// How far a translating door's leaf has travelled away from its closed
/// pose: `(fraction, vertical_rise)`, where the fraction is 0 at closed and
/// 1 at fully open and the rise is how much height the leaf has gained in
/// world units. `None` for entities that are not translating doors, and for
/// a door with no travel (its endpoints coincide, so there is no progress to
/// measure and nothing to wait for).
///
/// The rise is what tells an approaching actor whether the gap under a
/// raising leaf is tall enough to walk through yet; a leaf that slides
/// sideways never gains any, so a caller must also accept a full fraction.
pub fn door_open_progress(world: &World, entity_id: EntityId) -> Option<(f32, f32)> {
    use cgmath::InnerSpace;
    let (from_closed, travel) = door_travel(world, entity_id)?;
    let fraction = (from_closed.dot(travel) / travel.magnitude2()).clamp(0.0, 1.0);
    Some((fraction, from_closed.y))
}

/// Whether a cell-gating entity is an obstacle A* must not cross.
///
/// Closed-but-unlocked translating doors stay pathable because a pursuing AI
/// opens them on arrival. Entities that are not translating doors cannot be
/// operated by `StdDoor`, so they remain walls.
pub fn door_blocks_pathfinding(world: &World, entity_id: EntityId) -> bool {
    match door_is_closed(world, entity_id) {
        Some(true) => {
            let permanently_closed = world
                .borrow::<View<dark::properties::PropTranslatingDoor>>()
                .unwrap()
                .get(entity_id)
                .map(|door| door.is_permanently_closed())
                .unwrap_or(false);
            permanently_closed || is_entity_locked(world, entity_id)
        }
        Some(false) => false,
        None => true,
    }
}

pub fn get_all_links_with_template<TData>(
    world: &World,
    producing_entity_id: EntityId,
    filter_fn: fn(&Link) -> Option<TData>,
) -> Vec<(i32, TData)> {
    let links = world.borrow::<View<Links>>().unwrap();
    let mut linked_entities = Vec::new();
    if let Ok(switch_links) = links.get(producing_entity_id) {
        for link in &switch_links.to_links {
            if let Some(data) = filter_fn(&link.link) {
                linked_entities.push((link.to_template_id, data))
            }
        }
    };

    linked_entities
}

/// True when the entity authors any `Corpse` or `Flinderize` link
/// (inheritance-aware, like every other link lookup: the links are merged down
/// the template hierarchy at instantiation). Objects that do - droids link a
/// `Corpse` explosion, and often `Flinderize` parts - are replaced by those
/// links when they die instead of leaving a body behind.
pub fn has_death_links(world: &World, entity_id: EntityId) -> bool {
    !get_all_links_with_template(world, entity_id, |link| match link {
        Link::Corpse(_) | Link::Flinderize(_) => Some(()),
        _ => None,
    })
    .is_empty()
}

/// `weapon`'s selected fire setting, 0 when it has no gun state. Ammo-type
/// selection and the firing description both key off this, so they agree.
pub fn current_gun_setting(world: &World, weapon: EntityId) -> i32 {
    world
        .borrow::<View<PropGunState>>()
        .ok()
        .and_then(|states| states.get(weapon).ok().map(|state| state.setting))
        .unwrap_or(0)
}

/// The firing description for `weapon`'s currently selected fire setting, or
/// `None` when it is not a gun. The setting comes from the weapon's live
/// `PropGunState` (0 when it has none), and an index the archetype does not
/// author falls back to setting 0.
pub fn active_gun_setting(world: &World, weapon: EntityId) -> Option<GunSettingDesc> {
    let setting = current_gun_setting(world, weapon);
    world
        .borrow::<View<PropBaseGunDesc>>()
        .ok()
        .and_then(|descs| {
            descs
                .get(weapon)
                .ok()
                .map(|desc| desc.setting(setting).clone())
        })
}

/// A weapon's selectable `Projectile` links (its ammo types), filtered to the
/// current gun setting and ordered by `ProjectileOptions.order`. This is the
/// canonical ammo-type list - firing, ammo-type cycling, and the HUD all derive
/// from it so they agree. A link with a negative `setting` matches any setting;
/// otherwise it must match the weapon's current `PropGunState.setting`
/// (defaulting to 0 when the weapon has no gun state).
pub fn ordered_projectile_links(world: &World, weapon: EntityId) -> Vec<(i32, ProjectileOptions)> {
    ordered_projectile_links_for_setting(world, weapon, current_gun_setting(world, weapon))
}

/// [`ordered_projectile_links`] against an arbitrary fire setting - what the
/// ammo list *would* be in that mode, which a mode switch needs before it
/// commits to the new setting.
pub fn ordered_projectile_links_for_setting(
    world: &World,
    weapon: EntityId,
    setting: i32,
) -> Vec<(i32, ProjectileOptions)> {
    let mut links = all_projectile_links(world, weapon);
    links.retain(|(_, opts)| opts.setting < 0 || opts.setting == setting);
    links.sort_by_key(|(_, opts)| opts.order);
    links
}

fn all_projectile_links(world: &World, weapon: EntityId) -> Vec<(i32, ProjectileOptions)> {
    get_all_links_with_template(world, weapon, |link| match link {
        Link::Projectile(data) => Some(*data),
        _ => None,
    })
}

/// Whether a gun offers a second fire mode at all. Two independent signals in
/// the shipped data, either of which is enough: the gun links ammo that belongs
/// to setting 1 (the shotgun's double load, the laser's overcharge), or it
/// NAMES the mode in its display strings (the pistol's BURST and the assault
/// rifle's AUTO fire the same ammo, so links alone miss them). Guns with
/// neither - the psi amp, turrets - have one mode.
///
/// The second setting's magazine is deliberately not a signal: a gun that never
/// authored setting 1 still carries the editor's default record there, clip
/// included, so `clip != 0` would give the psi amp a mode it does not have.
fn has_second_fire_mode(second_header: Option<&str>, links: &[(i32, ProjectileOptions)]) -> bool {
    second_header.is_some() || links.iter().any(|(_, opts)| opts.setting == 1)
}

/// Whether `weapon` can switch fire modes. See [`has_second_fire_mode`].
pub fn can_cycle_gun_setting(world: &World, weapon: EntityId) -> bool {
    let links = all_projectile_links(world, weapon);
    let second_header = gun_setting_header(world, weapon, 1);
    has_second_fire_mode(second_header.as_deref(), &links)
}

/// The player's effective skill. Authored factory skills are a mission-local
/// floor (Earth's training guns), reconstructed on load without granting
/// permanent career upgrades. Dark stores both in its player property; our
/// split runtime/persistent sheet combines them here. Missing data means zero.
pub(crate) fn player_skill_level(world: &World, skill: crate::player_stats::Skill) -> i32 {
    let trained = world
        .borrow::<shipyard::UniqueView<crate::quest_info::QuestInfo>>()
        .map(|quests| quests.player_stats().skill_level(skill))
        .unwrap_or(0);
    use crate::player_stats::Skill;
    let index = match skill {
        Skill::StandardWeapons => 0,
        Skill::EnergyWeapons => 1,
        Skill::HeavyWeapons => 2,
        Skill::ExoticWeapons => 3,
        _ => return trained,
    };
    let authored = world
        .borrow::<shipyard::UniqueView<crate::mission::PlayerInfo>>()
        .ok()
        .and_then(|player| {
            world
                .borrow::<View<dark::properties::PropBaseWeaponDesc>>()
                .ok()
                .and_then(|skills| {
                    skills
                        .get(player.entity_id)
                        .ok()
                        .map(|skills| skills.0[index])
                })
        })
        .unwrap_or(0);
    trained.max(authored)
}

/// A gun's condition (`PropGunState`, 0..100), or `None` for a weapon that
/// tracks none - a melee weapon, the psi amp.
pub(crate) fn gun_condition(world: &World, entity_id: EntityId) -> Option<f32> {
    world
        .borrow::<View<PropGunState>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|g| g.condition))
}

/// An object's short display name: its authored `P$ObjShortName` (an object
/// string), falling back to the symbolic name every entity has. This is the
/// terse name the original uses where a whole sentence will not fit - a panel's
/// title bar, a status message - as opposed to the rollover's full name.
pub fn object_short_name(world: &World, entity_id: EntityId) -> Option<String> {
    let short = world
        .borrow::<View<PropObjShortName>>()
        .ok()
        .and_then(|v| {
            v.get(entity_id)
                .ok()
                .map(|n| crate::scripts::gui::localized_fallback(&n.0))
        })
        .filter(|name| !name.is_empty());
    short.or_else(|| {
        world
            .borrow::<View<PropSymName>>()
            .ok()
            .and_then(|v| v.get(entity_id).ok().map(|n| n.0.clone()))
    })
}

/// The short header for `weapon`'s fire setting `setting` - "NORM" / "BURST" -
/// or `None` when the gun names no such setting. Resolved from the gun's own
/// `P$SHead1`/`P$SHead2` against the matching string table, which also covers
/// the guns that author no header property of their own.
pub fn gun_setting_header(world: &World, weapon: EntityId, setting: i32) -> Option<String> {
    let raw = match setting {
        0 => world
            .borrow::<View<PropGunSettingHeader1>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|header| header.0.clone())),
        1 => world
            .borrow::<View<PropGunSettingHeader2>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|header| header.0.clone())),
        _ => return None,
    };
    let tables = world
        .borrow::<UniqueView<crate::mission::mission_core::GlobalGunSettingHeaders>>()
        .ok()?;
    resolve_setting_string(world, weapon, raw, tables.0.get(setting as usize)?)
}

/// Resolve one of a gun's fire-setting object strings against its string table.
/// The gun's own property is only half the answer: a gun that authors no
/// property of its own is still named by the table, keyed on its symbolic name.
fn resolve_setting_string(
    world: &World,
    weapon: EntityId,
    raw: Option<String>,
    table: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let sym_name = world
        .borrow::<View<PropSymName>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().map(|name| name.0.clone()));
    dark::importers::resolve_gun_setting_string(raw.as_deref(), sym_name.as_deref(), table)
}

/// The description text for `weapon`'s fire setting `setting` - "This is the
/// normal single-shot firing mode." - or `None` when the gun names no such
/// setting. Resolved from the gun's own `P$Sett1`/`P$Sett2` against the
/// matching string table, exactly as [`gun_setting_header`] resolves the short
/// header beside it.
pub fn gun_setting_description(world: &World, weapon: EntityId, setting: i32) -> Option<String> {
    let raw = match setting {
        0 => world
            .borrow::<View<PropGunSettingText1>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|text| text.0.clone())),
        1 => world
            .borrow::<View<PropGunSettingText2>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|text| text.0.clone())),
        _ => return None,
    };
    let tables = world
        .borrow::<UniqueView<crate::mission::mission_core::GlobalGunSettingTexts>>()
        .ok()?;
    resolve_setting_string(world, weapon, raw, tables.0.get(setting as usize)?)
}

/// The ammo index to select after a fire-mode switch. `order` is the ammo
/// type's identity (the shotgun's pellets carry the same `order` in both
/// modes), so the selection follows its order across the switch and falls back
/// to the first entry when the new mode has no counterpart.
///
/// Every shipped gun happens to author the same order set in both modes, which
/// makes this the identity mapping today - it is what keeps the switch honest
/// if a mode ever offers a different set.
pub fn remap_selected_ammo(
    current: &[(i32, ProjectileOptions)],
    selected: usize,
    next: &[(i32, ProjectileOptions)],
) -> usize {
    if current.is_empty() {
        return 0;
    }
    // Wrap the index the way every other reader of `RuntimePropSelectedAmmo`
    // does, so the switch remaps the ammo the HUD and the shot agree is chosen.
    let (_, opts) = &current[selected % current.len()];
    next.iter()
        .position(|(_, candidate)| candidate.order == opts.order)
        .unwrap_or(0)
}

/// Whether `weapon` may select a different projectile type: it needs two or
/// more selectable projectiles, and any rounds already loaded must be
/// returnable to the backpack, since `cycle_ammo` ejects the magazine first.
/// Loaded rounds are never converted - they go back as the ammo type they are -
/// so an unreturnable magazine (a projectile with no clip archetype) is the one
/// case that still has to be fired off before the type can change.
pub fn can_cycle_ammo(world: &World, weapon: EntityId) -> bool {
    if ordered_projectile_links(world, weapon).len() < 2 {
        return false;
    }
    let is_empty = world
        .borrow::<View<PropGunState>>()
        .ok()
        .and_then(|states| states.get(weapon).ok().map(|state| state.ammo <= 0))
        .unwrap_or(false);
    is_empty || crate::mission::reload::can_unload(world, weapon)
}

pub fn get_first_link_with_template_and_data<TData: Clone>(
    world: &World,
    producing_entity_id: EntityId,
    filter_fn: fn(&Link) -> Option<TData>,
) -> Option<(i32, TData)> {
    let all_links = get_all_links_with_template(world, producing_entity_id, filter_fn);
    all_links
        .get(0)
        .map(|(template_id, data)| (*template_id, data.clone()))
}

/// The impact effect (spang) a projectile spawns when it hits `victim`, from
/// the projectile's authored spang links:
/// - a creature hit spawns the `HitSpang` whose victim archetype class the
///   victim descends from (blood for hybrids/midwives, sparks for
///   robots/turrets, ...);
/// - anything else falls back to the `MissSpang` (terrain spang).
///
/// Any victim tries the HitSpang match first - not just hitbox hits, so
/// victims without fitted hitboxes (and catch-all victim classes like the
/// laser's `Physical`) still spang - and falls back to the terrain spang when
/// the projectile has no spang authored for that victim's class. `None` when
/// the projectile has no spang links at all. Shared by the fast (raycast) and
/// slow (physics `Collided`) projectile impact paths.
pub fn choose_impact_spang(world: &World, projectile: EntityId, victim: EntityId) -> Option<i32> {
    choose_hit_spang(world, projectile, victim).or_else(|| {
        get_first_link_with_template_and_data(world, projectile, |link| {
            if matches!(link, Link::MissSpang) {
                Some(())
            } else {
                None
            }
        })
        .map(|(spang_template, ())| spang_template)
    })
}

/// The spang (impact effect) a projectile spawns on `victim`, from the
/// projectile's `HitSpang` links: each link targets a victim archetype class
/// (Hybrids, Robots, ...) and carries the spang template to spawn. Links are
/// scanned most-derived-projectile-template first (entity links merge
/// ancestors root-first, so without the reversal a base class's link would
/// shadow an override authored on a derived projectile); the first link whose
/// class the victim is or descends from wins. `victim` may be a hitbox proxy
/// (resolved to its parent creature). `None` when the projectile has no spang
/// authored for this victim's class.
fn choose_hit_spang(world: &World, projectile: EntityId, victim: EntityId) -> Option<i32> {
    let victim = resolve_proxy_entity(world, victim);
    let victim_template = entity_class_template_id(world, victim)?;
    let hierarchy = world.borrow::<UniqueView<GlobalTemplateHierarchy>>().ok()?;
    get_all_links_with_template(world, projectile, |link| {
        if let Link::HitSpang(spang_template) = link {
            Some(*spang_template)
        } else {
            None
        }
    })
    .into_iter()
    .rev()
    .find(|(victim_class, _)| hierarchy.is_or_descends_from(victim_template, *victim_class))
    .map(|(_, spang_template)| spang_template)
}

pub fn for_each_link(
    world: &World,
    producing_entity_id: EntityId,
    foreach_fn: &mut dyn FnMut(&ToLink),
) {
    let links = world.borrow::<View<Links>>().unwrap();
    if let Ok(switch_links) = links.get(producing_entity_id) {
        for link in &switch_links.to_links {
            foreach_fn(link);
        }
    };
}

pub fn get_all_links_with_data<TData>(
    world: &World,
    producing_entity_id: EntityId,
    filter_fn: fn(&Link) -> Option<TData>,
) -> Vec<(EntityId, TData)> {
    let links = world.borrow::<View<Links>>().unwrap();
    let mut linked_entities = Vec::new();
    if let Ok(switch_links) = links.get(producing_entity_id) {
        for link in &switch_links.to_links {
            if let (Some(to_entity_id), Some(data)) = (link.to_entity_id, filter_fn(&link.link)) {
                linked_entities.push((to_entity_id.0, data))
            }
        }
    };

    linked_entities
}

pub fn get_all_links_of_type(
    world: &World,
    producing_entity_id: EntityId,
    link_to_match: Link,
) -> Vec<EntityId> {
    let links = world.borrow::<View<Links>>().unwrap();
    let mut linked_entities = Vec::new();
    if let Ok(switch_links) = links.get(producing_entity_id) {
        for link in &switch_links.to_links {
            if link.link == link_to_match && link.to_entity_id.is_some() {
                linked_entities.push(link.to_entity_id.unwrap().0)
            }
        }
    };

    linked_entities
}

pub fn get_entities_by_name(world: &World, name: &str) -> Vec<EntityId> {
    let mut entities = Vec::new();
    world.run(|v_prop_symyname: View<PropSymName>| {
        for (id, symname) in v_prop_symyname.iter().with_id() {
            if name.eq_ignore_ascii_case(&symname.0) {
                entities.push(id);
            }
        }
    });

    entities
}

pub fn get_first_entity_by_name(world: &World, name: &str) -> Option<EntityId> {
    let entities = get_entities_by_name(world, name);
    entities.get(0).copied()
}

pub fn template_id_string(world: &World, entity_id: &EntityId) -> String {
    let v_template_id = world.borrow::<View<PropTemplateId>>().unwrap();
    let maybe_template = v_template_id.get(*entity_id);
    format!("{:?}", maybe_template)
}

pub fn get_first_link_with_data<TData: Copy>(
    world: &World,
    producing_entity_id: EntityId,
    filter_fn: fn(&Link) -> Option<TData>,
) -> Option<(EntityId, TData)> {
    let all_links = get_all_links_with_data(world, producing_entity_id, filter_fn);
    all_links.get(0).copied()
}

pub fn get_first_link_of_type(
    world: &World,
    producing_entity_id: EntityId,
    link_type: Link,
) -> Option<EntityId> {
    let all_links = get_all_links_of_type(world, producing_entity_id, link_type);
    all_links.get(0).copied()
}

/// Every item the player is currently carrying: each wielded/hand-held entity
/// plus everything nested under it, and the backpack (inventory) entity's
/// contents - following `Contains` links to depth 2 (mirrors the item set the
/// save system and the debug inventory enumerate). The hand/inventory container
/// entities themselves are excluded; a wielded item *is* its hand entity, so it
/// is included. Empty when there is no player (e.g. a debug scene without one).
pub fn player_carried_items(world: &World) -> Vec<EntityId> {
    let player = match world.borrow::<UniqueView<crate::mission::PlayerInfo>>() {
        Ok(player) => player,
        Err(_) => return Vec::new(),
    };

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    // Hands first: the hand entity itself is the wielded item, plus its contents.
    for hand in [player.left_hand_entity_id, player.right_hand_entity_id]
        .into_iter()
        .flatten()
    {
        if seen.insert(hand) {
            out.push(hand);
        }
        collect_contained_items(world, hand, 2, &mut seen, &mut out);
    }

    // Backpack: only the inventory container's contents (the container is not an item).
    seen.insert(player.inventory_entity_id);
    collect_contained_items(world, player.inventory_entity_id, 2, &mut seen, &mut out);

    out
}

/// The player entity with its current and maximum hit points. `None` when the
/// scene has no player or the player has no health pool (e.g. a bare debug
/// scene).
pub fn player_hit_points(world: &World) -> Option<(EntityId, i32, i32)> {
    let player = world
        .borrow::<UniqueView<crate::mission::PlayerInfo>>()
        .ok()?
        .entity_id;
    let current = world
        .borrow::<View<dark::properties::PropHitPoints>>()
        .ok()?
        .get(player)
        .ok()?
        .hit_points;
    let maximum = world
        .borrow::<View<dark::properties::PropMaxHitPoints>>()
        .ok()?
        .get(player)
        .ok()?
        .hit_points
        .min(i32::MAX as u32) as i32;
    Some((player, current, maximum))
}

/// The player's persistent nanite stat balance (0 if there is no player /
/// `QuestInfo`, e.g. a debug scene).
fn stat_nanite_balance(world: &World) -> i32 {
    world
        .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
        .map(|quests| quests.player_stats().nanites)
        .unwrap_or(0)
}

/// Split `amount` (assumed positive) into the stat-first debit and the
/// legacy-stack remainder. Shared by `spend_player_nanites` and
/// `debit_player_nanites` so the two spend paths can't drift on which nanites
/// get spent first.
fn stat_first_split(world: &World, amount: i32) -> (i32, i32) {
    // `.max(0)` guards a corrupt/negative stat balance (e.g. an old save): a
    // negative `stat_debit` here would inflate `remaining` past `amount` and
    // overcharge the carried stacks.
    let stat_debit = amount.min(stat_nanite_balance(world).max(0));
    (stat_debit, amount - stat_debit)
}

/// Build the effects that pay `amount` nanites: debits the player's stat
/// balance first (`Effect::SpendNanites`), then any legacy carried stacks
/// (identified by their inherited inventory icon `nan_ic` - pre-existing
/// saves, panel-taken piles) for the remainder. Returns `None` without side
/// effects when the combined total is insufficient.
pub fn spend_player_nanites(world: &World, amount: i32) -> Option<Effect> {
    if amount <= 0 {
        return Some(Effect::NoEffect);
    }

    let (stat_debit, remaining) = stat_first_split(world, amount);

    let mut effects = Vec::new();
    if stat_debit > 0 {
        effects.push(Effect::SpendNanites { amount: stat_debit });
    }
    if remaining > 0 {
        let (nanite_stacks, debits) = player_nanite_payment_plan(world, remaining)?;
        for ((entity_id, stack), paid) in nanite_stacks.into_iter().zip(debits) {
            if paid == 0 {
                continue;
            }
            if paid == stack {
                effects.push(Effect::DestroyEntity { entity_id });
            } else {
                effects.push(Effect::AdjustStackCount {
                    entity_id,
                    delta: -paid,
                });
            }
        }
    }
    Some(Effect::combine(effects))
}

/// Atomically debit the player's nanite balance: the stat first, then any
/// legacy carried stacks for the remainder. Returns the legacy entities whose
/// stack count reached zero so the mission can remove their runtime state
/// before vending an item. A stale or insufficient plan leaves every stack
/// (and the stat) unchanged.
pub fn debit_player_nanites(world: &World, amount: i32) -> Option<Vec<EntityId>> {
    if amount <= 0 {
        return None;
    }

    let (stat_debit, remaining) = stat_first_split(world, amount);

    let (nanite_stacks, debits) = if remaining > 0 {
        player_nanite_payment_plan(world, remaining)?
    } else {
        (Vec::new(), Vec::new())
    };

    let mut stacks = world
        .borrow::<ViewMut<dark::properties::PropStackCount>>()
        .ok()?;
    // Acquire every fallible borrow up front, before any mutation, so no
    // ordering of the mutations below can leave one applied and the other
    // not - a late-failing borrow here must not have already spent the stat
    // or a stack.
    let mut quests = if stat_debit > 0 {
        Some(
            world
                .borrow::<UniqueViewMut<crate::quest_info::QuestInfo>>()
                .ok()?,
        )
    } else {
        None
    };

    // Validate every live stack before applying any mutation.
    for ((entity_id, _), paid) in nanite_stacks.iter().zip(&debits) {
        if *paid > 0 && stacks.get(*entity_id).ok()?.0 < *paid {
            return None;
        }
    }

    let mut exhausted = Vec::new();
    for ((entity_id, _), paid) in nanite_stacks.into_iter().zip(debits) {
        if paid == 0 {
            continue;
        }
        let stack = (&mut stacks).get(entity_id).ok()?;
        stack.0 -= paid;
        if stack.0 == 0 {
            exhausted.push(entity_id);
        }
    }

    if let Some(quests) = quests.as_mut() {
        quests.player_stats_mut().spend_nanites(stat_debit);
    }

    Some(exhausted)
}

/// The player's total spendable nanite balance: the persistent stat plus any
/// legacy carried stacks (pre-existing saves, panel-taken piles - see
/// `PlayerStats::nanites`). Uses the same identification and traversal as the
/// legacy half of [`spend_player_nanites`], so the UI balance and the debit
/// path cannot disagree.
pub fn player_nanite_total(world: &World) -> i32 {
    stat_nanite_balance(world).saturating_add(
        player_nanite_stacks(world)
            .unwrap_or_default()
            .into_iter()
            .map(|(_, stack)| stack)
            .fold(0, i32::saturating_add),
    )
}

fn player_nanite_payment_plan(
    world: &World,
    amount: i32,
) -> Option<(Vec<(EntityId, i32)>, Vec<i32>)> {
    let nanite_stacks = player_nanite_stacks(world)?;
    let debits = plan_stack_payment(
        &nanite_stacks
            .iter()
            .map(|(_, stack)| *stack)
            .collect::<Vec<_>>(),
        amount,
    )?;
    Some((nanite_stacks, debits))
}

fn player_nanite_stacks(world: &World) -> Option<Vec<(EntityId, i32)>> {
    let icons = world.borrow::<View<dark::properties::PropObjIcon>>().ok()?;
    let stacks = world
        .borrow::<View<dark::properties::PropStackCount>>()
        .ok()?;
    Some(
        player_carried_items(world)
            .into_iter()
            .filter_map(|entity| {
                let icon = icons.get(entity).ok()?;
                let stack = stacks.get(entity).ok()?.0;
                (icon.0.eq_ignore_ascii_case("nan_ic") && stack > 0).then_some((entity, stack))
            })
            .collect(),
    )
}

/// Plan an atomic payment across ordered stacks. Each returned entry is the
/// amount debited from the corresponding stack.
fn plan_stack_payment(stacks: &[i32], amount: i32) -> Option<Vec<i32>> {
    if amount <= 0 {
        return Some(vec![0; stacks.len()]);
    }
    let total = stacks
        .iter()
        .copied()
        .filter(|stack| *stack > 0)
        .fold(0, i32::saturating_add);
    if total < amount {
        return None;
    }

    let mut remaining = amount;
    Some(
        stacks
            .iter()
            .map(|stack| {
                let paid = remaining.min((*stack).max(0));
                remaining -= paid;
                paid
            })
            .collect(),
    )
}

fn collect_contained_items(
    world: &World,
    entity_id: EntityId,
    depth: u32,
    seen: &mut std::collections::HashSet<EntityId>,
    out: &mut Vec<EntityId>,
) {
    if depth == 0 {
        return;
    }
    for_each_link(world, entity_id, &mut |link| {
        if matches!(link.link, Link::Contains(_)) {
            if let Some(to_entity_id) = link.to_entity_id {
                let child = to_entity_id.0;
                if seen.insert(child) {
                    out.push(child);
                    collect_contained_items(world, child, depth - 1, seen, out);
                }
            }
        }
    });
}

/// The `SetQuestBit` effect for an entity's authored quest-bit pair
/// (`PropQuestBitName` + `PropQuestBitValue`, defaulting to `COMPLETE` when no
/// value is authored). `None` when the entity carries no quest-bit name.
/// Shared by every trap that applies a quest bit as part of its action.
pub fn set_quest_bit_effect(world: &World, entity_id: EntityId) -> Option<Effect> {
    use dark::properties::{PropQuestBitName, PropQuestBitValue, QuestBitValue};

    let v_qbname = world.borrow::<View<PropQuestBitName>>().unwrap();
    let v_qbval = world.borrow::<View<PropQuestBitValue>>().unwrap();

    let quest_bit_value = v_qbval
        .get(entity_id)
        .map(|v| v.0)
        .unwrap_or(QuestBitValue::COMPLETE);

    v_qbname
        .get(entity_id)
        .ok()
        .map(|qb_name| Effect::SetQuestBit {
            quest_bit_name: qb_name.0.to_owned(),
            quest_bit_value,
        })
}

pub fn get_all_switch_links(world: &World, producing_entity_id: EntityId) -> Vec<EntityId> {
    let links = world.borrow::<View<Links>>().unwrap();
    let mut linked_entities = Vec::new();
    if let Ok(switch_links) = links.get(producing_entity_id) {
        for link in &switch_links.to_links {
            match &link.link {
                dark::properties::Link::SwitchLink => {
                    if link.to_entity_id.is_some() {
                        linked_entities.push(link.to_entity_id.unwrap().0)
                    }
                }
                _ => (),
            }
        }
    };

    linked_entities
}
pub fn send_to_all_switch_links_and_self(
    world: &World,
    producing_entity_id: EntityId,
    message: MessagePayload,
) -> Effect {
    let send_to_switchlinks_eff =
        send_to_all_switch_links(world, producing_entity_id, message.clone());
    let send_to_self = Effect::Send {
        msg: Message {
            payload: message,
            to: producing_entity_id,
        },
    };
    Effect::Combined {
        effects: vec![send_to_switchlinks_eff, send_to_self],
    }
}

pub fn get_environmental_sound_query(
    world: &World,
    entity_id: EntityId,
    event_type: &str,
    additional_tags: Vec<(&str, &str)>,
) -> Option<EnvSoundQuery> {
    let v_class_tag = world.borrow::<View<PropClassTag>>().unwrap();
    let mut class_tags = v_class_tag
        .get(entity_id)
        .map(|p| p.class_tags())
        .unwrap_or(vec![]);

    if !class_tags.is_empty() {
        let mut query = vec![("event", event_type)];
        query.append(&mut class_tags);
        query.append(&mut additional_tags.clone());
        Some(EnvSoundQuery::from_tag_values(query))
    } else {
        None
    }
}

pub fn play_environmental_sound(
    world: &World,
    entity_id: EntityId,
    event_type: &str,
    additional_tags: Vec<(&str, &str)>,
    audio_handle: AudioHandle,
) -> Effect {
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let maybe_env_sound_query =
        get_environmental_sound_query(world, entity_id, event_type, additional_tags);

    // An entity can be animating without owning a physics transform (the
    // animation players and `id_to_physics` are separate maps, and ownership
    // changes hands around death), so a missing transform is a silent no-sound
    // rather than a panic - there is nowhere to place the sound.
    let (Some(query), Ok(transform)) = (maybe_env_sound_query, v_transform.get(entity_id)) else {
        return Effect::NoEffect;
    };

    {
        let position = transform.0.transform_point(point3(0.0, 0.0, 0.0));
        Effect::PlayEnvironmentalSound {
            audio_handle,
            query,
            position: point3_to_vec3(position),
        }
    }
}

/// Material tag used for impact-schema lookups when the hit surface has no
/// resolvable material. World geometry has no per-texture material lookup in
/// the port yet, so terrain hits (and entities without material tags) all
/// sound like the ship's metal bulkheads.
pub const DEFAULT_IMPACT_MATERIAL: &str = "metal";

/// The collision-schema material tag ("fleshtarget", "metal", ...) for
/// whatever was hit: the victim's inherited `PropMaterial` (a tag string like
/// "Material FleshTarget", authored via archetypes such as `MatFlesh`),
/// falling back to the default for world hits / untagged entities. Hitbox
/// proxies resolve to their parent creature.
fn get_impact_material(world: &World, hit_entity_id: EntityId) -> String {
    let victim = resolve_proxy_entity(world, hit_entity_id);
    world
        .borrow::<View<PropMaterial>>()
        .ok()
        .and_then(|v_material| {
            let raw = &v_material.get(victim).ok()?.0;
            // "Material FleshTarget" -> "fleshtarget"
            let mut tokens = raw.split_whitespace();
            while let Some(token) = tokens.next() {
                if token.eq_ignore_ascii_case("material") {
                    return tokens.next().map(|value| value.to_ascii_lowercase());
                }
            }
            None
        })
        .unwrap_or_else(|| DEFAULT_IMPACT_MATERIAL.to_owned())
}

/// Footstep sound for a creature whose animation just reached an authored
/// foot-plant frame (`MotionFlags::LEFT_FOOT_STEP` / `RIGHT_FOOT_STEP`).
///
/// The schema keys footsteps on `event=footstep` plus the creature's class
/// tags (`creaturetype=oncegrunt`, `=monkey`, `=droid`, ...); most creature
/// types resolve on those alone to a four-sample set (`ft_monk1..4`). Hybrids
/// (`oncegrunt`) branch further on `material` - the creature's *own* material,
/// i.e. what its feet are made of - and `material2`, the surface underfoot.
/// The port has no per-texture lookup for world geometry, so `material2` is
/// the default bulkhead metal, which is what most of the ship is; on a hybrid
/// that resolves to `ft_ogm*`.
///
/// Creature types the schema authors no footsteps for (swarms, apparitions,
/// SHODAN) resolve to nothing and fall through silently.
pub fn play_footstep_sound(world: &World, entity_id: EntityId) -> Effect {
    let own_material = get_impact_material(world, entity_id);
    play_environmental_sound(
        world,
        entity_id,
        "footstep",
        vec![
            ("material", &own_material),
            ("material2", DEFAULT_IMPACT_MATERIAL),
        ],
        AudioHandle::new(),
    )
}

/// Impact/collision sound for `entity_id` (a projectile or melee weapon)
/// hitting `hit_entity_id`, played at the impact point. The schema query is
/// the entity's class tags (ammotype for bullets, weapontype for melee) plus
/// event=collision and the hit surface's material - e.g. a pistol bullet on a
/// wall resolves (event=collision, ammotype=std, material=metal) -> `bulmet*`,
/// on a hybrid (event=collision, ammotype=std, material=fleshtarget) ->
/// `bulftar*`.
pub fn play_impact_sound(
    world: &World,
    entity_id: EntityId,
    hit_entity_id: EntityId,
    position: Vector3<f32>,
) -> Effect {
    let material = get_impact_material(world, hit_entity_id);
    let maybe_query =
        get_environmental_sound_query(world, entity_id, "collision", vec![("material", &material)]);

    if let Some(query) = maybe_query {
        Effect::PlayEnvironmentalSound {
            audio_handle: AudioHandle::new(),
            query,
            position,
        }
    } else {
        Effect::NoEffect
    }
}

pub fn send_to_all_switch_links(
    world: &World,
    producing_entity_id: EntityId,
    message: MessagePayload,
) -> Effect {
    let entities = get_all_switch_links(world, producing_entity_id);
    let effects = entities
        .iter()
        .map(|to| Effect::Send {
            msg: Message {
                to: *to,
                payload: message.clone(),
            },
        })
        .collect::<Vec<Effect>>();

    Effect::Combined { effects }
}

/// Hydrate a single template in a scratch world, grab a component, and drop it again.
/// Useful for debugging/inspecting template properties without spawning into the main world.
pub fn hydrate_template_component<T>(
    template_id: i32,
    entity_info: &SystemShock2EntityInfo,
) -> Option<T>
where
    T: Component + Clone + Send + Sync,
{
    let mut temp_world = World::new();
    let dummy_entity = temp_world.add_entity(());

    initialize_entity_with_props(
        template_id,
        entity_info,
        &mut temp_world,
        dummy_entity,
        &HashMap::new(), // empty obj_name_map for utility function
    );

    // Try to get the component from the hydrated entity
    let v_component = temp_world.borrow::<View<T>>();
    if let Ok(view) = v_component {
        if let Ok(component) = view.get(dummy_entity) {
            return Some(component.clone());
        }
    }

    None
}
pub fn invert(msg: MessagePayload) -> MessagePayload {
    match msg {
        MessagePayload::TurnOff { from } => MessagePayload::TurnOn { from },
        MessagePayload::TurnOn { from } => MessagePayload::TurnOff { from },
        m => m,
    }
}

pub fn change_to_last_model(world: &World, entity_id: EntityId) -> Effect {
    let v_prop_tweqmodelconfig = world.borrow::<View<PropTweqModelConfig>>().unwrap();

    if let Ok(model_config) = v_prop_tweqmodelconfig.get(entity_id) {
        let model_names = &model_config.model_names;
        if !model_names.is_empty() {
            let model_name = model_names.last().unwrap();
            Effect::ChangeModel {
                entity_id,
                model_name: model_name.to_owned(),
            }
        } else {
            Effect::NoEffect
        }
    } else {
        Effect::NoEffect
    }
}

pub fn change_to_first_model(world: &World, entity_id: EntityId) -> Effect {
    let v_prop_tweqmodelconfig = world.borrow::<View<PropTweqModelConfig>>().unwrap();

    if let Ok(model_config) = v_prop_tweqmodelconfig.get(entity_id) {
        let model_names = &model_config.model_names;
        if !model_names.is_empty() {
            let model_name = model_names.get(0).unwrap();
            Effect::ChangeModel {
                entity_id,
                model_name: model_name.to_owned(),
            }
        } else {
            Effect::NoEffect
        }
    } else {
        Effect::NoEffect
    }
}

#[cfg(test)]
mod tests {
    use super::{
        active_gun_setting, debit_player_nanites, door_blocks_pathfinding, door_is_closed,
        door_open_progress, has_second_fire_mode, is_always_collected, is_nanite_pickup,
        plan_stack_payment, player_nanite_total, remap_selected_ammo, spend_player_nanites,
        stat_nanite_balance,
    };
    use crate::mission::PlayerInfo;
    use crate::quest_info::QuestInfo;
    use crate::runtime_props::RuntimePropTransform;
    use cgmath::{Matrix4, Quaternion, Vector3, vec3};
    use dark::properties::{
        GunSettingDesc, Link, Links, ProjectileOptions, PropBaseGunDesc, PropGunState, PropObjIcon,
        PropStackCount, PropTranslatingDoor, ToLink, WrappedEntityId,
    };
    use shipyard::{EntityId, Get, View, World};

    fn gun_desc() -> PropBaseGunDesc {
        let setting = |clip| GunSettingDesc {
            clip,
            ..GunSettingDesc::default()
        };
        PropBaseGunDesc {
            settings: [setting(10), setting(20), setting(30)],
        }
    }

    fn gun_state(setting: i32) -> PropGunState {
        PropGunState {
            ammo: 0,
            condition: 100.0,
            setting,
            modification: 0,
            silence_value: 0.0,
        }
    }

    #[test]
    fn active_gun_setting_follows_the_live_gun_state() {
        let mut world = World::new();
        let weapon = world.add_entity((gun_desc(), gun_state(1)));

        assert_eq!(active_gun_setting(&world, weapon).unwrap().clip, 20);
    }

    #[test]
    fn active_gun_setting_defaults_to_the_first_setting_without_a_gun_state() {
        let mut world = World::new();
        let weapon = world.add_entity(gun_desc());

        assert_eq!(active_gun_setting(&world, weapon).unwrap().clip, 10);
    }

    #[test]
    fn active_gun_setting_is_absent_for_a_non_gun() {
        let mut world = World::new();
        let not_a_gun = world.add_entity(gun_state(0));

        assert!(active_gun_setting(&world, not_a_gun).is_none());
    }

    fn projectile(order: i32, setting: i32) -> ProjectileOptions {
        ProjectileOptions { order, setting }
    }

    /// The shotgun: pellets and slugs in each mode, the same `order` in both.
    fn shotgun_ammo() -> (Vec<(i32, ProjectileOptions)>, Vec<(i32, ProjectileOptions)>) {
        (
            vec![(-524, projectile(0, 0)), (-516, projectile(1, 0))],
            vec![(-3423, projectile(0, 1)), (-3422, projectile(1, 1))],
        )
    }

    /// The pistol: one ammo set shared by both modes, so only its BURST header
    /// says the second mode exists.
    #[test]
    fn a_gun_that_names_its_second_mode_has_one() {
        let shared_ammo = [
            (-362, projectile(1, -1)),
            (-492, projectile(2, -1)),
            (-33, projectile(3, -1)),
        ];

        assert!(has_second_fire_mode(Some("BURST"), &shared_ammo));
    }

    /// The shotgun: setting-1 ammo, so the links alone settle it.
    #[test]
    fn a_gun_with_setting_specific_ammo_has_a_second_fire_mode() {
        let (normal, double) = shotgun_ammo();
        let links = [normal, double].concat();

        assert!(has_second_fire_mode(None, &links));
    }

    /// The psi amp: no setting-1 ammo and no name for a second mode.
    #[test]
    fn a_gun_with_neither_has_no_second_fire_mode() {
        assert!(!has_second_fire_mode(None, &[(-362, projectile(1, -1))]));
        assert!(!has_second_fire_mode(None, &[]));
    }

    #[test]
    fn a_switch_keeps_the_selected_ammo_order() {
        let (normal, double) = shotgun_ammo();

        // Slugs (order 1) stay slugs; pellets (order 0) stay pellets.
        assert_eq!(remap_selected_ammo(&normal, 1, &double), 1);
        assert_eq!(remap_selected_ammo(&normal, 0, &double), 0);
        assert_eq!(remap_selected_ammo(&double, 1, &normal), 1);
    }

    #[test]
    fn a_switch_falls_back_to_the_first_ammo_when_the_order_is_gone() {
        let (normal, _) = shotgun_ammo();
        let only_pellets = vec![(-3423, projectile(0, 1))];

        assert_eq!(remap_selected_ammo(&normal, 1, &only_pellets), 0);
    }

    #[test]
    fn a_switch_wraps_an_out_of_range_selection_like_every_other_reader() {
        let (normal, double) = shotgun_ammo();

        assert_eq!(remap_selected_ammo(&normal, 7, &double), 1, "7 % 2 = slugs");
        assert_eq!(remap_selected_ammo(&[], 0, &double), 0, "no ammo at all");
    }

    fn door_world(
        closed: Vector3<f32>,
        open: Vector3<f32>,
        state: i32,
        at: Vector3<f32>,
    ) -> (World, EntityId) {
        let mut world = World::new();
        let entity_id = world.add_entity((
            PropTranslatingDoor {
                door_type: 1,
                closed: 0.0,
                open: 0.0,
                speed: 0.0,
                axis: 0,
                state,
                base_closed_location: closed,
                base_open_location: open,
                base_location: closed,
            },
            RuntimePropTransform(Matrix4::from_translation(at)),
        ));
        (world, entity_id)
    }

    #[test]
    fn a_zero_travel_door_authored_open_is_not_reported_closed() {
        // hydro2's survey-lab doors: open and closed endpoints coincide, so
        // the distance comparison is degenerate and must not decide (#602).
        let at = vec3(18.0, -0.4, 41.8);
        let (world, door) = door_world(at, at, 1, at);

        assert_eq!(door_is_closed(&world, door), Some(false));
    }

    #[test]
    fn a_zero_travel_door_authored_closed_is_still_reported_closed() {
        let at = vec3(18.0, -0.4, 41.8);
        let (world, door) = door_world(at, at, 0, at);

        assert_eq!(door_is_closed(&world, door), Some(true));
    }

    #[test]
    fn a_zero_travel_door_authored_closed_blocks_pathfinding() {
        // medsci1's space shields have no lock, but have nowhere to move:
        // A* must treat their below-door cells as permanently sealed (#606).
        let at = vec3(-15.5, 1.4, 61.0);
        let (world, door) = door_world(at, at, 0, at);

        assert!(door_blocks_pathfinding(&world, door));
    }

    #[test]
    fn a_permanently_open_door_does_not_block_pathfinding() {
        let at = vec3(18.0, -0.4, 41.8);
        let (world, door) = door_world(at, at, 1, at);

        assert!(!door_blocks_pathfinding(&world, door));
    }

    #[test]
    fn an_unlocked_travelling_door_does_not_block_pathfinding() {
        let closed = vec3(10.3, -0.4, 42.0);
        let open = vec3(10.3, -0.4, 44.3);
        let (world, door) = door_world(closed, open, 0, closed);

        assert!(!door_blocks_pathfinding(&world, door));
    }

    #[test]
    fn a_normal_door_is_reported_from_its_live_position() {
        let closed = vec3(10.3, -0.4, 42.0);
        let open = vec3(10.3, -0.4, 44.3);
        let (closed_world, at_closed) = door_world(closed, open, 0, closed);
        let (open_world, at_open) = door_world(closed, open, 0, open);

        assert_eq!(door_is_closed(&closed_world, at_closed), Some(true));
        assert_eq!(door_is_closed(&open_world, at_open), Some(false));
    }

    #[test]
    fn a_rising_leaf_reports_its_progress_and_the_gap_beneath_it() {
        let closed = vec3(39.2, 0.5, -37.6);
        let open = vec3(39.2, 4.1, -37.6);
        let half = vec3(39.2, 2.3, -37.6);
        let (world, door) = door_world(closed, open, 0, half);

        let (fraction, rise) = door_open_progress(&world, door).unwrap();
        assert!((fraction - 0.5).abs() < 1e-4, "fraction {fraction}");
        assert!((rise - 1.8).abs() < 1e-4, "rise {rise}");
    }

    #[test]
    fn a_sideways_leaf_reports_progress_but_no_gain_in_height() {
        let closed = vec3(10.3, -0.4, 42.0);
        let open = vec3(10.3, -0.4, 44.3);
        let (world, door) = door_world(closed, open, 0, open);

        let (fraction, rise) = door_open_progress(&world, door).unwrap();
        assert!((fraction - 1.0).abs() < 1e-4, "fraction {fraction}");
        assert_eq!(rise, 0.0);
    }

    #[test]
    fn a_door_with_nowhere_to_go_has_no_progress_to_report() {
        let at = vec3(18.0, -0.4, 41.8);
        let (world, door) = door_world(at, at, 1, at);

        assert_eq!(door_open_progress(&world, door), None);
    }

    #[test]
    fn stack_payment_is_atomic_when_total_is_insufficient() {
        assert_eq!(plan_stack_payment(&[1, 1], 3), None);
    }

    #[test]
    fn stack_payment_consumes_full_stacks_before_partial_stack() {
        assert_eq!(plan_stack_payment(&[2, 5, 4], 9), Some(vec![2, 5, 2]));
    }

    #[test]
    fn stack_payment_ignores_non_positive_entries() {
        assert_eq!(plan_stack_payment(&[-1, 0, 5], 3), Some(vec![0, 0, 3]));
    }

    #[test]
    fn live_nanite_debit_crosses_stacks_and_refuses_a_second_stale_purchase() {
        let mut world = World::new();
        let first = world.add_entity((PropObjIcon("nan_ic".to_owned()), PropStackCount(2)));
        let second = world.add_entity((PropObjIcon("nan_ic".to_owned()), PropStackCount(3)));
        let inventory = world.add_entity(Links {
            to_links: vec![
                ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(first)),
                    link: Link::Contains(0),
                },
                ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(second)),
                    link: Link::Contains(0),
                },
            ],
        });
        let player = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });

        assert_eq!(debit_player_nanites(&world, 3), Some(vec![first]));
        assert_eq!(
            debit_player_nanites(&world, 3),
            None,
            "a second same-tick purchase must revalidate the mutated balance"
        );

        let stacks = world.borrow::<View<PropStackCount>>().unwrap();
        assert_eq!(stacks.get(first).unwrap().0, 0);
        assert_eq!(stacks.get(second).unwrap().0, 2);
        drop(stacks);
        assert_eq!(
            debit_player_nanites(&world, 0),
            None,
            "the authoritative debit path must reject a free purchase"
        );
    }

    fn world_with_stat_nanites(amount: i32) -> World {
        let world = World::new();
        let mut quests = QuestInfo::new();
        quests.player_stats_mut().award_nanites(amount);
        world.add_unique(quests);
        world
    }

    /// As [`world_with_stat_nanites`], plus one legacy carried nanite stack
    /// (in the player's backpack) of `carried_amount` - the setup shared by
    /// every test that exercises the stat-then-legacy-stack split. Returns
    /// the stack entity so callers can assert on it directly.
    fn world_with_stat_and_carried_nanites(
        stat_amount: i32,
        carried_amount: i32,
    ) -> (World, EntityId) {
        let mut world = world_with_stat_nanites(stat_amount);
        let stack = world.add_entity((
            PropObjIcon("nan_ic".to_owned()),
            PropStackCount(carried_amount),
        ));
        let inventory = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(stack)),
                link: Link::Contains(0),
            }],
        });
        let player = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        (world, stack)
    }

    /// End-to-end coverage of the authoritative live-mutation path
    /// (`debit_player_nanites`, the same call `gui::replicator`'s
    /// `Effect::ReplicatorPurchase` handler in `mission_core` makes) with
    /// BOTH a stat balance and a carried legacy stack present. Every world
    /// nanite pickup now collects straight into the stat and never leaves a
    /// carried entity behind, so a live mission can no longer reach this
    /// stat-then-legacy-stack crossing without an old save or a debug-spawned
    /// pile - this is the unit-level stand-in for that scenario.
    #[test]
    fn live_nanite_debit_crosses_from_the_stat_into_a_carried_stack_and_exhausts_it() {
        let (world, stack) = world_with_stat_and_carried_nanites(5, 8);

        // 6 nanites: 5 from the stat, 1 from the carried stack.
        let exhausted = debit_player_nanites(&world, 6).expect("balance covers the cost");
        assert!(
            exhausted.is_empty(),
            "a partial stack debit must not report the stack as exhausted"
        );
        assert_eq!(
            stat_nanite_balance(&world),
            0,
            "the stat should be fully drained before the carried stack is touched"
        );
        assert_eq!(
            world
                .borrow::<View<PropStackCount>>()
                .unwrap()
                .get(stack)
                .unwrap()
                .0,
            7,
            "8 - 1 = 7 remaining in the carried stack"
        );

        // A further debit of 7 exactly exhausts the carried stack (the stat
        // is already at zero), so it must be reported for destruction - the
        // mission's ReplicatorPurchase handler destroys exactly these ids.
        let exhausted =
            debit_player_nanites(&world, 7).expect("the remaining carried stack covers this");
        assert_eq!(
            exhausted,
            vec![stack],
            "an exactly-exhausted carried stack must be returned for destruction"
        );
        assert_eq!(
            world
                .borrow::<View<PropStackCount>>()
                .unwrap()
                .get(stack)
                .unwrap()
                .0,
            0
        );

        // Both the stat and the carried stack are now spent.
        assert_eq!(
            debit_player_nanites(&world, 1),
            None,
            "a debit against an exhausted stat and stack must be refused"
        );
    }

    #[test]
    fn player_nanite_total_combines_stat_and_legacy_carried_stacks() {
        let (world, _first) = world_with_stat_and_carried_nanites(15, 2);

        assert_eq!(player_nanite_total(&world), 17);
    }

    #[test]
    fn spend_player_nanites_debits_the_stat_before_legacy_stacks() {
        let (world, first) = world_with_stat_and_carried_nanites(5, 2);

        // 6 nanites: 5 come from the stat, 1 from the legacy stack.
        let effect = spend_player_nanites(&world, 6).expect("balance covers the cost");
        let effects = match effect {
            crate::scripts::Effect::Combined { effects } => effects,
            other => vec![other],
        };
        assert!(
            effects.iter().any(
                |e| matches!(e, crate::scripts::Effect::SpendNanites { amount } if *amount == 5)
            )
        );
        assert!(effects.iter().any(|e| matches!(
            e,
            crate::scripts::Effect::AdjustStackCount { entity_id, delta }
                if *entity_id == first && *delta == -1
        )));

        // 100 nanites is more than the combined balance (7) - refused, no
        // partial effect.
        assert!(spend_player_nanites(&world, 100).is_none());
    }

    #[test]
    fn is_nanite_pickup_false_without_a_template_hierarchy() {
        // No `GlobalTemplateHierarchy` unique registered (e.g. a bare test
        // world) - must not panic, and must not misidentify anything.
        let mut world = World::new();
        let entity = world.add_entity(());
        assert!(!is_nanite_pickup(&world, entity));
    }

    /// The category, and the world paths that read it: a squeeze at a pickup
    /// (and the flat crosshair pickup, which shares
    /// `uses_scripted_world_frob`) must Frob all four rather than grab them.
    #[test]
    fn every_always_collected_category_frobs_in_the_world() {
        use crate::test_support::{CollectedKind, spawn_collected, spawn_ordinary_loot};

        for kind in CollectedKind::ALL {
            let mut world = World::new();
            let pickup = spawn_collected(&mut world, kind);
            assert!(
                is_always_collected(&world, pickup),
                "{kind:?} belongs to the always-collected category"
            );
            assert!(
                crate::virtual_hand::uses_scripted_world_frob(&world, pickup),
                "{kind:?} must be taken through its script in the world"
            );
        }

        let mut world = World::new();
        let loot = spawn_ordinary_loot(&mut world);
        assert!(!is_always_collected(&world, loot));
        assert!(!crate::virtual_hand::uses_scripted_world_frob(&world, loot));
    }

    /// A disc whose log slot is unset - the gamesys archetypes, and anything
    /// spawned from them without one - frobs to nothing, so it must keep the
    /// ordinary pickup path instead of being routed at a collect that would
    /// silently strand it.
    #[test]
    fn a_disc_without_a_readable_log_is_not_always_collected() {
        let mut world = World::new();
        let disc = world.add_entity(dark::properties::PropScripts {
            scripts: vec!["LogDiscScript".to_owned()],
            inherits: true,
        });
        assert!(!is_always_collected(&world, disc));
    }
}

#[cfg(test)]
mod projectile_contact_tests {
    use super::*;
    use crate::{
        mission::stim_response::GlobalContactStims,
        runtime_props::{RuntimePropProxyEntity, RuntimePropShotModifiers},
        scripts::DamageImpact,
    };
    use cgmath::vec3;
    use dark::properties::{ReceptronEffect, ReceptronOptions};

    #[test]
    fn contact_damage_resolves_parent_receptrons_then_forwards_through_the_limb() {
        let mut world = World::new();
        world.add_unique(GlobalContactStims(HashMap::from([(-362, vec![(-3, 2.0)])])));
        let victim = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: -3,
                to_entity_id: None,
                link: Link::Receptron(ReceptronOptions {
                    order: 1,
                    effect: ReceptronEffect::Damage {
                        multiplier: 3.0,
                        use_intensity: true,
                    },
                }),
            }],
        });
        let limb = world.add_entity(RuntimePropProxyEntity(victim));
        let projectile = world.add_entity((
            PropTemplateId { template_id: -362 },
            RuntimePropShotModifiers {
                stim: 1.5,
                ..Default::default()
            },
        ));
        let impact = DamageImpact {
            direction: vec3(1.0, 0.0, 0.0),
            point: vec3(2.0, 3.0, 4.0),
            bone: None,
        };
        let Effect::Send { msg } =
            projectile_contact_effects(&world, projectile, limb, Some(impact))
        else {
            panic!("authored contact damage must reach the struck limb");
        };
        assert_eq!(msg.to, limb);
        let MessagePayload::Damage {
            amount,
            impact: Some(result),
        } = msg.payload
        else {
            panic!("missing impact")
        };
        assert_eq!(amount, 9.0); // 2 * 1.5 * 3; the hitbox applies limb scaling later.
        assert_eq!(result.bone, None);
        assert_eq!(result.point, impact.point);
        assert_eq!(result.direction, impact.direction);
    }

    #[test]
    fn non_damage_and_immune_contacts_do_not_send_damage_messages() {
        let mut world = World::new();
        world.add_unique(GlobalContactStims(HashMap::from([(
            -1352,
            vec![(-1486, 8.0)],
        )])));
        let projectile = world.add_entity(PropTemplateId { template_id: -1352 });
        let immune = world.add_entity(());
        assert!(matches!(
            projectile_contact_effects(&world, projectile, immune, None),
            Effect::NoEffect
        ));
        let victim = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: -1486,
                to_entity_id: None,
                link: Link::Receptron(ReceptronOptions {
                    order: 82,
                    effect: ReceptronEffect::Freeze {
                        duration_multiplier: 1,
                    },
                }),
            }],
        });
        assert!(matches!(
            Effect::flatten(vec![projectile_contact_effects(&world, projectile, victim, None)]).as_slice(),
            [Effect::Send { msg: Message { to, payload: MessagePayload::Freeze { duration_seconds } } }]
                if *to == victim && *duration_seconds == 8.0
        ));
    }
}
