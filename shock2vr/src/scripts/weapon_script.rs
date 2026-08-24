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
        RuntimePropFlatAim, RuntimePropModelBounds, RuntimePropReloading, RuntimePropSelectedAmmo,
        RuntimePropTransform, RuntimePropVhots,
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

/// How far a gunshot carries to alert AIs (50 Dark feet). One value for all
/// guns for now; per-weapon loudness is a follow-up.
const GUNSHOT_NOISE_RADIUS: f32 = 50.0 / SCALE_FACTOR;

#[derive(Clone, Copy, Debug, PartialEq)]
struct MuzzleGeometry {
    point: cgmath::Point3<f32>,
    axis: cgmath::Vector3<f32>,
}

/// Resolve the fire point and direction from the same local-space geometry the
/// renderer uses. Dark's guns are authored lengthwise on either X (25AE hand
/// models and several classic models) or Z (the other classic world models),
/// always toward the negative end. A real vhot wins for the fire point, chosen
/// geometrically rather than by type/file position; a model without one uses
/// the centre of its front bounding-box face instead of the grip.
fn resolve_muzzle_geometry(
    bounds: Option<collision::Aabb3<f32>>,
    vhots: &[dark::ss2_bin_obj_loader::Vhot],
) -> MuzzleGeometry {
    let axis = bounds
        .map(|bounds| {
            let dimensions = bounds.max - bounds.min;
            if dimensions.z > dimensions.x {
                -cgmath::Vector3::unit_z()
            } else {
                -cgmath::Vector3::unit_x()
            }
        })
        .unwrap_or_else(|| -cgmath::Vector3::unit_x());

    let point = vhots
        .iter()
        .max_by(|a, b| {
            a.point
                .to_vec()
                .dot(axis)
                .total_cmp(&b.point.to_vec().dot(axis))
        })
        .map(|vhot| vhot.point)
        .or_else(|| {
            bounds.map(|bounds| {
                let centre = bounds.min + (bounds.max - bounds.min) / 2.0;
                if axis.x != 0.0 {
                    point3(bounds.min.x, centre.y, centre.z)
                } else {
                    point3(centre.x, centre.y, bounds.min.z)
                }
            })
        })
        .unwrap_or_else(|| point3(0.0, 0.0, 0.0));

    MuzzleGeometry { point, axis }
}

/// The weapon entity's current world position (from its live transform).
fn weapon_world_position(world: &World, entity_id: EntityId) -> Option<cgmath::Vector3<f32>> {
    let v_transform = world.borrow::<View<RuntimePropTransform>>().ok()?;
    let transform = v_transform.get(entity_id).ok()?;
    Some(transform.0.transform_point(point3(0.0, 0.0, 0.0)).to_vec())
}

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

                // A weapon with no Projectile link is melee. In flat mode the
                // trigger starts the authored player-arm swing; its MF_TRIGGER1
                // animation event resolves the short aimed raycast later, at
                // visible impact. (VR has no flat aim and damages through its
                // trigger-gated physical contact handler.)
                if maybe_projectile.is_none() {
                    if world
                        .borrow::<View<RuntimePropFlatAim>>()
                        .unwrap()
                        .get(entity_id)
                        .is_ok()
                    {
                        return Effect::FlatMeleeSwing { entity_id };
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

                // Only a real gunshot (a fired projectile) raises noise - a
                // projectile-less weapon that falls through the melee gate
                // must not emit a phantom gunshot.
                let is_gunshot = maybe_projectile.is_some();
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
                // A gunshot is loud: nearby AIs hear it and investigate the
                // shooter, even without line of sight. The noise comes from
                // the weapon (in the player's hands), so its position stands
                // in for the shooter's.
                if is_gunshot {
                    if let Some(origin) = weapon_world_position(world, entity_id) {
                        effects.push(Effect::RaiseNoise {
                            origin,
                            radius: GUNSHOT_NOISE_RADIUS,
                        });
                    }
                }
                Effect::Multiple(effects)
            }
            MessagePayload::AnimationFlagTriggered { motion_flags }
                if motion_flags.contains(dark::motion::MotionFlags::TRIGGER1)
                    && ordered_projectile_links(world, entity_id).is_empty() =>
            {
                let Ok(aim) = world
                    .borrow::<View<RuntimePropFlatAim>>()
                    .unwrap()
                    .get(entity_id)
                    .copied()
                else {
                    return Effect::NoEffect;
                };
                flat_melee_hit(physics, aim, world)
            }
            MessagePayload::TriggerRelease => Effect::NoEffect,
            _ => Effect::NoEffect,
        }
    }
}

