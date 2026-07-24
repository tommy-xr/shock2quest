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
    EuclideanSpace, Matrix4, Point3, Quaternion, Rotation, SquareMatrix, Transform, Vector3, Zero,
    num_traits::abs, vec3,
};
use dark::{
    BitmapAnimation, SCALE_FACTOR,
    importers::{ANIMATION_CLIP_IMPORTER, BITMAP_ANIMATION_IMPORTER, MODELS_IMPORTER},
    model::Model,
    motion::AnimationPlayer,
    properties::{
        FrobFlag, InternalPropOriginalModelName, Link, Links, PhysicsModelType, PoseType,
        PropClassTag, PropCollisionType, PropCreature, PropCreaturePose, PropFrobInfo,
        PropHUDSelect, PropHasRefs, PropHitPoints, PropImmobile, PropKeySrc, PropModelName,
        PropPhysAttr, PropPhysDimensions, PropPhysState, PropPhysType, PropPosition,
        PropRenderType, PropScale, PropSymName, PropTemplateId, PropTripFlags, RenderType,
        StimPropagator, StimSourceOptions, TemplateLinks, WrappedEntityId,
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
    _additional_options: CreateEntityOptions,
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

    // Create model, if we can
    let maybe_model = create_model(world, asset_cache, entity_id);
    let maybe_just_model = maybe_model.clone().map(|m| m.0);

    // Create bitmap animation, if no model
    let bitmap_animation = if maybe_model.is_none() {
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
        create_physics_representation(world, physics, &maybe_just_model.as_ref(), entity_id)
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

    let v_collision_type = world.borrow::<View<PropCollisionType>>().unwrap();

    if v_collision_type.get(entity_id).is_ok() {
        processed_scripts.push("internal_collision_type".to_owned());
    }

    let v_creature = world.borrow::<View<PropCreature>>().unwrap();
    let v_hp = world.borrow::<View<PropHitPoints>>().unwrap();
    // If there is hitpoint, but the item is not a creature, just use the simple damage method...
    // Otherwise, the hitbox / creature logic will take care of handling damage
    if v_hp.get(entity_id).is_ok() && v_creature.get(entity_id).is_err() {
        processed_scripts.push("internal_simple_health".to_owned());
    }

    let v_keysrc = world.borrow::<View<PropKeySrc>>().unwrap();
    if v_keysrc.get(entity_id).is_ok() {
        processed_scripts.push("internal_keycard".to_owned());
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
    drop(v_collision_type);
    drop(v_creature);
    drop(v_hp);
    drop(v_keysrc);

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

        // HACK:
        // Move decals up slightly, to avoid z-fighting with level geometry...
        let forward = qrotation.rotate_vector(vec3(0.0, 0.1, 0.0));
        let translation = Matrix4::from_translation(pos.position + forward);

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
        let (model, animation_player) = {
            if let Ok(death_pose) = v_death_pose.get(entity_id) {
                let animation_clip =
                    asset_cache.get(&ANIMATION_CLIP_IMPORTER, &format!("{}_.mc", death_pose.0));
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
    let (
        v_pos,
        v_phys_attr,
        _v_phys_type,
        _v_phys_dimensions,
        v_frob_info,
        v_hud_select,
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
    // Contacts and impulses can still wake/push it.
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
                CollisionGroup::entity(),
                false,
                dynamics_options,
            );
            physics.set_enabled_rotations(entity_id, false, false, false);
            physics.sleep_body(rigid_body_handle);
            return Some(rigid_body_handle);
        }
    }

    // Frobbable item, let's see what we can do...
    if let (Ok(pos), Ok(frob_info)) = (v_pos.get(entity_id), v_frob_info.get(entity_id)) {
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
            let bbox = creature_def.bounding_size;
            let radius = bbox.x.max(bbox.z) / 2.0;
            let creature_shape = PhysicsShape::Capsule {
                height: radius.max(bbox.y - radius * 2.0),
                radius,
            };
            rigid_body_handle = physics.add_dynamic(
                entity_id,
                pos.position + vec3(0.0, SCALE_FACTOR / 6.0, 0.0) /* bump up so that character is not stuck in geometry */,
                qrotation,
                vec3(0.0, -creature_def.physics_offset_height, 0.0),
                creature_shape,
                // TODO: Kinematic experiment
                //is_sensor,
                CollisionGroup::entity(),
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

        if let (Ok(pos), Ok(dimensions), Ok(phys_type)) = (
            v_pos.get(entity_id),
            v_dimensions.get(entity_id),
            v_phys_type.get(entity_id),
        ) {
            let qrotation = pos.rotation;

            let mut is_sensor = false;
            let scale_factor = v_scale
                .get(entity_id)
                .map(|p| p.0)
                .unwrap_or(vec3(1.0, 1.0, 1.0));
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

            let size = vec3(
                dimensions.size.x.abs() * scale_factor.x.abs(),
                dimensions.size.y.abs() * scale_factor.y.abs(),
                dimensions.size.z.abs() * scale_factor.z.abs(),
            );

            let shape = match phys_type.phys_type {
                PhysicsModelType::ORIENTED_BOUNDING_BOX => PhysicsShape::Cuboid(size),
                PhysicsModelType::SPHERE => {
                    PhysicsShape::Sphere(dimensions.radius0.abs().max(dimensions.radius1.abs()))
                }
                _ => {
                    warn!("unhandled physics type: {:?}", phys_type);
                    return None;
                }
            };

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
            let group = if is_climbable {
                CollisionGroup::climbable_entity()
            } else {
                CollisionGroup::entity()
            };

            let rigid_body_handle = if !immobile && phys_type.phys_type == PhysicsModelType::SPHERE
            {
                physics_log!(DEBUG, "Creating dynamic hitbox entity");
                physics.add_dynamic(
                    entity_id,
                    pos.position,
                    qrotation,
                    dimensions.offset0,
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
                    dimensions.offset0,
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
}

impl Default for CreateEntityOptions {
    fn default() -> Self {
        CreateEntityOptions {
            force_visible: false,
            attach_to: None,
            transient_fx: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{Links, ToLink};

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
