use std::{cell::RefCell, time::Duration};

use cgmath::{Deg, InnerSpace, vec3};
use dark::{
    SCALE_FACTOR,
    motion::MotionQueryItem,
    properties::{AIScriptedAction, AIScriptedActionType, PropLocalPlayer, PropPosition},
};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect, Message,
        ai::{
            ai_util,
            steering::{
                self, ChaseEntitySteeringStrategy, CollisionAvoidanceSteeringStrategy, Steering,
                SteeringOutput, SteeringStrategy,
            },
        },
        script_util,
    },
    time::Time,
};

use super::Behavior;

/// Watchdog: running sequences are protected from alertness/signal/damage
/// preemption, so a stalled action (a Goto with an unreachable target, an
/// animation whose queue was silently swallowed) must not wedge the AI
/// forever. Generous enough for the longest authored beats (the rec1 cortez
/// performance clip runs ~14s).
const ACTION_TIMEOUT_SECONDS: f32 = 30.0;

pub struct ScriptedSequenceBehavior {
    /// The AI entity performing this sequence.
    owner: EntityId,
    /// The signal that started this sequence (None for watch-obj/TurnOn
    /// sequences). A re-fire of the SAME signal is ignored while running;
    /// a different signal's response may replace the sequence (this is how
    /// a watch-obj's Signal action hands off to the real signal response).
    origin_signal: Option<String>,
    actions: Vec<AIScriptedAction>,
    queued_effects: Vec<Effect>,
    current_action_idx: i32,
    current_scripted_action: Box<RefCell<dyn ScriptedAction>>,
    finished: bool,
    /// Time spent on the current action; drives the watchdog.
    action_elapsed: f32,
    /// Set by the watchdog: the current action is force-completed.
    timed_out: bool,
}

impl ScriptedSequenceBehavior {
    pub fn new(
        world: &World,
        owner: EntityId,
        origin_signal: Option<String>,
        actions: Vec<AIScriptedAction>,
    ) -> ScriptedSequenceBehavior {
        let current_behavior = get_behavior_from_action(world, owner, &actions[0]);
        let initial_effect = current_behavior.borrow().initial_effect();

        ScriptedSequenceBehavior {
            owner,
            origin_signal,
            actions,
            queued_effects: vec![initial_effect],
            current_action_idx: 0,
            current_scripted_action: current_behavior,
            finished: false,
            action_elapsed: 0.0,
            timed_out: false,
        }
    }
}

