use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Quaternion, Rotation, Rotation3, Transform, point3,
};
use dark::{
    SCALE_FACTOR,
    properties::{GunFlashOptions, Link, ProjectileOptions},
};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::{entity_creator::CreateEntityOptions, mission_core::GlobalTemplateClassTags},
    physics::{InternalCollisionGroups, PhysicsWorld, RayCastResult},
    runtime_props::{
        RuntimePropFlatAim, RuntimePropReloading, RuntimePropSelectedAmmo, RuntimePropTransform,
        RuntimePropVhots,
    },
    util::{get_rotation_from_forward_vector, resolve_proxy_entity},
    vr_config,
};

/// How far in front of the camera a flat-aimed projectile spawns, so a slow
/// physics projectile (e.g. a grenade) clears the player's own collider instead
/// of spawning inside it. World units.
const FLAT_MUZZLE_CLEARANCE: f32 = SCALE_FACTOR;

/// Reach of a flat melee swing (raycast along the crosshair ray), in world units.
const MELEE_RANGE: f32 = 1.2;
/// Damage dealt by a flat melee hit. TODO: derive from the weapon's `Melee Typ`.
const MELEE_DAMAGE: f32 = 6.0;

use super::{
    Effect, Message, MessagePayload, Script,
    script_util::{
        get_all_links_with_template, ordered_projectile_links, play_environmental_sound,
    },
};

/// Get ammunition type from projectile template using pre-populated class tag map
fn get_ammotype_from_projectile_template(
    template_id: i32,
    class_tag_map: &std::collections::HashMap<i32, std::collections::HashMap<String, String>>,
) -> Option<String> {
    let template_tags = class_tag_map.get(&template_id)?;
    template_tags.get("ammotype").cloned()
}

pub struct WeaponScript;
impl WeaponScript {
    pub fn new() -> WeaponScript {
        WeaponScript
    }
}

impl Script for WeaponScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TriggerPull => {
                // Firing is blocked while a reload is in progress.
                if world
                    .borrow::<View<RuntimePropReloading>>()
                    .ok()
                    .and_then(|v| v.get(entity_id).ok().map(|r| !r.is_done()))
                    == Some(true)
                {
                    return Effect::NoEffect;
                }

                //Create muzzle flash
                let muzzle_flashes =
                    get_all_links_with_template(world, entity_id, |link| match link {
                        Link::GunFlash(data) => Some(*data),
                        _ => None,
                    });

                // Pick the selected ammo type: guns carry several Projectile
                // links (standard / HE / AP, ...); RuntimePropSelectedAmmo indexes
                // into the ordered, setting-filtered list (absent = the first).
                let projectiles = ordered_projectile_links(world, entity_id);
                let selected_ammo = world
                    .borrow::<View<RuntimePropSelectedAmmo>>()
                    .ok()
                    .and_then(|v| v.get(entity_id).ok().map(|s| s.0))
                    .unwrap_or(0);
                let maybe_projectile = projectiles
                    .get(selected_ammo % projectiles.len().max(1))
                    .cloned();

                // A weapon with no Projectile link is melee. In flat mode a swing
                // is a short forward raycast along the crosshair ray
                // (RuntimePropFlatAim); a hit deals melee damage. (VR melee
                // damages via physical collision - MeleeWeapon's Collided handler
                // - and has no flat aim, so it just no-ops here.)
                if maybe_projectile.is_none() {
                    if let Ok(aim) = world
                        .borrow::<View<RuntimePropFlatAim>>()
                        .unwrap()
                        .get(entity_id)
                        .copied()
                    {
                        return melee_swing(physics, entity_id, aim, world);
                    }
                }

                // Ammo gating: weapons that carry a `PropGunState` are limited by
                // their clip. An empty clip dry-fires (no shot/flash); a live shot
                // consumes one round (the per-shot `m_ammoUsage` from BaseGunDesc
                // is a TODO - one round per pull for now). Weapons without a gun
                // state (e.g. unlimited debug weapons) are unaffected.
                let maybe_ammo = world
                    .borrow::<View<dark::properties::PropGunState>>()
                    .ok()
                    .and_then(|v| v.get(entity_id).ok().map(|g| g.ammo));
                if maybe_ammo == Some(0) {
                    return dry_fire(world, entity_id);
                }

