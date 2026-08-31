use dark::properties::{PropEcoState, PropEcoType, PropEcology, PropHitPoints, PropTemplateId};
use rand::Rng;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

use crate::{mission::mission_core::GlobalTemplateHierarchy, physics::PhysicsWorld, time::Time};

use super::{
    Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::{entity_class_template_id, send_to_all_switch_links},
};

const PHYSICAL_TEMPLATE_ID: i32 = -11;
const ECOLOGY_STATE_NORMAL: i32 = 0;
/// The alert column of `P$EcoState` - also what the security-alert HUD/audio
/// feedback watches for (`crate::security_alert`).
pub(crate) const ECOLOGY_STATE_ALERT: i32 = 2;
const SCRIPT_STATE_KEY: &str = "shock2vr.trigger_ecology";

#[derive(Serialize, Deserialize)]
struct TriggerEcologyState {
    seconds_until_poll: f32,
    recovery_seconds_remaining: Option<f32>,
}

/// Retail `TriggerEcology`: periodically count live physical objects carrying
/// this trigger's EcoType and pulse its SwitchLinks when population is low.
/// A security `Alarm` switches to the alert-column population profile until
/// the authored recovery window expires or a `Reset` arrives.
pub struct TriggerEcology {
    seconds_until_poll: f32,
    recovery_seconds_remaining: Option<f32>,
}

impl TriggerEcology {
    pub fn new() -> Self {
        Self {
            seconds_until_poll: 0.0,
            recovery_seconds_remaining: None,
        }
    }

    /// Whether this entity is a physical population member (a creature) rather
    /// than one of the ecology's own non-physical markers, which carry the same
    /// EcoType.
    ///
    /// The entity's *concrete* template is what answers that: the canonical
    /// class id alone is the nearest negative ancestor, which for an object
    /// whose metaprops sort ahead of its archetype is a **metaprop** (earth's
    /// "DopeyDroid" and its third training droid both resolve to `Docile`,
    /// which does not descend from `Physical`) - so those creatures were never
    /// counted and their ecology spawned another every period, forever.
    ///
    /// The canonical id is the fallback for entities whose concrete template
    /// this mission's hierarchy does not know - one carried in from another
    /// mission, whose positive template id belongs to that mission's id space
    /// and could alias an unrelated object here.
    fn is_physical(
        world: &World,
        hierarchy: &GlobalTemplateHierarchy,
        templates: Option<&View<PropTemplateId>>,
        entity: EntityId,
    ) -> bool {
        let concrete = templates
            .and_then(|templates| templates.get(entity).ok().map(|id| id.template_id))
            .filter(|template| hierarchy.0.contains_key(template));
        concrete
            .or_else(|| entity_class_template_id(world, entity))
            .is_some_and(|template| hierarchy.is_or_descends_from(template, PHYSICAL_TEMPLATE_ID))
    }

    fn population(world: &World, ecology_type: i32) -> usize {
        let Ok(ecology_types) = world.borrow::<View<PropEcoType>>() else {
            return 0;
        };
        let Ok(hierarchy) = world.borrow::<UniqueView<GlobalTemplateHierarchy>>() else {
            return 0;
        };
        let hit_points = world.borrow::<View<PropHitPoints>>().ok();
        let templates = world.borrow::<View<PropTemplateId>>().ok();
        ecology_types
            .iter()
            .with_id()
            .filter(|(entity, candidate)| {
                candidate.0 == ecology_type
                    && hit_points
                        .as_ref()
                        .and_then(|hit_points| hit_points.get(*entity).ok())
                        .is_none_or(|hit_points| hit_points.hit_points > 0)
                    && Self::is_physical(world, &hierarchy, templates.as_ref(), *entity)
            })
            .count()
    }

    fn should_spawn(population: i32, minimum: i32, maximum: i32, random_hit: bool) -> bool {
        population < maximum && (population < minimum || random_hit)
    }

    fn eco_state(world: &World, entity_id: EntityId) -> i32 {
        world
            .borrow::<View<PropEcoState>>()
            .ok()
            .and_then(|states| states.get(entity_id).ok().map(|state| state.0))
            .unwrap_or(ECOLOGY_STATE_NORMAL)
    }

