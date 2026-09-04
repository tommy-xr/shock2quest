use std::collections::HashMap;

use cgmath::{EuclideanSpace, InnerSpace, Vector3, vec3};
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
/// The rule is physical: a swing damages because the weapon was closing on
/// what it hit fast enough ([`closing_speed`] against
/// [`crate::dev_params::MELEE_FREE_SWING_SPEED`]), not because a button was
/// down. A short per-victim cooldown makes one swing - which crosses a body
/// over several contact frames - bill at most once.
///
/// Physical contact is *never* the damage trigger outside VR - a flat swing is
/// `WeaponScript`'s aimed short-range raycast, so bumping a wielded wrench into
/// scenery must do nothing. The whole script is therefore inert in flat mode
/// rather than guarding individual messages.
///
/// The impact *sound*, unlike the damage, is not gated on the swing threshold
/// or on the weapon being held: a wrench meeting a bulkhead, a bench or the
/// floor makes a noise either way, and a dropped one clatters when it lands.
/// That is intentional - the silence was the complaint.
pub struct HeldMeleeWeapon {
    /// Remaining seconds before each already-hit victim may be hit again -
    /// the dedupe that turns a multi-frame contact into one billed hit.
    free_swing_cooldowns: HashMap<EntityId, f32>,
    /// Remaining seconds before this weapon may thud against the same contact
    /// partner again. Independent of the damage cooldowns above: a wall makes
    /// a noise whether or not the contact is billable.
    sound_cooldowns: HashMap<EntityId, f32>,
}

impl HeldMeleeWeapon {
    pub fn new() -> Self {
        Self {
            free_swing_cooldowns: HashMap::new(),
            sound_cooldowns: HashMap::new(),
        }
    }
}

/// How long one victim is immune to a further free swing from the same weapon.
/// A single controller swing crosses a body over several frames, so without
/// this a swing bills once per contact frame; long enough to cost one hit per
/// swing, short enough not to eat a genuine second swing.
const FREE_SWING_COOLDOWN_SECONDS: f32 = 0.4;

/// How long one contact partner stays silent after this weapon thuds against
/// it. Rapier reports a fresh contact edge every time the solver separates and
/// re-touches, so a weapon chattering against a wall would otherwise
/// machine-gun impact sounds. Shorter than [`FREE_SWING_COOLDOWN_SECONDS`] on
/// purpose: two deliberate taps in quick succession should both be audible
/// even though only the first one is billed.
///
/// Note the key is an *entity*, and a mission's entire static geometry is one
/// collider owning one entity - so this is one thud per 0.15 s for the whole
/// level, plus one per prop. That is the right granularity for an impact
/// sound, and deliberately coarser than "per surface" would be.
const IMPACT_SOUND_COOLDOWN_SECONDS: f32 = 0.15;

/// Approach speed below which a contact makes no sound at all, in world units
/// per second. Only something that arrived *at* the surface clangs: a weapon
/// resting against one, or dragged along one, is silent.
///
/// Measured in `debug_melee` (see its module docs): a held weapon at rest
/// reads ~0.005 and a brisk controller sweep peaks at ~1.6. This sits well
/// clear of rest while staying far below the free-swing *damage* threshold
/// (`MELEE_FREE_SWING_SPEED`), so a light tap that does no damage still clinks.
const IMPACT_SOUND_MIN_SPEED: f32 = 0.1;

/// The swing/graze gate: minimum closing speed for a contact to damage.
fn free_swing_speed_threshold() -> f32 {
    crate::dev_params::get(crate::dev_params::MELEE_FREE_SWING_SPEED)
}

