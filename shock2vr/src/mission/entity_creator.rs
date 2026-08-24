use std::{collections::HashMap, rc::Rc};

use crate::{
    creature::get_creature_definition,
    physics::DynamicPhysicsOptions,
    runtime_props::*,
    time::Time,
    util::{get_rotation_from_matrix, has_refs, point3_to_vec3},
};
use engine::physics_log;

use cgmath::{
    EuclideanSpace, Matrix4, Point3, Quaternion, SquareMatrix, Transform, Vector3, Zero,
    num_traits::abs, vec3,
};
use collision::Aabb3;
use dark::{
    BitmapAnimation, SCALE_FACTOR,
    importers::{ANIMATION_CLIP_IMPORTER, BITMAP_ANIMATION_IMPORTER, MODELS_IMPORTER},
    model::Model,
    motion::AnimationPlayer,
    properties::{
        FrobFlag, InternalPropOriginalModelName, Link, Links, PhysicsModelType, PoseType, PropAI,
        PropClassTag, PropCollisionType, PropCreature, PropCreaturePose, PropFrobInfo,
        PropHUDSelect, PropHasRefs, PropHitPoints, PropImmobile, PropKeySrc, PropLimbModel,
        PropModelName, PropPhysAttr, PropPhysDimensions, PropPhysState, PropPhysType,
        PropPlayerGun, PropPosition, PropRenderType, PropScale, PropSymName, PropTemplateId,
        PropTranslatingDoor, PropTripFlags, RenderType, StimPropagator, StimSourceOptions,
        TemplateLinks, WrappedEntityId,
    },
    ss2_entity_info,
};
use engine::assets::asset_cache::AssetCache;
use rapier3d::prelude::RigidBodyHandle;
use shipyard::{EntitiesView, EntityId, Get, UniqueView, View, ViewMut, World};
use tracing::warn;

use crate::{
    physics::{CollisionGroup, PhysicsShape, PhysicsWorld},
    runtime_props::RuntimePropTransform,
    scripts::ScriptWorld,
};

#[derive(Clone)]
pub struct EntityCreationInfo {
    pub entity_id: EntityId,
    pub model: Option<(Model, Option<AnimationPlayer>)>,
    pub bitmap_animation: Option<Rc<BitmapAnimation>>,
    pub rigid_body: Option<RigidBodyHandle>,
    #[allow(dead_code)]
    pub scripts: Vec<String>,
}

fn needs_internal_simple_health(
    has_hit_points: bool,
    is_creature: bool,
    authored_scripts: &[String],
) -> bool {
    !is_creature
        && (has_hit_points
            || authored_scripts
                .iter()
                .any(|script| script.eq_ignore_ascii_case("TriggerDestroy")))
}

fn needs_internal_triggered_melee(has_limb_model: bool) -> bool {
    has_limb_model
}

fn needs_internal_ai(has_ai: bool, authored_scripts: &[String]) -> bool {
    has_ai
        && !authored_scripts
            .iter()
            .any(|script| script.eq_ignore_ascii_case("BaseMonster"))
}

/// Whether `template_id` (or an ancestor) is one of the nanite-pile
/// templates - see `scripts::script_util::NANITE_PILE_TEMPLATE_IDS` for why
/// this is narrower than the shared `Nanites` base template.
fn is_nanite_pickup_template(
    entity_info: &ss2_entity_info::SystemShock2EntityInfo,
    template_id: i32,
) -> bool {
    crate::scripts::script_util::is_nanite_pile_template(
        ss2_entity_info::get_hierarchy(entity_info),
        template_id,
    )
}

pub fn create_entity_with_position(
    template_id: i32,
    position: Point3<f32>,
    orientation: Quaternion<f32>,
    root_transform: Matrix4<f32>,
    world: &mut World,
    physics: &mut PhysicsWorld,
    asset_cache: &mut AssetCache,
    script_world: &mut ScriptWorld,
    entity_info: &ss2_entity_info::SystemShock2EntityInfo,
    obj_name_map: &HashMap<i32, String>, // name override map
    template_to_entity_id: &HashMap<i32, WrappedEntityId>, // realized entities from level start
    additional_options: CreateEntityOptions,
) -> EntityCreationInfo {
    // Create initial entity
    let entity_id = world.add_entity(());

    // Add props, based on inheritance
    initialize_entity_with_props(template_id, entity_info, world, entity_id, obj_name_map);

    if additional_options.force_visible {
        world.add_component(entity_id, PropHasRefs(true));
        world.add_component(entity_id, PropRenderType(RenderType::Normal));
    };

    initialize_links_for_entity(
        template_id,
        entity_id,
        entity_info,
        template_to_entity_id,
        world,
    );

    let scale = {
        let v_scale = world.borrow::<View<PropScale>>().unwrap();
        v_scale
            .get(entity_id)
            .map(|p| p.0)
            .unwrap_or(vec3(1.0, 1.0, 1.0))
    };

    let _time_in_seconds = {
        let u_time = world.borrow::<UniqueView<Time>>().unwrap();
        u_time.total.as_secs_f32()
    };

    let transformed_position = root_transform.transform_point(position);

    let transform_rotation = get_rotation_from_matrix(&root_transform);

    // Override position, rotation props
    world.add_component(
        entity_id,
        PropPosition {
            position: point3_to_vec3(transformed_position),
            rotation: transform_rotation * orientation,
            cell: 0,
        },
    );

    let transform = root_transform
        * Matrix4::from_translation(position.to_vec())
        * Matrix4::from(orientation)
        * Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z);

    world.add_component(entity_id, RuntimePropTransform(transform));

    // Optionally bolt this entity to a parent's transform (e.g. a muzzle flash to
    // its weapon) so it tracks the parent each frame. Capture the spawn-time
    // relative pose; if the parent has no transform yet, fall back to no
    // attachment (the entity stays at its initial pose).
    if let Some(parent) = additional_options.attach_to {
        let maybe_local = {
            let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
            v_transform
                .get(parent)
                .ok()
                .and_then(|p| p.0.invert())
                .map(|inv_parent| inv_parent * transform)
        };
        if let Some(local_transform) = maybe_local {
            world.add_component(
                entity_id,
                RuntimePropAttachment {
                    parent,
                    local_transform,
                },
            );
        }
    }

    if additional_options.transient_fx {
        world.add_component(entity_id, crate::runtime_props::RuntimePropTransientFx);
    }

    if let Some(origin) = additional_options.projectile_raycast_origin {
        world.add_component(entity_id, RuntimePropProjectileRayOrigin(origin));
    }

    if additional_options.launch_projectile {
        world.add_component(entity_id, RuntimePropLaunchedProjectile);
    }

    create_entity_core(
        entity_id,
        template_id,
        world,
        physics,
        asset_cache,
        script_world,
        entity_info,
        template_to_entity_id,
        obj_name_map,
        additional_options,
    )
}

pub fn initialize_entity(
    entity_id: EntityId,
    template_id: i32,
    world: &mut World,
    physics: &mut PhysicsWorld,
    asset_cache: &mut AssetCache,
    script_world: &mut ScriptWorld,
    entity_info: &ss2_entity_info::SystemShock2EntityInfo,
    obj_name_map: &HashMap<i32, String>, // name override map
    template_to_entity_id: &HashMap<i32, WrappedEntityId>, // realized entities from level start
    additional_options: CreateEntityOptions,
) -> EntityCreationInfo {
    let scale = {
        let v_scale = world.borrow::<View<PropScale>>().unwrap();
        v_scale
            .get(entity_id)
            .map(|p| p.0)
            .unwrap_or(vec3(1.0, 1.0, 1.0))
    };

    let v_position = world.borrow::<View<PropPosition>>().unwrap();
    let maybe_position = v_position.get(entity_id).cloned();
    drop(v_position);

    if let Ok(position) = maybe_position {
        let transform = Matrix4::from_translation(position.position)
            * Matrix4::from(position.rotation)
            * Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z);

        world.add_component(entity_id, RuntimePropTransform(transform));
    };

    create_entity_core(
        entity_id,
        template_id,
        world,
        physics,
        asset_cache,
        script_world,
        entity_info,
        template_to_entity_id,
        obj_name_map,
        additional_options,
    )
}

