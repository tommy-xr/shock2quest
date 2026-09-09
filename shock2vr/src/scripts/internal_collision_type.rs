use cgmath::{Deg, EuclideanSpace, InnerSpace, Matrix4, Quaternion, Rotation3, SquareMatrix, vec3};
use dark::properties::{CollisionType, PropCollisionType};
use shipyard::{EntityId, Get, View, World};

use crate::{
    mission::entity_creator::CreateEntityOptions,
    physics::PhysicsWorld,
    scripts::script_util::{choose_impact_spang, projectile_contact_effects},
    util::{get_position_from_transform, get_rotation_from_forward_vector},
};

use super::{Effect, MessagePayload, Script, script_util::play_impact_sound};

// Script to handle collision type
pub struct InternalCollisionType {
    collision_flags: CollisionType,
    impact_handled: bool,
}

impl InternalCollisionType {
    pub fn new() -> InternalCollisionType {
        InternalCollisionType {
            collision_flags: CollisionType::empty(),
            impact_handled: false,
        }
    }
}

impl Script for InternalCollisionType {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_collision_flags = world.borrow::<View<PropCollisionType>>().unwrap();

        if let Ok(collision_flags) = v_collision_flags.get(entity_id) {
            self.collision_flags = collision_flags.collision_type;
        }

        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Collided { with, contact } => {
                // Only impact-payload entities (projectiles / fragile props
                // flagged to slay or destroy themselves on contact) deal
                // collision damage. A plain BOUNCE creature must not: any two
                // creatures with a collision type would otherwise damage each
                // other 1/frame on contact, killing low-HP crew during
                // crowded scripted scenes (e.g. the command2 flee cutscene,
                // where Suarez died bumping the other fleeing actors).
                let is_impact = self
                    .collision_flags
                    .intersects(CollisionType::SLAY_ON_IMPACT | CollisionType::DESTROY_ON_IMPACT);
                if !is_impact || self.impact_handled {
                    return Effect::NoEffect;
                }

                // A capsule and limb (or two world faces) can enqueue contact
                // messages before destruction is applied. One projectile has
                // one terminal impact, including damage, sound and spang.
                self.impact_handled = true;
                let initial_effect = if self.collision_flags.contains(CollisionType::SLAY_ON_IMPACT)
                {
                    Effect::SlayEntity { entity_id }
                } else {
                    Effect::DestroyEntity { entity_id }
                };
                let damage_effect = projectile_contact_effects(
                    world,
                    entity_id,
                    *with,
                    contact.map(|contact| crate::scripts::DamageImpact {
                        direction: contact.normal,
                        point: contact.point,
                        bone: None,
                    }),
                );
                let mut effects = vec![initial_effect, damage_effect];

                let position = get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0));

                // Impact effect, from the projectile's authored spang links
                // (HitSpang matched by victim class, MissSpang fallback) -
                // the same selection as the fast (raycast) projectile path.
                // This is what makes slow projectiles (laser/fusion bolts,
                // grenades) show their authored impact FX. At most one spang
                // per projectile: an impact starting contact with several
                // colliders at once (a corner, a creature capsule + its
                // hitbox proxy) queues multiple Collided messages before the
                // slay/destroy effect lands.
                if let Some(template_id) = choose_impact_spang(world, entity_id, *with) {
                    // The physics collision event carries no contact normal,
                    // and spang orientation is minor cosmetics, so
                    // approximate the impact facing with the reversed
                    // velocity. This is read post-solve, so it may already be
                    // deflected by the contact; if the solver stopped the
                    // projectile outright, fall back to straight up.
                    let facing = physics
                        .get_velocity(entity_id)
                        .filter(|v| v.magnitude2() > 1e-6)
                        .map(|v| -v.normalize())
                        .unwrap_or(vec3(0.0, 1.0, 0.0));
                    effects.push(Effect::CreateEntity {
                        template_id,
                        position,
                        orientation: get_rotation_from_forward_vector(facing)
                            * Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Deg(90.0)),
                        root_transform: Matrix4::identity(),
                        options: CreateEntityOptions {
                            transient_fx: true,
                            ..CreateEntityOptions::default()
                        },
                    });
                }

                // Impact sound (material-tagged collision schema), unless the
                // collision type opts out. FULL_COLLISION_SOUND needs no
                // special handling: impact sounds always play at full volume
                // here (no velocity scaling).
                if !self
                    .collision_flags
                    .contains(CollisionType::NO_COLLISION_SOUND)
                {
                    effects.push(play_impact_sound(
                        world,
                        entity_id,
                        *with,
                        position.to_vec(),
                    ));
                }

                Effect::Multiple(effects)
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terminal_impact_is_handled_once_even_before_destruction_is_applied() {
        let mut world = World::new();
        let projectile = world.add_entity(PropCollisionType {
            collision_type: CollisionType::DESTROY_ON_IMPACT | CollisionType::NO_COLLISION_SOUND,
        });
        world.add_component(
            projectile,
            crate::runtime_props::RuntimePropTransform(Matrix4::identity()),
        );
        let victim = world.add_entity(());
        let physics = PhysicsWorld::new();
        let mut script = InternalCollisionType::new();
        script.initialize(projectile, &world);
        let message = MessagePayload::Collided {
            with: victim,
            contact: None,
        };
        assert!(matches!(
            script.handle_message(projectile, &world, &physics, &message),
            Effect::Multiple(_)
        ));
        assert!(matches!(
            script.handle_message(projectile, &world, &physics, &message),
            Effect::NoEffect
        ));
    }
}