impl Script for HeldMeleeWeapon {
    /// Expire the per-victim cooldowns (free-swing damage, impact sound).
    fn update(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        time: &crate::time::Time,
    ) -> Effect {
        let elapsed = time.elapsed.as_secs_f32();
        for cooldowns in [&mut self.free_swing_cooldowns, &mut self.sound_cooldowns] {
            cooldowns.retain(|_, remaining| {
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
            MessagePayload::Collided { with, contact } => {
                // A creature is struck through its hitboxes: the contact
                // arrives from the limb proxy, and the *creature* is what owns
                // the receptrons, the material and the velocity.
                //
                // The capsule contact of that same swing is dropped so the
                // blow is attributed to the limb rather than to the cylinder
                // around it - whichever of the two the frame dispatched first.
                // (It is not what stops double billing: the cooldown, keyed on
                // the creature, already does that.) Only when a limb contact
                // is actually on offer: a creature with no live proxies - an
                // authored corpse carries `PropCreature` but is never animated
                // - keeps being hit, and heard, on its capsule.
                let owner = crate::util::resolve_proxy_entity(world, *with);
                let is_capsule_contact = owner == *with;
                let is_held = self.is_held(world, entity_id);
                if is_capsule_contact
                    && is_held
                    && crate::creature::has_live_hit_boxes(world, owner)
                {
                    return Effect::NoEffect;
                }

                // Damage and sound are separate questions. Damage stays gated
                // by the swing threshold and the victim's authored receptrons;
                // *hitting something* is audible regardless - a wrench on a
                // bulkhead or a bench does nothing but must still clang.

                // A held weapon rides the player, so the player's own motion
                // is not part of the swing. A loose one on the floor does not
                // ride anything, and nothing may be subtracted from it.
                let player_velocity = if is_held {
                    physics.player_velocity()
                } else {
                    vec3(0.0, 0.0, 0.0)
                };
                let damage = self
                    .may_damage(entity_id, owner, physics, *contact, player_velocity)
                    .then(|| authored_contact_damage(world, entity_id, owner, is_held))
                    .flatten()
                    // Adrenaline Overproduction scales the *player's* swing,
                    // so only a weapon in their hand gets the bonus (a wrench
                    // knocked into a creature is nobody's swing).
                    .map(|amount| {
                        if is_held {
                            amount * crate::scripts::berserk::melee_damage_multiplier(world)
                        } else {
                            amount
                        }
                    });

                let mut effects = Vec::new();
                if let Some(amount) = damage {
                    // Addressed to the hitbox, not the creature: forwarding it
                    // is what stamps the struck joint onto the blow.
                    effects.push(contact_damage_effect(*with, amount, *contact));
                }
                // A blow that lands is always audible, as it always was. The
                // operator is a bitwise `|`, not `||`, so the guard still runs
                // (and arms the cooldown) even when damage forces the sound.
                if self.may_play_impact_sound(entity_id, owner, physics, *contact)
                    | damage.is_some()
                {
                    let sound = impact_sound_effect(entity_id, owner, world);
                    if !matches!(sound, Effect::NoEffect) {
                        effects.push(sound);
                    }
                }

                if effects.is_empty() {
                    Effect::NoEffect
                } else {
                    Effect::Multiple(effects)
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

impl HeldMeleeWeapon {
    /// Whether this weapon is in the player's hand. Only a held weapon reaches
    /// a creature's hitboxes, so only a held weapon has a limb contact to
    /// prefer over its capsule; a wrench lying on the floor keeps clattering
    /// when a creature walks into it.
    fn is_held(&self, world: &World, entity_id: EntityId) -> bool {
        world
            .borrow::<View<crate::runtime_props::RuntimePropVrGripOffset>>()
            .is_ok_and(|grips| grips.get(entity_id).is_ok())
    }

    /// Whether this contact opens a hit: the weapon was actually moving at
    /// contact, and this victim has not just been billed. The short cooldown
    /// is what makes a swing a *swing* - without it a weapon left leaning on
    /// a creature would bill every frame it stayed there.
    fn may_damage(
        &mut self,
        entity_id: EntityId,
        with: EntityId,
        physics: &PhysicsWorld,
        contact: Option<crate::physics::CollisionContact>,
        player_velocity: Vector3<f32>,
    ) -> bool {
        if self.free_swing_cooldowns.contains_key(&with) {
            return false;
        }
        if closing_speed(entity_id, with, physics, contact, player_velocity)
            < free_swing_speed_threshold()
        {
            return false;
        }
        self.free_swing_cooldowns
            .insert(with, FREE_SWING_COOLDOWN_SECONDS);
        true
    }

    /// Whether this contact should be heard. Contacts repeat while a weapon
    /// rests on or scrapes along a surface, and the damage path's own dedupe
    /// does not cover the (much more common) contacts that deal no damage - so
    /// the sound carries its own guard: the weapon must have been closing on
    /// what it hit, and that partner then stays quiet for a moment.
    ///
    /// The speed is taken *along the contact normal*, not as a raw magnitude.
    /// A held weapon carries the player's whole locomotion velocity, so a
    /// wrench brushing a corridor wall while walking reads fast by magnitude
    /// while barely closing on the wall at all - and would otherwise clang
    /// every 0.15 s for the length of the corridor. `abs` because the normal's
    /// orientation depends on which collider Rapier listed first.
    ///
    /// The damage rule above projects the same way but measures a different
    /// quantity - it also divides out the victim's motion and the player's own
    /// locomotion, and reads the hand rather than the weapon body. The two
    /// stay separate because they answer different questions at different
    /// thresholds: audible is a much lower bar than damaging, a contact too
    /// gentle to bill should still clink, and a wrench carried head-on into a
    /// bulkhead should clang even though walking is not a swing.
    /// Whether this contact thuds. Same speed question as [`Self::may_damage`],
    /// a lower bar - and the same reason for reading the sweep's own
    /// measurement: a swing stopped on a limb has no live velocity left to
    /// read, so without it exactly the blows that now land are the ones that
    /// would fall silent.
    fn may_play_impact_sound(
        &mut self,
        entity_id: EntityId,
        with: EntityId,
        physics: &PhysicsWorld,
        contact: Option<crate::physics::CollisionContact>,
    ) -> bool {
        if self.sound_cooldowns.contains_key(&with) {
            return false;
        }
        let velocity = physics
            .get_velocity(entity_id)
            .unwrap_or_else(|| vec3(0.0, 0.0, 0.0));
        let speed = match contact {
            Some(contact) => velocity.dot(contact.normal).abs(),
            None => velocity.magnitude(),
        };
        if speed < IMPACT_SOUND_MIN_SPEED {
            return false;
        }
        self.sound_cooldowns
            .insert(with, IMPACT_SOUND_COOLDOWN_SECONDS);
        true
    }
}

fn is_vr(world: &World) -> bool {
    world
        .borrow::<UniqueView<GlobalPresentationMode>>()
        .map(|mode| mode.0 == PresentationMode::Vr)
        .unwrap_or(false)
}

/// How fast the two bodies were closing on each other, at the point where they
/// touched, along the surface they touched on.
///
/// Four things this is not, each of which was wrong in a way that showed:
///
/// - **Not the weapon's centre-of-mass velocity.** A weapon swung about the
///   wrist moves its *head* fast while its centre barely moves, so a wrist
///   flick under-read badly. `velocity_at_point` carries the `omega x r` term.
/// - **Not the weapon's world velocity.** A held weapon rides the player, so
///   walking into a creature read as a full-speed swing and billed a free hit.
/// - **Not the weapon body's own velocity.** It lags the hand and then catches
///   up in one step, which is speed the player never produced; the driven hand
///   target is what the player actually did.
/// - **Not a raw magnitude.** Sliding a weapon *along* a surface is fast but
///   closes on nothing. Subtracting the victim's motion also makes a creature
///   that charges onto a held blade impale itself.
///
/// The exception is a contact that already knows: see `closing_speed` on
/// [`crate::physics::CollisionContact`]. The arithmetic itself lives in
/// [`crate::physics::relative_swing_speed`].
fn closing_speed(
    weapon: EntityId,
    victim: EntityId,
    physics: &PhysicsWorld,
    contact: Option<crate::physics::CollisionContact>,
    player_velocity: Vector3<f32>,
) -> f32 {
    // A blow the swing sweep found carries the speed it measured. It has to:
    // the sweep stops the weapon on the limb, so by the time this runs the
    // weapon is standing still and every live-velocity reading is zero.
    if let Some(speed) = contact.and_then(|contact| contact.closing_speed) {
        return speed;
    }
    let Some(contact) = contact else {
        let weapon_velocity = physics
            .get_velocity(weapon)
            .unwrap_or_else(|| vec3(0.0, 0.0, 0.0));
        let victim_velocity = physics
            .get_velocity(victim)
            .unwrap_or_else(|| vec3(0.0, 0.0, 0.0));
        return crate::physics::relative_swing_speed(
            weapon_velocity,
            player_velocity,
            victim_velocity,
            None,
        );
    };
    let at = |entity| {
        physics
            .velocity_at_point(entity, contact.point)
            .unwrap_or_else(|| vec3(0.0, 0.0, 0.0))
    };
    let speed_of = |weapon_velocity| {
        crate::physics::relative_swing_speed(
            weapon_velocity,
            player_velocity,
            at(victim),
            Some(contact.normal),
        )
    };
    // A held weapon is billed on the SLOWER of two readings - what the hand
    // did, and what the weapon did - because each alone is wrong in one
    // direction. The weapon body reports its own catch-up after an obstruction
    // lets go (measured at 34 u/s against a hand doing 10); the hand keeps
    // reading a swing while the weapon is pinned against the body it already
    // struck, which bills leaning on a creature as a second blow. A loose
    // weapon has no hand and answers for itself.
    match physics.held_melee_target_velocity_at_point(weapon, contact.point) {
        Some(hand) => speed_of(hand).min(speed_of(at(weapon))),
        None => speed_of(at(weapon)),
    }
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
///
/// Lethal Weapon scales a *held* weapon's blow - that is the player's own
/// swing. A loose weapon something else blunders into is not.
fn authored_contact_damage(
    world: &World,
    weapon: EntityId,
    victim: EntityId,
    is_held: bool,
) -> Option<f32> {
    let template_id = entity_class_template_id(world, weapon)?;
    let damage = contact_stim_damage(world, template_id, victim);
    let damage = crate::scripts::gui::lethal_weapon_damage(
        damage,
        is_held
            && crate::scripts::gui::player_has_os_trait(
                world,
                crate::scripts::gui::TRAIT_LETHAL_WEAPON,
            ),
    );
    (damage > 0.0).then_some(damage)
}

fn contact_damage_effect(
    with: EntityId,
    amount: f32,
    contact: Option<crate::physics::CollisionContact>,
) -> Effect {
    Effect::Send {
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
    }
}

/// Impact sound for a contact: the weapon's collision schema (weapontype class
/// tag + hit material - wrench on metal clangs, on a creature thuds), unless
/// its collision type opts out. Needs no damage value: unmaterialed surfaces
/// (world geometry, a bench) fall back to the default material tag.
fn impact_sound_effect(entity_id: EntityId, with: EntityId, world: &World) -> Effect {
    let no_sound = world
        .borrow::<View<PropCollisionType>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|c| c.collision_type))
        .is_some_and(|flags| flags.contains(CollisionType::NO_COLLISION_SOUND));
    if no_sound {
        return Effect::NoEffect;
    }
    let position = get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0));
    play_impact_sound(world, entity_id, with, position.to_vec())
}

fn melee_impact(
    entity_id: EntityId,
    with: EntityId,
    world: &World,
    amount: f32,
    contact: Option<crate::physics::CollisionContact>,
) -> Effect {
    Effect::Multiple(vec![
        contact_damage_effect(with, amount, contact),
        impact_sound_effect(entity_id, with, world),
    ])
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
        Link, Links, PropClassTag, PropTemplateId, ReceptronEffect, ReceptronOptions, ToLink,
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
    /// A body at `velocity`, so `closing_speed` can be exercised against real
    /// Rapier bodies rather than a stub - the `omega x r` term is the point,
    /// and a stub would not have one.
    fn moving_body(
        physics: &mut PhysicsWorld,
        entity: EntityId,
        position: cgmath::Vector3<f32>,
        velocity: cgmath::Vector3<f32>,
    ) {
        physics.add_dynamic(
            entity,
            position,
            cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            velocity,
            crate::physics::PhysicsShape::Cuboid(vec3(0.1, 0.1, 0.1)),
            crate::physics::CollisionGroup::entity(),
            false,
            crate::physics::DynamicPhysicsOptions::default(),
        );
        physics.set_velocity(entity, velocity);
    }

    /// The player standing still: whatever the weapon is doing is the swing.
    const STILL_PLAYER: cgmath::Vector3<f32> = cgmath::Vector3::new(0.0, 0.0, 0.0);

    fn head_on_contact(point: cgmath::Vector3<f32>) -> Option<crate::physics::CollisionContact> {
        Some(crate::physics::CollisionContact {
            point,
            normal: vec3(1.0, 0.0, 0.0),
            closing_speed: None,
        })
    }

    /// A blow found by the swing sweep carries the speed the sweep measured.
    /// It has to: the sweep stops the weapon on the limb, so by the time the
    /// blow is read every live velocity is zero and the swing gate would
    /// reject its own hit.
    ///
    /// Negative-first: without the carried speed this reads 0.
    #[test]
    fn a_swept_blow_uses_the_speed_the_sweep_measured() {
        let mut physics = PhysicsWorld::new();
        let weapon = EntityId::from_inner(1).unwrap();
        let victim = EntityId::from_inner(2).unwrap();
        let swept = Some(crate::physics::CollisionContact {
            point: vec3(0.0, 0.0, 0.0),
            normal: vec3(1.0, 0.0, 0.0),
            closing_speed: Some(7.5),
        });

        assert_eq!(
            closing_speed(weapon, victim, &physics, swept, STILL_PLAYER),
            7.5
        );
        // Neither body exists in this world, so an ordinary contact - which
        // reads the live bodies - has nothing to report.
        let _ = &mut physics;
        assert_eq!(
            closing_speed(
                weapon,
                victim,
                &physics,
                head_on_contact(vec3(0.0, 0.0, 0.0)),
                STILL_PLAYER
            ),
            0.0
        );
    }

    /// The bug this rule exists to fix: a held weapon rides the player, so
    /// walking into something billed a free hit on anything it brushed.
    ///
    /// Note what this does and does not prove. It guards the *threshold*, not
    /// the measure - walking reads ~1.8 either way, so it passes against the
    /// old centre-of-mass magnitude too. What it catches is the shipped gate
    /// dropping back under walking pace, which is what the original 0.5 did.
    /// It reads the real constant so it cannot drift from what ships; the
    /// measure itself is covered by the two tests below, which do fail
    /// against the old one.
    #[test]
    fn carrying_a_weapon_at_walking_pace_is_not_a_swing() {
        let gate = crate::dev_params::spec(crate::dev_params::MELEE_FREE_SWING_SPEED).default;
        let mut physics = PhysicsWorld::new();
        let weapon = EntityId::from_inner(1).unwrap();
        let victim = EntityId::from_inner(2).unwrap();
        // 1.8 u/s is the walking figure measured in held_melee_drive.
        moving_body(
            &mut physics,
            weapon,
            vec3(0.0, 0.0, 0.0),
            vec3(1.8, 0.0, 0.0),
        );
        moving_body(
            &mut physics,
            victim,
            vec3(1.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
        );

        let speed = closing_speed(
            weapon,
            victim,
            &physics,
            head_on_contact(vec3(0.5, 0.0, 0.0)),
            STILL_PLAYER,
        );

        assert!(
            speed < gate,
            "walking pace must fall below the swing gate {gate}, read {speed}"
        );
    }

    /// Walking a held weapon into something is not a swing. The weapon rides
    /// the player, so its world velocity is the player's - measured at 10
    /// units/s, five times the gate.
    ///
    /// Negative-first: without the player's motion divided out this reads the
    /// full 10.
    #[test]
    fn walking_a_weapon_into_a_still_victim_is_not_a_swing() {
        let gate = crate::dev_params::spec(crate::dev_params::MELEE_FREE_SWING_SPEED).default;
        let mut physics = PhysicsWorld::new();
        let weapon = EntityId::from_inner(1).unwrap();
        let victim = EntityId::from_inner(2).unwrap();
        let walking = vec3(10.0, 0.0, 0.0);
        moving_body(&mut physics, weapon, vec3(0.0, 0.0, 0.0), walking);
        moving_body(
            &mut physics,
            victim,
            vec3(1.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
        );

        let speed = closing_speed(
            weapon,
            victim,
            &physics,
            head_on_contact(vec3(0.5, 0.0, 0.0)),
            walking,
        );

        assert!(
            speed < gate,
            "a carried weapon must fall below the swing gate {gate}, read {speed}"
        );
    }

    /// The other half: a real swing thrown while walking still bills, because
    /// only the player's share comes out.
    #[test]
    fn swinging_while_walking_is_still_a_swing() {
        let gate = crate::dev_params::spec(crate::dev_params::MELEE_FREE_SWING_SPEED).default;
        let mut physics = PhysicsWorld::new();
        let weapon = EntityId::from_inner(1).unwrap();
        let victim = EntityId::from_inner(2).unwrap();
        let walking = vec3(10.0, 0.0, 0.0);
        moving_body(
            &mut physics,
            weapon,
            vec3(0.0, 0.0, 0.0),
            walking + vec3(4.0, 0.0, 0.0),
        );
        moving_body(
            &mut physics,
            victim,
            vec3(1.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
        );

        let speed = closing_speed(
            weapon,
            victim,
            &physics,
            head_on_contact(vec3(0.5, 0.0, 0.0)),
            walking,
        );

        assert!(
            speed > gate,
            "a swing thrown while walking must clear the gate {gate}, read {speed}"
        );
    }

    /// The same rule read the other way: a creature charging onto a held blade
    /// impales itself. The weapon is still, the victim is not, and the closing
    /// speed is real - which weapon-velocity-alone could never see.
    #[test]
    fn a_victim_charging_a_still_weapon_is_a_real_impact() {
        let mut physics = PhysicsWorld::new();
        let weapon = EntityId::from_inner(1).unwrap();
        let victim = EntityId::from_inner(2).unwrap();
        moving_body(
            &mut physics,
            weapon,
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
        );
        moving_body(
            &mut physics,
            victim,
            vec3(1.0, 0.0, 0.0),
            vec3(-4.0, 0.0, 0.0),
        );

        let speed = closing_speed(
            weapon,
            victim,
            &physics,
            head_on_contact(vec3(0.5, 0.0, 0.0)),
            STILL_PLAYER,
        );

        assert!(
            speed > 2.5,
            "a charging victim must register as an impact, read {speed}"
        );
    }

    /// Backing away from a creature that chases at the same speed: the weapon
    /// and the victim keep station, so nothing is closing and nothing may be
    /// billed - however fast both are travelling through the world.
    ///
    /// Negative-first: subtracting the player's motion from a world-frame
    /// victim reads the full chase speed here.
    #[test]
    fn retreating_from_a_creature_that_keeps_pace_is_not_a_blow() {
        let gate = crate::dev_params::spec(crate::dev_params::MELEE_FREE_SWING_SPEED).default;
        let mut physics = PhysicsWorld::new();
        let weapon = EntityId::from_inner(1).unwrap();
        let victim = EntityId::from_inner(2).unwrap();
        let retreat = vec3(-10.0, 0.0, 0.0);
        moving_body(&mut physics, weapon, vec3(0.0, 0.0, 0.0), retreat);
        moving_body(&mut physics, victim, vec3(1.0, 0.0, 0.0), retreat);

        let speed = closing_speed(
            weapon,
            victim,
            &physics,
            head_on_contact(vec3(0.5, 0.0, 0.0)),
            retreat,
        );

        assert!(
            speed < gate,
            "two bodies keeping station must fall below the swing gate {gate}, read {speed}"
        );
    }

    /// Sliding a weapon *along* a surface is fast but closes on nothing, so it
    /// must not bill - the reason the speed is projected onto the contact
    /// normal instead of taken as a magnitude.
    #[test]
    fn dragging_a_weapon_along_a_surface_does_not_bill() {
        let mut physics = PhysicsWorld::new();
        let weapon = EntityId::from_inner(1).unwrap();
        let victim = EntityId::from_inner(2).unwrap();
        // Moving hard along +Z, while the contact normal points along +X.
        moving_body(
            &mut physics,
            weapon,
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 9.0),
        );
        moving_body(
            &mut physics,
            victim,
            vec3(1.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
        );

        let speed = closing_speed(
            weapon,
            victim,
            &physics,
            head_on_contact(vec3(0.5, 0.0, 0.0)),
            STILL_PLAYER,
        );

        assert!(
            speed < 2.5,
            "motion across a surface closes on nothing, read {speed}"
        );
    }

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
        let mut script = HeldMeleeWeapon::new();

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

    /// Lethal Weapon scales the player's own swing - a held weapon - and
    /// leaves a loose one alone.
    #[test]
    fn lethal_weapon_scales_a_held_swing_only() {
        let landed_damage = |held: bool, traited: bool| {
            let (mut world, weapon, target) = test_world(PresentationMode::Vr);
            if held {
                world.add_component(
                    weapon,
                    crate::runtime_props::RuntimePropVrGripOffset(vec3(0.0, 0.0, 0.0)),
                );
            }
            let mut quests = crate::quest_info::QuestInfo::new();
            if traited {
                quests
                    .player_stats_mut()
                    .add_os_trait(crate::scripts::gui::TRAIT_LETHAL_WEAPON);
            }
            world.add_unique(quests);

            let mut script = HeldMeleeWeapon::new();
            let Effect::Multiple(effects) = collide(&mut script, &world, weapon, target) else {
                panic!("expected melee impact effects");
            };
            effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::Send { msg } => match msg.payload {
                        MessagePayload::Damage { amount, .. } => Some(amount),
                        _ => None,
                    },
                    _ => None,
                })
                .expect("a landed swing should send Damage")
        };

        assert_eq!(landed_damage(true, false), WEAPON_BASH_INTENSITY);
        assert!(
            (landed_damage(true, true) - WEAPON_BASH_INTENSITY * 1.35).abs() < 1e-4,
            "a held swing bills 1.35x with the trait"
        );
        assert_eq!(
            landed_damage(false, true),
            WEAPON_BASH_INTENSITY,
            "a loose weapon is not the player's swing"
        );
    }

    /// The physical VR contact must carry the same directional context as the
    /// flat aim ray so a lethal hit can seed the victim's death ragdoll.
    #[test]
    fn a_landed_vr_swing_carries_an_impact_for_the_death_reaction() {
        let (world, weapon, target) = test_world(PresentationMode::Vr);
        let mut script = HeldMeleeWeapon::new();

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
        let mut script = HeldMeleeWeapon::new();

        assert_damage(collide(&mut script, &world, weapon, target), target);
    }

    /// Type effectiveness still applies: a target with no WeaponBash receptron
    /// (a robot) takes nothing, and the swing emits no Damage at all.
    #[test]
    fn a_target_with_no_matching_receptron_takes_no_vr_melee_damage() {
        let (world, weapon, target) =
            test_world_with_victim_receptrons(PresentationMode::Vr, Vec::new());
        let mut script = HeldMeleeWeapon::new();

        assert!(matches!(
            collide(&mut script, &world, weapon, target),
            Effect::NoEffect
        ));
    }

    /// A contact from a weapon moving comfortably above the shipped free-swing
    /// gate, along the contact normal - the canonical billable swing.
    fn collide(
        script: &mut HeldMeleeWeapon,
        world: &World,
        weapon: EntityId,
        target: EntityId,
    ) -> Effect {
        collide_at_speed(script, world, weapon, target, &fast_swing(weapon))
    }

    /// A physics world whose weapon closes faster than the shipped gate.
    fn fast_swing(weapon: EntityId) -> PhysicsWorld {
        let gate = crate::dev_params::spec(crate::dev_params::MELEE_FREE_SWING_SPEED).default;
        physics_with_weapon_speed(weapon, gate + 1.0)
    }

    fn collide_at_speed(
        script: &mut HeldMeleeWeapon,
        world: &World,
        weapon: EntityId,
        target: EntityId,
        physics: &PhysicsWorld,
    ) -> Effect {
        script.handle_message(
            weapon,
            world,
            physics,
            &MessagePayload::Collided {
                with: target,
                contact: Some(crate::physics::CollisionContact {
                    point: vec3(2.0, 3.0, 4.0),
                    normal: vec3(1.0, 0.0, 0.0),
                    closing_speed: None,
                }),
            },
        )
    }

    /// A weapon that is audible on contact: `test_world`'s weapon deliberately
    /// opts out of collision sound, and the schema lookup is keyed on the
    /// weapon's class tag.
    fn audible_weapon(world: &mut World, weapon: EntityId) {
        world.add_component(
            weapon,
            (
                PropCollisionType {
                    collision_type: CollisionType::empty(),
                },
                PropClassTag::from_string("WeaponType Sword"),
            ),
        );
    }

    /// A physics world in which `weapon` is a body moving at `speed`, so the
    /// impact-sound speed floor can be exercised without a running game.
    fn physics_with_weapon_speed(weapon: EntityId, speed: f32) -> PhysicsWorld {
        let mut physics = PhysicsWorld::new();
        physics.add_dynamic(
            weapon,
            vec3(0.0, 0.0, 0.0),
            cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            crate::physics::PhysicsShape::Cuboid(vec3(1.0, 1.0, 1.0)),
            crate::physics::CollisionGroup::entity(),
            false,
            crate::physics::DynamicPhysicsOptions::default(),
        );
        physics.set_velocity(weapon, vec3(speed, 0.0, 0.0));
        physics
    }

    fn swinging(weapon: EntityId) -> PhysicsWorld {
        physics_with_weapon_speed(weapon, 1.0)
    }

    fn count_effects(effect: &Effect, matching: impl Fn(&Effect) -> bool) -> usize {
        let Effect::Multiple(effects) = effect else {
            return 0;
        };
        effects.iter().filter(|effect| matching(effect)).count()
    }

    fn sound_count(effect: &Effect) -> usize {
        count_effects(effect, |effect| {
            matches!(effect, Effect::PlayEnvironmentalSound { .. })
        })
    }

    fn damage_count(effect: &Effect) -> usize {
        count_effects(
            effect,
            |effect| matches!(effect, Effect::Send { msg } if matches!(msg.payload, MessagePayload::Damage { .. })),
        )
    }

    /// The report: hitting a wall, a bench, or anything else that takes no
    /// authored contact damage was completely silent, because the sound was
    /// emitted only from the damage path.
    #[test]
    fn a_contact_that_deals_no_damage_still_makes_an_impact_sound() {
        let (mut world, weapon, target) =
            test_world_with_victim_receptrons(PresentationMode::Vr, Vec::new());
        audible_weapon(&mut world, weapon);
        let physics = swinging(weapon);
        let mut script = HeldMeleeWeapon::new();

        let effect = collide_at_speed(&mut script, &world, weapon, target, &physics);
        assert_eq!(sound_count(&effect), 1, "got {effect:?}");
        assert_eq!(damage_count(&effect), 0, "got {effect:?}");
    }

    /// ...and a contact that *does* damage still does both.
    #[test]
    fn a_damaging_contact_makes_both_damage_and_an_impact_sound() {
        let (mut world, weapon, target) = test_world(PresentationMode::Vr);
        audible_weapon(&mut world, weapon);
        let physics = fast_swing(weapon);
        let mut script = HeldMeleeWeapon::new();

        let effect = collide_at_speed(&mut script, &world, weapon, target, &physics);
        assert_eq!(sound_count(&effect), 1, "got {effect:?}");
        assert_eq!(damage_count(&effect), 1, "got {effect:?}");
    }

    /// A weapon resting on (or dragged along) a surface re-contacts every
    /// frame. Without a guard of its own the sound would machine-gun, so the
    /// same surface stays quiet until the cooldown expires.
    #[test]
    fn repeated_contact_with_one_surface_makes_only_one_impact_sound() {
        let (mut world, weapon, target) =
            test_world_with_victim_receptrons(PresentationMode::Vr, Vec::new());
        audible_weapon(&mut world, weapon);
        let physics = swinging(weapon);
        let mut script = HeldMeleeWeapon::new();

        let first = collide_at_speed(&mut script, &world, weapon, target, &physics);
        assert_eq!(sound_count(&first), 1, "got {first:?}");
        for _ in 0..5 {
            let repeat = collide_at_speed(&mut script, &world, weapon, target, &physics);
            assert_eq!(sound_count(&repeat), 0, "got {repeat:?}");
        }

        // ...and a later swing at the same surface is audible again.
        script.update(
            weapon,
            &world,
            &physics,
            &crate::time::Time {
                elapsed: std::time::Duration::from_millis(200),
                total: std::time::Duration::from_millis(200),
            },
        );
        let later = collide_at_speed(&mut script, &world, weapon, target, &physics);
        assert_eq!(sound_count(&later), 1, "got {later:?}");
    }

    /// A graze below the swing gate bills nothing but is still audible: the
    /// sound floor (0.1) sits far below the damage gate on purpose.
    #[test]
    fn a_graze_below_the_swing_threshold_is_audible_but_harmless() {
        let (mut world, weapon, target) = test_world(PresentationMode::Vr);
        audible_weapon(&mut world, weapon);
        let physics = physics_with_weapon_speed(weapon, 1.0);
        let mut script = HeldMeleeWeapon::new();

        let effect = collide_at_speed(&mut script, &world, weapon, target, &physics);
        assert_eq!(damage_count(&effect), 0, "got {effect:?}");
        assert_eq!(sound_count(&effect), 1, "got {effect:?}");
    }

    /// A held weapon carries the player's locomotion velocity, so speed is
    /// measured along the contact normal: sliding a wrench *along* a wall
    /// while walking closes on nothing and must stay silent.
    #[test]
    fn a_weapon_moving_along_a_surface_makes_no_impact_sound() {
        let (mut world, weapon, target) =
            test_world_with_victim_receptrons(PresentationMode::Vr, Vec::new());
        audible_weapon(&mut world, weapon);
        // The collide helper's contact normal is +X; move fast, but across it.
        let mut physics = PhysicsWorld::new();
        physics.add_dynamic(
            weapon,
            vec3(0.0, 0.0, 0.0),
            cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            crate::physics::PhysicsShape::Cuboid(vec3(1.0, 1.0, 1.0)),
            crate::physics::CollisionGroup::entity(),
            false,
            crate::physics::DynamicPhysicsOptions::default(),
        );
        physics.set_velocity(weapon, vec3(0.0, 0.0, 5.0));
        let mut script = HeldMeleeWeapon::new();

        assert!(matches!(
            collide_at_speed(&mut script, &world, weapon, target, &physics),
            Effect::NoEffect
        ));
    }

    /// A weapon left leaning against a surface is silent: the contact repeats
    /// but nothing is moving.
    #[test]
    fn a_resting_weapon_makes_no_impact_sound() {
        let (mut world, weapon, target) =
            test_world_with_victim_receptrons(PresentationMode::Vr, Vec::new());
        audible_weapon(&mut world, weapon);
        let physics = physics_with_weapon_speed(weapon, 0.005);
        let mut script = HeldMeleeWeapon::new();

        assert!(matches!(
            collide_at_speed(&mut script, &world, weapon, target, &physics),
            Effect::NoEffect
        ));
    }

    /// One physical swing crosses a body over several contact frames; the
    /// per-victim cooldown makes it bill once, and a later swing bills again.
    #[test]
    fn one_swing_bills_each_victim_only_once() {
        let (world, weapon, target) = test_world(PresentationMode::Vr);
        let physics = fast_swing(weapon);
        let mut script = HeldMeleeWeapon::new();

        assert_damage(
            collide_at_speed(&mut script, &world, weapon, target, &physics),
            target,
        );
        assert!(matches!(
            collide_at_speed(&mut script, &world, weapon, target, &physics),
            Effect::NoEffect
        ));

        // ...and once the cooldown expires, a second swing is a second hit.
        script.update(
            weapon,
            &world,
            &physics,
            &crate::time::Time {
                elapsed: std::time::Duration::from_millis(500),
                total: std::time::Duration::from_millis(500),
            },
        );
        assert_damage(
            collide_at_speed(&mut script, &world, weapon, target, &physics),
            target,
        );
    }

    /// Flat melee is `WeaponScript`'s aimed raycast; a physically bumped
    /// wielded weapon must do nothing outside VR however fast it moves.
    #[test]
    fn physical_melee_contact_is_inert_outside_vr() {
        let (world, weapon, target) = test_world(PresentationMode::Flat);
        let mut script = HeldMeleeWeapon::new();

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
                    closing_speed: None,
                }),
            },
        );

        assert!(matches!(effect, Effect::NoEffect));
    }
}