pub fn create_entity_core(
    entity_id: EntityId,
    template_id: i32,
    world: &mut World,
    physics: &mut PhysicsWorld,
    asset_cache: &mut AssetCache,
    script_world: &mut ScriptWorld,
    entity_info: &ss2_entity_info::SystemShock2EntityInfo,
    _template_to_entity_id: &HashMap<i32, WrappedEntityId>, // realized entities from level start
    obj_map: &HashMap<i32, String>,
    additional_options: CreateEntityOptions,
) -> EntityCreationInfo {
    // Add template id
    world.add_component(entity_id, PropTemplateId { template_id });

    // Add spawn time prop
    let time_in_seconds = {
        let u_time = world.borrow::<UniqueView<Time>>().unwrap();
        u_time.total.as_secs_f32()
    };
    world.add_component(entity_id, RuntimePropSpawnTimeInSeconds(time_in_seconds));

    // Initialize sym name based on level obj map
    initialize_sym_name_from_obj_map(template_id, entity_id, entity_info, obj_map, world);

    // Add links, based on template
    // initialize_links_for_entity(
    //     template_id,
    //     entity_id,
    //     entity_info,
    //     template_to_entity_id,
    //     world,
    // );

    // A zero-travel door authored open has no retracted transform at which to
    // draw its leaf. Keep the logical entity and scripts, but omit its visual
    // just as its permanently-open state omits the collider (#608).
    let create_visual = should_create_visual(world, entity_id);

    // Create model, if we can
    let maybe_model = if create_visual {
        create_model(world, asset_cache, entity_id)
    } else {
        None
    };
    let maybe_just_model = maybe_model.clone().map(|m| m.0);

    // Create bitmap animation, if no model
    let bitmap_animation = if create_visual && maybe_model.is_none() {
        create_bitmap(world, asset_cache, entity_id)
    } else {
        None
    };

    if bitmap_animation.is_some() {
        let frame_count = bitmap_animation.clone().unwrap().total_frames();
        world.add_component(
            entity_id,
            RuntimeBitmapAnimationFrameCount(frame_count as u32),
        );
    }

    // Create physics representation
    let rigid_body = if has_refs(world, entity_id) {
        create_physics_representation_with_options(
            world,
            physics,
            &maybe_just_model.as_ref(),
            entity_id,
            additional_options.launch_projectile,
            additional_options.flinderize_debris,
        )
    } else {
        None
    };

    //let output_scripts = vec![];
    // Create scripts
    let v_scripts = world
        .borrow::<View<dark::properties::PropScripts>>()
        .unwrap();

    let mut processed_scripts = if let Ok(scripts) = v_scripts.get(entity_id) {
        // Map TrapSoundAmb -> TrapSound
        scripts
            .scripts
            .iter()
            .map(|s| {
                if s == "TrapSoundAmb" {
                    "TrapSound".to_owned()
                } else {
                    s.to_owned()
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    // Create any internal scripts to power some properties

    // Dark's AI/creature service is independent of object-script inheritance.
    // Most creatures inherit BaseMonster, which is where this port currently
    // constructs their concrete AI implementation, but mission actors can
    // deliberately replace their object scripts. Keep PropAI authoritative so
    // that override does not also erase animation, damage, and signal-response
    // handling (medsci1 ThreatenOG is the shipped example).
    let v_ai = world.borrow::<View<PropAI>>().unwrap();
    if needs_internal_ai(v_ai.get(entity_id).is_ok(), &processed_scripts) {
        processed_scripts.push("basemonster".to_owned());
    }

    let v_collision_type = world.borrow::<View<PropCollisionType>>().unwrap();

    if v_collision_type.get(entity_id).is_ok() {
        processed_scripts.push("internal_collision_type".to_owned());
    }

    let v_creature = world.borrow::<View<PropCreature>>().unwrap();
    let v_hp = world.borrow::<View<PropHitPoints>>().unwrap();
    // Ordinary HP-bearing props use simple health. Authored TriggerDestroy props
    // without HP use its existing no-HP fallback (Damage -> Slay), so their
    // destruction links can fire. Creatures keep damage ownership in hitboxes.
    if needs_internal_simple_health(
        v_hp.get(entity_id).is_ok(),
        v_creature.get(entity_id).is_ok(),
        &processed_scripts,
    ) {
        processed_scripts.push("internal_simple_health".to_owned());
    }

    let v_keysrc = world.borrow::<View<PropKeySrc>>().unwrap();
    if v_keysrc.get(entity_id).is_ok() {
        processed_scripts.push("internal_keycard".to_owned());
    }

    // Nanites (the game's money) are collected straight into a player stat
    // rather than the inventory - see `internal_nanites_script`. Identified
    // via template ancestry rather than the shared `nan_ic` icon: the icon is
    // also inherited by the `FakeNanites` bomb-trap decoy, which must keep
    // going through the ordinary pickup path instead of being auto-collected.
    if is_nanite_pickup_template(entity_info, template_id) {
        processed_scripts.push("internal_nanites".to_owned());
    }

    // Player melee weapons are authored by their first-person limb model, not
    // by a literal `wrench` script (that name belongs to Maintenance Tool
    // -2949). Give every such weapon the trigger-gated VR contact behavior;
    // ordinary dropped weapons remain harmless because the script is inactive.
    let v_limb_model = world.borrow::<View<PropLimbModel>>().unwrap();
    if needs_internal_triggered_melee(v_limb_model.get(entity_id).is_ok()) {
        processed_scripts.push("internal_triggered_melee_weapon".to_owned());
    }

    // A gun held under `physical_held_items` is stopped by the level but takes
    // part in no collision, so the only report that it touched anything is the
    // block its drive's sweep found (see `scripts::impact_sound`). Give every
    // gun the handler that turns that into a sound; it is inert whenever the
    // gun is not being held that way, and unlike the melee script above it
    // never deals damage - a gun is not a club.
    let is_player_gun = {
        let v_player_gun = world.borrow::<View<PropPlayerGun>>().unwrap();
        v_player_gun.get(entity_id).is_ok()
    };
    if is_player_gun && v_limb_model.get(entity_id).is_err() {
        processed_scripts.push("internal_held_item_impact_sound".to_owned());
    }

    // `MOVE` is an engine frob action, not an object script. Ordinary goodies
    // such as Med Patches and armor only inherit that flag, so give them the
    // internal handler that moves them into the backpack on Frob. Objects that
    // also request SCRIPT keep their existing authored ownership (notably
    // FrobQB's circuit-board/quest-item transfer). Nanite piles also inherit
    // MOVE but must go exclusively through `internal_nanites` above - a
    // second Frob handler here would race it to move the pile into the
    // backpack instead of awarding+destroying it.
    let needs_frob_move = {
        let v_frob_info = world.borrow::<View<PropFrobInfo>>().unwrap();
        v_frob_info.get(entity_id).is_ok_and(|frob| {
            !frob.world_action.contains(FrobFlag::SCRIPT)
                && (frob.world_action.contains(FrobFlag::MOVE)
                    || frob.world_action.contains(FrobFlag::USE_AMMO))
        }) && !is_nanite_pickup_template(entity_info, template_id)
    };
    if needs_frob_move {
        processed_scripts.push("internal_frob_move".to_owned());
    }

    // Explosion SFX templates (class tag "explosiontype", e.g. HE / Incendiary
    // Explosion) with radius stim sources (arSrcDesc) blast once on spawn.
    // Radius sources WITHOUT the tag (electrical sparks, Swarm, Rad Burst) are
    // periodic emitters in the original engine - not one-shot blasts - and are
    // not handled yet.
    let is_explosion = {
        let v_class_tag = world.borrow::<View<PropClassTag>>().unwrap();
        let v_links = world.borrow::<View<Links>>().unwrap();
        v_class_tag
            .get(entity_id)
            .map(|tag| tag.class_tags().iter().any(|(k, _)| *k == "explosiontype"))
            .unwrap_or(false)
            && v_links.get(entity_id).is_ok_and(|links| {
                links.to_links.iter().any(|l| {
                    matches!(
                        l.link,
                        Link::StimSource(StimSourceOptions {
                            propagator: StimPropagator::Radius { .. },
                            ..
                        })
                    )
                })
            })
    };
    // Release the property views before add_component below needs the world
    // mutably; nothing after this point reads them.
    drop(v_scripts);
    drop(v_ai);
    drop(v_collision_type);
    drop(v_creature);
    drop(v_hp);
    drop(v_keysrc);
    drop(v_limb_model);

    if is_explosion {
        processed_scripts.push("internal_explosion".to_owned());
        // One-shot SFX: never save explosion entities. Script state
        // (has_fired) is not persisted, so a saved mid-animation explosion
        // would re-detonate on every load.
        world.add_component(entity_id, RuntimePropDoNotSerialize);
    }

    // ...and remove any duplicates!
    processed_scripts.sort_unstable();
    processed_scripts.dedup();

    let mut output_scripts = Vec::new();
    for script in processed_scripts {
        output_scripts.push(script.to_owned());
        script_world.add_entity(entity_id, &script);
    }

    EntityCreationInfo {
        entity_id,
        bitmap_animation,
        model: maybe_model,
        rigid_body,
        scripts: output_scripts,
    }
}

fn should_create_visual(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropTranslatingDoor>>()
        .unwrap()
        .get(entity_id)
        .map(|door| !door.is_permanently_open())
        .unwrap_or(true)
}

fn initialize_sym_name_from_obj_map(
    template_id: i32,
    entity: EntityId,
    entity_info: &ss2_entity_info::SystemShock2EntityInfo,
    obj_map: &HashMap<i32, String>,
    world: &mut World,
) {
    // A mission entity hydrated by the populator already carries its
    // instance-specific sym name (e.g. "SlowDoorControl"); the obj map only
    // holds archetype names ("Marker"), so overwriting here would clobber the
    // designer-given name every by-name script lookup depends on.
    {
        let v_name = world.borrow::<View<PropSymName>>().unwrap();
        if v_name.contains(entity) {
            return;
        }
    }

    let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
    let mut ancestors = ss2_entity_info::get_ancestors(hierarchy, &template_id);
    ancestors.push(template_id);

    let _template_links = TemplateLinks::empty();
    for parent_id in ancestors {
        // Add name if specified in the obj map
        if let Some(name) = obj_map.get(&parent_id) {
            world.add_component(entity, PropSymName(name.to_owned()))
        }
    }
}

/// How far a decal is pushed off the surface it is stuck to, in **world**
/// units (1 world unit = `SCALE_FACTOR` Dark units). Small enough not to read
/// as floating at grazing angles, large enough to clear depth-buffer precision
/// at the ranges decals are legible from.
const DECAL_SURFACE_OFFSET: f32 = 0.02;

/// The local-space X translation that lifts a paper-thin decal off its
/// surface, or `None` when the model is not a decal.
///
/// A decal model has (near-)zero thickness along its local X axis (Dark's
/// forward) and its visible face points along local -X, i.e. out of the wall,
/// so the nudge is along -X.
///
/// `scale_x` is the X component of the entity's `PropScale` **as the renderer
/// applies it** - `RuntimePropTransform` uses the raw, signed value, and the
/// shipped data stores it negative (the property reader mirrors X). Since the
/// offset rides inside that transform, it is divided out here so the world
/// displacement is always `DECAL_SURFACE_OFFSET` along local -X, whatever the
/// entity is scaled by.
fn decal_local_x_offset(bbox: Aabb3<f32>, scale_x: f32) -> Option<f32> {
    let thickness = bbox.max.x - bbox.min.x;
    let extent = (bbox.max.y - bbox.min.y).max(bbox.max.z - bbox.min.z);

    // Paper-thin in absolute terms, not merely relative: a long pipe is 1%
    // as thick as it is long and is not a decal.
    if thickness > DECAL_SURFACE_OFFSET || extent <= DECAL_SURFACE_OFFSET {
        return None;
    }

    if scale_x.abs() < f32::EPSILON {
        return None;
    }

    Some(-DECAL_SURFACE_OFFSET / scale_x)
}

/// Decals (blood splatters, signs, bullet holes) are paper-thin models placed
/// exactly coplanar with the surface they are stuck to, so they z-fight with
/// the level geometry - they flicker, or vanish entirely, as the viewpoint
/// moves. Lift such a model off its surface along its own outward normal.
///
/// The offset is applied as the scene objects' *local* transform: the render
/// path overwrites the model transform with the entity's
/// `RuntimePropTransform` every frame, but composes it with the local one.
fn apply_decal_offset(model: &mut Model, scale_x: f32) {
    let Some(bbox) = model.bounding_box() else {
        return;
    };

    let Some(offset) = decal_local_x_offset(bbox, scale_x) else {
        return;
    };

    model.apply_local_transform(Matrix4::from_translation(vec3(offset, 0.0, 0.0)));
}

fn create_model(
    world: &mut World,
    asset_cache: &mut AssetCache,
    entity_id: EntityId,
) -> Option<(Model, Option<AnimationPlayer>)> {
    let (
        entities,
        v_prop_position,
        v_prop_model,
        v_creature_pose,
        v_death_pose,
        _v_hasrefs,
        _v_rendertype,
        v_scale,
        mut rv_vhots,
    ) = world
        .borrow::<(
            EntitiesView,
            View<PropPosition>,
            View<PropModelName>,
            View<PropCreaturePose>,
            View<RuntimePropDeathPose>,
            View<PropHasRefs>,
            View<PropRenderType>,
            View<PropScale>,
            ViewMut<RuntimePropVhots>,
        )>()
        .unwrap();

    if let (Ok(pos), Ok(model)) = (v_prop_position.get(entity_id), v_prop_model.get(entity_id)) {
        let model_name = model.0.to_owned();
        let maybe_model = asset_cache.get_opt(&MODELS_IMPORTER, &format!("{model_name}.BIN"));

        maybe_model.as_ref()?;

        let model = maybe_model.unwrap();
        let model_ref = model.as_ref();

        let vhots = model.vhots();
        entities.add_component(entity_id, &mut rv_vhots, RuntimePropVhots(vhots));

        let qrotation = pos.rotation;
        let rotation = Matrix4::<f32>::from(qrotation);
        let mut scale = Matrix4::<f32>::from_nonuniform_scale(1.0, 1.0, 1.0);

        let translation = Matrix4::from_translation(pos.position);

        if v_scale.contains(entity_id) {
            let scale_vec = v_scale.get(entity_id).unwrap().0;
            scale = Matrix4::<f32>::from_nonuniform_scale(
                abs(scale_vec.x),
                abs(scale_vec.y),
                abs(scale_vec.z),
            );
        }

        let transform = translation * rotation * scale;

        // Runtime-generated terminal death poses are separate from authored
        // P$CretPose data, so mission-placed corpse decorations retain their
        // historical frame-1 bake and physics behavior.
        let (mut model, animation_player) = {
            if let Ok(death_pose) = v_death_pose.get(entity_id) {
                let animation_clip = asset_cache.get(
                    &ANIMATION_CLIP_IMPORTER,
                    &format!("{}_.mc", death_pose.clip_name),
                );
                let animation_clip = match (model_ref.skeleton(), death_pose.floor_depth) {
                    (Some(skeleton), Some(floor_depth)) => {
                        Rc::new(dark::ss2_skeleton::ground_terminal_pose_to_floor(
                            skeleton,
                            &animation_clip,
                            floor_depth,
                        ))
                    }
                    _ => animation_clip,
                };
                let transformed_model = Model::transform(model_ref, transform);
                (
                    transformed_model,
                    Some(AnimationPlayer::from_completed_animation(animation_clip)),
                )
            } else if let Ok(creature_pose) = v_creature_pose.get(entity_id) {
                // let motion_db = { asset_cache.get(&MOTIONDB_IMPORTER, "motiondb.bin".to_owned()) };
                // TODO: We can only handle motion name props at the moment..
                if creature_pose.pose_type.contains(PoseType::MOTION_NAME) {
                    let motion_name = creature_pose.motion_or_tag_name.to_owned();
                    let animation_clip =
                        asset_cache.get(&ANIMATION_CLIP_IMPORTER, &format!("{}_.mc", motion_name));
                    let posed_model_ref = &model_ref.pose(&animation_clip);
                    let transformed_model = Model::transform(posed_model_ref, transform);
                    (transformed_model, None)
                } else {
                    // TAG poses (e.g. the CS9 rumblers' "cs 131" walk-in-place)
                    // aren't statically applied yet; keep such creatures
                    // animatable so scripts can drive the tagged motion.
                    let transformed_model = Model::transform(model_ref, transform);
                    let player = model.is_animated().then(AnimationPlayer::empty);
                    (transformed_model, player)
                }
            } else if model.is_animated() {
                // let animation_clip =
                //     asset_cache.get(&ANIMATION_CLIP_IMPORTER, "ogsshot1_.mc".to_owned());
                // // asset_cache.get(&ANIMATION_CLIP_IMPORTER, "ogpmelat2b1_.mc".to_owned());
                // let animation_player = AnimationPlayer::from_animation(&animation_clip);
                let animation_player = AnimationPlayer::empty();
                let transformed_model = Model::transform(model_ref, transform);
                (transformed_model, Some(animation_player))
            } else {
                let transformed_model = Model::transform(model_ref, transform);
                (transformed_model, None)
            }
        };

        // The raw, signed scale - matching `RuntimePropTransform`, which is what
        // the renderer composes the local offset with (note the model bake
        // above deliberately uses the absolute value instead).
        let render_scale_x = v_scale.get(entity_id).map(|s| s.0.x).unwrap_or(1.0);
        apply_decal_offset(&mut model, render_scale_x);

        Some((model, animation_player))
    } else {
        None
    }
}

///
/// create_bitmap
///
/// Create a bitmap animation for a given entity, if possible.
fn create_bitmap(
    world: &mut World,
    asset_cache: &mut AssetCache,
    entity_id: EntityId,
) -> Option<Rc<BitmapAnimation>> {
    let (v_prop_model, v_rendertype) = world
        .borrow::<(View<PropModelName>, View<PropRenderType>)>()
        .unwrap();

    if let Ok(model) = v_prop_model.get(entity_id) {
        // We have some sort of model, but need to refine

        if v_rendertype.contains(entity_id) {
            let render_type = v_rendertype.get(entity_id).unwrap();
            if render_type.0 == RenderType::EditorOnly || render_type.0 == RenderType::NoRender {
                return None;
            };
        }

        let model_name = model.0.to_owned();
        let maybe_model =
            asset_cache.get_opt(&BITMAP_ANIMATION_IMPORTER, &format!("{model_name}.pcx"));

        maybe_model.as_ref()?;

        let bitmap_animation = maybe_model.unwrap().clone();

        Some(bitmap_animation)
    } else {
        None
    }
}

/// Enforce containment at creation (projects/flat-ui.md §5.2/§6 PR 3): an
/// entity with an incoming `Contains` link lives inside its container's loot
/// panel and must have NO world presence - no render, no physics - until
/// taken (`GrabEntity`/`DropEntityInfo` clear or move the link and restore
/// `PropHasRefs`) or dropped. Mission data usually authors `P$HasRefs=false`
/// on contained items already (the Dark editor sets it when parenting an
/// object into a container); this makes the invariant structural instead of
/// data-dependent. Must run after links are hydrated and before the
/// instantiation loop: physics creation (`create_entity_core`) and model
/// rendering both gate on `has_refs`. `PropHasRefs` is a serialized
/// property, so the state round-trips through save/load.
pub fn suppress_contained_entity_world_presence(world: &mut World) {
    use shipyard::IntoIter;
    let contained: Vec<EntityId> = {
        let v_links = world.borrow::<View<Links>>().unwrap();
        v_links
            .iter()
            .flat_map(|links| links.to_links.iter())
            .filter(|link| matches!(link.link, Link::Contains(_)))
            .filter_map(|link| link.to_entity_id.map(|id| id.0))
            .collect()
    };
    for entity in contained {
        world.add_component(entity, PropHasRefs(false));
    }
}

pub fn initialize_links_for_entity(
    template_id: i32,
    entity_id: EntityId,
    entity_info: &ss2_entity_info::SystemShock2EntityInfo,
    template_to_entity_id: &HashMap<i32, WrappedEntityId>,
    world: &mut World,
) {
    let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
    let mut ancestors = ss2_entity_info::get_ancestors(hierarchy, &template_id);
    ancestors.push(template_id);

    let mut template_links = TemplateLinks::empty();
    for parent_id in ancestors {
        let maybe_template_links = entity_info.template_to_links.get(&parent_id);

        if let Some(parent_entity_links) = maybe_template_links {
            template_links = TemplateLinks::merge(&template_links, parent_entity_links);
        }

        // TODO: Set up PropSymName for overrides?
        // Add, override name if specified in name map
        // if let Some(name) = level.obj_map.get(&parent_id) {
        //     world.add_component(entity, PropSymName(name.to_owned()))
        // }
    }

    let entity_links = Links::from_template_links(&template_links, template_to_entity_id);
    world.add_component(entity_id, entity_links);
}

///
/// initialize_entity_with_props
///
/// Create all the prop components for the entity, based on the template and inheritance hierarchy
///
pub fn initialize_entity_with_props(
    template_id: i32,
    entity_info: &ss2_entity_info::SystemShock2EntityInfo,
    world: &mut World,
    entity_id: EntityId,
    obj_name_map: &HashMap<i32, String>,
) {
    let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
    let mut ancestors = ss2_entity_info::get_ancestors(hierarchy, &template_id);
    ancestors.push(template_id);

    world.add_component(entity_id, PropTemplateId { template_id });
    // A concrete object's positive ID belongs to this mission only. Preserve
    // its nearest gamesys archetype separately so carried objects retain a
    // stable class identity in later missions whose positive IDs may collide.
    let canonical_template_id = ancestors
        .iter()
        .rev()
        .copied()
        .find(|ancestor| *ancestor < 0)
        .unwrap_or(template_id);
    world.add_component(
        entity_id,
        RuntimePropCanonicalTemplateId(canonical_template_id),
    );

    for parent_id in ancestors {
        let maybe_parent_props = entity_info.entity_to_properties.get(&parent_id);

        match maybe_parent_props {
            None => {}
            Some(props) => {
                for prop in props {
                    prop.initialize(world, entity_id)
                }
            }
        }

        // Add, override name if specified in name map
        if let Some(name) = obj_name_map.get(&parent_id) {
            world.add_component(entity_id, PropSymName(name.to_owned()))
        }
    }

    // Augment any props

    let maybe_mod = {
        let maybe_model_name = world.borrow::<View<PropModelName>>().unwrap();
        if let Ok(model_name) = maybe_model_name.get(entity_id) {
            Some(model_name.0.clone())
        } else {
            None
        }
    };

    if let Some(model) = maybe_mod {
        world.add_component(entity_id, InternalPropOriginalModelName(model));
    }
}

/// Calibration constant mapping Dark's authored `elasticity` to a Rapier
/// restitution. Dark stores elasticity in roughly `[0, 1]` (its shipped default
/// is `1.0`, "fully elastic"); a Rapier restitution of `1.0` is a perfect,
/// energy-conserving bounce that never settles and looks unnatural. We scale by
/// this factor so the default-authored object (`elasticity == 1.0`) reproduces
/// the value dynamic bodies used before this change (`0.7`), while still letting
/// per-object variation drive the simulation. Tunable once a faithful
/// calibration pass is done (see `projects/object-physics-attrs.md`).
const ELASTICITY_TO_RESTITUTION: f32 = 0.7;

fn dark_elasticity_to_restitution(elasticity: f32) -> f32 {
    let restitution = elasticity * ELASTICITY_TO_RESTITUTION;
    // Guard against corrupt/non-finite authored values (some Dark objects carry
    // non-finite physics data - see `sanitize_collider_size`): `clamp` does not
    // sanitize NaN, and a NaN/inf restitution can destabilize the solver.
    if restitution.is_finite() {
        restitution.clamp(0.0, 1.0)
    } else {
        DynamicPhysicsOptions::default().restitution
    }
}

fn dark_friction(friction: f32) -> f32 {
    if friction.is_finite() {
        friction.max(0.0)
    } else {
        DynamicPhysicsOptions::default().friction
    }
}

pub fn create_physics_representation(
    world: &mut World,
    physics: &mut PhysicsWorld,
    maybe_model: &Option<&Model>,
    entity_id: EntityId,
) -> Option<RigidBodyHandle> {
    create_physics_representation_with_options(world, physics, maybe_model, entity_id, false, false)
}

fn create_physics_representation_with_options(
    world: &mut World,
    physics: &mut PhysicsWorld,
    maybe_model: &Option<&Model>,
    entity_id: EntityId,
    launch_projectile: bool,
    flinderize_debris: bool,
) -> Option<RigidBodyHandle> {
    // A door the authors left permanently open (no travel between its open and
    // closed endpoints, authored open) has nowhere to retract to: a collider
    // for it is a slab welded across the doorway that nothing can ever move,
    // sealing the rooms behind it (#602). Give it no collision at all.
    let is_permanently_open_door = world
        .borrow::<View<PropTranslatingDoor>>()
        .unwrap()
        .get(entity_id)
        .map(|door| door.is_permanently_open())
        .unwrap_or(false);
    if is_permanently_open_door {
        return None;
    }

    // Keep this storage out of the main borrow tuple: Shipyard 0.6 supports
    // tuples through arity ten, and launch handling only needs membership.
    let launched_object_is_immobile = world
        .borrow::<View<PropImmobile>>()
        .unwrap()
        .contains(entity_id);

    let (
        v_pos,
        v_phys_attr,
        v_phys_type,
        v_phys_dimensions,
        v_frob_info,
        v_hud_select,
        v_render_type,
        v_creature,
        v_creature_pose,
        v_death_pose,
    ) = world
        .borrow::<(
            View<PropPosition>,
            View<PropPhysAttr>,
            View<PropPhysType>,
            View<PropPhysDimensions>,
            View<PropFrobInfo>,
            View<PropHUDSelect>,
            View<PropRenderType>,
            View<PropCreature>,
            View<PropCreaturePose>,
            View<RuntimePropDeathPose>,
        )>()
        .unwrap();
    let default_size = 0.5 / SCALE_FACTOR;
    let default_size_vec = vec3(default_size, default_size, default_size);

    let min_size = 0.5 / SCALE_FACTOR;
    let min_size_vec = vec3(min_size, min_size, min_size);
    let dimensions = maybe_model
        .as_ref()
        .and_then(|model| model.bounding_box().map(|bbox| bbox.max - bbox.min))
        .unwrap_or(default_size_vec);
    let abs_dimensions = vec3(
        dimensions.x.abs().max(min_size_vec.x),
        dimensions.y.abs().max(min_size_vec.y),
        dimensions.z.abs().max(min_size_vec.z),
    );

    let dynamics_options = if let Ok(phys_attr) = v_phys_attr.get(entity_id) {
        DynamicPhysicsOptions {
            gravity_scale: phys_attr.gravity_scale,
            restitution: dark_elasticity_to_restitution(phys_attr.elasticity),
            friction: dark_friction(phys_attr.friction),
        }
    } else {
        DynamicPhysicsOptions::default()
    };

    // A restored generated death pose is a completed, resting corpse. Its
    // serialized P$Position is already the live Rapier body position:
    // applying the normal creature spawn lift again would raise it by
    // SCALE_FACTOR/6 (0.4167 world units) and make it visibly settle after
    // every load. Preserve the live creature's dynamic capsule geometry and
    // material, place it at the exact saved transform, then start it asleep.
    // The corpse group keeps it on the world and selectable for looting without
    // leaving a player-blocking creature capsule behind.
    if v_death_pose.get(entity_id).is_ok() {
        if let (Ok(pos), Ok(creature_type)) = (v_pos.get(entity_id), v_creature.get(entity_id)) {
            let creature_def = get_creature_definition(creature_type.0).unwrap();
            let bbox = creature_def.bounding_size;
            let radius = bbox.x.max(bbox.z) / 2.0;
            let creature_shape = PhysicsShape::Capsule {
                height: radius.max(bbox.y - radius * 2.0),
                radius,
            };
            let rigid_body_handle = physics.add_dynamic(
                entity_id,
                pos.position,
                pos.rotation,
                vec3(0.0, -creature_def.physics_offset_height, 0.0),
                creature_shape,
                CollisionGroup::corpse(),
                false,
                dynamics_options,
            );
            physics.set_enabled_rotations(entity_id, false, false, false);
            physics.sleep_body(rigid_body_handle);
            return Some(rigid_body_handle);
        }
    }

    // Dark's Tweq emitter hands the fresh object to launchProjectile. Preserve
    // that explicit creation mode here: frobbable emitted objects (Ops4's Grub
    // is one) would otherwise take the selectable-fixture branch below and
    // replace their authored moving sphere with a kinematic model-bounds box.
    let launched_projectile = launch_projectile
        || world
            .borrow::<View<RuntimePropLaunchedProjectile>>()
            .unwrap()
            .contains(entity_id);
    if launched_projectile && !launched_object_is_immobile {
        if let (Ok(pos), Ok(phys_type), Ok(dimensions)) = (
            v_pos.get(entity_id),
            v_phys_type.get(entity_id),
            v_phys_dimensions.get(entity_id),
        ) {
            let maybe_shape = match phys_type.phys_type {
                PhysicsModelType::ORIENTED_BOUNDING_BOX => {
                    Some(PhysicsShape::Cuboid(dimensions.size))
                }
                PhysicsModelType::SPHERE => Some(PhysicsShape::Sphere(
                    dimensions.radius0.abs().max(dimensions.radius1.abs()),
                )),
                _ => None,
            };
            if let Some(shape) = maybe_shape {
                return Some(physics.add_dynamic(
                    entity_id,
                    pos.position,
                    pos.rotation,
                    dimensions.offset0,
                    shape,
                    CollisionGroup::entity(),
                    false,
                    dynamics_options,
                ));
            }
        }
    }

    // Frobbable item, let's see what we can do...
    if let (Ok(pos), Ok(frob_info)) = (v_pos.get(entity_id), v_frob_info.get(entity_id)) {
        // Dark chooses frob targets from the objects submitted by its render
        // pipeline. A NoRender object is never submitted, regardless of an
        // inherited FrobInfo or model, so it cannot be picked directly. Keep
        // authored invisible physics (tripwires and other sensors have an
        // explicit PhysType), but do not invent the model-bounds selection
        // collider that exists here only to make a visible, typeless object
        // raycastable. Otherwise hidden switch relays can eclipse their
        // co-located visible control and bypass the authored link chain (#827).
        let is_no_render = v_render_type
            .get(entity_id)
            .is_ok_and(|render_type| render_type.0 == RenderType::NoRender);
        if is_no_render && v_phys_type.get(entity_id).is_err() {
            return None;
        }

        let qrotation = pos.rotation;

        let _is_sensor = true;

        // let dimensions = v_phys_dimensions
        //     .get(id)
        //     .map(|d| d.size)
        //     .ok()
        //     .or_else(|| {
        //         id_to_model.get(&id).and_then(|model| {
        //             model.bounding_box().map(|bbox| bbox.max - bbox.min)
        //         })
        //     })
        //     .unwrap_or(default_size);

        // TODO: Add dynamic rigid body for some items?
        // let shape = if let (Ok(phys_type), Ok(dimensions)) =
        //     (v_phys_type.get(entity_id), v_phys_dimensions.get(entity_id))
        // {
        //     match phys_type.phys_type {
        //         PhysicsModelType::OrientedBoundingBox => PhysicsShape::Cuboid(abs_dimensions),
        //         PhysicsModelType::Sphere => {
        //             PhysicsShape::Sphere(dimensions.radius0.abs().max(dimensions.radius1.abs()))
        //         }
        //         _ => panic!("unhandled physics type: {:?}", phys_type),
        //     }
        // } else {
        //     PhysicsShape::Cuboid(abs_dimensions)
        // };

        let rigid_body_handle;
        // Is a creature - so we need special handling for their bounding box
        if v_creature.get(entity_id).is_ok() && v_creature_pose.get(entity_id).is_err() {
            let creature_type = v_creature.get(entity_id).unwrap();
            let creature_def = get_creature_definition(creature_type.0).unwrap();
            let creature_shape = live_creature_shape(
                &creature_def,
                v_phys_type.get(entity_id).ok(),
                v_phys_dimensions.get(entity_id).ok(),
            );
            rigid_body_handle = physics.add_dynamic(
                entity_id,
                pos.position + vec3(0.0, SCALE_FACTOR / 6.0, 0.0) /* bump up so that character is not stuck in geometry */,
                qrotation,
                vec3(0.0, -creature_def.physics_offset_height, 0.0),
                creature_shape,
                // TODO: Kinematic experiment
                //is_sensor,
                CollisionGroup::actor(),
                false,
                dynamics_options,
            );
            physics.set_enabled_rotations(entity_id, false, false, false);
        } else if frob_info.world_action.contains(FrobFlag::MOVE) {
            let shape = PhysicsShape::Cuboid(abs_dimensions * 1.0);
            rigid_body_handle = physics.add_dynamic(
                entity_id,
                pos.position + vec3(0.0, SCALE_FACTOR / 6.0, 0.0) /* bump up so that character is not stuck in geometry */,
                qrotation,
                Vector3::zero(),
                shape,
                // TODO: Kinematic experiment
                //is_sensor,
                CollisionGroup::entity(),
                false,
                dynamics_options,
            );
        } else {
            let mut group = CollisionGroup::entity();
            if let Ok(hud_select) = v_hud_select.get(entity_id) {
                // HACK: Remove pick bias around fluidics computer
                if hud_select.0 {
                    group = CollisionGroup::selectable();
                }
            }
            // This collider is the *model bounding box*, built only so the
            // object can be frobbed and raycast - it is not an authored
            // collision volume. Dark makes an object physical by giving it a
            // `PhysType`; without one there is no physics model at all and
            // the object is walk-through (its solidity in retail is the
            // brushwork behind it). Leaving the box solid to characters fills
            // walk-in fixtures - the hydro2 Resurrection Station alcove is a
            // 2.2 x 4.0 x 2.5 box the player must stand inside - and wedges
            // the capsule against its faces with no way out (#801).
            if v_phys_type.get(entity_id).is_err() {
                group = group.non_solid_to_characters();
            }
            rigid_body_handle = physics.add_kinematic(
                entity_id,
                pos.position,
                qrotation,
                Vector3::zero(),
                abs_dimensions,
                // TODO: Kinematic experiment
                //is_sensor,
                group,
                false,
            );
        }
        Some(rigid_body_handle)
    } else {
        let (
            _v_state,
            v_dimensions,
            v_phys_type,
            v_collision_type,
            v_trip_flags,
            v_scale,
            _v_creature,
            v_immobile,
        ) = world
            .borrow::<(
                View<PropPhysState>,
                View<PropPhysDimensions>,
                View<PropPhysType>,
                View<PropCollisionType>,
                View<PropTripFlags>,
                View<PropScale>,
                View<PropCreature>,
                View<PropImmobile>,
            )>()
            .unwrap();
        let immobile = v_immobile.get(entity_id).is_ok();

        // Climbable surfaces (ladders: PropPhysAttr.climbable != 0) carry an
        // extra marker membership so player movement can detect contact.
        // Simplifications: `climbable` is plausibly a per-face bitmask in
        // the original engine (27 = the four vertical sides on ladders) -
        // any non-zero value marks the whole collider climbable here. And
        // only this (non-frobbable) creation branch checks it: all known
        // ladders are plain terrain objects; a frobbable climbable would
        // need the same treatment in the branch above.
        let is_climbable = v_phys_attr
            .get(entity_id)
            .map(|pa| pa.climbable != 0)
            .unwrap_or(false);

        // `P$PhysDims` is an instantiated, non-inherited property in Dark. A
        // concrete object can therefore inherit a physics type without storing
        // dimensions in the mission: the original PhysType listener loads its
        // model and initializes an instance PhysDims from the scaled model
        // bounds. Do the same here for every supported physical model, not only
        // climbable ones (#597). Objects with PhysType `None` and model-less
        // markers still receive no fallback.
        let maybe_dimensions = v_dimensions.get(entity_id).ok();
        let has_supported_physics_type = v_phys_type
            .get(entity_id)
            .map(|phys_type| {
                phys_type.phys_type == PhysicsModelType::ORIENTED_BOUNDING_BOX
                    || phys_type.phys_type == PhysicsModelType::SPHERE
            })
            .unwrap_or(false);
        let model_bounds = if maybe_dimensions.is_none() && has_supported_physics_type {
            // Raw model bounds - deliberately NOT `abs_dimensions`, whose
            // minimum-size clamp would make a fallback ladder 41% thicker
            // than the authored ladder standing next to it. Degenerate sizes
            // are already guarded by `sanitize_collider_size`.
            maybe_model
                .as_ref()
                .and_then(|model| model.bounding_box())
                .map(|bbox| {
                    (
                        bbox.max - bbox.min,
                        bbox.min.to_vec() + (bbox.max - bbox.min) / 2.0,
                    )
                })
        } else {
            None
        };
        if let (Ok(pos), Ok(phys_type)) = (v_pos.get(entity_id), v_phys_type.get(entity_id)) {
            let qrotation = pos.rotation;

            let mut is_sensor = false;
            // Model scale belongs to the rendered model. An explicit
            // P$PhysDims is already the independently authored collision
            // volume and must not be scaled again (Shodan's window strips
            // use a visual z-scale of 16 beside a 3.2-unit OBB). Only the
            // model-bounds fallback needs the model's scale applied.
            let scale_factor = if maybe_dimensions.is_some() {
                vec3(1.0, 1.0, 1.0)
            } else {
                v_scale
                    .get(entity_id)
                    .map(|p| p.0)
                    .unwrap_or(vec3(1.0, 1.0, 1.0))
            };
            let _maybe_collision_prop = v_collision_type.get(entity_id);
            let maybe_trip_flags = v_trip_flags.get(entity_id);

            // if let Ok(hud_select) = v_hud_select.get(id) {
            //     // HACK: Remove pick bias around fluidics computer
            //     if hud_select.0 == false {
            //         continue;
            //     }
            // }

            // if let Ok(collision_prop) = maybe_collision_prop {
            //     is_sensor = collision_prop.collision_type != 1;
            //     println!(
            //         "maybe a sensor, value is: {}",
            //         collision_prop.collision_type
            //     );
            // }

            // If it has trip flags at all, must be a sensor
            if maybe_trip_flags.is_ok() {
                is_sensor = true;
                // scale_factor *= 1.2;
            }

            // No authored dimensions and no usable model bounds: nothing to
            // build a collider from, so behave as before (no physics).
            let unscaled_size = match (maybe_dimensions, model_bounds) {
                (Some(dimensions), _) => dimensions.size,
                (None, Some((size, _))) => size,
                (None, None) => return None,
            };
            let size = vec3(
                unscaled_size.x.abs() * scale_factor.x.abs(),
                unscaled_size.y.abs() * scale_factor.y.abs(),
                unscaled_size.z.abs() * scale_factor.z.abs(),
            );

            let shape = match (phys_type.phys_type, maybe_dimensions) {
                (PhysicsModelType::ORIENTED_BOUNDING_BOX, _) => PhysicsShape::Cuboid(size),
                (PhysicsModelType::SPHERE, Some(dimensions)) => {
                    PhysicsShape::Sphere(dimensions.radius0.abs().max(dimensions.radius1.abs()))
                }
                // The leaf ladder templates say SPHERE where their parent says
                // OBB, but carry no radius to go with it (and a bounding
                // sphere would swallow the room: a 16' ladder becomes an 8'
                // ball). The bounding box is the only geometry we have.
                (PhysicsModelType::SPHERE, None) => PhysicsShape::Cuboid(size),
                _ => {
                    warn!("unhandled physics type: {:?}", phys_type);
                    return None;
                }
            };

            // `Lift 1 Walls` is authored as a zero-radius SPHERE with an
            // outgoing PhysAttach link. Treat that zero-volume marker as the
            // transform anchor for the visible wall shell, not a simulated
            // ball: its authored dimensions contribute no collision geometry.
            // Keep a collider-less kinematic body so the attachment machinery
            // can drive its render transform without inventing a model-bounds
            // OBB that would fill the lift's hollow interior.
            let is_zero_radius_phys_attach_anchor = phys_type.phys_type == PhysicsModelType::SPHERE
                && maybe_dimensions.is_some_and(|dimensions| {
                    dimensions.radius0 == 0.0 && dimensions.radius1 == 0.0
                })
                && world
                    .borrow::<View<Links>>()
                    .unwrap()
                    .get(entity_id)
                    .is_ok_and(|links| {
                        links
                            .to_links
                            .iter()
                            .any(|link| matches!(link.link, Link::PhysAttach(_)))
                    });

            let group = if is_climbable {
                CollisionGroup::climbable_entity()
            } else {
                CollisionGroup::entity()
            };

            // `SPHERE` is Dark's *moving* physics model - a simulated sphere
            // hull. Loose debris carries it (`Monster Parts` gibs, `Chair
            // Parts`, `Misc Parts`), while terrain, doors and level furniture
            // carry an OBB or are `Immobile`. Without authored `P$PhysDims`
            // there is no radius to simulate with, so the collider here is the
            // immovable kinematic model-bounds box of the #597 fallback - the
            // exact opposite of a body that yields. Retail actors shove debris
            // out of their way; this stand-in cannot move at all, and a gib
            // resting against a capsule can stop it permanently. Not stopping
            // either player or creature capsules is the closest this collider
            // gets to being pushed aside. An authored `P$Immobile` (the same
            // presence test the dynamic path below already uses) is what keeps
            // level fixtures solid, and climbable is excluded outright: the
            // climb probe queries as `PLAYER` too, so a non-solid ladder would
            // also be an unclimbable one.
            let is_movable_dimensionless_sphere = phys_type.phys_type == PhysicsModelType::SPHERE
                && maybe_dimensions.is_none()
                && !immobile
                && !is_climbable;
            let group = if is_movable_dimensionless_sphere {
                group.non_solid_to_characters()
            } else {
                group
            };

            let offset = match (maybe_dimensions, model_bounds) {
                (Some(dimensions), _) => dimensions.offset0,
                (None, Some((_, center))) => center,
                (None, None) => Vector3::zero(),
            };

            // Ordinary model-bounds fallbacks remain conservative kinematic
            // stand-ins: their box is not an authored collision volume. A
            // Flinderize target is different because the creation itself
            // carries an impulse and means the debris must be simulated. Use
            // its bounded model box for mass/inertia only in that explicit
            // mode; authored-dimension spheres keep their existing dynamic
            // path. Sensors stay kinematic so their trigger volume cannot
            // drift away from its authored location.
            let is_authored_dynamic_sphere = !immobile
                && maybe_dimensions.is_some()
                && phys_type.phys_type == PhysicsModelType::SPHERE;
            let is_dynamic_flinder =
                flinderize_debris && is_movable_dimensionless_sphere && !is_sensor;
            let rigid_body_handle = if is_zero_radius_phys_attach_anchor {
                physics.add_kinematic_anchor(entity_id, pos.position, qrotation)
            } else if is_authored_dynamic_sphere || is_dynamic_flinder {
                physics_log!(DEBUG, "Creating dynamic hitbox entity");
                physics.add_dynamic(
                    entity_id,
                    pos.position,
                    qrotation,
                    offset,
                    shape,
                    //size,
                    group,
                    is_sensor,
                    dynamics_options,
                )
            } else {
                physics.add_kinematic(
                    entity_id,
                    pos.position,
                    qrotation,
                    offset,
                    size,
                    group,
                    is_sensor,
                )
            };
            Some(rigid_body_handle)
        } else {
            None
        }
    }
}

fn live_creature_shape(
    creature_def: &crate::creature::CreatureDefinition,
    phys_type: Option<&PropPhysType>,
    dimensions: Option<&PropPhysDimensions>,
) -> PhysicsShape {
    let bbox = creature_def.bounding_size;
    let fallback_radius = bbox.x.max(bbox.z) / 2.0;
    let fallback = || PhysicsShape::Capsule {
        // Preserve the established fallback for creatures without a complete
        // authored sphere model. Small animation bounds can otherwise leave a
        // zero-length capsule segment, which Rapier does not accept here.
        height: fallback_radius.max(bbox.y - fallback_radius * 2.0),
        radius: fallback_radius,
    };

    let (Some(phys_type), Some(dimensions)) = (phys_type, dimensions) else {
        return fallback();
    };
    if phys_type.phys_type != PhysicsModelType::SPHERE
        || !(1..=2).contains(&phys_type.num_submodels)
    {
        return fallback();
    }

    // Dark drives a live creature's sphere submodels from animated joints and
    // applies the creature descriptor's per-submodel radii (`SetPhysSubModScale`),
    // rather than using the animation model's visual width as collision. The
    // instantiated P$PhysDims mirrors those radii. This importer's historical
    // representation stores each Dark radius at half its scaled value, so the
    // conversion is deliberately scoped to live-creature SPHERE models; loose
    // props keep their existing parser/physics semantics.
    let imported_radii = [dimensions.radius0, dimensions.radius1];
    let declared_radii = &imported_radii[..phys_type.num_submodels as usize];
    if declared_radii
        .iter()
        .any(|radius| !radius.is_finite() || *radius <= 0.0)
    {
        return fallback();
    }
    let radius = declared_radii.iter().copied().fold(0.0, f32::max) * 2.0;
    let segment_height = bbox.y - radius * 2.0;
    if !radius.is_finite() || radius <= 0.0 || !segment_height.is_finite() || segment_height <= 0.0
    {
        return fallback();
    }

    PhysicsShape::Capsule {
        height: segment_height,
        radius,
    }
}

#[derive(Clone, Debug)]
pub struct CreateEntityOptions {
    pub force_visible: bool,
    /// Bolt the new entity to this parent's transform for its lifetime (see
    /// `RuntimePropAttachment`). The spawn-time relative pose is captured and the
    /// child then tracks the parent each frame - used so a weapon's muzzle flash
    /// follows the moving first-person viewmodel instead of snapshotting it once.
    pub attach_to: Option<EntityId>,
    /// Mark the entity as a fire-and-forget effect (`RuntimePropTransientFx`):
    /// it is destroyed once its one-shot particle burst expires. Used for
    /// impact spangs so they don't accumulate at every bullet hole.
    pub transient_fx: bool,
    /// Override the collision-ray origin when this entity resolves as a fast
    /// projectile. Flat firing supplies the camera origin while retaining the
    /// forward spawn clearance needed by slow physics projectiles.
    pub projectile_raycast_origin: Option<Point3<f32>>,
    /// Create the authored physics model as a launched dynamic body. Dark's
    /// Tweq emitter calls `launchProjectile`; this keeps frobbable emitted
    /// archetypes from being reduced to kinematic selection colliders.
    pub launch_projectile: bool,
    /// This entity was created as a Flinderize target. Keeping this separate
    /// from the general model-bounds fallback lets only launched debris turn a
    /// dimension-less moving sphere into a simulated body.
    pub flinderize_debris: bool,
}

impl Default for CreateEntityOptions {
    fn default() -> Self {
        CreateEntityOptions {
            force_visible: false,
            attach_to: None,
            transient_fx: false,
            projectile_raycast_origin: None,
            launch_projectile: false,
            flinderize_debris: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{RUMBLER_HEIGHT, RUMBLER_WIDTH};
    use cgmath::{InnerSpace, point3};
    use collision::Aabb3;
    use dark::properties::{Links, ToLink};

    /// `BLOOD02.BIN`-shaped: zero thickness in X, 6x6 Dark units across
    /// (bounds are stored already divided by `SCALE_FACTOR`).
    fn decal_bbox() -> Aabb3<f32> {
        Aabb3::new(
            point3(-2.0e-6, -3.0 / SCALE_FACTOR, -3.0 / SCALE_FACTOR),
            point3(2.0e-6, 3.0 / SCALE_FACTOR, 3.0 / SCALE_FACTOR),
        )
    }

    /// The world displacement the renderer ends up applying: the local offset
    /// rides inside `RuntimePropTransform`, which scales by the raw signed X.
    fn world_offset(bbox: Aabb3<f32>, scale_x: f32) -> Option<f32> {
        decal_local_x_offset(bbox, scale_x).map(|local| local * scale_x)
    }

    #[test]
    fn decal_is_lifted_off_its_surface() {
        let offset = decal_local_x_offset(decal_bbox(), 1.0).unwrap();
        assert!(offset < 0.0, "decal must move along local -X, got {offset}");
    }

    /// medsci1's entity 772 (`Blood Splatter Small`) carries
    /// `PropScale([-1.013279, 0.667, 1.0])`, and the renderer applies that sign,
    /// so an uncompensated local offset is mirrored *into* the floor.
    #[test]
    fn decal_lift_survives_a_mirrored_scale() {
        for scale_x in [1.0, -1.013_279, 0.5, -3.0] {
            let world = world_offset(decal_bbox(), scale_x).unwrap();
            assert!(
                (world + DECAL_SURFACE_OFFSET).abs() < 1.0e-6,
                "scale {scale_x} displaced the decal by {world}, want {}",
                -DECAL_SURFACE_OFFSET
            );
        }
    }

    #[test]
    fn a_degenerate_scale_is_left_alone() {
        assert_eq!(decal_local_x_offset(decal_bbox(), 0.0), None);
    }

    /// `pipe312.BIN` is 1% as thick as it is long, but it is a 312-unit pipe,
    /// not a decal - the flatness test must be absolute, not a ratio.
    #[test]
    fn a_long_thin_pipe_is_not_a_decal() {
        let pipe = Aabb3::new(
            point3(0.0, 0.0, 0.0),
            point3(
                3.0 / SCALE_FACTOR,
                3.005 / SCALE_FACTOR,
                312.0 / SCALE_FACTOR,
            ),
        );
        assert_eq!(decal_local_x_offset(pipe, 1.0), None);
    }

    #[test]
    fn an_ordinary_prop_is_not_a_decal() {
        let crate_bbox = Aabb3::new(point3(-1.0, -1.0, -1.0), point3(1.0, 1.0, 1.0));
        assert_eq!(decal_local_x_offset(crate_bbox, 1.0), None);
    }

    /// A ladder-shaped entity: climbable terrain with a physics type but - like
    /// most shipped ladders - no `P$PhysDims` at all.
    fn add_ladder(world: &mut World, phys_type: PhysicsModelType, climbable: u32) -> EntityId {
        world.add_entity((
            PropPosition {
                position: vec3(0.0, 0.0, 0.0),
                cell: 0,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            PropPhysType {
                phys_type,
                num_submodels: 1,
                remove_on_sleep: false,
                is_special: false,
            },
            PropPhysAttr {
                gravity_scale: 1.0,
                mass: 30.0,
                density: 1.0,
                elasticity: 1.0,
                friction: 0.0,
                cog: Vector3::zero(),
                rotation_axes: 7,
                rest_axes: 63,
                climbable,
                edge_trigger: false,
            },
            PropImmobile(true),
        ))
    }

    /// `ladder.bin`'s bounds: 16 SS2 ft tall, flat against a wall.
    fn ladder_model() -> Model {
        Model::from_glb(
            vec![],
            Aabb3::new(
                Point3::new(-0.823, -3.2, -0.071),
                Point3::new(0.823, 3.2, 0.071),
            ),
            None,
        )
    }

    const LADDER_SIZE: Vector3<f32> = Vector3::new(1.646, 6.4, 0.142);

    #[test]
    fn trigger_destroy_prop_without_hit_points_still_owns_weapon_damage() {
        let trigger_destroy = vec!["TriggerDestroy".to_owned()];

        assert!(
            needs_internal_simple_health(false, false, &trigger_destroy),
            "a non-creature TriggerDestroy prop must translate Damage into Slay"
        );
        assert!(
            needs_internal_simple_health(true, false, &[]),
            "ordinary HP-bearing props keep incremental simple health"
        );
        assert!(
            !needs_internal_simple_health(false, false, &[]),
            "ordinary non-creatures without HP or TriggerDestroy stay unaffected"
        );
        assert!(
            !needs_internal_simple_health(true, true, &trigger_destroy),
            "creatures retain ownership of their hitbox damage path"
        );
    }

    #[test]
    fn authored_limb_model_is_the_generic_player_melee_marker() {
        assert!(
            needs_internal_triggered_melee(true),
            "player Wrench -928 and the other authored melee weapons carry PropLimbModel"
        );
        assert!(
            !needs_internal_triggered_melee(false),
            "Maintenance Tool -2949's literal Wrench script and guns must not gain player melee"
        );
    }

    #[test]
    fn ai_property_survives_an_object_script_inheritance_override() {
        let threat_en_og_scripts = vec![
            "TransientCorpse".to_owned(),
            "triggerdestroy".to_owned(),
            "creaturecontainer".to_owned(),
        ];

        assert!(
            needs_internal_ai(true, &threat_en_og_scripts),
            "ThreatenOG's explicit non-inheriting scripts must not remove engine AI"
        );
        assert!(
            !needs_internal_ai(true, &["BaseMonster".to_owned()]),
            "an inherited BaseMonster already owns AI and must not be duplicated"
        );
        assert!(
            !needs_internal_ai(false, &threat_en_og_scripts),
            "ordinary scripted objects must not gain monster AI"
        );
    }

    /// A live Rumbler authors two 1.5 SS2-foot physics spheres. The property
    /// importer's historical radius representation is half the scaled Dark
    /// radius, so those values appear here as 0.3 world units. Its animated
    /// model is five feet wide, but that is not its locomotion hull: Dark's
    /// creature descriptor drives the two authored spheres independently.
    ///
    /// Negative-first: creature creation previously ignored both sphere
    /// radii and built a 1.0-world-unit-radius capsule from animation bounds,
    /// too broad for Rick1's shipped WALK|SMALL_CREATURE junction.
    #[test]
    fn live_creature_uses_authored_sphere_width_with_animation_height() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let entity_id = world.add_entity((
            PropPosition {
                position: vec3(0.0, 4.0, 0.0),
                cell: 0,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            PropCreature(3),
            PropFrobInfo {
                world_action: FrobFlag::SCRIPT,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
            PropPhysType {
                phys_type: PhysicsModelType::SPHERE,
                num_submodels: 2,
                remove_on_sleep: false,
                is_special: true,
            },
            PropPhysDimensions {
                radius0: 0.3,
                radius1: 0.3,
                offset0: Vector3::zero(),
                offset1: Vector3::zero(),
                size: Vector3::zero(),
                unk1: 0,
                unk2: 0,
            },
        ));

        let handle = create_physics_representation(&mut world, &mut physics, &None, entity_id)
            .expect("a live creature should get a dynamic physics body");
        let (radius, segment_height) = physics
            .capsule_dimensions(handle)
            .expect("the live creature body should remain a capsule");

        assert!(
            (radius - 0.6).abs() < 0.001,
            "expected radius 0.6, got {radius}"
        );
        assert!(
            (segment_height + radius * 2.0 - RUMBLER_HEIGHT).abs() < 0.001,
            "authored width must not change the full animation-derived height"
        );
        let body = physics
            .debug_list_bodies()
            .into_iter()
            .find(|body| body.entity_id == Some(entity_id.inner() as i32))
            .expect("the creature body should remain introspectable");
        assert_eq!(body.body_type, "dynamic");
        assert!(body.blocks_player && body.blocks_actor);
        assert!(body.collision_groups.iter().any(|group| group == "actor"));
    }

    fn capsule(shape: PhysicsShape) -> (f32, f32) {
        match shape {
            PhysicsShape::Capsule { height, radius } => (radius, height),
            other => panic!("expected creature capsule, got {other:?}"),
        }
    }

    fn sphere_type(num_submodels: u32) -> PropPhysType {
        PropPhysType {
            phys_type: PhysicsModelType::SPHERE,
            num_submodels,
            remove_on_sleep: false,
            is_special: true,
        }
    }

    fn sphere_dimensions(radius0: f32, radius1: f32) -> PropPhysDimensions {
        PropPhysDimensions {
            radius0,
            radius1,
            offset0: vec3(9.0, 8.0, 7.0),
            offset1: vec3(-6.0, -5.0, -4.0),
            size: Vector3::zero(),
            unk1: 0,
            unk2: 0,
        }
    }

    #[test]
    fn live_creature_shape_uses_largest_declared_sphere_radius() {
        let rumbler = get_creature_definition(3).unwrap();
        let two_spheres = sphere_type(2);
        let dimensions = sphere_dimensions(0.25, 0.3);
        let (radius, segment_height) = capsule(live_creature_shape(
            &rumbler,
            Some(&two_spheres),
            Some(&dimensions),
        ));

        assert!((radius - 0.6).abs() < 0.001);
        assert!((segment_height + 2.0 * radius - RUMBLER_HEIGHT).abs() < 0.001);
    }

    #[test]
    fn invalid_or_incomplete_creature_spheres_keep_animation_fallback() {
        let rumbler = get_creature_definition(3).unwrap();
        let fallback = capsule(live_creature_shape(&rumbler, None, None));
        assert_eq!(fallback, (RUMBLER_WIDTH / 2.0, RUMBLER_WIDTH / 2.0));

        let mut obb = sphere_type(2);
        obb.phys_type = PhysicsModelType::ORIENTED_BOUNDING_BOX;
        let invalid_cases = [
            (Some(sphere_type(0)), Some(sphere_dimensions(0.3, 0.3))),
            (Some(sphere_type(3)), Some(sphere_dimensions(0.3, 0.3))),
            (Some(sphere_type(2)), Some(sphere_dimensions(0.3, 0.0))),
            (Some(sphere_type(2)), Some(sphere_dimensions(0.3, f32::NAN))),
            (Some(sphere_type(2)), Some(sphere_dimensions(0.7, 0.7))),
            (Some(obb), Some(sphere_dimensions(0.3, 0.3))),
            (Some(sphere_type(2)), None),
        ];
        for (phys_type, dimensions) in invalid_cases {
            assert_eq!(
                capsule(live_creature_shape(
                    &rumbler,
                    phys_type.as_ref(),
                    dimensions.as_ref(),
                )),
                fallback,
            );
        }
    }

    #[test]
    fn shipped_creature_sphere_radii_match_original_descriptor_envelopes() {
        for (creature_type, imported, expected_radius) in [
            (0, [0.2, 0.24], 0.48),
            (3, [0.3, 0.3], 0.6),
            (4, [0.39, 0.0], 0.78),
            (6, [0.16, 0.2], 0.4),
        ] {
            let creature = get_creature_definition(creature_type).unwrap();
            let num_submodels = if imported[1] > 0.0 { 2 } else { 1 };
            let dimensions = sphere_dimensions(imported[0], imported[1]);
            let (radius, segment_height) = capsule(live_creature_shape(
                &creature,
                Some(&sphere_type(num_submodels)),
                Some(&dimensions),
            ));
            assert!((radius - expected_radius).abs() < 0.001);
            assert!((segment_height + radius * 2.0 - creature.bounding_size.y).abs() < 0.001);
        }
    }

    /// A wall fixture the player can frob but never pick up or move - a
    /// console, a card slot, the Resurrection Station casing. `phys_type` is
    /// `None` for the shipped case that has no `P$PhysType` in its chain.
    fn add_wall_fixture(world: &mut World, phys_type: Option<PhysicsModelType>) -> EntityId {
        let entity_id = world.add_entity((
            PropPosition {
                position: vec3(0.0, 0.0, 0.0),
                cell: 0,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            PropFrobInfo {
                world_action: FrobFlag::SCRIPT,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
            PropHUDSelect(false),
            PropImmobile(true),
        ));
        if let Some(phys_type) = phys_type {
            world.add_component(
                entity_id,
                PropPhysType {
                    phys_type,
                    num_submodels: 1,
                    remove_on_sleep: false,
                    is_special: false,
                },
            );
        }
        entity_id
    }

    /// A frobbable fixture's collider comes from its model bounding box, not
    /// from an authored collision volume. Dark only builds a physics model for
    /// an object that carries a `PhysType`, so one without any must not be
    /// solid to characters - hydro2's Resurrection Station is a 2.2 x 4.0 x
    /// 2.5 box around the pad the player has to stand on, and a capsule
    /// overlapping it has no direction left to resolve into (#801). It still
    /// gets the collider: frob and selection raycasts need it.
    ///
    /// Negative-first: before the fix the typeless case also blocked.
    #[test]
    fn a_frobbable_without_phys_type_does_not_block_characters() {
        for (phys_type, should_block) in [
            (None, false),
            (Some(PhysicsModelType::ORIENTED_BOUNDING_BOX), true),
        ] {
            let mut world = World::new();
            let mut physics = PhysicsWorld::new();
            let entity_id = add_wall_fixture(&mut world, phys_type);
            let model = ladder_model();

            let handle =
                create_physics_representation(&mut world, &mut physics, &Some(&model), entity_id)
                    .expect("a frobbable fixture should always get a frob collider");

            assert_eq!(
                physics.collider_blocks_player(handle),
                should_block,
                "phys_type {phys_type:?} should{} block the player",
                if should_block { "" } else { " not" }
            );
            assert_eq!(
                physics.collider_blocks_actor(handle),
                should_block,
                "phys_type {phys_type:?} should{} block actors",
                if should_block { "" } else { " not" }
            );
            // Either way it stays in the world and keeps its selectable /
            // entity membership, so nothing about frobbing changes.
            let bodies = physics.debug_list_bodies();
            assert_eq!(bodies.len(), 1, "expected exactly one body in the world");
            assert!(
                bodies[0]
                    .collision_groups
                    .iter()
                    .any(|g| g == "entity" || g == "selectable"),
                "a frob collider must stay raycastable, got {:?}",
                bodies[0].collision_groups
            );
            assert_eq!(bodies[0].blocks_player, should_block);
            assert_eq!(bodies[0].blocks_actor, should_block);
        }
    }

    /// Dark's pick pass weighs only objects submitted by the renderer. A
    /// NoRender relay with inherited FrobInfo must therefore remain callable
    /// by scripts/links without gaining this engine's synthetic frob body.
    /// Authored invisible physics is independent and must survive: tripwires
    /// use NoRender + PhysType for their sensor volumes.
    #[test]
    fn no_render_frob_collider_requires_authored_phys_type() {
        for (phys_type, should_have_body) in [
            (None, false),
            (Some(PhysicsModelType::ORIENTED_BOUNDING_BOX), true),
        ] {
            let mut world = World::new();
            let mut physics = PhysicsWorld::new();
            let entity_id = add_wall_fixture(&mut world, phys_type);
            world.add_component(entity_id, PropRenderType(RenderType::NoRender));
            let model = ladder_model();

            let handle =
                create_physics_representation(&mut world, &mut physics, &Some(&model), entity_id);

            assert_eq!(
                handle.is_some(),
                should_have_body,
                "NoRender fixture with PhysType {phys_type:?} should{} keep a body",
                if should_have_body { "" } else { " not" }
            );
            assert_eq!(
                physics.debug_list_bodies().len(),
                usize::from(should_have_body)
            );
        }
    }

    /// Dark's Tweq emitter launches its created object. Ops4's Grub is
    /// frobbable for targeting, but also authors a moving sphere; the launch
    /// path must win over the ordinary kinematic selection-collider fallback.
    #[test]
    fn launched_frobbable_sphere_uses_authored_dynamic_physics() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let entity_id = world.add_entity((
            PropPosition {
                position: vec3(1.0, 2.0, 3.0),
                cell: 0,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            PropFrobInfo {
                world_action: FrobFlag::SCRIPT,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
            PropHUDSelect(true),
            PropPhysType {
                phys_type: PhysicsModelType::SPHERE,
                num_submodels: 1,
                remove_on_sleep: false,
                is_special: true,
            },
            PropPhysDimensions {
                radius0: 0.2,
                radius1: 0.0,
                offset0: vec3(0.0, 0.36, 0.0),
                offset1: Vector3::zero(),
                size: Vector3::zero(),
                unk1: 0,
                unk2: 0,
            },
        ));

        create_physics_representation_with_options(
            &mut world,
            &mut physics,
            &None,
            entity_id,
            true,
            false,
        )
        .expect("an authored launched sphere should get a body");
        physics.set_velocity(entity_id, vec3(-10.0, 0.0, 0.0));

        let bodies = physics.debug_list_bodies();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].body_type, "dynamic");
        assert_eq!(bodies[0].linear_velocity, [-10.0, 0.0, 0.0]);
        assert!(bodies[0].collision_groups.iter().any(|g| g == "entity"));
    }

    /// A non-frobbable object with a physics type but no `P$PhysDims` - the
    /// #597 model-bounds fallback. `immobile` separates level furniture (and
    /// the ladder leaves whose own template says SPHERE) from loose debris.
    fn add_dimensionless_object(
        world: &mut World,
        phys_type: PhysicsModelType,
        immobile: bool,
    ) -> EntityId {
        let entity_id = world.add_entity((
            PropPosition {
                position: vec3(0.0, 0.0, 0.0),
                cell: 0,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            PropPhysType {
                phys_type,
                num_submodels: 1,
                remove_on_sleep: false,
                is_special: false,
            },
        ));
        if immobile {
            world.add_component(entity_id, PropImmobile(true));
        }
        entity_id
    }

    /// `eggbit.bin`'s bounds, the annelid gib a smashed Floor Pod flinderizes
    /// into: measured live off its collider at 0.66 x 0.74 x 0.31 wu.
    fn gib_model() -> Model {
        Model::from_glb(
            vec![],
            Aabb3::new(
                Point3::new(-0.331, -0.368, -0.155),
                Point3::new(0.331, 0.368, 0.155),
            ),
            None,
        )
    }

    /// Dark's SPHERE physics model is a simulated, movable sphere hull - what
    /// gibs and other debris carry. With no authored `P$PhysDims` this engine
    /// can only stand in an immovable kinematic box, which never yields, so a
    /// gib that comes to rest against the player capsule wedges it forever:
    /// the character controller has no depenetration pass (#803). Such debris
    /// must therefore not be solid to characters. `Immobile` objects - ladder
    /// leaves that say SPHERE, level furniture - stay solid, as does anything
    /// with the static OBB model.
    ///
    /// Negative-first: before the fix the debris case blocked too.
    #[test]
    fn dimensionless_movable_debris_does_not_block_characters() {
        for (phys_type, immobile, should_block) in [
            (PhysicsModelType::SPHERE, false, false),
            (PhysicsModelType::SPHERE, true, true),
            (PhysicsModelType::ORIENTED_BOUNDING_BOX, false, true),
        ] {
            let mut world = World::new();
            let mut physics = PhysicsWorld::new();
            let entity_id = add_dimensionless_object(&mut world, phys_type, immobile);
            let model = gib_model();

            let handle =
                create_physics_representation(&mut world, &mut physics, &Some(&model), entity_id)
                    .expect("the model-bounds fallback should still build a collider");

            assert_eq!(
                physics.collider_blocks_player(handle),
                should_block,
                "{phys_type:?} (immobile: {immobile}) should{} block the player",
                if should_block { "" } else { " not" }
            );
            assert_eq!(
                physics.collider_blocks_actor(handle),
                should_block,
                "{phys_type:?} (immobile: {immobile}) should{} block actors",
                if should_block { "" } else { " not" }
            );
            // Debris keeps its collider and its `entity` membership: it is
            // still shootable, selectable, and solid to physical props.
            let bodies = physics.debug_list_bodies();
            assert_eq!(bodies.len(), 1, "expected exactly one body in the world");
            assert_eq!(bodies[0].body_type, "kinematic");
            assert!(
                bodies[0].collision_groups.iter().any(|g| g == "entity"),
                "debris must stay raycastable, got {:?}",
                bodies[0].collision_groups
            );
            assert_eq!(bodies[0].blocks_player, should_block);
            assert_eq!(bodies[0].blocks_actor, should_block);
        }
    }

    /// A dimension-less moving sphere only becomes simulated when it was
    /// explicitly spawned by Flinderize. Ordinary model-bounds fallbacks keep
    /// the conservative kinematic policy established by #597/#767.
    ///
    /// Negative-first: before #815, both cases were kinematic and ignored the
    /// Flinderize impulse and gravity.
    #[test]
    fn only_flinderized_dimensionless_debris_becomes_dynamic() {
        for (flinderize_debris, expected_body_type) in [(false, "kinematic"), (true, "dynamic")] {
            let mut world = World::new();
            let mut physics = PhysicsWorld::new();
            let entity_id = add_dimensionless_object(&mut world, PhysicsModelType::SPHERE, false);
            let model = gib_model();

            create_physics_representation_with_options(
                &mut world,
                &mut physics,
                &Some(&model),
                entity_id,
                false,
                flinderize_debris,
            )
            .expect("dimension-less debris should retain a model-bounds collider");

            let bodies = physics.debug_list_bodies();
            assert_eq!(bodies.len(), 1);
            assert_eq!(bodies[0].body_type, expected_body_type);
            assert!(
                bodies[0].mass.is_finite() && bodies[0].mass > 0.0 && bodies[0].mass < 1.0,
                "small gib bounds should yield a finite, bounded mass, got {}",
                bodies[0].mass
            );
            assert!(!bodies[0].blocks_player);
            assert!(!bodies[0].blocks_actor);
        }
    }

    /// Model scale is a render transform, while `P$PhysDims` is the separately
    /// authored collision volume. Shodan's long window strips make the
    /// distinction observable: applying their visual z-scale to the OBB grows
    /// a 3.2-unit collider to 51.2 units and seals an unrelated passage.
    #[test]
    fn authored_physics_dimensions_are_independent_of_model_scale() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let authored_size = vec3(3.2, 3.2, 3.2);
        let entity_id = world.add_entity((
            PropPosition {
                position: vec3(14.4, 0.0, 8.0),
                cell: 0,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            PropPhysType {
                phys_type: PhysicsModelType::ORIENTED_BOUNDING_BOX,
                num_submodels: 6,
                remove_on_sleep: false,
                is_special: false,
            },
            PropPhysDimensions {
                radius0: 0.0,
                radius1: 0.0,
                offset0: Vector3::zero(),
                offset1: Vector3::zero(),
                size: authored_size,
                unk1: 0,
                unk2: 0,
            },
            PropScale(vec3(-0.888_888_9, 1.333_333_4, 16.0)),
            PropImmobile(true),
        ));

        let handle = create_physics_representation(&mut world, &mut physics, &None, entity_id)
            .expect("authored OBB should create a collider");
        let actual_size = physics
            .cuboid_full_size(handle)
            .expect("authored OBB should remain a cuboid");

        assert!(
            (actual_size - authored_size).magnitude() < 0.001,
            "render scale must not alter authored physics dimensions: expected {authored_size:?}, got {actual_size:?}"
        );
    }

    /// A climbable entity with no `PropPhysDimensions` still gets a collider,
    /// sized from the model bounds and tagged climbable (issue #589 - 24 of
    /// eng1's 30 ladders had no collider at all, so they were neither solid nor
    /// climbable). Negative-first: before the fallback, the missing dimensions
    /// property made `create_physics_representation` return `None`.
    #[test]
    fn climbable_without_dimensions_gets_collider_from_model_bounds() {
        for phys_type in [
            PhysicsModelType::ORIENTED_BOUNDING_BOX,
            // The leaf ladder templates say SPHERE while their parent says OBB;
            // with no authored radius the model box is all we have.
            PhysicsModelType::SPHERE,
        ] {
            let mut world = World::new();
            let mut physics = PhysicsWorld::new();
            let entity_id = add_ladder(&mut world, phys_type, 27);
            let model = ladder_model();

            let handle =
                create_physics_representation(&mut world, &mut physics, &Some(&model), entity_id)
                    .expect("climbable entity without dimensions should still get a collider");

            // The full model bounds, unclamped: an authored ladder standing
            // next to a fallback one must have the same solidity.
            let size = physics
                .cuboid_full_size(handle)
                .expect("collider should be a box matching the model bounds");
            assert!(
                (size - LADDER_SIZE).magnitude() < 0.01,
                "collider should match the model bounds {LADDER_SIZE:?}, got {size:?}"
            );

            let bodies = physics.debug_list_bodies();
            assert_eq!(bodies.len(), 1, "expected exactly one body in the world");
            assert_eq!(
                bodies[0].body_type, "kinematic",
                "a dimension-less ladder must not become a falling dynamic prop"
            );
            let groups = &bodies[0].collision_groups;
            assert!(
                groups.iter().any(|g| g == "climbable"),
                "ladder collider should be climbable, got {groups:?}"
            );
        }
    }

    /// `P$PhysDims` is an instantiated, non-inherited Dark property: a concrete
    /// object can inherit `P$PhysType` from its archetype without carrying raw
    /// dimensions in the mission. Dark creates those instance dimensions from
    /// the model bounds, so ordinary OBB props need the same fallback as ladders.
    #[test]
    fn non_climbable_without_dimensions_gets_collider_from_model_bounds() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let entity_id = add_ladder(&mut world, PhysicsModelType::ORIENTED_BOUNDING_BOX, 0);
        let model = ladder_model();

        let handle =
            create_physics_representation(&mut world, &mut physics, &Some(&model), entity_id)
                .expect("an ordinary dimension-less OBB should use its model bounds");
        let size = physics
            .cuboid_full_size(handle)
            .expect("the model-bounds fallback should create an OBB");
        assert!(
            (size - LADDER_SIZE).magnitude() < 0.01,
            "collider should match the model bounds {LADDER_SIZE:?}, got {size:?}"
        );
    }

    /// A non-immobile climbable object with no dimensions must not become a
    /// dynamic body: there is no authored radius for the SPHERE path, so the
    /// fallback builds static geometry, never a gravity-driven prop. It also
    /// stays solid to the player: the climb probe queries as `PLAYER`, so a
    /// ladder taken out of the player's collision filter as debris would be
    /// unclimbable as well as walk-through.
    #[test]
    fn dimensionless_sphere_never_becomes_dynamic() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let entity_id = add_ladder(&mut world, PhysicsModelType::SPHERE, 27);
        world.remove::<(PropImmobile,)>(entity_id);
        let model = ladder_model();

        let handle =
            create_physics_representation(&mut world, &mut physics, &Some(&model), entity_id)
                .expect("climbable entity without dimensions should still get a collider");

        let bodies = physics.debug_list_bodies();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].body_type, "kinematic");
        assert!(
            physics.collider_blocks_player(handle),
            "a climbable must stay solid to the player"
        );
    }

    /// The shipped `Lift 1 Walls` uses a zero-radius SPHERE as its physical
    /// attachment anchor. The anchor must be kinematic so the PhysAttach link
    /// can drive it with the lift; simulating the zero-volume marker as a
    /// dynamic ball makes attachment registration fail and lets the visible
    /// walls separate from the moving platform.
    #[test]
    fn zero_radius_phys_attach_sphere_is_a_kinematic_anchor() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let parent = world.add_entity((
            PropPosition {
                position: vec3(2.0, 3.0, 4.0),
                cell: 0,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            PropPhysType {
                phys_type: PhysicsModelType::ORIENTED_BOUNDING_BOX,
                num_submodels: 6,
                remove_on_sleep: false,
                is_special: false,
            },
            PropPhysDimensions {
                radius0: 0.0,
                radius1: 0.0,
                offset0: Vector3::zero(),
                offset1: Vector3::zero(),
                size: vec3(2.4, 0.4, 2.4),
                unk1: 0,
                unk2: 0,
            },
        ));
        let child = world.add_entity((
            PropPosition {
                position: vec3(2.0, 4.0, 4.0),
                cell: 0,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            PropPhysType {
                phys_type: PhysicsModelType::SPHERE,
                num_submodels: 1,
                remove_on_sleep: false,
                is_special: false,
            },
            PropPhysDimensions {
                radius0: 0.0,
                radius1: 0.0,
                offset0: Vector3::zero(),
                offset1: Vector3::zero(),
                size: Vector3::zero(),
                unk1: 0,
                unk2: 0,
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(parent)),
                    link: Link::PhysAttach(dark::properties::PhysAttachOptions {
                        offset: vec3(0.0, 1.0, 0.0),
                    }),
                }],
            },
        ));

        let _parent_handle = create_physics_representation(&mut world, &mut physics, &None, parent)
            .expect("the lift floor should get its authored OBB");
        let _child_handle = create_physics_representation(&mut world, &mut physics, &None, child)
            .expect("the attached wall shell should retain a transform anchor");

        let child_body = physics
            .debug_list_bodies()
            .into_iter()
            .find(|body| body.entity_id == Some(child.inner() as i32))
            .expect("the attached shell should have a body");
        assert_eq!(child_body.body_type, "kinematic");
        assert!(
            physics.get_aabb2(child).is_none(),
            "a zero-radius authored anchor must not invent collision geometry"
        );
        assert!(physics.attach_kinematic(child, parent, vec3(0.0, 1.0, 0.0)));
    }

    /// `PhysType::NONE` means the archetype deliberately disables physical
    /// representation. A renderable model does not override that decision.
    #[test]
    fn dimensionless_none_never_gets_model_bounds_collider() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let entity_id = add_ladder(&mut world, PhysicsModelType::NONE, 0);
        let model = ladder_model();

        assert_eq!(
            create_physics_representation(&mut world, &mut physics, &Some(&model), entity_id),
            None
        );
        assert!(physics.debug_list_bodies().is_empty());
    }

    fn contains_link(to: EntityId) -> ToLink {
        ToLink {
            to_template_id: 0,
            to_entity_id: Some(WrappedEntityId(to)),
            link: Link::Contains(0),
        }
    }

    fn has_refs_of(world: &World, entity: EntityId) -> Option<bool> {
        let v = world.borrow::<View<PropHasRefs>>().unwrap();
        v.get(entity).ok().map(|p| p.0)
    }

    /// A translating door as the retail data authors it: a thin oriented box
    /// at `at`, travelling from `closed` to `open`.
    fn add_door(
        world: &mut World,
        at: Vector3<f32>,
        closed: Vector3<f32>,
        open: Vector3<f32>,
        state: i32,
    ) -> EntityId {
        world.add_entity((
            PropPosition {
                position: at,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                cell: 0,
            },
            PropPhysType {
                phys_type: PhysicsModelType::ORIENTED_BOUNDING_BOX,
                num_submodels: 1,
                remove_on_sleep: false,
                is_special: false,
            },
            PropPhysDimensions {
                radius0: 0.0,
                radius1: 0.0,
                offset0: Vector3::zero(),
                offset1: Vector3::zero(),
                size: vec3(2.4, 3.2, 0.2),
                unk1: 0,
                unk2: 0,
            },
            dark::properties::PropTranslatingDoor {
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
        ))
    }

    #[test]
    fn a_permanently_open_door_gets_no_collider() {
        // hydro2 obj 135: open == closed and authored open, so it can never
        // move aside - a collider there seals the doorway forever (#602).
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let at = vec3(18.0, -0.4, 41.8);
        let door = add_door(&mut world, at, at, at, 1);

        assert_eq!(
            create_physics_representation(&mut world, &mut physics, &None, door),
            None
        );
    }

    #[test]
    fn a_permanently_open_door_gets_no_visual() {
        // The authored open and closed locations of hydro2 obj 135 coincide,
        // so there is no retracted transform at which its leaf can be drawn.
        let mut world = World::new();
        let at = vec3(18.0, -0.4, 41.8);
        let permanently_open = add_door(&mut world, at, at, at, 1);
        let permanently_closed = add_door(&mut world, at, at, at, 0);
        let ordinary = add_door(&mut world, at, at, at + vec3(0.0, 0.0, 2.4), 0);

        assert!(!should_create_visual(&world, permanently_open));
        assert!(should_create_visual(&world, permanently_closed));
        assert!(should_create_visual(&world, ordinary));
    }

    #[test]
    fn an_ordinary_door_still_gets_a_collider() {
        // hydro2 obj 529, the control: real travel, so it blocks while closed.
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let closed = vec3(10.3, -0.4, 42.0);
        let open = vec3(10.3, -0.4, 44.3);
        let door = add_door(&mut world, closed, closed, open, 0);

        assert!(create_physics_representation(&mut world, &mut physics, &None, door).is_some());
    }

    #[test]
    fn a_zero_travel_door_authored_closed_still_gets_a_collider() {
        // e.g. medsci1's space shields: no travel, authored closed - they stay
        // solid walls.
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let at = vec3(-15.5, 1.4, 61.0);
        let door = add_door(&mut world, at, at, at, 0);

        assert!(create_physics_representation(&mut world, &mut physics, &None, door).is_some());
    }

    #[test]
    fn contained_entities_lose_world_presence() {
        let mut world = World::new();
        let loot = world.add_entity(());
        let world_placed = world.add_entity(());
        let container = world.add_entity(Links {
            to_links: vec![contains_link(loot)],
        });

        suppress_contained_entity_world_presence(&mut world);

        // The Contains target is suppressed; nothing else is touched.
        assert_eq!(has_refs_of(&world, loot), Some(false));
        assert_eq!(has_refs_of(&world, world_placed), None);
        assert_eq!(has_refs_of(&world, container), None);
    }

    #[test]
    fn containment_overrides_an_authored_visible_flag() {
        // A contained item whose data inconsistently says HasRefs(true) (the
        // invariant is structural, not data-dependent).
        let mut world = World::new();
        let loot = world.add_entity(PropHasRefs(true));
        let _container = world.add_entity(Links {
            to_links: vec![contains_link(loot)],
        });

        suppress_contained_entity_world_presence(&mut world);

        assert_eq!(has_refs_of(&world, loot), Some(false));
    }

    #[test]
    fn non_contains_links_do_not_suppress() {
        // Trap/script references (SwitchLink etc.) must not hide entities.
        let mut world = World::new();
        let door = world.add_entity(());
        let _switch = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(door)),
                link: Link::SwitchLink,
            }],
        });

        suppress_contained_entity_world_presence(&mut world);

        assert_eq!(has_refs_of(&world, door), None);
    }
}
