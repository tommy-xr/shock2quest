use cgmath::{Deg, Matrix4, Quaternion, Rotation, Rotation3, Transform, point3};
use dark::properties::{GunFlashOptions, Link, ProjectileOptions};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::{entity_creator::CreateEntityOptions, mission_core::GlobalTemplateClassTags},
    physics::PhysicsWorld,
    runtime_props::{RuntimePropTransform, RuntimePropVhots},
    vr_config,
};

use super::{
    Effect, MessagePayload, Script,
    script_util::{
        get_all_links_with_template, get_first_link_with_template_and_data,
        play_environmental_sound,
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
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TriggerPull => {
                //Create muzzle flash
                let muzzle_flashes =
                    get_all_links_with_template(world, entity_id, |link| match link {
                        Link::GunFlash(data) => Some(*data),
                        _ => None,
                    });

                // TODO: Handle setting or ammo type? This just picks the very first projectile
                let maybe_projectile =
                    get_first_link_with_template_and_data(world, entity_id, |link| match link {
                        Link::Projectile(data) => Some(*data),
                        _ => None,
                    });

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

                Effect::Multiple(vec![sound_effect, muzzle_flash_effect, projectile_effect])
            }
            MessagePayload::TriggerRelease => Effect::NoEffect,
            _ => Effect::NoEffect,
        }
    }
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
        options: CreateEntityOptions::default(),
    }
}

fn create_projectile(
    world: &World,
    entity_id: EntityId,
    projectile_template_id: i32,
    _options: &ProjectileOptions,
) -> Effect {
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
        },
    }
}
