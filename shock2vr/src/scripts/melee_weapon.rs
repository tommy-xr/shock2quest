use std::collections::{HashMap, HashSet};

use cgmath::{EuclideanSpace, InnerSpace, vec3};
use dark::properties::{CollisionType, PropCollisionType};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    PresentationMode,
    mission::{GlobalPresentationMode, stim_response::contact_stim_damage},
    physics::PhysicsWorld,
    util::get_position_from_transform,
};

use super::{
    Effect, Message, MessagePayload, Script,
    script_util::{entity_class_template_id, play_impact_sound},
};

// Script to handle collision type
pub struct MeleeWeapon {}

impl MeleeWeapon {
    pub fn new() -> MeleeWeapon {
        MeleeWeapon {}
    }
}

/// Contact damage for the player's authored melee weapons (`PropLimbModel`).
///
/// Dark opens melee collision only during an attack window. In VR the trigger
/// edge is that production attack gesture: contacts before it, after release,
/// and after dropping the weapon are harmless. A target can be damaged only
/// once per pull even if the physical swing chatters across several contacts.
///
/// Physical contact is *never* the damage trigger outside VR - a flat swing is
/// `WeaponScript`'s aimed short-range raycast, so bumping a wielded wrench into
/// scenery must do nothing. The whole script is therefore inert in flat mode
/// rather than guarding individual messages.
pub struct TriggeredMeleeWeapon {
    attack_active: bool,
    hit_entities: HashSet<EntityId>,
    /// Remaining seconds before each already-hit victim may be hit again.
    /// Only used by the free-swing rule: the trigger rule re-arms on the next
    /// pull instead, so it has nothing to expire.
    free_swing_cooldowns: HashMap<EntityId, f32>,
}

impl TriggeredMeleeWeapon {
    pub fn new() -> Self {
        Self {
            attack_active: false,
            hit_entities: HashSet::new(),
            free_swing_cooldowns: HashMap::new(),
        }
    }
}

/// How long one victim is immune to a further free swing from the same weapon.
/// A single controller swing crosses a body over several frames, so without
/// this a swing bills once per contact frame; long enough to cost one hit per
/// swing, short enough not to eat a genuine second swing.
const FREE_SWING_COOLDOWN_SECONDS: f32 = 0.4;

/// The physical alternative to the trigger window: a swing damages because it
/// was *moving*, not because a button was down. `None` when the shipped
/// trigger rule is in force.
fn free_swing_speed_threshold() -> Option<f32> {
    let threshold = crate::dev_params::get(crate::dev_params::MELEE_FREE_SWING_SPEED);
    (threshold > 0.0).then_some(threshold)
}