impl Behavior for ScriptedSequenceBehavior {
    fn name(&self) -> &'static str {
        "ScriptedSequence"
    }

    fn origin_signal(&self) -> Option<&str> {
        self.origin_signal.as_deref()
    }

    fn scripted_state(&self) -> super::ScriptedState {
        if self.finished {
            super::ScriptedState::Finished
        } else {
            super::ScriptedState::Running
        }
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &crate::scripts::MessagePayload,
    ) -> Effect {
        if matches!(msg, crate::scripts::MessagePayload::AnimationCompleted) {
            self.current_scripted_action
                .borrow_mut()
                .on_animation_completed();
        }
        Effect::NoEffect
    }

    fn animation(&self) -> Vec<MotionQueryItem> {
        self.current_scripted_action.borrow().animation()
    }

    fn turn_speed(&self) -> Deg<f32> {
        self.current_scripted_action.borrow().turn_speed()
    }

    fn is_locomotion(&self) -> bool {
        self.current_scripted_action.borrow().is_locomotion()
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        // Watchdog: force-complete a stalled action and nudge the sequence
        // forward through the normal completion path (advancement is driven
        // by AnimationCompleted, which a stalled action may never produce).
        self.action_elapsed += time.elapsed.as_secs_f32();
        if !self.timed_out && !self.finished && self.action_elapsed > ACTION_TIMEOUT_SECONDS {
            self.timed_out = true;
            tracing::warn!(
                "scripted sequence action {} timed out after {ACTION_TIMEOUT_SECONDS}s; advancing",
                self.current_action_idx
            );
            self.queued_effects.push(Effect::Send {
                msg: crate::scripts::Message {
                    to: entity_id,
                    payload: crate::scripts::MessagePayload::AnimationCompleted,
                },
            });
        }

        let queued_effects = Effect::combine(self.queued_effects.clone());
        self.queued_effects = vec![];

        let maybe_output = self.current_scripted_action.borrow_mut().update(
            current_heading,
            world,
            physics,
            entity_id,
            time,
        );

        if let Some((steering_output, eff)) = maybe_output {
            Some((steering_output, Effect::combine(vec![queued_effects, eff])))
        } else {
            Some((Steering::from_current(current_heading), queued_effects))
        }
    }

    fn next_behavior(
        &mut self,
        world: &shipyard::World,
        _physics: &crate::physics::PhysicsWorld,
        entity_id: shipyard::EntityId,
    ) -> super::NextBehavior {
        // Already ended (update() hands us back to a normal behavior; a
        // repeat completion must not re-emit the final action's effect).
        if self.finished {
            return super::NextBehavior::NoOpinion;
        }

        let action_complete = self.timed_out
            || self
                .current_scripted_action
                .borrow()
                .is_complete(entity_id, world);
        if action_complete {
            let outgoing_effect = self.current_scripted_action.borrow().completion_effect();
            self.queued_effects.push(outgoing_effect);
            self.timed_out = false;
            self.action_elapsed = 0.0;

            if self.current_action_idx >= ((self.actions.len() as i32) - 1) {
                self.finished = true;
                super::NextBehavior::NoOpinion
            } else {
                self.current_action_idx += 1;
                let behavior = get_behavior_from_action(
                    world,
                    self.owner,
                    &self.actions[self.current_action_idx as usize],
                );
                self.current_scripted_action = behavior;
                let incoming_effect = self.current_scripted_action.borrow().initial_effect();
                self.queued_effects.push(incoming_effect);

                super::NextBehavior::Stay
            }
        } else {
            super::NextBehavior::Stay
        }
    }
}

fn get_behavior_from_action(
    world: &World,
    owner: EntityId,
    action: &AIScriptedAction,
) -> Box<RefCell<dyn ScriptedAction>> {
    let current_behavior: Box<RefCell<dyn ScriptedAction>> = match &action.action_type {
        AIScriptedActionType::ScriptMessage(message) => Box::new(RefCell::new(
            ScriptMessageScriptedAction::new(owner, message.clone()),
        )),
        AIScriptedActionType::Signal {
            entity_name,
            signal,
        } => Box::new(RefCell::new(SendSignalScriptedAction::new(
            world,
            entity_name,
            signal.clone(),
        ))),
        AIScriptedActionType::Play(action_name) => Box::new(RefCell::new(
            PlayAnimationScriptedAction::new(action_name.clone()),
        )),
        AIScriptedActionType::Face { entity_name } => {
            Box::new(RefCell::new(FaceScriptedAction::new(world, &entity_name)))
        }
        AIScriptedActionType::Frob(entity_name) => {
            Box::new(RefCell::new(FrobScriptedAction::new(world, entity_name)))
        }
        AIScriptedActionType::Goto {
            waypoint_name,
            speed: _, // TODO: Incorporate speed
        } => Box::new(RefCell::new(GotoScriptedAction::new(world, &waypoint_name))),

        AIScriptedActionType::MetaProperty {
            action_type,
            arg1,
            arg2: _,
        } => Box::new(RefCell::new(MetaPropertyScriptedAction::new(
            owner,
            action_type,
            arg1,
        ))),

        AIScriptedActionType::Wait(duration) => {
            Box::new(RefCell::new(WaitScriptedAction::new(*duration)))
        }
        _ => Box::new(RefCell::new(NoopScriptedAction)),
    };
    current_behavior
}