                // Include projectile class tags (ie, ammotype) and weaponmode for sound lookup
                let mut projectile_class_tags: Vec<(String, String)> =
                    if let Some((projectile_template_id, _)) = &maybe_projectile {
                        let class_tags = world
                            .borrow::<UniqueView<GlobalTemplateClassTags>>()
                            .unwrap();
                        get_ammotype_from_projectile_template(
                            *projectile_template_id,
                            &class_tags.0,
                        )
                        .map(|ammotype| vec![("ammotype".to_string(), ammotype)])
                        .unwrap_or_default()
                    } else {
                        Vec::new()
                    };

                // Add weaponmode=0 for shoot mode
                projectile_class_tags.push(("weaponmode".to_string(), "0".to_string()));

                let additional_sound_tags = projectile_class_tags
                    .iter()
                    .map(|(tag, value)| (tag.as_str(), value.as_str()))
                    .collect::<Vec<_>>();

                let sound_effect = play_environmental_sound(
                    world,
                    entity_id,
                    "shoot",
                    additional_sound_tags,
                    AudioHandle::new(),
                );

                let projectile_effect = Effect::Multiple(
                    maybe_projectile
                        .into_iter()
                        .map(|(template_id, options)| {
                            create_projectile(world, entity_id, template_id, &options)
                        })
                        .collect(),
                );

                let muzzle_flash_effect = Effect::Multiple(
                    muzzle_flashes
                        .into_iter()
                        .map(|(template_id, options)| {
                            create_muzzle_flash(world, entity_id, template_id, &options)
                        })
                        .collect(),
                );
                // let offset = obj_rotation * vec3(0.0128545, 0.5026805, -3.0933015) / SCALE_FACTOR;

                // let muzzle_flash_effect = Effect::CreateEntity {
                //     template_id: -2653,
                //     position: position + offset,
                //     orientation: *obj_rotation
                //         * Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Rad(PI / 2.0)),
                // };

                // Consume a round when the weapon tracks ammo.
                let mut effects = vec![sound_effect, muzzle_flash_effect, projectile_effect];
                if maybe_ammo.is_some() {
                    effects.push(Effect::AdjustAmmo {
                        entity_id,
                        delta: -1,
                    });
                }
                Effect::Multiple(effects)
            }
            MessagePayload::TriggerRelease => Effect::NoEffect,
            _ => Effect::NoEffect,
        }
    }
}

/// A flat melee swing: play the swing animation, and raycast a short distance
/// along the crosshair ray - a hit deals melee damage (hitbox proxies resolve to
/// their parent). The swing animation plays whether or not the swing connects.
fn melee_swing(
    physics: &PhysicsWorld,
    entity_id: EntityId,
    aim: RuntimePropFlatAim,
    world: &World,
) -> Effect {
    let mut effects = vec![Effect::FlatMeleeSwing { entity_id }];

    let hit = physics.ray_cast(
        aim.origin,
        aim.forward.normalize() * MELEE_RANGE,
        InternalCollisionGroups::ENTITY
            | InternalCollisionGroups::HITBOX
            | InternalCollisionGroups::SELECTABLE,
    );
    if let Some(RayCastResult {
        maybe_entity_id: Some(target),
        ..
    }) = hit
    {
        let target = resolve_proxy_entity(world, target);
        effects.push(Effect::Send {
            msg: Message {
                to: target,
                payload: MessagePayload::Damage {
                    amount: MELEE_DAMAGE,
                },
            },
        });
    }
    Effect::Multiple(effects)
}

/// An empty-clip dry fire: no projectile or muzzle flash, just the weapon's
/// "dryfire" click (best-effort - resolves via the gun's sound schema, like the
/// "shoot" event). Reached when a `PropGunState` weapon has 0 rounds.
fn dry_fire(world: &World, entity_id: EntityId) -> Effect {
    play_environmental_sound(world, entity_id, "dryfire", vec![], AudioHandle::new())
}