impl Script for TriggeredMeleeWeapon {
    /// Expire free-swing cooldowns. Nothing to do under the trigger rule.
    fn update(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        time: &crate::time::Time,
    ) -> Effect {
        if !self.free_swing_cooldowns.is_empty() {
            let elapsed = time.elapsed.as_secs_f32();
            self.free_swing_cooldowns.retain(|_, remaining| {
                *remaining -= elapsed;
                *remaining > 0.0
            });
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
        if !is_vr(world) {
            return Effect::NoEffect;
        }

        match msg {
            MessagePayload::TriggerPull => {
                self.attack_active = true;
                self.hit_entities.clear();
                Effect::NoEffect
            }
            MessagePayload::TriggerRelease | MessagePayload::Drop => {
                self.attack_active = false;
                self.hit_entities.clear();
                self.free_swing_cooldowns.clear();
                Effect::NoEffect
            }
            MessagePayload::Collided { with, contact } => {
                if !self.may_damage(entity_id, *with, physics) {
                    return Effect::NoEffect;
                }
                let Some(amount) = authored_contact_damage(world, entity_id, *with) else {
                    return Effect::NoEffect;
                };
                melee_impact(entity_id, *with, world, amount, *contact)
            }
            _ => Effect::NoEffect,
        }
    }
}

impl TriggeredMeleeWeapon {
    /// Whether this contact opens a hit, under whichever rule is in force.
    ///
    /// Both rules bill a victim at most once per swing; they differ in what a
    /// swing *is*. Under the trigger rule it is the window a pull opens, and
    /// the next pull re-arms it. Under the free-swing rule it is the weapon
    /// actually moving at contact, and a short cooldown stands in for the
    /// release edge that no longer exists - otherwise a weapon left leaning on
    /// a creature would bill every frame it stayed there.
    fn may_damage(&mut self, entity_id: EntityId, with: EntityId, physics: &PhysicsWorld) -> bool {
        let Some(threshold) = free_swing_speed_threshold() else {
            return self.attack_active && self.hit_entities.insert(with);
        };
        if self.free_swing_cooldowns.contains_key(&with) {
            return false;
        }
        let speed = physics
            .get_velocity(entity_id)
            .map(|velocity| velocity.magnitude())
            .unwrap_or(0.0);
        if speed < threshold {
            return false;
        }
        self.free_swing_cooldowns
            .insert(with, FREE_SWING_COOLDOWN_SECONDS);
        true
    }
}

fn is_vr(world: &World) -> bool {
    world
        .borrow::<UniqueView<GlobalPresentationMode>>()
        .map(|mode| mode.0 == PresentationMode::Vr)
        .unwrap_or(false)
}

/// Damage the weapon's own authored `Contact` stims deal to this victim,
/// resolved through the victim's receptrons (armor, immunities, Amplify) -
/// the same data path AI melee uses (`ai_util::melee_attack`). The player
/// Wrench (-928) authors WeaponBash at 6/9, so a VR swing now costs what the
/// gamesys says it costs instead of a flat placeholder.
///
/// `None` means "this contact does no authored damage" (a wall, a victim with
/// no receptron for the stim): the caller emits nothing at all, so a swing at
/// scenery is silent rather than a free 1-point tap.
fn authored_contact_damage(world: &World, weapon: EntityId, victim: EntityId) -> Option<f32> {
    let template_id = entity_class_template_id(world, weapon)?;
    let damage = contact_stim_damage(world, template_id, victim);
    (damage > 0.0).then_some(damage)
}

fn melee_impact(
    entity_id: EntityId,
    with: EntityId,
    world: &World,
    amount: f32,
    contact: Option<crate::physics::CollisionContact>,
) -> Effect {
    let damage_effect = Effect::Send {
        msg: Message {
            to: with,
            payload: MessagePayload::Damage {
                amount,
                impact: contact.map(|contact| crate::scripts::DamageImpact {
                    direction: contact.normal,
                    point: contact.point,
                    bone: None,
                }),
            },
        },
    };
    // Impact sound: the weapon's collision schema (weapontype class tag + hit
    // material - wrench on metal clangs, on a creature thuds), unless its
    // collision type opts out.
    let no_sound = world
        .borrow::<View<PropCollisionType>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|c| c.collision_type))
        .is_some_and(|flags| flags.contains(CollisionType::NO_COLLISION_SOUND));
    let sound_effect = if no_sound {
        Effect::NoEffect
    } else {
        let position = get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0));
        play_impact_sound(world, entity_id, with, position.to_vec())
    };
    Effect::Multiple(vec![damage_effect, sound_effect])
}

impl Script for MeleeWeapon {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        // The legacy literal `wrench` script can still back a VR physical
        // weapon, but contact is never a flat damage source. Flat player melee
        // resolves once from WeaponScript at the authored swing event.
        if !is_vr(world) {
            return Effect::NoEffect;
        }