/// Dark scripted actions reserve `player` as a target name. The runtime
/// player is synthetic and intentionally has no `PropSymName`, so ordinary
/// by-name lookup cannot resolve it.
fn resolve_scripted_target(world: &World, entity_name: &str) -> Option<EntityId> {
    if entity_name.eq_ignore_ascii_case("player") {
        return world
            .borrow::<View<PropLocalPlayer>>()
            .ok()
            .and_then(|players| players.iter().with_id().next().map(|(id, _)| id));
    }
    script_util::get_first_entity_by_name(world, entity_name)
}

/// ScriptedAction
/// animation:
/// update (&mut self, etc)
/// handle_message
/// is_complete

trait ScriptedAction {
    fn animation(&self) -> Vec<MotionQueryItem> {
        vec![]
    }

    fn turn_speed(&self) -> Deg<f32> {
        Deg(180.0)
    }

    fn initial_effect(&self) -> Effect {
        Effect::NoEffect
    }

    fn completion_effect(&self) -> Effect {
        Effect::NoEffect
    }

    fn is_locomotion(&self) -> bool {
        false
    }

    /// Called when the entity's current animation clip finishes (also fires
    /// when a motion query fails, so waiting on this cannot deadlock).
    fn on_animation_completed(&mut self) {}

    fn is_complete(&self, _entity_id: EntityId, _world: &World) -> bool {
        true
    }

    fn update(
        &mut self,
        current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        Some((Steering::from_current(current_heading), Effect::NoEffect))
    }
}

pub struct PlayAnimationScriptedAction {
    animation_name: String,
    /// Play is a timed beat: it holds the sequence until its clip actually
    /// finishes (or its motion query fails, which also reports completion).
    /// Without this the next action's animation replaces the clip a frame
    /// after it starts.
    completed: bool,
}

impl PlayAnimationScriptedAction {
    pub fn new(animation_name: String) -> PlayAnimationScriptedAction {
        PlayAnimationScriptedAction {
            animation_name,
            completed: false,
        }
    }
}

impl ScriptedAction for PlayAnimationScriptedAction {
    fn turn_speed(&self) -> Deg<f32> {
        Deg(0.0)
    }

    fn on_animation_completed(&mut self) {
        self.completed = true;
    }

    fn is_complete(&self, _entity_id: EntityId, _world: &World) -> bool {
        self.completed
    }
    fn animation(self: &PlayAnimationScriptedAction) -> Vec<MotionQueryItem> {
        if self.animation_name.find(",").is_some() {
            return self
                .animation_name
                .split(",")
                .map(|s| MotionQueryItem::new(s.to_ascii_lowercase().trim()))
                .collect::<Vec<MotionQueryItem>>();
        }

        if let Some(index) = self.animation_name.find(' ') {
            let (motion, value_str) = self.animation_name.split_at(index);
            if let Ok(value) = value_str.trim().parse::<i32>() {
                // I'm assuming the number is an i32, adjust as needed
                return vec![MotionQueryItem::with_value(motion, value)];
            }
        }

        vec![MotionQueryItem::new(&self.animation_name)]
    }
}

#[allow(dead_code)]
pub struct IdleScriptedAction;

impl ScriptedAction for IdleScriptedAction {
    fn animation(self: &IdleScriptedAction) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("idlegesture")]
    }
}

/// Sends a named script message (as a Signal) to the performing entity's own
/// scripts - e.g. the apparition sequences bracket their performance with
/// ScriptMessage("ApparBegin") / ScriptMessage("ApparEnd"), which the
/// Apparition script turns into materialize/vanish.
pub struct ScriptMessageScriptedAction {
    owner: EntityId,
    message: String,
}

impl ScriptMessageScriptedAction {
    pub fn new(owner: EntityId, message: String) -> ScriptMessageScriptedAction {
        ScriptMessageScriptedAction { owner, message }
    }
}