    fn authored_alert_recovery(world: &World, entity_id: EntityId) -> Option<f32> {
        world
            .borrow::<View<PropEcology>>()
            .ok()
            .and_then(|ecologies| {
                ecologies
                    .get(entity_id)
                    .ok()
                    .map(|ecology| ecology.recovery_seconds[ECOLOGY_STATE_ALERT as usize])
            })
    }

    fn poll(&mut self, entity_id: EntityId, world: &World, state: i32) -> Effect {
        let ecologies = world.borrow::<View<PropEcology>>().unwrap();
        let ecology_types = world.borrow::<View<PropEcoType>>().unwrap();
        let Ok(ecology) = ecologies.get(entity_id) else {
            return Effect::NoEffect;
        };
        let Ok(ecology_type) = ecology_types.get(entity_id) else {
            return Effect::NoEffect;
        };
        // Retail's script switches only on Normal and Alert; every other
        // state (notably Hacked) falls through to "do nothing", so a hacked
        // ecology pauses spawning regardless of its authored hacked column.
        let state_index = match state {
            ECOLOGY_STATE_NORMAL => ECOLOGY_STATE_NORMAL as usize,
            ECOLOGY_STATE_ALERT => ECOLOGY_STATE_ALERT as usize,
            _ => return Effect::NoEffect,
        };
        let minimum = ecology.min_count[state_index];
        let maximum = ecology.max_count[state_index];
        let random_chance = ecology.random_chance[state_index];
        let population = Self::population(world, ecology_type.0) as i32;
        let random_hit = random_chance > 0 && rand::thread_rng().gen_range(0..random_chance) == 0;
        if Self::should_spawn(population, minimum, maximum, random_hit) {
            send_to_all_switch_links(world, entity_id, MessagePayload::TurnOn { from: entity_id })
        } else {
            Effect::NoEffect
        }
    }
}

impl Script for TriggerEcology {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let ecologies = world.borrow::<View<PropEcology>>().unwrap();
        self.seconds_until_poll = ecologies
            .get(entity_id)
            .map(|ecology| ecology.period_seconds.max(0.0))
            .unwrap_or(0.0);
        drop(ecologies);
        // Legacy saves persist the alerted `P$EcoState` but carry no private
        // script payload, so conservatively reconstruct a full authored
        // recovery window. Current saves hydrate the exact remaining timer
        // and skip this fresh initialization path.
        if Self::eco_state(world, entity_id) == ECOLOGY_STATE_ALERT {
            self.recovery_seconds_remaining =
                Self::authored_alert_recovery(world, entity_id).filter(|seconds| *seconds > 0.0);
        }
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let elapsed = time.elapsed.as_secs_f32();
        let period_seconds = world
            .borrow::<View<PropEcology>>()
            .ok()
            .and_then(|ecologies| {
                ecologies
                    .get(entity_id)
                    .ok()
                    .map(|ecology| ecology.period_seconds.max(0.0))
            });
        let Some(period_seconds) = period_seconds else {
            return Effect::NoEffect;
        };

        let mut effects = Vec::new();
        // The state transition below is only emitted as an effect, so track
        // the post-expiry state locally: a poll due on the expiry frame must
        // already use the normal profile.
        let mut state = Self::eco_state(world, entity_id);
        if state == ECOLOGY_STATE_ALERT
            && let Some(remaining) = &mut self.recovery_seconds_remaining
        {
            *remaining -= elapsed;
            if *remaining <= 0.0 {
                self.recovery_seconds_remaining = None;
                state = ECOLOGY_STATE_NORMAL;
                effects.push(Effect::SetEcologyState {
                    entity_id,
                    state: ECOLOGY_STATE_NORMAL,
                });
                effects.push(send_to_all_switch_links(
                    world,
                    entity_id,
                    MessagePayload::Reset { from: entity_id },
                ));
            }
        }