        match msg {
            // Legacy literal `wrench` script (Maintenance Tool -2949).
            MessagePayload::Collided { with, contact } => {
                melee_impact(entity_id, *with, world, 1.0, *contact)
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use dark::properties::{
        Link, Links, PropTemplateId, ReceptronEffect, ReceptronOptions, ToLink,
    };

    use crate::mission::stim_response::GlobalContactStims;

    use super::*;

    /// Stand-ins for the shipped chain the runtime resolves: the player Wrench
    /// (-928) emits WeaponBash on contact, and a vulnerable victim carries a
    /// x1 WeaponBash damage receptron.
    const WRENCH: i32 = -928;
    const WEAPON_BASH: i32 = -3058;
    const WEAPON_BASH_INTENSITY: f32 = 9.0;

    /// A world where the weapon's authored contact stim resolves against the
    /// target's receptron - so a landed swing costs WEAPON_BASH_INTENSITY.
    fn test_world(mode: PresentationMode) -> (World, EntityId, EntityId) {
        test_world_with_victim_receptrons(mode, vec![(WEAPON_BASH, damage_receptron(16, 1.0))])
    }

    fn test_world_with_victim_receptrons(
        mode: PresentationMode,
        receptrons: Vec<(i32, ReceptronOptions)>,
    ) -> (World, EntityId, EntityId) {
        let mut world = World::new();
        world.add_unique(GlobalPresentationMode(mode));
        world.add_unique(GlobalContactStims(HashMap::from([(
            WRENCH,
            vec![(WEAPON_BASH, WEAPON_BASH_INTENSITY)],
        )])));
        let weapon = world.add_entity((
            PropCollisionType {
                collision_type: CollisionType::NO_COLLISION_SOUND,
            },
            PropTemplateId {
                template_id: WRENCH,
            },
        ));
        let target = world.add_entity(Links {
            to_links: receptrons
                .into_iter()
                .map(|(to_template_id, options)| ToLink {
                    to_template_id,
                    to_entity_id: None,
                    link: Link::Receptron(options),
                })
                .collect(),
        });
        (world, weapon, target)
    }

    fn damage_receptron(order: i32, multiplier: f32) -> ReceptronOptions {
        ReceptronOptions {
            order,
            effect: ReceptronEffect::Damage {
                multiplier,
                use_intensity: true,
            },
        }
    }

    fn assert_damage(effect: Effect, target: EntityId) {
        let Effect::Multiple(effects) = effect else {
            panic!("expected melee impact effects, got {effect:?}");
        };
        assert!(effects.iter().any(|effect| {
            matches!(
                effect,
                Effect::Send { msg }
                    if msg.to == target
                        && matches!(
                            msg.payload,
                            MessagePayload::Damage { amount, .. }
                                if (amount - WEAPON_BASH_INTENSITY).abs() < f32::EPSILON
                        )
            )
        }));
    }

    /// The regression behind the damage fix: a landed VR swing must cost the
    /// weapon's *authored* WeaponBash intensity, not a flat placeholder.
    #[test]
    fn a_landed_vr_swing_deals_the_weapons_authored_contact_damage() {
        let (world, weapon, target) = test_world(PresentationMode::Vr);
        let mut script = TriggeredMeleeWeapon::new();
        script.handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TriggerPull,
        );

        let Effect::Multiple(effects) = collide(&mut script, &world, weapon, target) else {
            panic!("expected melee impact effects");
        };
        let amount = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Send { msg } => match msg.payload {
                    MessagePayload::Damage { amount, .. } => Some(amount),
                    _ => None,
                },
                _ => None,
            })
            .expect("a landed swing should send Damage");
        assert!(
            (amount - WEAPON_BASH_INTENSITY).abs() < f32::EPSILON,
            "got {amount}"
        );
    }

    /// The physical VR contact must carry the same directional context as the
    /// flat aim ray so a lethal hit can seed the victim's death ragdoll.
    #[test]
    fn a_landed_vr_swing_carries_an_impact_for_the_death_reaction() {
        let (world, weapon, target) = test_world(PresentationMode::Vr);
        let mut script = TriggeredMeleeWeapon::new();
        script.handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TriggerPull,
        );

        let Effect::Multiple(effects) = collide(&mut script, &world, weapon, target) else {
            panic!("expected melee impact effects");
        };
        let impact = effects.iter().find_map(|effect| match effect {
            Effect::Send { msg } => match msg.payload {
                MessagePayload::Damage { impact, .. } => Some(impact),
                _ => None,
            },
            _ => None,
        });
        assert!(
            matches!(
                impact,
                Some(Some(crate::scripts::DamageImpact {
                    direction,
                    point,
                    bone: None,
                })) if direction == vec3(1.0, 0.0, 0.0)
                    && point == vec3(2.0, 3.0, 4.0)
            ),
            "a VR contact damage message must describe its physical impact"
        );
    }

    /// Campaign save/load retains the concrete MedSci1 object id in
    /// `PropTemplateId` and restores the stable Wrench archetype beside it.
    /// Contact damage must use the latter: positive id 990 has no entry in the
    /// gamesys-wide contact-stim table.
    #[test]
    fn a_restored_concrete_wrench_uses_its_canonical_template_for_damage() {
        let (mut world, weapon, target) = test_world(PresentationMode::Vr);
        world.add_component(
            weapon,
            (
                PropTemplateId { template_id: 990 },
                crate::runtime_props::RuntimePropCanonicalTemplateId(WRENCH),
            ),
        );
        let mut script = TriggeredMeleeWeapon::new();
        script.handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TriggerPull,
        );

        assert_damage(collide(&mut script, &world, weapon, target), target);
    }

    /// Type effectiveness still applies: a target with no WeaponBash receptron
    /// (a robot) takes nothing, and the swing emits no Damage at all.
    #[test]
    fn a_target_with_no_matching_receptron_takes_no_vr_melee_damage() {
        let (world, weapon, target) =
            test_world_with_victim_receptrons(PresentationMode::Vr, Vec::new());
        let mut script = TriggeredMeleeWeapon::new();
        script.handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TriggerPull,
        );

        assert!(matches!(
            collide(&mut script, &world, weapon, target),
            Effect::NoEffect
        ));
    }

    fn collide(
        script: &mut TriggeredMeleeWeapon,
        world: &World,
        weapon: EntityId,
        target: EntityId,
    ) -> Effect {
        script.handle_message(
            weapon,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::Collided {
                with: target,
                contact: Some(crate::physics::CollisionContact {
                    point: vec3(2.0, 3.0, 4.0),
                    normal: vec3(1.0, 0.0, 0.0),
                }),
            },
        )
    }

    #[test]
    fn authored_melee_contact_is_harmless_until_vr_trigger_pull() {
        let (world, weapon, target) = test_world(PresentationMode::Vr);
        let mut script = TriggeredMeleeWeapon::new();

        assert!(matches!(
            collide(&mut script, &world, weapon, target),
            Effect::NoEffect
        ));
        script.handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TriggerPull,
        );
        assert_damage(collide(&mut script, &world, weapon, target), target);
    }

    #[test]
    fn one_vr_trigger_pull_can_damage_each_contact_only_once() {
        let (world, weapon, target) = test_world(PresentationMode::Vr);
        let mut script = TriggeredMeleeWeapon::new();
        script.handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TriggerPull,
        );

        assert_damage(collide(&mut script, &world, weapon, target), target);
        assert!(matches!(
            collide(&mut script, &world, weapon, target),
            Effect::NoEffect
        ));
    }

    #[test]
    fn release_and_drop_close_the_vr_melee_damage_window() {
        for close in [MessagePayload::TriggerRelease, MessagePayload::Drop] {
            let (world, weapon, target) = test_world(PresentationMode::Vr);
            let mut script = TriggeredMeleeWeapon::new();
            script.handle_message(
                weapon,
                &world,
                &PhysicsWorld::new(),
                &MessagePayload::TriggerPull,
            );
            script.handle_message(weapon, &world, &PhysicsWorld::new(), &close);

            assert!(matches!(
                collide(&mut script, &world, weapon, target),
                Effect::NoEffect
            ));
        }
    }

    #[test]
    fn flat_trigger_does_not_arm_physical_melee_damage() {
        let (world, weapon, target) = test_world(PresentationMode::Flat);
        let mut script = TriggeredMeleeWeapon::new();
        script.handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TriggerPull,
        );

        assert!(matches!(
            collide(&mut script, &world, weapon, target),
            Effect::NoEffect
        ));
    }

    #[test]
    fn legacy_melee_collision_is_inert_outside_vr() {
        let (world, weapon, target) = test_world(PresentationMode::Flat);

        let effect = MeleeWeapon::new().handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Collided {
                with: target,
                contact: Some(crate::physics::CollisionContact {
                    point: vec3(2.0, 3.0, 4.0),
                    normal: vec3(1.0, 0.0, 0.0),
                }),
            },
        );

        assert!(matches!(effect, Effect::NoEffect));
    }
}
