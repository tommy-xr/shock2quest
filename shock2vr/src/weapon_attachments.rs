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

pub(crate) const LASER_RANGE: f32 = 50.0;

pub(crate) struct LaserTrace {
    pub origin: Point3<f32>,
    pub end: Point3<f32>,
    pub normal: Option<Vector3<f32>>,
}

/// Two bounded casts let the flat sight converge on the crosshair without
/// sending a beam through cover between the camera and the visible muzzle.
fn trace_laser(
    aim_origin: Point3<f32>,
    direction: Vector3<f32>,
    muzzle: Point3<f32>,
    cast: impl Fn(Point3<f32>, Vector3<f32>, f32) -> Option<(Point3<f32>, Vector3<f32>)>,
) -> LaserTrace {
    let target = cast(aim_origin, direction, LASER_RANGE)
        .map(|(point, _)| point)
        .unwrap_or(aim_origin + direction * LASER_RANGE);
    let delta = target - muzzle;
    let length = delta.magnitude();
    let hit = (length > 1.0e-5)
        .then(|| cast(muzzle, delta / length, length.min(LASER_RANGE) + 0.002))
        .flatten();
    LaserTrace {
        origin: muzzle,
        end: hit.map(|h| h.0).unwrap_or_else(|| {
            if length > 1.0e-5 {
                muzzle + delta * (length.min(LASER_RANGE) / length)
            } else {
                muzzle
            }
        }),
        normal: hit.map(|h| h.1),
    }
}

pub(crate) fn laser_trace(
    world: &World,
    physics: &crate::physics::PhysicsWorld,
    weapon: EntityId,
) -> Option<LaserTrace> {
    use crate::{physics::InternalCollisionGroups, runtime_props::RuntimePropViewmodelToWorld};
    use cgmath::Transform;
    if !enabled(world, weapon, WeaponAccessory::Laser) {
        return None;
    }
    let (mut origin, direction) = aim(world, weapon)?;
    let player = world
        .borrow::<shipyard::UniqueView<crate::mission::PlayerInfo>>()
        .ok()?;
    let can_hit = |entity| {
        entity != player.entity_id
            && !crate::wielded_weapon::held_in_hand(world, entity)
            && !(crate::creature::has_live_hit_boxes(world, entity)
                && crate::creature::hit_boxes_cover_body(world, entity))
    };
    let mut muzzle = origin;
    if let Some(mapping) = world
        .borrow::<View<RuntimePropViewmodelToWorld>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().copied())
    {
        let transform = world
            .borrow::<View<RuntimePropTransform>>()
            .ok()?
            .get(weapon)
            .ok()?
            .0;
        muzzle = mapping.0.transform_point(
            transform.transform_point(crate::weapon_muzzle::resolve(world, weapon).point),
        );
        muzzle =
            crate::weapon_muzzle::clamp_projectile_spawn(physics, origin, muzzle, 0.0, &can_hit);
    } else {
        // Match projectile creation when a VR barrel penetrates thin cover.
        let transform = world
            .borrow::<View<RuntimePropTransform>>()
            .ok()?
            .get(weapon)
            .ok()?
            .0;
        let gun_origin = transform.transform_point(Point3::new(0.0, 0.0, 0.0));
        muzzle = crate::weapon_muzzle::clamp_projectile_spawn(
            physics, gun_origin, muzzle, 0.0, &can_hit,
        );
        origin = muzzle;
    }
    Some(trace_laser(
        origin,
        direction,
        muzzle,
        |start, dir, range| {
            physics
                .ray_cast2_with_entity_filter(
                    start,
                    dir,
                    range,
                    InternalCollisionGroups::WORLD
                        | InternalCollisionGroups::ENTITIES
                        | InternalCollisionGroups::HITBOX
                        | InternalCollisionGroups::SELECTABLE,
                    Some(player.entity_id),
                    true,
                    &can_hit,
                )
                .map(|hit| (hit.hit_point, hit.hit_normal))
        },
    ))
}