/// Resolve the authored hit event of a flat melee swing: raycast a short
/// distance along the current crosshair ray and damage the hit entity (hitbox
/// proxies resolve to their parent).
fn flat_melee_hit(physics: &PhysicsWorld, aim: RuntimePropFlatAim, world: &World) -> Effect {
    let hit = physics.ray_cast(
        aim.origin,
        aim.forward.normalize() * MELEE_RANGE,
        InternalCollisionGroups::ENTITIES
            | InternalCollisionGroups::HITBOX
            | InternalCollisionGroups::SELECTABLE,
    );
    if let Some(RayCastResult {
        maybe_entity_id: Some(target),
        hit_point,
        ..
    }) = hit
    {
        let target = resolve_proxy_entity(world, target);
        return Effect::Send {
            msg: Message {
                to: target,
                payload: MessagePayload::Damage {
                    amount: MELEE_DAMAGE,
                    // Swing direction + contact point seed the victim's
                    // death-ragdoll reaction. No bone: melee resolves a hitbox
                    // proxy to its parent BEFORE sending (so HitBoxScript
                    // never stamps it) - the ragdoll's nearest-body-to-point
                    // fallback picks the struck limb from the contact point
                    // instead.
                    impact: Some(crate::scripts::DamageImpact {
                        direction: aim.forward.normalize(),
                        point: hit_point.to_vec(),
                        bone: None,
                    }),
                },
            },
        };
    }
    Effect::NoEffect
}

/// An empty-clip dry fire: no projectile or muzzle flash, just the weapon's
/// "dryfire" click (best-effort - resolves via the gun's sound schema, like the
/// "shoot" event). Reached when a `PropGunState` weapon has 0 rounds.
fn dry_fire(world: &World, entity_id: EntityId) -> Effect {
    play_environmental_sound(world, entity_id, "dryfire", vec![], AudioHandle::new())
}

