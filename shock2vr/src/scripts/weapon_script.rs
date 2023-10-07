use std::f32::consts::PI;

use cgmath::{
    point3, vec3, Deg, EuclideanSpace, Matrix4, Quaternion, Rad, Rotation, Rotation3, Transform,
};
use dark::properties::{
    GunFlashOptions, InternalPropOriginalModelName, Link, ProjectileOptions, PropLimbModel,
    PropPlayerGun,
};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::{
    physics::PhysicsWorld,
    runtime_props::{RuntimePropTransform, RuntimePropVhots},
    vr_config,
};

use super::{
    script_util::{
        get_all_links_with_template, get_first_link_with_template_and_data,
        play_environmental_sound,
    },
    Effect, MessagePayload, Script,
};

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
                let sound_effect =
                    play_environmental_sound(world, entity_id, "shoot", vec![], AudioHandle::new());
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
        .unwrap_or(vec3(0.0, 0.0, 0.0));

    let transform = v_transform.get(entity_id).unwrap();

    let orientation = Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Rad(0.0));
    // let rot_matrix: Matrix4<f32> =
    //     Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Rad(PI / 2.0)).into();

    // let xform_matrix = Matrix4::from_translation(vhot_offset);
    println!("debug!! vhots: {:?} vhot_offset: {:?}", vhots, vhot_offset);
    Effect::CreateEntity {
        template_id: muzzle_flash_template_id,
        position: vhot_offset,
        orientation,
        root_transform: transform.0,
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
    let vhot_offset = vhots.get(0).map(|v| v.point).unwrap_or(vec3(0.0, 0.0, 0.0));

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
    let forward = vec3(0.0, 0.0, -1.0);
    //let vhot_p = point3(vhot_offset.x, vhot_offset.y, vhot_offset.z);
    //let position = rot_matrix.transform_point(vhot_p);

    Effect::CreateEntity {
        template_id: projectile_template_id,
        position: forward,
        orientation: Quaternion::from_angle_y(Deg(90.0)),
        root_transform: transform.0 * rot_matrix * projectile_rotation,
    }
}
