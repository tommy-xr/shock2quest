//! Equipped-weapon attachment effects, shared by flat and VR presentations.
use cgmath::{EuclideanSpace, InnerSpace, Point3, Vector3, Vector4};
use engine::scene::light::SpotLight;
use shipyard::{EntityId, Get, View, World};

use crate::{
    runtime_props::{RuntimePropFlatAim, RuntimePropTransform},
    vr_config::Handedness,
    weapon_upgrades::{WeaponAccessory, WeaponUpgrades},
};

pub(crate) fn enabled(world: &World, weapon: EntityId, accessory: WeaponAccessory) -> bool {
    crate::wielded_weapon::held_in_hand(world, weapon)
        && world
            .borrow::<View<WeaponUpgrades>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|u| u.accessory_enabled(accessory)))
            .unwrap_or(false)
}

/// Nominal shot ray, before random spread: flat uses its authored camera ray;
/// VR uses the same rendered muzzle frame as projectile creation.
pub(crate) fn aim(world: &World, weapon: EntityId) -> Option<(Point3<f32>, Vector3<f32>)> {
    if let Some(aim) = world
        .borrow::<View<RuntimePropFlatAim>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().copied())
    {
        return (aim.forward.magnitude2() > 1.0e-8).then(|| (aim.origin, aim.forward.normalize()));
    }
    let transform = world
        .borrow::<View<RuntimePropTransform>>()
        .ok()?
        .get(weapon)
        .ok()?
        .0;
    let frame = crate::weapon_muzzle::resolve(world, weapon).shot_frame(transform);
    Some((
        Point3::from_vec(frame.w.truncate()),
        frame.z.truncate().normalize(),
    ))
}

pub(crate) fn flashlights(world: &World) -> Vec<SpotLight> {
    [Handedness::Left, Handedness::Right]
        .into_iter()
        .filter_map(|hand| crate::wielded_weapon::weapon_in_hand(world, hand))
        .filter(|weapon| enabled(world, *weapon, WeaponAccessory::Flashlight))
        .filter_map(|weapon| aim(world, weapon))
        .map(|(origin, direction)| SpotLight {
            position: origin.to_vec(),
            direction,
            color_intensity: Vector4::new(
                1.0,
                1.0,
                0.8,
                crate::dev_params::get(crate::dev_params::SPOTLIGHT_INTENSITY),
            ),
            inner_cone_angle: (crate::dev_params::get(crate::dev_params::SPOTLIGHT_CONE) / 2.0)
                .to_radians(),
            outer_cone_angle: crate::dev_params::get(crate::dev_params::SPOTLIGHT_CONE)
                .to_radians(),
            range: crate::dev_params::get(crate::dev_params::SPOTLIGHT_RANGE),
        })
        .collect()
}

pub(crate) fn set_enabled(
    world: &mut World,
    weapon: EntityId,
    accessory: WeaponAccessory,
    enabled: bool,
) {
    if !crate::wielded_weapon::held_in_hand(world, weapon)
        && !crate::scripts::gui::WeaponSettingsTarget::permits_device_job(world, weapon)
    {
        return;
    }
    let mut upgrades = crate::weapon_installation::state(world, weapon);
    if upgrades.set_accessory_enabled(accessory, enabled).is_ok() {
        world.add_component(weapon, upgrades);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mission::PlayerInfo,
        weapon_upgrades::{UpgradeSource, WeaponUpgrade},
    };
    use cgmath::{Matrix4, Quaternion, point3, vec3};
    use dark::properties::PropGunState;
    use shipyard::UniqueViewMut;

    #[test]
    fn lights_follow_equipped_weapons_and_preserve_switches_when_stowed() {
        let mut world = World::new();
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        let mut upgrades = WeaponUpgrades::default()
            .with_upgrade(
                WeaponUpgrade::Flashlight,
                &WeaponUpgrade::ALL,
                UpgradeSource::Device,
                0,
            )
            .unwrap();
        upgrades
            .set_accessory_enabled(WeaponAccessory::Flashlight, true)
            .unwrap();
        let gun = PropGunState {
            ammo: 12,
            condition: 100.0,
            setting: 0,
            modification: 0,
            silence_value: 0.0,
        };
        let left = world.add_entity((
            gun.clone(),
            upgrades.clone(),
            RuntimePropFlatAim {
                origin: point3(1.0, 2.0, 3.0),
                forward: vec3(0.0, 0.0, -2.0),
            },
        ));
        let right = world.add_entity((
            gun,
            upgrades.clone(),
            RuntimePropTransform(Matrix4::from_translation(vec3(5.0, 0.0, 0.0))),
        ));
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            inventory_entity_id: inventory,
            left_hand_entity_id: Some(left),
            right_hand_entity_id: Some(right),
        });
        let lights = flashlights(&world);
        assert_eq!(lights.len(), 2);
        assert_eq!(lights[0].position, vec3(1.0, 2.0, 3.0));
        assert_eq!(lights[0].direction, vec3(0.0, 0.0, -1.0));
        assert_eq!(lights[1].position, vec3(5.0, 0.0, 0.0));
        assert_eq!(lights[1].direction, vec3(-1.0, 0.0, 0.0));
        set_enabled(&mut world, left, WeaponAccessory::Flashlight, false);
        assert_eq!(flashlights(&world).len(), 1);
        assert!(enabled(&world, right, WeaponAccessory::Flashlight));
        world
            .borrow::<UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .right_hand_entity_id = None;
        assert!(
            flashlights(&world).is_empty(),
            "holstered guns emit no light"
        );
        let saved =
            serde_json::to_string(&crate::weapon_installation::state(&world, right)).unwrap();
        let restored: WeaponUpgrades = serde_json::from_str(&saved).unwrap();
        assert!(restored.accessory_enabled(WeaponAccessory::Flashlight));
        world.add_component(right, restored);
        world
            .borrow::<UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .left_hand_entity_id = Some(right);
        assert_eq!(
            flashlights(&world).len(),
            1,
            "same weapon works in the other hand"
        );
        set_enabled(&mut world, right, WeaponAccessory::Laser, true);
        assert!(
            !enabled(&world, right, WeaponAccessory::Laser),
            "uninstalled accessories cannot be enabled"
        );
    }
}
