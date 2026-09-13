use std::cell::RefCell;

use cgmath::{Deg, InnerSpace, Point3, Quaternion, Rotation3, Vector3};
use dark::{
    SCALE_FACTOR,
    motion::MotionQueryItem,
    properties::{PropAIRangedCombat, PropAIRangedRanges, PropPosition},
};
use shipyard::{EntityId, Get, View, World};

use super::{Behavior, ChaseBehavior, NextBehavior, RangedAttackBehavior};
use crate::{
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::{
        Effect,
        ai::{
            ai_util,
            steering::{ChasePlayerSteeringStrategy, SteeringOutput, SteeringStrategy},
        },
    },
    time::Time,
};

#[derive(Clone, Copy)]
pub(super) struct StandOff {
    pub trigger: f32,
    pub ideal: f32,
    firing_delay: f32,
    moving_fire: i32,
}

pub(super) fn authored_stand_off(world: &World, entity: EntityId) -> Option<StandOff> {
    let props = world.borrow::<View<PropAIRangedCombat>>().ok()?;
    let prop = props.get(entity).ok()?;
    let ideal = (prop.ideal_distance.max(prop.minimum_distance) as f32 / SCALE_FACTOR)
        .min(super::RANGED_MAX_ATTACK_DISTANCE - 0.2);
    if ideal <= 0.0 {
        return None;
    }
    let short = world
        .borrow::<View<PropAIRangedRanges>>()
        .ok()
        .and_then(|ranges| ranges.get(entity).ok().copied())
        .unwrap_or_default()
        .0[1];
    Some(StandOff {
        // The grenade hybrid's AIRCProp minimum is zero. Its companion
        // AIRCRange short band still calls for giving ground near a target.
        trigger: (prop.minimum_distance.max(0) as f32 / SCALE_FACTOR)
            .max(short)
            .min(ideal),
        ideal,
        firing_delay: prop.firing_delay.max(0.0),
        moving_fire: prop.fire_while_moving.clamp(0, 5),
    })
}

/// A short reverse stride must have both body clearance and continuous floor
/// support. Physical collision remains authoritative; these probes prevent a
/// blind retreat off a ledge or into an already-obstructed corridor.
pub(super) fn can_back_off(world: &World, physics: &PhysicsWorld, entity: EntityId) -> bool {
    let positions = world.borrow::<View<PropPosition>>().unwrap();
    let Ok(position) = positions.get(entity) else {
        return false;
    };
    let origin = Point3::new(
        position.position.x,
        position.position.y,
        position.position.z,
    );
    let backwards =
        -(Quaternion::from_angle_y(ai_util::current_yaw(entity, world)) * Vector3::unit_z());
    const STRIDE: f32 = 1.2;
    if physics.projectile_spawn_distance(origin, backwards, STRIDE, 0.45, &|other| other != entity)
        < STRIDE - 0.02
    {
        return false;
    }
    let floor = |point: Point3<f32>| {
        physics
            .ray_cast2_as_actor(
                point,
                -Vector3::unit_y(),
                3.0,
                InternalCollisionGroups::WORLD | InternalCollisionGroups::ENTITY,
                Some(entity),
                true,
            )
            .filter(|hit| hit.hit_normal.y > 0.55)
            .map(|hit| hit.hit_point.y)
    };
    let Some(start_floor) = floor(origin) else {
        return false;
    };
    [0.3, 0.6, 0.9, STRIDE].into_iter().all(|distance| {
        floor(origin + backwards * distance)
            .is_some_and(|height| (height - start_floor).abs() < 0.35)
    })
}