impl ScriptedAction for ScriptMessageScriptedAction {
    fn animation(&self) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }

    fn initial_effect(&self) -> Effect {
        Effect::Send {
            msg: Message {
                to: self.owner,
                payload: crate::scripts::MessagePayload::Signal {
                    name: self.message.clone(),
                },
            },
        }
    }
}

/// Sends an AI signal to a named entity - the watch-obj pseudo-scripts use
/// this to kick a signal response (e.g. the ectoplasm watches send
/// "apparition" to the apparition entity).
pub struct SendSignalScriptedAction {
    target: Option<EntityId>,
    signal: String,
}

impl SendSignalScriptedAction {
    pub fn new(world: &World, entity_name: &str, signal: String) -> SendSignalScriptedAction {
        SendSignalScriptedAction {
            target: resolve_scripted_target(world, entity_name),
            signal,
        }
    }
}

impl ScriptedAction for SendSignalScriptedAction {
    fn animation(&self) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }

    fn initial_effect(&self) -> Effect {
        match self.target {
            Some(to) => Effect::Send {
                msg: Message {
                    to,
                    payload: crate::scripts::MessagePayload::Signal {
                        name: self.signal.clone(),
                    },
                },
            },
            None => Effect::NoEffect,
        }
    }
}

pub struct NoopScriptedAction;

impl ScriptedAction for NoopScriptedAction {
    fn animation(self: &NoopScriptedAction) -> Vec<MotionQueryItem> {
        // vec![MotionQueryItem::with_value("cs", 2)]
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }
}

pub struct MetaPropertyScriptedAction {
    owner: EntityId,
    name: String,
    add: Option<bool>,
}

impl MetaPropertyScriptedAction {
    fn new(owner: EntityId, action_type: &str, name: &str) -> Self {
        let add = if action_type.eq_ignore_ascii_case("add") {
            Some(true)
        } else if action_type.eq_ignore_ascii_case("remove") {
            Some(false)
        } else {
            None
        };
        Self {
            owner,
            name: name.to_owned(),
            add,
        }
    }
}

impl ScriptedAction for MetaPropertyScriptedAction {
    fn animation(&self) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }

    fn initial_effect(&self) -> Effect {
        self.add
            .map(|add| Effect::SetMetaProperty {
                entity_id: self.owner,
                name: self.name.clone(),
                add,
            })
            .unwrap_or(Effect::NoEffect)
    }
}

pub struct WaitScriptedAction {
    remaining_duration_in_seconds: f32,
}

impl WaitScriptedAction {
    pub fn new(time: Duration) -> WaitScriptedAction {
        WaitScriptedAction {
            remaining_duration_in_seconds: time.as_secs_f32(),
        }
    }
}

impl ScriptedAction for WaitScriptedAction {
    fn animation(self: &WaitScriptedAction) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }

    fn update(
        &mut self,
        current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        self.remaining_duration_in_seconds -= time.elapsed.as_secs_f32();
        Some((Steering::from_current(current_heading), Effect::NoEffect))
    }

    fn is_complete(&self, _entity_id: EntityId, _world: &World) -> bool {
        self.remaining_duration_in_seconds <= 0.0
    }
}

pub struct FrobScriptedAction(Option<EntityId>);

impl FrobScriptedAction {
    pub fn new(world: &World, entity_name: &str) -> FrobScriptedAction {
        let maybe_entity = resolve_scripted_target(world, entity_name);
        FrobScriptedAction(maybe_entity)
    }
}

impl ScriptedAction for FrobScriptedAction {
    fn animation(self: &FrobScriptedAction) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }

    fn completion_effect(&self) -> Effect {
        if let Some(entity_id) = self.0 {
            Effect::Send {
                msg: Message {
                    to: entity_id,
                    payload: crate::scripts::MessagePayload::Frob,
                },
            }
        } else {
            Effect::NoEffect
        }
    }
}

pub struct GotoScriptedAction {
    target: GotoTarget,
    steering_strategy: Box<dyn SteeringStrategy>,
}