pub(super) fn create_muzzle_flash(
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

    let bounds = world
        .borrow::<View<RuntimePropModelBounds>>()
        .ok()
        .and_then(|bounds| bounds.get(entity_id).ok().map(|bounds| bounds.0));
    let fallback = resolve_muzzle_geometry(bounds, &vhots).point;

    let vhot_offset = vhots
        .get(options.vhot as usize)
        .map(|v| v.point)
        .unwrap_or(fallback);

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

pub(super) fn create_projectile(
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
                    projectile_raycast_origin: Some(aim.origin),
                    ..CreateEntityOptions::default()
                },
            };
        }
    }

    // VR: the weapon is a physical object in the hand, so the shot is defined
    // entirely by the *rendered* weapon - it leaves the model's muzzle vhot
    // travelling down the model's barrel. Both are read from the weapon's own
    // live transform, so no per-weapon aim correction is involved.
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let v_vhots = world.borrow::<View<RuntimePropVhots>>().unwrap();

    let vhots = v_vhots
        .get(entity_id)
        .map(|vhots| vhots.0.clone())
        .unwrap_or_default();
    let bounds = world
        .borrow::<View<RuntimePropModelBounds>>()
        .ok()
        .and_then(|bounds| bounds.get(entity_id).ok().map(|bounds| bounds.0));
    let muzzle = resolve_muzzle_geometry(bounds, &vhots);

    let transform = v_transform.get(entity_id).unwrap();

    Effect::CreateEntity {
        template_id: projectile_template_id,
        position: point3(0.0, 0.0, 0.0),
        // HACK: Not sure why we need to do this, but seems projectile
        // models are rotated 90 degrees
        orientation: Quaternion::from_angle_y(Deg(90.0)),
        // Projectile velocity is `root_transform * (0, 0, magnitude)`, so put
        // the muzzle at the origin and aim +Z down the barrel.
        root_transform: transform.0
            * Matrix4::from_translation(muzzle.point.to_vec())
            * Matrix4::from(get_rotation_from_forward_vector(muzzle.axis)),
        options: CreateEntityOptions {
            force_visible: true,
            ..CreateEntityOptions::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, point3, vec3};
    use dark::{motion::MotionFlags, properties::Links};

    use crate::physics::{CollisionGroup, PhysicsWorld};

    use super::*;

    fn flat_melee_fixture() -> (World, PhysicsWorld, EntityId, EntityId) {
        let mut world = World::new();
        let weapon = world.add_entity((
            Links::empty(),
            RuntimePropFlatAim {
                origin: point3(0.0, 0.0, 0.0),
                forward: vec3(0.0, 0.0, 1.0),
            },
        ));
        let target = world.add_entity(());

        let mut physics = PhysicsWorld::new();
        physics.add_kinematic(
            target,
            vec3(0.0, 0.0, 0.6),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.2, 0.2, 0.2),
            CollisionGroup::selectable(),
            false,
        );
        // Populate Rapier's broad phase before the first query, as the real
        // mission loop does each frame.
        let player = world.add_entity(());
        let mut player_handle = physics.create_player(vec3(100.0, 100.0, 100.0), player);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player_handle);

        assert!(
            includes_damage_to(
                flat_melee_hit(
                    &physics,
                    RuntimePropFlatAim {
                        origin: point3(0.0, 0.0, 0.0),
                        forward: vec3(0.0, 0.0, 1.0),
                    },
                    &world,
                ),
                target,
            ),
            "fixture must put a damageable target inside melee reach"
        );

        (world, physics, weapon, target)
    }

    fn includes_damage_to(effect: Effect, target: EntityId) -> bool {
        Effect::flatten(vec![effect]).into_iter().any(|effect| {
            matches!(
                effect,
                Effect::Send { msg }
                    if msg.to == target
                        && matches!(msg.payload, MessagePayload::Damage { .. })
            )
        })
    }

    /// Where a VR shot starts and which way it flies, in world space, from the
    /// `CreateEntity` the script returns.
    fn vr_fire_geometry(
        transform: Matrix4<f32>,
        bounds: collision::Aabb3<f32>,
        vhots: Vec<dark::ss2_bin_obj_loader::Vhot>,
    ) -> (cgmath::Vector3<f32>, cgmath::Vector3<f32>) {
        let mut world = World::new();
        let weapon = world.add_entity((
            Links::empty(),
            RuntimePropTransform(transform),
            RuntimePropModelBounds(bounds),
            RuntimePropVhots(vhots),
        ));

        let effect = create_projectile(
            &world,
            weapon,
            -1,
            &dark::properties::ProjectileOptions {
                order: 0,
                setting: 0,
            },
        );
        let Effect::CreateEntity {
            position,
            root_transform,
            ..
        } = effect
        else {
            panic!("firing must create the projectile entity");
        };
        // Projectile velocity is root_transform * (0, 0, magnitude).
        let origin = root_transform.transform_point(position).to_vec();
        let forward = root_transform.transform_vector(vec3(0.0, 0.0, 1.0));
        (origin, forward.normalize())
    }

    fn muzzle_vhot(point: cgmath::Point3<f32>) -> dark::ss2_bin_obj_loader::Vhot {
        dark::ss2_bin_obj_loader::Vhot {
            vhot_type: dark::ss2_bin_obj_loader::VhotType::Unknown,
            point,
        }
    }

    fn muzzle_flash_offset(
        bounds: collision::Aabb3<f32>,
        vhots: Vec<dark::ss2_bin_obj_loader::Vhot>,
        index: u32,
    ) -> cgmath::Point3<f32> {
        let mut world = World::new();
        let weapon = world.add_entity((
            Links::empty(),
            dark::properties::PropModelName("test_weapon".to_owned()),
            RuntimePropTransform(Matrix4::from_scale(1.0)),
            RuntimePropModelBounds(bounds),
            RuntimePropVhots(vhots),
        ));
        let effect = create_muzzle_flash(
            &world,
            weapon,
            -1,
            &GunFlashOptions {
                vhot: index,
                flags: 0,
            },
        );
        let Effect::CreateEntity { position, .. } = effect else {
            panic!("a muzzle flash link must create its effect");
        };
        position
    }

    /// A VR shot leaves the model's muzzle vhot travelling down the barrel
    /// (the model's -X), for any pose the hand happens to hold the weapon in.
    #[test]
    fn a_vr_shot_leaves_the_muzzle_vhot_along_the_barrel() {
        // A deliberately awkward pose: yawed, pitched and translated, so a
        // wrong fire axis cannot coincide with the right one.
        let rotation = Quaternion::from_angle_y(Deg(35.0)) * Quaternion::from_angle_x(Deg(-20.0));
        let translation = vec3(3.0, 1.5, -4.0);
        let transform = Matrix4::from_translation(translation) * Matrix4::from(rotation);
        let vhot = point3(-0.77, -0.03, 0.06);

        let bounds = collision::Aabb3::new(point3(-0.8, -0.3, -0.2), point3(0.8, 0.3, 0.2));
        let (origin, forward) = vr_fire_geometry(transform, bounds, vec![muzzle_vhot(vhot)]);

        let expected_origin = translation + rotation * vhot.to_vec();
        let expected_forward = rotation * vec3(-1.0, 0.0, 0.0);
        assert!(
            (origin - expected_origin).magnitude() < 1e-5,
            "shot started at {origin:?}, not at the muzzle {expected_origin:?}"
        );
        assert!(
            (forward - expected_forward).magnitude() < 1e-5,
            "shot flew {forward:?}, not down the barrel {expected_forward:?}"
        );
    }

    /// A no-vhot viewmodel fires from the end of its actual barrel geometry,
    /// not from the grip at the model origin.
    #[test]
    fn a_vr_shot_from_a_vhotless_model_falls_back_to_the_barrel_tip() {
        let rotation = Quaternion::from_angle_y(Deg(-90.0));
        let transform = Matrix4::from_translation(vec3(1.0, 2.0, 3.0)) * Matrix4::from(rotation);
        let bounds = collision::Aabb3::new(point3(-1.2, -0.2, -0.4), point3(0.6, 0.2, 0.4));

        let (origin, forward) = vr_fire_geometry(transform, bounds, vec![]);

        let expected_origin = transform.transform_point(point3(-1.2, 0.0, 0.0)).to_vec();
        assert!((origin - expected_origin).magnitude() < 1e-5);
        assert!((forward - rotation * vec3(-1.0, 0.0, 0.0)).magnitude() < 1e-5);
    }

    /// Classic world models such as ar15_w are Z-long. Their muzzle vhot is
    /// not necessarily first by type, so both the barrel axis and muzzle choice
    /// come from geometry rather than list position.
    #[test]
    fn a_classic_z_long_weapon_uses_the_frontmost_vhot_and_z_barrel() {
        let rotation = Quaternion::from_angle_y(Deg(23.0));
        let translation = vec3(-2.0, 1.0, 4.0);
        let transform = Matrix4::from_translation(translation) * Matrix4::from(rotation);
        let bounds = collision::Aabb3::new(point3(-0.1, -0.4, -1.2), point3(0.1, 0.4, 1.2));
        let breech = dark::ss2_bin_obj_loader::Vhot {
            vhot_type: dark::ss2_bin_obj_loader::VhotType::Unknown,
            point: point3(0.11, 0.2, -0.1),
        };
        let muzzle = dark::ss2_bin_obj_loader::Vhot {
            vhot_type: dark::ss2_bin_obj_loader::VhotType::LightSource,
            point: point3(0.0, 0.2, -1.24),
        };

        let (origin, forward) = vr_fire_geometry(transform, bounds, vec![breech, muzzle.clone()]);

        assert!((origin - (translation + rotation * muzzle.point.to_vec())).magnitude() < 1e-5);
        assert!((forward - rotation * vec3(0.0, 0.0, -1.0)).magnitude() < 1e-5);
    }

    /// A no-vhot weapon's projectile and flash must share the geometry-derived
    /// fire point; otherwise the shot leaves the barrel while the visible flash
    /// remains behind at the grip.
    #[test]
    fn a_vhotless_muzzle_flash_uses_the_barrel_tip_fallback() {
        let bounds = collision::Aabb3::new(point3(-1.4, -0.3, -0.2), point3(0.7, 0.3, 0.2));

        let flash = muzzle_flash_offset(bounds, vec![], 0);

        assert_eq!(flash, point3(-1.4, 0.0, 0.0));
    }

    /// An in-range GunFlash vhot remains a literal authored file index. The
    /// projectile's geometric "frontmost" selection must not rewrite casing
    /// and flash attachment semantics.
    #[test]
    fn a_muzzle_flash_uses_its_authored_vhot_index() {
        let bounds = collision::Aabb3::new(point3(-0.1, -0.4, -1.2), point3(0.1, 0.4, 1.2));
        let muzzle = muzzle_vhot(point3(0.0, 0.2, -1.24));
        let breech = muzzle_vhot(point3(0.11, 0.2, -0.1));

        let casing = muzzle_flash_offset(bounds, vec![muzzle, breech.clone()], 1);

        assert_eq!(casing, breech.point);
    }

    #[test]
    fn flat_melee_trigger_starts_the_animation_without_hitting_early() {
        let (world, physics, weapon, target) = flat_melee_fixture();

        let effect = WeaponScript::new().handle_message(
            weapon,
            &world,
            &physics,
            &MessagePayload::TriggerPull,
        );

        assert!(
            !includes_damage_to(effect, target),
            "the trigger edge is only the start of the visible swing"
        );
    }

    #[test]
    fn flat_melee_hits_on_the_authored_player_swing_trigger() {
        let (world, physics, weapon, target) = flat_melee_fixture();

        let effect = WeaponScript::new().handle_message(
            weapon,
            &world,
            &physics,
            &MessagePayload::AnimationFlagTriggered {
                motion_flags: MotionFlags::TRIGGER1,
            },
        );

        assert!(
            includes_damage_to(effect, target),
            "leftswing's MF_TRIGGER1 frame must resolve the aimed melee hit"
        );
    }
}