        self.seconds_until_poll -= elapsed;
        if self.seconds_until_poll <= 0.0 {
            self.seconds_until_poll = period_seconds;
            effects.push(self.poll(entity_id, world, state));
        }
        Effect::combine(effects)
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Alarm { .. } => {
                if Self::eco_state(world, entity_id) != ECOLOGY_STATE_NORMAL {
                    // A repeated Alarm must not extend the retail recovery
                    // timer, and hacked ecologies ignore security alarms.
                    return Effect::NoEffect;
                }
                // An authored recovery of zero means this ecology has no
                // alert profile; entering it would expire (and Reset the
                // linked devices) on the very next frame.
                let Some(recovery_seconds) = Self::authored_alert_recovery(world, entity_id)
                    .filter(|seconds| *seconds > 0.0)
                else {
                    return Effect::NoEffect;
                };
                self.recovery_seconds_remaining = Some(recovery_seconds);
                Effect::SetEcologyState {
                    entity_id,
                    state: ECOLOGY_STATE_ALERT,
                }
            }
            MessagePayload::Reset { .. } => {
                if Self::eco_state(world, entity_id) != ECOLOGY_STATE_ALERT {
                    return Effect::NoEffect;
                }
                self.recovery_seconds_remaining = None;
                Effect::combine(vec![
                    Effect::SetEcologyState {
                        entity_id,
                        state: ECOLOGY_STATE_NORMAL,
                    },
                    send_to_all_switch_links(
                        world,
                        entity_id,
                        MessagePayload::Reset { from: entity_id },
                    ),
                ])
            }
            _ => Effect::NoEffect,
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &TriggerEcologyState {
                seconds_until_poll: self.seconds_until_poll,
                recovery_seconds_remaining: self.recovery_seconds_remaining,
            },
            SCRIPT_STATE_KEY,
        )
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: TriggerEcologyState = state.decode(1, SCRIPT_STATE_KEY)?;
        self.seconds_until_poll = restored.seconds_until_poll;
        self.recovery_seconds_remaining = restored.recovery_seconds_remaining;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use dark::properties::{Link, Links, PropTemplateId, ToLink, WrappedEntityId};

    use super::*;
    use crate::runtime_props::RuntimePropCanonicalTemplateId;

    fn ecology_props(
        min_count: [i32; 3],
        max_count: [i32; 3],
        recovery_seconds: [f32; 3],
        random_chance: [i32; 3],
    ) -> PropEcology {
        PropEcology {
            period_seconds: 15.0,
            min_count,
            max_count,
            recovery_seconds,
            random_chance,
        }
    }

    fn step(script: &mut TriggerEcology, ecology: EntityId, world: &World, seconds: u64) -> Effect {
        script.update(
            ecology,
            world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_secs(seconds),
                total: Duration::from_secs(seconds),
            },
        )
    }

    fn sends_to(effect: Effect, target: EntityId) -> bool {
        Effect::flatten(vec![effect])
            .into_iter()
            .any(|effect| matches!(effect, Effect::Send { msg } if msg.to == target))
    }

    fn sets_state(effect: Effect, target: EntityId, expected: i32) -> bool {
        Effect::flatten(vec![effect]).into_iter().any(|effect| {
            matches!(
                effect,
                Effect::SetEcologyState { entity_id, state }
                    if entity_id == target && state == expected
            )
        })
    }

    #[test]
    fn pulses_when_matching_physical_population_is_below_minimum() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::from([(
            -196,
            vec![PHYSICAL_TEMPLATE_ID],
        )])));
        let generator = world.add_entity(());
        let ecology = world.add_entity((
            PropTemplateId { template_id: 292 },
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_NORMAL),
            ecology_props([1, 0, 0], [1, 0, 0], [0.0; 3], [1, 0, 0]),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 293,
                    to_entity_id: Some(WrappedEntityId(generator)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let mut script = TriggerEcology::new();
        script.initialize(ecology, &world);

        assert!(sends_to(step(&mut script, ecology, &world, 15), generator));

        world.add_entity((PropEcoType(2501), RuntimePropCanonicalTemplateId(-196)));
        let effect = step(&mut script, ecology, &world, 15);
        assert!(!sends_to(effect, generator));
    }

    #[test]
    fn counts_a_spawn_whose_canonical_class_is_a_metaprop() {
        // earth's "DopeyDroid" archetype object: its nearest negative ancestor
        // is the `Docile` metaprop (-1073, not under Physical), while its own
        // template (597) does descend from Physical. Miscounting it as
        // non-population makes the ecology respawn one droid every period.
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::from([
            (597, vec![PHYSICAL_TEMPLATE_ID, -1073]),
            (-1073, vec![-4]),
        ])));
        let generator = world.add_entity(());
        let ecology = world.add_entity((
            PropEcoType(41),
            PropEcoState(ECOLOGY_STATE_NORMAL),
            ecology_props([1, 0, 0], [1, 0, 0], [0.0; 3], [0, 0, 0]),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 598,
                    to_entity_id: Some(WrappedEntityId(generator)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        world.add_entity((
            PropEcoType(41),
            PropTemplateId { template_id: 597 },
            RuntimePropCanonicalTemplateId(-1073),
        ));
        let mut script = TriggerEcology::new();
        script.initialize(ecology, &world);

        assert!(!sends_to(step(&mut script, ecology, &world, 15), generator));
    }

    #[test]
    fn hacked_ecology_is_paused_even_with_nonzero_population_targets() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let generator = world.add_entity(());
        let ecology = world.add_entity((
            PropTemplateId { template_id: 292 },
            PropEcoType(2501),
            PropEcoState(1),
            ecology_props([0, 1, 0], [0, 1, 0], [0.0; 3], [0, 0, 0]),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 293,
                    to_entity_id: Some(WrappedEntityId(generator)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let mut script = TriggerEcology::new();
        script.initialize(ecology, &world);

        // Even though the hacked column authors min 1 / max 1 against an
        // empty population, retail's script never reads it: hacked pauses.
        assert!(!sends_to(step(&mut script, ecology, &world, 15), generator));
    }

    #[test]
    fn alarm_selects_alert_profile_and_starts_authored_recovery_once() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let camera = world.add_entity(());
        let ecology = world.add_entity((
            PropTemplateId { template_id: 292 },
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_NORMAL),
            ecology_props([0, 0, 2], [0, 0, 2], [0.0, 0.0, 120.0], [0, 0, 0]),
        ));
        let mut script = TriggerEcology::new();
        script.initialize(ecology, &world);

        let effect = script.handle_message(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Alarm { from: camera },
        );

        assert!(sets_state(effect, ecology, ECOLOGY_STATE_ALERT));
        assert_eq!(script.recovery_seconds_remaining, Some(120.0));

        // Simulate the mission effect applier landing the transition.
        world.add_component(ecology, PropEcoState(ECOLOGY_STATE_ALERT));
        script.recovery_seconds_remaining = Some(17.0);
        let repeated = script.handle_message(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Alarm { from: camera },
        );
        assert!(matches!(repeated, Effect::NoEffect));
        assert_eq!(
            script.recovery_seconds_remaining,
            Some(17.0),
            "repeated Alarm must not extend the retail recovery timer"
        );
    }

    #[test]
    fn alarm_without_authored_recovery_never_enters_alert() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let camera = world.add_entity(());
        let ecology = world.add_entity((
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_NORMAL),
            ecology_props([1, 0, 0], [1, 0, 0], [0.0; 3], [0, 0, 0]),
        ));
        let mut script = TriggerEcology::new();
        script.initialize(ecology, &world);

        let effect = script.handle_message(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Alarm { from: camera },
        );

        assert!(matches!(effect, Effect::NoEffect));
        assert_eq!(script.recovery_seconds_remaining, None);
    }

    #[test]
    fn alarm_without_authored_eco_state_still_emits_the_transition() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let camera = world.add_entity(());
        // Authored like medsci1's ecology 71: no explicit P$EcoState.
        let ecology = world.add_entity((
            PropEcoType(2501),
            ecology_props([0, 0, 2], [0, 0, 2], [0.0, 0.0, 120.0], [0, 0, 0]),
        ));
        let mut script = TriggerEcology::new();
        script.initialize(ecology, &world);

        let effect = script.handle_message(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Alarm { from: camera },
        );

        // The mission's SetEcologyState handler add_components, so the
        // missing authored component is created when the effect lands.
        assert!(sets_state(effect, ecology, ECOLOGY_STATE_ALERT));
    }

    #[test]
    fn recovery_expiry_resets_state_and_linked_security_devices() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let camera = world.add_entity(());
        let ecology = world.add_entity((
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_ALERT),
            ecology_props([0, 0, 2], [0, 0, 2], [0.0, 0.0, 120.0], [0, 0, 0]),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 294,
                    to_entity_id: Some(WrappedEntityId(camera)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let mut script = TriggerEcology::new();
        script.seconds_until_poll = 10.0;
        script.recovery_seconds_remaining = Some(1.0);

        let effects = Effect::flatten(vec![step(&mut script, ecology, &world, 1)]);

        assert!(effects.iter().any(|effect| {
            matches!(
                effect,
                Effect::SetEcologyState { entity_id, state }
                    if *entity_id == ecology && *state == ECOLOGY_STATE_NORMAL
            )
        }));
        assert!(effects.iter().any(|effect| {
            matches!(
                effect,
                Effect::Send { msg }
                    if msg.to == camera
                        && matches!(msg.payload, MessagePayload::Reset { from } if from == ecology)
            )
        }));
    }

    #[test]
    fn recovery_expiry_frame_still_runs_a_due_poll() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let generator = world.add_entity(());
        let ecology = world.add_entity((
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_ALERT),
            // The normal profile wants population; the poll due on the same
            // frame recovery expires must run against it, not be swallowed.
            ecology_props([1, 0, 0], [1, 0, 0], [0.0, 0.0, 120.0], [0, 0, 0]),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 293,
                    to_entity_id: Some(WrappedEntityId(generator)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let mut script = TriggerEcology::new();
        script.seconds_until_poll = 1.0;
        script.recovery_seconds_remaining = Some(1.0);

        let effect = step(&mut script, ecology, &world, 1);

        assert!(sends_to(effect, generator));
    }

    #[test]
    fn alert_profile_pulses_until_its_population_minimum_is_met() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let generator = world.add_entity(());
        let ecology = world.add_entity((
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_ALERT),
            ecology_props([0, 0, 2], [0, 0, 2], [0.0, 0.0, 120.0], [0, 0, 0]),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 293,
                    to_entity_id: Some(WrappedEntityId(generator)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let mut script = TriggerEcology::new();
        script.seconds_until_poll = 15.0;
        script.recovery_seconds_remaining = Some(120.0);

        assert!(sends_to(step(&mut script, ecology, &world, 15), generator));
    }

    #[test]
    fn script_state_round_trips_both_clocks() {
        let mut before_save = TriggerEcology::new();
        before_save.seconds_until_poll = 6.5;
        before_save.recovery_seconds_remaining = Some(91.25);

        let state = before_save.save_state().unwrap();
        let mut after_load = TriggerEcology::new();
        after_load
            .restore_state(&state, &ScriptRestoreContext::new(&HashMap::new()))
            .unwrap();

        assert_eq!(after_load.seconds_until_poll, 6.5);
        assert_eq!(after_load.recovery_seconds_remaining, Some(91.25));
    }

    #[test]
    fn legacy_alerted_save_reconstructs_a_full_recovery_window() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let ecology = world.add_entity((
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_ALERT),
            ecology_props([0, 0, 2], [0, 0, 2], [0.0, 0.0, 120.0], [0, 0, 0]),
        ));
        let mut script = TriggerEcology::new();

        script.initialize(ecology, &world);

        assert_eq!(script.recovery_seconds_remaining, Some(120.0));
    }

    #[test]
    fn random_spawn_never_exceeds_maximum() {
        assert!(TriggerEcology::should_spawn(0, 1, 2, false));
        assert!(TriggerEcology::should_spawn(1, 1, 2, true));
        assert!(!TriggerEcology::should_spawn(1, 1, 2, false));
        assert!(!TriggerEcology::should_spawn(2, 1, 2, true));
    }
}