pub(crate) fn render_lasers(
    world: &World,
    physics: &crate::physics::PhysicsWorld,
) -> Vec<engine::scene::SceneObject> {
    use cgmath::{Matrix4, vec3};
    use engine::scene::{SceneObject, SceneObjectDebugTag};
    let mut objects = Vec::new();
    let seconds = world
        .borrow::<shipyard::UniqueView<crate::time::Time>>()
        .map(|time| time.total.as_secs_f32())
        .unwrap_or(0.0);
    for weapon in [Handedness::Left, Handedness::Right]
        .into_iter()
        .filter_map(|hand| crate::wielded_weapon::weapon_in_hand(world, hand))
    {
        let Some(trace) = laser_trace(world, physics, weapon) else {
            continue;
        };
        if (trace.end - trace.origin).magnitude2() < 1.0e-8 {
            continue;
        }
        let forward = (trace.end - trace.origin).normalize();
        let tangent = if forward.y.abs() < 0.9 {
            vec3(0.0, 1.0, 0.0)
        } else {
            vec3(1.0, 0.0, 0.0)
        };
        let right = tangent.cross(forward).normalize();
        for (core, radius) in [(false, 0.018), (true, 0.0025)] {
            let mut beam = SceneObject::new(
                engine::scene::laser_material::create_beam(core, seconds),
                Box::new(engine::scene::cylinder::Cylinder),
            );
            beam.set_transform(Matrix4::from_cols(
                (right * radius).extend(0.0),
                (forward.cross(right) * radius).extend(0.0),
                (trace.end - trace.origin).extend(0.0),
                trace.origin.to_homogeneous(),
            ));
            beam.set_depth_write(false);
            beam.set_backface_culling(Some(engine::scene::FrontFaceWinding::CounterClockwise));
            beam.set_debug_tag(Some(std::rc::Rc::new(SceneObjectDebugTag {
                entity_id: Some(weapon.inner()),
                source: Some(
                    if core {
                        "weapon_laser_core"
                    } else {
                        "weapon_laser_halo"
                    }
                    .into(),
                ),
                ..Default::default()
            })));
            objects.push(beam);
        }
        let Some(normal) = trace.normal.filter(|n| n.magnitude2() > 1.0e-8) else {
            continue;
        };
        let normal = normal.normalize();
        let tangent = if normal.y.abs() < 0.9 {
            vec3(0.0, 1.0, 0.0)
        } else {
            vec3(1.0, 0.0, 0.0)
        };
        let right = tangent.cross(normal).normalize();
        // Surface-aligned, fixed world size: never a HUD marker or a billboard
        // that can poke through a nearby surface when the head moves.
        let mut dot = SceneObject::new(
            engine::scene::laser_material::create(vec3(1.0, 0.015, 0.01)),
            Box::new(engine::scene::quad::create()),
        );
        dot.set_transform(Matrix4::from_cols(
            (right * 0.11).extend(0.0),
            (normal.cross(right) * 0.11).extend(0.0),
            normal.extend(0.0),
            (trace.end + normal * 0.001).to_homogeneous(),
        ));
        dot.set_depth_write(false);
        dot.set_backface_culling(None);
        dot.set_debug_tag(Some(std::rc::Rc::new(SceneObjectDebugTag {
            entity_id: Some(weapon.inner()),
            source: Some("weapon_laser_dot".into()),
            ..Default::default()
        })));
        objects.push(dot);
    }
    objects
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
    fn laser_stops_at_nearest_solid_ignores_sensors_and_has_no_dot_on_miss() {
        use crate::physics::{CollisionGroup, InternalCollisionGroups, PhysicsWorld};
        let mut physics = PhysicsWorld::new();
        for (id, z, sensor) in [(20, 2.0, false), (22, 1.0, true), (23, 4.0, false)] {
            physics.add_kinematic(
                EntityId::from_inner(id).unwrap(),
                vec3(0.0, 0.0, z),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(4.0, 4.0, 0.02),
                CollisionGroup::entity(),
                sensor,
            );
        }
        let mut player =
            physics.create_player(vec3(10.0, 0.0, 0.0), EntityId::from_inner(21).unwrap());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let cast = |start, dir, range| {
            physics
                .ray_cast2(
                    start,
                    dir,
                    range,
                    InternalCollisionGroups::ENTITIES,
                    None,
                    true,
                )
                .map(|hit| (hit.hit_point, hit.hit_normal))
        };
        let hit = trace_laser(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            point3(0.3, -0.2, 0.1),
            cast,
        );
        assert!((hit.end.z - 1.99).abs() < 0.001);
        assert!(hit.end.x.abs() < 0.001, "converges at camera aim point");
        assert_eq!(hit.normal, Some(vec3(0.0, 0.0, -1.0)));
        let miss = trace_laser(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, -1.0),
            point3(0.0, 0.0, 0.0),
            cast,
        );
        assert!(miss.normal.is_none());
        assert_eq!((miss.end - miss.origin).magnitude(), LASER_RANGE);
        let edge = trace_laser(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            point3(0.0, 0.0, 1.99),
            cast,
        );
        assert!(edge.end.z.is_finite(), "zero-length target is safe");
    }

    #[test]
    fn vr_barrel_cannot_project_through_selectable_cover() {
        use crate::physics::{CollisionGroup, PhysicsWorld};
        let mut world = World::new();
        let player_id = world.add_entity(());
        let mut upgrades = WeaponUpgrades::default()
            .with_upgrade(
                WeaponUpgrade::Laser,
                &WeaponUpgrade::ALL,
                UpgradeSource::Device,
                0,
            )
            .unwrap();
        upgrades
            .set_accessory_enabled(WeaponAccessory::Laser, true)
            .unwrap();
        let weapon = world.add_entity((
            PropGunState {
                ammo: 12,
                condition: 100.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            upgrades,
            RuntimePropTransform(Matrix4::from_scale(1.0)),
            crate::weapon_muzzle::MuzzleFallback {
                point: point3(-2.0, 0.0, 0.0),
                axis: vec3(-1.0, 0.0, 0.0),
            },
        ));
        world.add_unique(PlayerInfo {
            entity_id: player_id,
            inventory_entity_id: player_id,
            left_hand_entity_id: Some(weapon),
            right_hand_entity_id: None,
            pos: vec3(10.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
        });
        let mut physics = PhysicsWorld::new();
        physics.add_kinematic(
            world.add_entity(()),
            vec3(-1.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.02, 4.0, 4.0),
            CollisionGroup::selectable(),
            false,
        );
        let mut player = physics.create_player(vec3(10.0, 0.0, 0.0), player_id);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let trace = laser_trace(&world, &physics, weapon).unwrap();
        assert!(
            trace.origin.x > -0.99,
            "emitter remains on firing side of cover"
        );
        assert!(
            (trace.end.x + 0.99).abs() < 0.001,
            "dot stops on selectable cover: {:?}",
            trace.end
        );
        assert!(trace.normal.is_some());
        let rendered = render_lasers(&world, &physics);
        assert_eq!(rendered.len(), 3, "one core, one halo, one dot");
        for beam in &rendered[..2] {
            use cgmath::Transform;
            let start = beam.get_transform().transform_point(point3(0.0, 0.0, 0.0));
            let end = beam.get_transform().transform_point(point3(0.0, 0.0, 1.0));
            assert!((start - trace.origin).magnitude() < 1.0e-5);
            assert!(
                (end - trace.end).magnitude() < 1.0e-5,
                "beam ends at the hit, without a stale trail"
            );
        }
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .left_hand_entity_id = None;
        assert!(
            laser_trace(&world, &physics, weapon).is_none(),
            "holstering removes the sight"
        );
    }

    #[test]
    fn cover_between_offset_muzzle_and_crosshair_wins() {
        let trace = trace_laser(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            point3(0.3, 0.0, 0.0),
            |start, dir, _| {
                let distance = if start.x > 0.1 { 0.2 } else { 5.0 };
                Some((start + dir * distance, -dir))
            },
        );
        assert!((trace.end - trace.origin).magnitude() < 0.201);
    }

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