enum GotoTarget {
    Player,
    Entity(EntityId),
    Missing,
}

/// A script orders travel to the live player, independently of combat memory.
struct ScriptedPlayerSteering;

fn scripted_player_position(world: &World) -> Option<cgmath::Vector3<f32>> {
    world
        .borrow::<shipyard::UniqueView<crate::mission::PlayerInfo>>()
        .ok()
        .map(|player| player.pos)
}

impl SteeringStrategy for ScriptedPlayerSteering {
    fn steer(
        &mut self,
        _heading: Deg<f32>,
        world: &World,
        _physics: &PhysicsWorld,
        entity: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        let target = scripted_player_position(world)?;
        let positions = world.borrow::<View<PropPosition>>().ok()?;
        let position = positions.get(entity).ok()?.position;
        Some((
            Steering::turn_to_point(
                crate::util::vec3_to_point3(position),
                crate::util::vec3_to_point3(target),
            ),
            Effect::NoEffect,
        ))
    }
}

impl GotoScriptedAction {
    pub fn new(world: &World, entity_name: &str) -> GotoScriptedAction {
        let mut steering_strategies: Vec<Box<dyn SteeringStrategy>> = vec![Box::new(
            CollisionAvoidanceSteeringStrategy::conservative(), /* conservative so we can focus on the chase */
        )];

        let target = if entity_name.eq_ignore_ascii_case("player") {
            steering_strategies.push(Box::new(ScriptedPlayerSteering));
            GotoTarget::Player
        } else if let Some(entity) = resolve_scripted_target(world, entity_name) {
            steering_strategies.push(Box::new(ChaseEntitySteeringStrategy::new(entity)));
            GotoTarget::Entity(entity)
        } else {
            GotoTarget::Missing
        };

        GotoScriptedAction {
            target,
            steering_strategy: steering::chained(steering_strategies),
        }
    }
}

impl ScriptedAction for GotoScriptedAction {
    fn is_locomotion(&self) -> bool {
        true
    }

    fn turn_speed(&self) -> Deg<f32> {
        Deg(540.0)
    }
    fn animation(self: &GotoScriptedAction) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("locomote"),
            MotionQueryItem::with_value("direction", 0).optional(),
            MotionQueryItem::new("locourgent").optional(),
        ]
    }
    fn update(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        self.steering_strategy
            .steer(current_heading, world, physics, entity_id, time)
    }

    fn is_complete(&self, entity_id: EntityId, world: &World) -> bool {
        match self.target {
            GotoTarget::Player => scripted_player_position(world).is_none_or(|target| {
                let positions = world.borrow::<View<PropPosition>>().unwrap();
                positions.get(entity_id).is_ok_and(|position| {
                    let delta = target - position.position;
                    vec3(delta.x, 0.0, delta.z).magnitude() < (3.0 / SCALE_FACTOR)
                })
            }),
            GotoTarget::Entity(target_entity_id) => {
                let v_prop_pos = world.borrow::<View<PropPosition>>().unwrap();
                if let Ok(target_pos) = v_prop_pos.get(target_entity_id) {
                    if let Ok(entity_pos) = v_prop_pos.get(entity_id) {
                        let from = vec3(entity_pos.position.x, 0.0, entity_pos.position.z);
                        let to = vec3(target_pos.position.x, 0.0, target_pos.position.z);
                        let distance = (from - to).magnitude();

                        // HACK: This is an arbitrary value that I just tested with some sequences
                        // (ie, in rec1). I'm not sure the best criteria for this step yet.
                        return distance < (3.0 / SCALE_FACTOR);
                    }
                }
                true
            }
            GotoTarget::Missing => true,
        }
    }
}

pub struct FaceScriptedAction {
    target_id: Option<EntityId>,
    steering_strategy: Box<dyn SteeringStrategy>,
}