fn create_muzzle_flash(
    world: &World,
    entity_id: EntityId,
    muzzle_flash_template_id: i32,
    options: &GunFlashOptions,
) -> Effect {
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let v_vhots = world.borrow::<View<RuntimePropVhots>>().unwrap();

    let vhots = v_vhots
        .get(entity_id)
        .map(|vhots| vhots.0.clone())
        .unwrap_or_default();

    let vhot_offset = vhots
        .get(options.vhot as usize)
        .map(|v| v.point)
        .unwrap_or(point3(0.0, 0.0, 0.0));

    let transform = v_transform.get(entity_id).unwrap();

    let adjustments = vr_config::get_vr_hand_model_adjustments_from_entity(
        entity_id,
        world,
        vr_config::Handedness::Left,
    );
    let orientation = adjustments.rotation.invert() * Quaternion::from_angle_y(Deg(90.0));

    Effect::CreateEntity {
        template_id: muzzle_flash_template_id,
        position: vhot_offset,
        orientation,
        root_transform: transform.0,
        // Bolt the flash to the firing weapon so it tracks the (per-frame
        // re-placed) first-person viewmodel during its brief lifetime instead of
        // staying pinned at the fire-time pose.
        options: CreateEntityOptions {
            attach_to: Some(entity_id),
            ..CreateEntityOptions::default()
        },
    }
}

fn create_projectile(
    world: &World,
    entity_id: EntityId,
    projectile_template_id: i32,
    _options: &ProjectileOptions,
) -> Effect {
    // Flatscreen camera-origin aim: if the weapon carries a flat fire ray, spawn
    // the projectile just ahead of the camera travelling straight along the
    // crosshair, ignoring the offset/rotated barrel transform. This makes both
    // hitscan bullets and slow physics projectiles (grenades) track the
    // crosshair. VR/AI weapons have no RuntimePropFlatAim and fall through.
    {
        let v_flat_aim = world.borrow::<View<RuntimePropFlatAim>>().unwrap();
        if let Ok(aim) = v_flat_aim.get(entity_id) {
            let origin = aim.origin + aim.forward.normalize() * FLAT_MUZZLE_CLEARANCE;
            // get_rotation_from_forward_vector puts `forward` in the +Z column,
            // and projectile velocity is root_transform * (0,0,mag) - so the
            // shot travels along the crosshair ray.
            let rot: Matrix4<f32> =
                get_rotation_from_forward_vector(aim.forward.normalize()).into();
            return Effect::CreateEntity {
                template_id: projectile_template_id,
                position: point3(0.0, 0.0, 0.0),
                orientation: Quaternion::from_angle_y(Deg(90.0)),
                root_transform: Matrix4::from_translation(origin.to_vec()) * rot,
                options: CreateEntityOptions {
                    force_visible: true,
                    ..CreateEntityOptions::default()
                },
            };
        }
    }

    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let v_vhots = world.borrow::<View<RuntimePropVhots>>().unwrap();

    let vhots = v_vhots
        .get(entity_id)
        .map(|vhots| vhots.0.clone())
        .unwrap_or_default();
    let vhot = vhots
        .get(0)
        .map(|v| v.point)
        .unwrap_or(point3(0.0, 0.0, 0.0));

    let transform = v_transform.get(entity_id).unwrap();

    let adjustments = vr_config::get_vr_hand_model_adjustments_from_entity(
        entity_id,
        world,
        // TODO: I guess we don't care about handedness for now,
        // since it only affects the flipping of the weapon... but truly we should consider it.
        vr_config::Handedness::Left,
    );

    let rotation = adjustments.rotation;
    let projectile_rotation: Matrix4<f32> =
        vr_config::get_projectile_rotation_from_entity(entity_id, world).into();
    let rot_matrix: Matrix4<f32> = rotation.into();
    let inv_rot_matrix: Matrix4<f32> = rotation.invert().into();

    // Adjust the vhot position to be in the same coordinate space as the weapon
    let position = inv_rot_matrix.transform_point(vhot);

    Effect::CreateEntity {
        template_id: projectile_template_id,
        position,
        // HACK: Not sure why we need to do this, but seems projectile
        // models are rotated 90 degrees
        orientation: Quaternion::from_angle_y(Deg(90.0)),
        root_transform: transform.0 * rot_matrix * projectile_rotation,
        options: CreateEntityOptions {
            force_visible: true,
            ..CreateEntityOptions::default()
        },
    }
}
