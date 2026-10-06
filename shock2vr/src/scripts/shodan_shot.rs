use dark::properties::PropTemplateId;
use shipyard::{EntityId, IntoIter, IntoWithId, UniqueView, View, World};

use crate::{
    mission::mission_core::{GlobalEntityMetadata, GlobalTemplateHierarchy},
    physics::PhysicsWorld,
};

use super::{
    Effect, MessagePayload, Script, internal_collision_type::terminal_impact_effects,
    script_util::entity_class_template_id,
};

/// Retail ShodanShot (Physics): ignore ShodanShields descendants and slay on
/// every other contact. The gamesys deliberately authors BOUNCE, leaving the
/// terminal impact to this script rather than InternalCollisionType.
/// See https://thiefmissions.com/telliamed/allscripts.html (ShodanShot).
/// Retail allobjs DLL PhysCollision at RVA 0x19f40 checks shield ancestry,
/// returns 1 (ignore) for shields, and calls Slay(self, self) otherwise.
pub struct ShodanShot {
    impact_handled: bool,
}

impl ShodanShot {
    pub fn new() -> Self {
        Self {
            impact_handled: false,
        }
    }
}

fn is_shield(world: &World, entity: EntityId) -> bool {
    let Some(template) = entity_class_template_id(world, entity) else {
        return false;
    };
    let Ok(metadata) = world.borrow::<UniqueView<GlobalEntityMetadata>>() else {
        return false;
    };
    let Some(shields) = metadata.0.get("shodanshields") else {
        return false;
    };
    world
        .borrow::<UniqueView<GlobalTemplateHierarchy>>()
        .is_ok_and(|hierarchy| hierarchy.is_or_descends_from(template, shields.template_id))
}

/// Derive physical exclusions during body instantiation, before the first
/// physics step (ordinary script initialization runs after that step). The
/// mission applies this Effect; this function only reads authored identities.
/// Recomputed for loaded bodies, so no saved runtime IDs or lifecycle bypass.
pub(crate) fn initial_collision_effect(entity_id: EntityId, world: &World) -> Effect {
    let templates = world.borrow::<View<PropTemplateId>>().unwrap();
    let others = templates
        .iter()
        .with_id()
        .filter_map(|(entity, _)| is_shield(world, entity).then_some(entity))
        .collect();
    Effect::IgnoreCollisionPairs { entity_id, others }
}

impl Script for ShodanShot {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let MessagePayload::Collided { with, contact } = msg else {
            return Effect::NoEffect;
        };
        if self.impact_handled || is_shield(world, *with) {
            return Effect::NoEffect;
        }
        self.impact_handled = true;
        // Preserve the projectile's authored ShodanStim and corpse explosion;
        // this is the same terminal-contact path as other slow projectiles.
        terminal_impact_effects(world, physics, entity_id, *with, *contact, true, true)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use cgmath::{Matrix4, SquareMatrix};

    use super::*;
    use crate::{mission::mission_core::EntityMetadata, runtime_props::RuntimePropTransform};

    fn fixture(offset: usize) -> (World, EntityId, EntityId, EntityId) {
        let mut world = World::new();
        for _ in 0..offset {
            world.add_entity(());
        }
        world.add_unique(GlobalEntityMetadata(HashMap::from([(
            "shodanshields".into(),
            EntityMetadata {
                template_id: -100,
                obj_icon: None,
                obj_short_name: None,
                obj_name: None,
            },
        )])));
        world.add_unique(GlobalTemplateHierarchy(HashMap::from([
            (800, vec![-101]),
            (-101, vec![-100]),
        ])));
        let shot = world.add_entity((
            PropTemplateId { template_id: -3496 },
            RuntimePropTransform(Matrix4::identity()),
        ));
        let shield = world.add_entity(PropTemplateId { template_id: 800 });
        let victim = world.add_entity(PropTemplateId { template_id: 801 });
        (world, shot, shield, victim)
    }

    #[test]
    fn shodan_shot_excludes_only_shield_descendants_on_fresh_or_restored_initialization() {
        // Instantiation rebuilds the same derived rule for fresh and restored
        // bodies, instead of serializing runtime IDs.
        for offset in [0, 7] {
            let (world, shot, shield, _) = fixture(offset);
            let Effect::IgnoreCollisionPairs { entity_id, others } =
                initial_collision_effect(shot, &world)
            else {
                panic!("missing physical shield exclusions")
            };
            assert_eq!(entity_id, shot);
            assert_eq!(others, vec![shield]);
        }
    }

    #[test]
    fn shodan_shot_passes_shield_then_slays_on_ordinary_contact_once() {
        let (world, shot, shield, victim) = fixture(0);
        let physics = PhysicsWorld::new();
        let mut script = ShodanShot::new();
        assert!(matches!(
            script.handle_message(
                shot,
                &world,
                &physics,
                &MessagePayload::Collided {
                    with: shield,
                    contact: None,
                }
            ),
            Effect::NoEffect
        ));
        let contact = MessagePayload::Collided {
            with: victim,
            contact: None,
        };
        let Effect::Multiple(effects) = script.handle_message(shot, &world, &physics, &contact)
        else {
            panic!("ordinary contact must use the terminal impact path")
        };
        assert!(effects.iter().any(|effect| matches!(effect,
            Effect::SlayEntity { entity_id } if *entity_id == shot)));
        assert!(matches!(
            script.handle_message(shot, &world, &physics, &contact),
            Effect::NoEffect
        ));
    }

    #[test]
    fn shodan_shot_authored_bounce_delivers_one_stim16_and_one_slay() {
        use crate::{
            mission::stim_response::GlobalContactStims,
            scripts::internal_collision_type::InternalCollisionType,
        };
        use dark::properties::{
            CollisionType, PropCollisionType, ReceptronEffect, ReceptronOptions,
        };
        use dark::properties::{Link, Links, ToLink};

        let (mut world, shot, _, victim) = fixture(0);
        world.add_unique(GlobalContactStims(HashMap::from([(
            -3496,
            vec![(-4351, 16.0)],
        )])));
        world.add_component(
            shot,
            PropCollisionType {
                collision_type: CollisionType::BOUNCE,
            },
        );
        world.add_component(
            victim,
            Links {
                to_links: vec![ToLink {
                    to_template_id: -4351,
                    to_entity_id: None,
                    link: Link::Receptron(ReceptronOptions {
                        order: 1,
                        effect: ReceptronEffect::Damage {
                            multiplier: 1.0,
                            use_intensity: true,
                        },
                    }),
                }],
            },
        );
        let physics = PhysicsWorld::new();
        let mut shot_script = ShodanShot::new();
        let mut collision_script = InternalCollisionType::new();
        collision_script.initialize(shot, &world);
        let contact = MessagePayload::Collided {
            with: victim,
            contact: None,
        };
        let mut effects = Vec::new();
        // Multiple collider contacts can arrive before the terminal effect is
        // applied. Both authored and internal scripts see each notification.
        for _ in 0..2 {
            effects.push(shot_script.handle_message(shot, &world, &physics, &contact));
            effects.push(collision_script.handle_message(shot, &world, &physics, &contact));
        }
        let flattened = Effect::flatten(effects);
        assert_eq!(
            flattened
                .iter()
                .filter(|effect| matches!(effect,
            Effect::SlayEntity { entity_id } if *entity_id == shot))
                .count(),
            1
        );
        let damage = flattened
            .iter()
            .filter_map(|effect| match effect {
                Effect::Send { msg } if msg.to == victim => match msg.payload {
                    MessagePayload::Damage { amount, .. } => Some(amount),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(damage, vec![16.0]);
    }
}