pub struct BackOffBehavior {
    stand_off: StandOff,
    remaining_delay: f32,
    stopped: bool,
    clip_start: Option<Vector3<f32>>,
}
impl BackOffBehavior {
    pub(super) fn new(stand_off: StandOff) -> Self {
        Self {
            stand_off,
            remaining_delay: stand_off.firing_delay,
            stopped: false,
            clip_start: None,
        }
    }
}
impl Behavior for BackOffBehavior {
    fn name(&self) -> &'static str {
        "BackOff"
    }
    fn animation(&self) -> Vec<MotionQueryItem> {
        // Required direction: falling back to a forward walk would close
        // the distance while the behavior claims to retreat.
        vec![
            MotionQueryItem::new("locomote"),
            MotionQueryItem::with_value("direction", 4),
        ]
    }
    fn is_locomotion(&self) -> bool {
        true
    }
    fn holds_position(&self) -> bool {
        self.stopped
    }
    fn steer(
        &mut self,
        heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        if self.clip_start.is_none() {
            self.clip_start = world
                .borrow::<View<PropPosition>>()
                .ok()
                .and_then(|positions| positions.get(entity).ok().map(|p| p.position));
        }
        self.remaining_delay = (self.remaining_delay - time.elapsed.as_secs_f32()).max(0.0);
        self.stopped = ai_util::chase_target_distance(world, entity)
            .is_none_or(|distance| distance >= self.stand_off.ideal)
            || !can_back_off(world, physics, entity);
        ChasePlayerSteeringStrategy.steer(heading, world, physics, entity, time)
    }
    fn next_behavior(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        entity: EntityId,
    ) -> NextBehavior {
        let target = ai_util::chase_target(world, entity);
        let clear =
            target.is_some_and(|target| ai_util::has_line_of_fire(entity, world, physics, target));
        if self.stopped || !clear {
            return NextBehavior::Next(
                super::attack_behavior_for_distance(world, physics, entity)
                    .unwrap_or_else(|| Box::new(RefCell::new(ChaseBehavior::new()))),
            );
        }
        let position = world
            .borrow::<View<PropPosition>>()
            .unwrap()
            .get(entity)
            .ok()
            .map(|p| p.position);
        let progressed = self
            .clip_start
            .zip(position)
            .is_some_and(|(start, end)| (end - start).magnitude2() > 0.0025);
        self.clip_start = position;
        // A missing reverse clip or an obstructed stride must not trap the
        // actor retrying a movement that cannot happen. It can still shoot.
        if !progressed {
            return NextBehavior::Next(Box::new(RefCell::new(RangedAttackBehavior)));
        }

        // Dark's moving-fire preference is a squared chance, not a boolean.
        // It pauses travel for an authored attack clip rather than inventing
        // a projectile timer or firing without a weapon animation.
        use rand::Rng;
        if self.remaining_delay <= 0.0
            && rand::thread_rng().gen_range(0..=50) < self.stand_off.moving_fire.pow(2)
        {
            return NextBehavior::Next(Box::new(RefCell::new(RangedAttackBehavior)));
        }
        NextBehavior::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mission::PlayerInfo, physics::CollisionGroup};
    use cgmath::vec3;
    use dark::properties::{
        AIProjectileOptions, AITargetMethod, Link, Links, PropHitPoints, ToLink,
    };

    fn fixture(
        melee: bool,
        minimum: i32,
        ideal: i32,
        floor: bool,
    ) -> (World, PhysicsWorld, EntityId) {
        let mut world = World::new();
        let mut links = vec![ToLink {
            to_template_id: -1,
            to_entity_id: None,
            link: Link::AIProjectile(AIProjectileOptions {
                targeting_method: AITargetMethod::StraightLine,
                delay: 0.0,
                should_lead_target: false,
                ammo: 0,
                accuracy: 0,
                select_time: 0.0,
                joint: 0,
                vhot: 0,
            }),
        }];
        if melee {
            links.push(ToLink {
                to_template_id: -2,
                to_entity_id: None,
                link: Link::Weapon,
            });
        }
        let entity = world.add_entity((
            PropPosition {
                position: vec3(0.0, 1.0, 0.0),
                rotation: Quaternion::from_angle_y(Deg(0.0)),
                cell: 0,
            },
            PropHitPoints { hit_points: 15 },
            Links { to_links: links },
            PropAIRangedCombat {
                minimum_distance: minimum,
                ideal_distance: ideal,
                firing_delay: 0.0,
                cover_desire: 0,
                decay_speed: 0.8,
                fire_while_moving: 0,
                contain_projectile: 0,
            },
        ));
        world.add_component(
            entity,
            crate::runtime_props::RuntimePropTransform(cgmath::Matrix4::from_translation(vec3(
                0.0, 1.0, 0.0,
            ))),
        );
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 1.0, 1.0),
            rotation: Quaternion::from_angle_y(Deg(0.0)),
            entity_id: EntityId::dead(),
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: EntityId::dead(),
        });
        let mut physics = PhysicsWorld::new();
        if floor {
            let ground = world.add_entity(());
            physics.add_kinematic(
                ground,
                vec3(0.0, -0.5, 0.0),
                Quaternion::from_angle_y(Deg(0.0)),
                Vector3::zero(),
                vec3(20.0, 1.0, 20.0),
                CollisionGroup::entity(),
                false,
            );
        }
        let mut player = physics.create_player(vec3(30.0, 1.0, 30.0), EntityId::dead());
        physics.update(Vector3::zero(), &mut player);
        (world, physics, entity)
    }
    use cgmath::Zero;

    #[test]
    fn gun_only_creatures_back_off_but_cornered_and_melee_creatures_still_attack() {
        for (melee, minimum, ideal, floor, expected) in [
            (false, 10, 40, true, "BackOff"),
            (false, 0, 20, true, "BackOff"),
            (false, 10, 40, false, "RangedAttack"),
            (true, 10, 40, true, "MeleeAttack"),
        ] {
            let (world, physics, entity) = fixture(melee, minimum, ideal, floor);
            let selected =
                super::super::attack_behavior_for_distance(&world, &physics, entity).unwrap();
            assert_eq!(selected.borrow().name(), expected);
        }
    }

    #[test]
    fn reverse_clearance_rejects_walls_and_missing_support() {
        let (mut world, mut physics, entity) = fixture(false, 10, 40, true);
        assert!(can_back_off(&world, &physics, entity));
        let wall = world.add_entity(());
        physics.add_kinematic(
            wall,
            vec3(0.0, 1.0, -1.0),
            Quaternion::from_angle_y(Deg(0.0)),
            Vector3::zero(),
            vec3(3.0, 4.0, 0.2),
            CollisionGroup::entity(),
            false,
        );
        let mut player = physics.create_player(vec3(30.0, 1.0, 30.0), EntityId::dead());
        physics.update(Vector3::zero(), &mut player);
        assert!(!can_back_off(&world, &physics, entity));
        let (world, physics, entity) = fixture(false, 10, 40, false);
        assert!(!can_back_off(&world, &physics, entity));
    }

    #[test]
    fn reverse_stride_rejects_a_ledge_even_when_the_actor_has_floor() {
        let (mut world, mut physics, entity) = fixture(false, 10, 40, false);
        let platform = world.add_entity(());
        physics.add_kinematic(
            platform,
            vec3(0.0, -0.5, 1.0),
            Quaternion::from_angle_y(Deg(0.0)),
            Vector3::zero(),
            vec3(10.0, 1.0, 3.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player = physics.create_player(vec3(30.0, 1.0, 30.0), EntityId::dead());
        physics.update(Vector3::zero(), &mut player);
        assert!(!can_back_off(&world, &physics, entity));
    }

    #[test]
    fn missing_or_stalled_reverse_motion_falls_back_to_firing() {
        let (world, physics, entity) = fixture(false, 10, 40, true);
        let mut behavior = BackOffBehavior::new(authored_stand_off(&world, entity).unwrap());
        behavior.clip_start = Some(vec3(0.0, 1.0, 0.0));
        let NextBehavior::Next(next) = behavior.next_behavior(&world, &physics, entity) else {
            panic!("must not stall");
        };
        assert_eq!(next.borrow().name(), "RangedAttack");
    }
}