impl FaceScriptedAction {
    pub fn new(world: &World, entity_name: &str) -> FaceScriptedAction {
        let maybe_entity = resolve_scripted_target(world, entity_name);

        let mut steering_strategies: Vec<Box<dyn SteeringStrategy>> = vec![];

        if let Some(ent) = maybe_entity {
            steering_strategies.push(Box::new(ChaseEntitySteeringStrategy::new(ent)))
        }

        FaceScriptedAction {
            target_id: maybe_entity,
            steering_strategy: steering::chained(steering_strategies),
        }
    }
}

impl ScriptedAction for FaceScriptedAction {
    fn turn_speed(&self) -> Deg<f32> {
        Deg(180.0)
    }
    fn animation(self: &FaceScriptedAction) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("__NULL_ACTION__")]
    }
    fn update(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        self.steering_strategy
            .steer(current_heading, world, physics, entity_id, time)
    }

    fn is_complete(&self, entity_id: EntityId, world: &World) -> bool {
        let v_prop_pos = world.borrow::<View<PropPosition>>().unwrap();
        if let Some(target_entity_id) = self.target_id {
            if let Ok(target_pos) = v_prop_pos.get(target_entity_id) {
                if let Ok(entity_pos) = v_prop_pos.get(entity_id) {
                    let current_yaw = ai_util::current_yaw(entity_id, world);
                    let yaw_between_vectors =
                        ai_util::yaw_between_vectors(entity_pos.position, target_pos.position);

                    let delta = (current_yaw - yaw_between_vectors).0.abs();

                    return delta < 1.0;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission::PlayerInfo;
    use cgmath::{One, Quaternion};
    use dark::properties::PropSymName;

    #[test]
    fn reserved_player_target_resolves_without_a_symbolic_name() {
        let mut world = World::new();
        let player = world.add_entity(PropLocalPlayer {});
        let named = world.add_entity(PropSymName("Waypoint".to_owned()));

        assert_eq!(resolve_scripted_target(&world, "PLAYER"), Some(player));
        assert_eq!(resolve_scripted_target(&world, "waypoint"), Some(named));
    }

    #[test]
    fn metaproperty_action_emits_a_pure_world_effect() {
        let mut world = World::new();
        let owner = world.add_entity(());
        let action = AIScriptedAction {
            action_type: AIScriptedActionType::MetaProperty {
                action_type: "Remove".to_owned(),
                arg1: "Docile".to_owned(),
                arg2: String::new(),
            },
        };

        let behavior = get_behavior_from_action(&world, owner, &action);
        assert!(matches!(
            behavior.borrow().initial_effect(),
            Effect::SetMetaProperty {
                entity_id,
                ref name,
                add: false,
            } if entity_id == owner && name == "Docile"
        ));
    }

    #[test]
    fn goto_player_uses_live_player_position_and_requests_locomotion() {
        let mut world = World::new();
        let owner = world.add_entity(PropPosition {
            position: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::one(),
            cell: 0,
        });
        let player = world.add_entity(PropLocalPlayer {});
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 10.0),
            rotation: Quaternion::one(),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });

        let action = GotoScriptedAction::new(&world, "player");
        assert!(action.is_locomotion());
        assert!(!action.is_complete(owner, &world));
        // Scripted orders target the live player even when combat awareness
        // still points to the actor's own position.
        world.add_component(
            owner,
            crate::runtime_props::RuntimePropAITargetAwareness {
                last_known_pos: vec3(0.0, 0.0, 0.0),
                has_line_of_sight: false,
            },
        );
        assert!(
            !action.is_complete(owner, &world),
            "stale combat memory must not complete Goto player"
        );
        let (steering, _) = ScriptedPlayerSteering
            .steer(
                Deg(90.0),
                &world,
                &PhysicsWorld::new(),
                owner,
                &Time::default(),
            )
            .unwrap();
        assert_eq!(
            steering.desired_heading,
            Deg(0.0),
            "face the live player, not remembered combat position"
        );
    }
}
