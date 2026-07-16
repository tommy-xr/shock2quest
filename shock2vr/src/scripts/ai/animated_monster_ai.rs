use std::{cell::RefCell, collections::HashSet};

use cgmath::{Deg, EuclideanSpace, MetricSpace, Quaternion, Rotation3, vec3, vec4};
use dark::{
    SCALE_FACTOR,
    motion::{MotionFlags, MotionQueryItem},
    properties::{
        AIAlertLevel, Link, PropAIAlertCap, PropAIAwareDelay, PropAISignalResponse, PropPosition,
    },
};
use rand;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::{GlobalPathfinding, GlobalTemplateIdMap, PlayerInfo},
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::script_util,
    time::Time,
};

use super::{
    Effect, Message, MessagePayload, Script,
    ai_debug_util::{self, AlertnessDebugConfig, FovDebugConfig},
    ai_util::*,
    alertness::{self, AlertnessState, AlertnessTimings},
    behavior::*,
    steering::{Steering, SteeringOutput},
};
// Default timing constants for monsters (in seconds)
const DEFAULT_ESCALATE_SECONDS: f32 = 1.5;
const DEFAULT_DECAY_SECONDS: f32 = 3.0;

/// How close (XZ) a pursuing AI must be to a door on its route to interact
/// with it - generous enough to open it while approaching, not so wide it
/// opens doors it merely passes near (12 Dark feet).
const DOOR_INTERACT_RANGE: f32 = 12.0 / SCALE_FACTOR;
/// Minimum cosine between the AI's heading and the direction to the door,
/// applied only beyond `DOOR_FACING_RANGE` - a door the AI is nearly on top
/// of gets opened regardless of facing (its center may be behind the AI once
/// it's in the doorway), but a distant door must be roughly ahead so the AI
/// doesn't open ones off to the side while passing.
const DOOR_FACING_MIN_DOT: f32 = 0.2;
const DOOR_FACING_RANGE: f32 = 5.0 / SCALE_FACTOR;
/// How often a pursuing AI polls for a blocking door. The scan (a linear
/// cell lookup plus a small graph BFS) runs at this rate, not every frame.
const DOOR_POLL_INTERVAL: f32 = 0.3;
/// After opening a door, wait this long before interacting again - long
/// enough that the door has left its closed position, so TurnOn (and its
/// sound) isn't re-sent while it swings.
const DOOR_INTERACT_COOLDOWN: f32 = 2.0;
/// After giving up at a locked door, wait this long before re-frustrating -
/// bounds the "thwarted" gesture for an AI whose alert cap keeps it in a
/// pursuing state even after the give-up's alertness drop.
const DOOR_GIVEUP_COOLDOWN: f32 = 8.0;

/// Configuration for monster alertness behavior
#[derive(Clone)]
struct MonsterConfig {
    alert_cap: PropAIAlertCap,
    timings: AlertnessTimings,
}

/// Alert cap for monsters without a PropAIAlertCap property
fn default_alert_cap() -> PropAIAlertCap {
    PropAIAlertCap {
        max_level: AIAlertLevel::High,
        min_level: AIAlertLevel::Lowest,
        min_relax: AIAlertLevel::Low,
    }
}

pub struct AnimatedMonsterAI {
    last_hit_sensor: Option<EntityId>,
    current_behavior: Box<RefCell<dyn Behavior>>,
    current_heading: Deg<f32>,
    is_dead: bool,
    /// Script updates seen since entering death. Guards the crumple->ragdoll
    /// handoff against a stale AnimationCompleted: a clip that happened to
    /// finish in the very tick the killing blow landed is dispatched after
    /// `is_dead` is set, and would otherwise hand off before the crumple has
    /// even started. The real crumple completion arrives seconds later.
    updates_since_death: u32,
    /// The blow that killed this creature (direction/point/bone), stashed at
    /// the lethal Damage so the crumple->ragdoll handoff can seed the corpse's
    /// physical reaction.
    death_impact: Option<crate::scripts::DamageImpact>,
    took_damage: bool,
    animation_seq: u32,
    locomotion_seq: u32,

    played_ai_watch_obj: HashSet<EntityId>,

    /// Alertness state tracking
    alertness: AlertnessState,
    /// Alertness configuration (loaded from entity properties)
    config: Option<MonsterConfig>,
    /// Behavior name last published for debug introspection
    published_behavior: Option<&'static str>,
    /// Where the player was last seen; investigated by SearchBehavior when
    /// alertness decays after losing contact
    last_known_player_pos: Option<cgmath::Vector3<f32>>,
    /// Awareness state last published to the ECS, so the component tracks
    /// the script's knowledge exactly (including forgetting) without
    /// per-frame effect churn
    published_awareness: Option<(cgmath::Vector3<f32>, bool)>,
    /// Throttle between door interactions (open / locked-door give-up)
    door_cooldown: f32,
}

impl AnimatedMonsterAI {
    pub fn idle() -> AnimatedMonsterAI {
        AnimatedMonsterAI {
            is_dead: false,
            updates_since_death: 0,
            death_impact: None,
            took_damage: false,
            current_behavior: Box::new(RefCell::new(IdleBehavior)),
            current_heading: Deg(0.0),
            animation_seq: 0,
            locomotion_seq: 0,
            last_hit_sensor: None,
            played_ai_watch_obj: HashSet::new(),
            alertness: AlertnessState::default(),
            config: None,
            published_behavior: None,
            last_known_player_pos: None,
            published_awareness: None,
            door_cooldown: 0.0,
        }
    }

    pub fn new() -> AnimatedMonsterAI {
        AnimatedMonsterAI {
            is_dead: false,
            updates_since_death: 0,
            death_impact: None,
            took_damage: false,
            // Start with IdleBehavior - alertness will drive behavior changes
            current_behavior: Box::new(RefCell::new(IdleBehavior)),
            current_heading: Deg(0.0),
            animation_seq: 0,
            locomotion_seq: 0,
            last_hit_sensor: None,
            played_ai_watch_obj: HashSet::new(),
            alertness: AlertnessState::default(),
            config: None,
            published_behavior: None,
            last_known_player_pos: None,
            published_awareness: None,
            door_cooldown: 0.0,
        }
    }

    fn build_config(world: &World, entity_id: EntityId) -> Option<MonsterConfig> {
        // Apparitions are non-interactive replays: they must never notice the
        // player (an invisible ghost escalating to chase would wander off its
        // authored mark). No config = no alertness processing.
        if let Ok(v_class_tag) = world.borrow::<View<dark::properties::PropClassTag>>() {
            if let Ok(tag) = v_class_tag.get(entity_id) {
                if tag
                    .class_tags()
                    .iter()
                    .any(|(k, v)| *k == "creaturetype" && *v == "apparition")
                {
                    return None;
                }
            }
        }

        let (v_alert_cap, v_aware_delay): (View<PropAIAlertCap>, View<PropAIAwareDelay>) =
            world.borrow().ok()?;

        let alert_cap = v_alert_cap
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or_else(default_alert_cap);

        // Build default aware delay for monsters (faster than cameras/turrets)
        let default_aware_delay = PropAIAwareDelay {
            to_two: (DEFAULT_ESCALATE_SECONDS * 1000.0) as u32,
            to_three: (DEFAULT_ESCALATE_SECONDS * 1000.0) as u32,
            two_reuse: (DEFAULT_DECAY_SECONDS * 1000.0) as u32,
            three_reuse: (DEFAULT_DECAY_SECONDS * 1000.0) as u32,
            ignore_range: (DEFAULT_DECAY_SECONDS * 1000.0) as u32,
        };

        let aware_delay = v_aware_delay
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(default_aware_delay);

        let timings = AlertnessTimings::from_aware_delay(&aware_delay);

        Some(MonsterConfig { alert_cap, timings })
    }

    /// Get the appropriate behavior for the current alertness level
    fn behavior_for_alertness(
        &self,
        world: &World,
        _physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> Box<RefCell<dyn Behavior>> {
        match self.alertness.current_level {
            AIAlertLevel::Lowest => self.idle_behavior(world, entity_id),
            AIAlertLevel::Low => Box::new(RefCell::new(WanderBehavior::new())),
            AIAlertLevel::Moderate => Box::new(RefCell::new(ChaseBehavior::new())),
            AIAlertLevel::High => {
                // Attack only when in range; otherwise chase to close the
                // distance (ChaseBehavior::next_behavior escalates back to
                // an attack on arrival via the same shared helper). Without
                // the range check, a far-away High AI stood still swinging.
                attack_behavior_for_distance(world, entity_id)
                    .unwrap_or_else(|| Box::new(RefCell::new(ChaseBehavior::new())))
            }
        }
    }

    /// The behavior for a fully-calm (Lowest) AI: patrol an authored route if
    /// it is flagged to and a route exists, otherwise stand idle. Falls back to
    /// idle when the mission has no patrol network reachable from here.
    fn idle_behavior(&self, world: &World, entity_id: EntityId) -> Box<RefCell<dyn Behavior>> {
        if is_patroller(world, entity_id) {
            let (position, _) = get_position_and_forward(world, entity_id);
            if let Some((point, goal)) = nearest_patrol_point(world, position.to_vec()) {
                return Box::new(RefCell::new(PatrolBehavior::new(point, goal)));
            }
        }
        Box::new(RefCell::new(IdleBehavior))
    }

    fn apply_steering_output(
        &mut self,
        steering_output: SteeringOutput,
        time: &Time,
        entity_id: EntityId,
    ) -> Effect {
        let turn_velocity = self.current_behavior.borrow().turn_speed().0;
        let delta =
            clamp_to_minimal_delta_angle(steering_output.desired_heading - self.current_heading);

        let turn_amount = if delta.0 < 0.0 {
            (-turn_velocity * time.elapsed.as_secs_f32()).max(delta.0)
        } else {
            (turn_velocity * time.elapsed.as_secs_f32()).min(delta.0)
        };

        self.current_heading = Deg(self.current_heading.0 + turn_amount);

        Effect::SetRotation {
            entity_id,
            rotation: Quaternion::from_angle_y(self.current_heading),
        }
    }

    fn try_tickle_sensor(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> Effect {
        let (position, forward) = get_position_and_forward(world, entity_id);

        let down_amount = 2.0 / SCALE_FACTOR;
        let down_vector = vec3(0.0, -down_amount, 0.0);

        let distance = 8.0 / SCALE_FACTOR;

        let _direction = forward + down_vector;

        let maybe_hit_result = physics.ray_cast2(
            position,
            forward + down_vector,
            distance,
            InternalCollisionGroups::ALL_COLLIDABLE,
            Some(entity_id),
            false,
        );

        let maybe_hit_sensor = if maybe_hit_result.is_some() {
            let hit_result = maybe_hit_result.unwrap();

            if hit_result.is_sensor {
                hit_result.maybe_entity_id
            } else {
                None
            }
        } else {
            None
        };

        let sensor_effect = if maybe_hit_sensor != self.last_hit_sensor {
            match maybe_hit_sensor {
                Some(sensor_id) => Effect::Send {
                    msg: Message {
                        to: sensor_id,
                        payload: MessagePayload::SensorBeginIntersect { with: entity_id },
                    },
                },
                None => {
                    if let Some(sensor_id) = self.last_hit_sensor {
                        Effect::Send {
                            msg: Message {
                                to: sensor_id,
                                payload: MessagePayload::SensorEndIntersect { with: entity_id },
                            },
                        }
                    } else {
                        Effect::NoEffect
                    }
                }
            }
        } else {
            Effect::NoEffect
        };

        let color = if maybe_hit_sensor.is_some() {
            vec4(1.0, 1.0, 0.0, 1.0)
        } else {
            vec4(0.0, 1.0, 1.0, 1.0)
        };

        self.last_hit_sensor = maybe_hit_sensor;

        let debug_effect = Effect::DrawDebugLines {
            lines: vec![(
                position,
                position + ((forward + down_vector) * distance),
                color,
            )],
        };

        Effect::combine(vec![sensor_effect, debug_effect])
    }

    fn next_selection(
        &mut self,
        is_locomotion: bool,
    ) -> dark::motion::MotionQuerySelectionStrategy {
        if is_locomotion {
            let seq = self.locomotion_seq;
            self.locomotion_seq = self.locomotion_seq.wrapping_add(1);
            dark::motion::MotionQuerySelectionStrategy::Sequential(seq)
        } else {
            let seq = self.animation_seq;
            self.animation_seq = self.animation_seq.wrapping_add(1);
            dark::motion::MotionQuerySelectionStrategy::Sequential(seq)
        }
    }

    /// Force alertness to `level` (clamped by the alert cap), reset the
    /// visibility timers, and swap in the canonical behavior for the
    /// resulting level. Callers must check the AI is alive first.
    /// React to a stimulus that locates the player at `origin` - a hit taken,
    /// or a noise heard. Refreshes the last-known position (so a searching or
    /// chasing AI turns toward the fresh cue) and escalates toward Moderate
    /// (chase) when not already alerted. No-op for a dead AI or one mid
    /// scripted sequence (which `force_alertness` would otherwise preempt).
    fn alert_to_position(
        &mut self,
        origin: cgmath::Vector3<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> Effect {
        // No config = an AI that doesn't process alertness at all (e.g. an
        // apparition replay - see build_config); it must never notice the
        // player, so a hit or a noise leaves it be.
        let Some(config) = self.config.as_ref() else {
            return Effect::NoEffect;
        };
        if self.is_dead
            || is_killed(entity_id, world)
            || self.current_behavior.borrow().scripted_state() == ScriptedState::Running
        {
            return Effect::NoEffect;
        }
        // Refresh even when already alerted - the cue reveals where the
        // player is now, redirecting a stale chase or a search.
        self.last_known_player_pos = Some(origin);

        let cap = config.alert_cap.clone();
        // Only force when the clamped target actually raises the level (so a
        // capped AI isn't reset / re-animated), or when a search should
        // re-aggro toward the fresh cue.
        let target = alertness::clamp_level(AIAlertLevel::Moderate, &cap);
        let escalates = matches!(
            self.alertness.current_level,
            AIAlertLevel::Lowest | AIAlertLevel::Low
        ) && target != self.alertness.current_level;
        let searching = self.current_behavior.borrow().name() == "Search";
        if escalates || searching {
            self.force_alertness(AIAlertLevel::Moderate, world, physics, entity_id)
        } else {
            Effect::NoEffect
        }
    }

    /// Set the alertness level and swap to the matching behavior WITHOUT
    /// starting that behavior's animation - for callers that immediately play
    /// a different clip over it (the thwarted gesture); the completion
    /// handler starts the behavior's clip when that clip finishes.
    fn force_alertness_state(
        &mut self,
        level: AIAlertLevel,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> Effect {
        let cap = self
            .config
            .as_ref()
            .map(|c| c.alert_cap.clone())
            .unwrap_or_else(default_alert_cap);
        alertness::set_level(&mut self.alertness, level, &cap);
        // Forced level starts fresh: no accumulated visibility time pushing
        // an immediate escalation or decay
        self.alertness.visible_time = 0.0;
        self.alertness.hidden_time = 0.0;

        self.current_behavior = self.behavior_for_alertness(world, physics, entity_id);
        alertness::sync_alertness_effect(entity_id, &self.alertness)
    }

    fn force_alertness(
        &mut self,
        level: AIAlertLevel,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> Effect {
        let sync_effect = self.force_alertness_state(level, world, physics, entity_id);
        let is_locomotion = self.current_behavior.borrow().is_locomotion();
        let selection_strategy = self.next_selection(is_locomotion);
        // Play (replace), never queue, on a behavior change: queueing pushes
        // the new clip on top and leaves the interrupted clip behind it, and
        // every later clip completion then exposes that stale entry at frame 0
        // for one tick (a visible pose flash + zero-velocity hiccup each walk
        // stride) before the completion handler re-queues. The other
        // behavior-change sites below Play for the same reason (the wound
        // reaction runs on an already-empty queue and Plays only for the
        // smoother minimum crossfade).
        Effect::combine(vec![
            sync_effect,
            Effect::PlayAnimationBySchema {
                entity_id,
                motion_queries: vec![self.current_behavior.borrow().animation()],
                selection_strategy,
            },
        ])
    }

    /// Switch to DeadBehavior and start the death animation and sound. The
    /// crumple interrupts whatever clip is playing (cross-fading from its
    /// current pose) and clears the animation queue, so no interrupted clip
    /// resumes under the corpse. Latching `is_dead` up front makes the
    /// AnimationCompleted death branch a no-op afterwards (including the
    /// re-dispatch queued when no crumple motion is found), so the crumple
    /// and death sound play only once.
    fn enter_death(&mut self, world: &World, entity_id: EntityId) -> Effect {
        self.current_behavior = Box::new(RefCell::new(DeadBehavior {}));
        self.is_dead = true;

        let death_sound_effect = if let Some(voice_index) =
            crate::scripts::speech_util::resolve_entity_voice_index(world, entity_id)
        {
            // Randomly choose between loud and soft death sound
            let concept = if rand::random::<bool>() {
                "comdieloud".to_string()
            } else {
                "comdiesoft".to_string()
            };

            Effect::PlaySpeech {
                entity_id,
                voice_index,
                concept,
                tags: vec![],
            }
        } else {
            Effect::NoEffect
        };

        // Death clips are keyed differently per creature: hybrid-style
        // deaths hang directly under the crumple key, but human deaths sit
        // one level deeper, under crumple -> die. Prefer the creature's
        // directly-keyed clips; only when there are none, retry through the
        // die level (verified across creature types with `cargo dq motion`).
        let death_animation = Effect::PlayAnimationBySchema {
            entity_id,
            motion_queries: vec![
                vec![MotionQueryItem::new("crumple")],
                vec![MotionQueryItem::new("crumple"), MotionQueryItem::new("die")],
            ],
            selection_strategy: dark::motion::MotionQuerySelectionStrategy::Random,
        };

        Effect::combine(vec![death_sound_effect, death_animation])
    }

    /// Publish the current behavior name for debug introspection. Update
    /// runs each frame, so this covers every behavior-change site with at
    /// most one frame of lag (e.g. handle_message changes, or the
    /// AIWatchObj early-return, publish on the next update)
    fn publish_behavior(&mut self, entity_id: EntityId) -> Effect {
        let behavior_name = self.current_behavior.borrow().name();
        if self.published_behavior != Some(behavior_name) {
            self.published_behavior = Some(behavior_name);
            Effect::SetAIProperty {
                entity_id,
                update: crate::scripts::AIPropertyUpdate::Behavior {
                    name: behavior_name.to_string(),
                },
            }
        } else {
            Effect::NoEffect
        }
    }

    /// While pursuing, open the (unlocked) door gating the AI's route so it
    /// can follow the player through, or give up at a locked one. The graph
    /// treats doors as passable, so the AI paths straight at a closed door
    /// and its body is blocked - this is what actually gets it through.
    fn handle_doors(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Effect {
        self.door_cooldown = (self.door_cooldown - time.elapsed.as_secs_f32()).max(0.0);
        if self.door_cooldown > 0.0 {
            return Effect::NoEffect;
        }
        // Only actively-pursuing behaviors bother with doors (cheap check
        // left off the cooldown so a state change is noticed promptly)
        if !matches!(self.current_behavior.borrow().name(), "Chase" | "Search") {
            return Effect::NoEffect;
        }
        // The scan below (cell_from_position is O(cells) + a graph BFS) runs
        // at a few Hz, not every frame - arm the poll cooldown up front,
        // regardless of whether a door is found. The act paths override it
        // with a longer value.
        self.door_cooldown = DOOR_POLL_INTERVAL;

        let Some(service) = world
            .borrow::<UniqueView<GlobalPathfinding>>()
            .ok()
            .and_then(|g| g.0.clone())
        else {
            return Effect::NoEffect;
        };
        let (position, forward) = get_position_and_forward(world, entity_id);
        let pos = position.to_vec();
        let Some(cell) = service.cell_from_position(pos) else {
            return Effect::NoEffect;
        };
        let doors = service.doors_near_cell(cell);
        if doors.is_empty() {
            return Effect::NoEffect;
        }
        let Ok(id_map) = world.borrow::<UniqueView<GlobalTemplateIdMap>>() else {
            return Effect::NoEffect;
        };

        for (door_obj, door_center) in doors {
            let (dx, dz) = (door_center.x - pos.x, door_center.z - pos.z);
            let dist = (dx * dx + dz * dz).sqrt();
            if dist > DOOR_INTERACT_RANGE || dist < 1e-3 {
                continue;
            }
            // Beyond arm's reach, only open a door roughly ahead (not one off
            // to the side we're merely passing); nearer ones we open anyway,
            // since the doorway centre can be behind us once we're in it.
            if dist > DOOR_FACING_RANGE {
                let fwd_len = (forward.x * forward.x + forward.z * forward.z).sqrt();
                if fwd_len < 1e-3
                    || (forward.x * dx + forward.z * dz) / (fwd_len * dist) < DOOR_FACING_MIN_DOT
                {
                    continue;
                }
            }
            let Some(door_ent) = id_map.0.get(&door_obj).map(|w| w.0) else {
                continue;
            };
            // Only act on a door that's actually closed (skip open / opening
            // ones, and non-door objects)
            if script_util::door_is_closed(world, door_ent) != Some(true) {
                continue;
            }

            if script_util::is_entity_locked(world, door_ent) {
                // Can't follow through a locked door: show frustration and
                // give up the pursuit (drop to a wander), so the player can't
                // lure the AI into off-limits areas. Forget the last-known
                // position so it doesn't immediately re-path to the door. The
                // longer cooldown keeps the "thwarted" gesture from replaying
                // rapidly for an AI whose alert cap won't let it drop below a
                // pursuing level.
                self.door_cooldown = DOOR_GIVEUP_COOLDOWN;
                self.last_known_player_pos = None;
                // State-only downgrade: the thwarted gesture replaces the
                // queue this frame (a second Play here would blend from an
                // unseen frame-0 pose and waste a clip load); the completion
                // handler starts the Low behavior's clip after the gesture.
                let downgrade =
                    self.force_alertness_state(AIAlertLevel::Low, world, physics, entity_id);
                let thwarted = Effect::PlayAnimationBySchema {
                    entity_id,
                    motion_queries: vec![vec![MotionQueryItem::new("thwarted")]],
                    selection_strategy: dark::motion::MotionQuerySelectionStrategy::Random,
                };
                return Effect::combine(vec![downgrade, thwarted]);
            }
            // Unlocked: open it and keep chasing through. The longer cooldown
            // avoids re-sending TurnOn (and replaying the open sound) while
            // the door is still swinging.
            self.door_cooldown = DOOR_INTERACT_COOLDOWN;
            return Effect::Send {
                msg: Message {
                    to: door_ent,
                    payload: MessagePayload::TurnOn { from: entity_id },
                },
            };
        }
        Effect::NoEffect
    }
}

impl Script for AnimatedMonsterAI {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.current_heading = current_yaw(entity_id, world);

        // Load alertness configuration from entity properties
        self.config = Self::build_config(world, entity_id);

        // Initialize alertness state
        let alertness_effect = if let Some(config) = &self.config {
            let initial_level = alertness::clamp_level(AIAlertLevel::Lowest, &config.alert_cap);
            self.alertness = AlertnessState::new(initial_level);
            alertness::sync_alertness_effect(entity_id, &self.alertness)
        } else {
            Effect::NoEffect
        };

        let is_locomotion = self.current_behavior.borrow().is_locomotion();
        let selection_strategy = self.next_selection(is_locomotion);
        let animation_effect = Effect::QueueAnimationBySchema {
            entity_id,
            motion_queries: vec![self.current_behavior.borrow().animation()],
            selection_strategy,
        };

        Effect::combine(vec![alertness_effect, animation_effect])
    }
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        // Dead AIs are inert - no alertness, scripted sequences, steering, or
        // sensors. A corpse keeps a live script, and any alertness level
        // change here (escalation while the player is visible, or decay)
        // would replace DeadBehavior and resurrect it. Still publish the
        // behavior so introspection shows "Dead", and release a sensor the
        // ray was intersecting at death so its end-intersect isn't stranded.
        if self.is_dead || is_killed(entity_id, world) {
            self.updates_since_death = self.updates_since_death.saturating_add(1);
            let sensor_release_effect = match self.last_hit_sensor.take() {
                Some(sensor_id) => Effect::Send {
                    msg: Message {
                        to: sensor_id,
                        payload: MessagePayload::SensorEndIntersect { with: entity_id },
                    },
                },
                None => Effect::NoEffect,
            };
            return Effect::combine(vec![
                sensor_release_effect,
                self.publish_behavior(entity_id),
            ]);
        }

        let delta = time.elapsed.as_secs_f32();

        // Monster FOV is 60 degrees half-angle (matches FovDebugConfig::monster())
        // Monster rotation is set directly via Effect::SetRotation, so pose.rotation
        // already contains the heading. Pass Deg(0.0) to avoid applying it twice.
        const MONSTER_FOV_HALF_ANGLE: f32 = 60.0;
        let is_visible =
            is_player_visible_in_fov(entity_id, world, physics, Deg(0.0), MONSTER_FOV_HALF_ANGLE);

        // Remember where the player was last seen, for SearchBehavior
        if is_visible {
            if let Ok(player) = world.borrow::<shipyard::UniqueView<PlayerInfo>>() {
                self.last_known_player_pos = Some(player.pos);
            }
        }

        // Publish what this AI knows about its target: chase steering
        // pursues the last-known position (frozen when sight breaks), not
        // the player's true location. Published only on change, and CLEARED
        // when the script forgets (search consumed it / fully calmed) so a
        // stale component can't hijack the true-position fallback forever.
        let desired_awareness = self.last_known_player_pos.map(|pos| (pos, is_visible));
        let awareness_effect = if desired_awareness != self.published_awareness {
            self.published_awareness = desired_awareness;
            match desired_awareness {
                Some((pos, visible)) => Effect::SetAIProperty {
                    entity_id,
                    update: crate::scripts::AIPropertyUpdate::TargetAwareness {
                        last_known_pos: pos,
                        has_line_of_sight: visible,
                    },
                },
                None => Effect::SetAIProperty {
                    entity_id,
                    update: crate::scripts::AIPropertyUpdate::ClearTargetAwareness,
                },
            }
        } else {
            Effect::NoEffect
        };

        // Update alertness state
        let (alertness_effect, behavior_change_effect) = if let Some(config) = &self.config {
            if let Some((old_level, new_level)) = alertness::process_alertness_update(
                &mut self.alertness,
                is_visible,
                delta,
                &config.timings,
                &config.alert_cap,
            ) {
                // Level changed - sync to ECS and potentially change behavior
                let sync_effect = alertness::sync_alertness_effect(entity_id, &self.alertness);

                // A running scripted sequence (e.g. the rec1 Cortez window
                // scene) must not be preempted - or have its current clip
                // replaced - by alertness swings; the alertness state still
                // updates and takes effect once the sequence finishes. (The
                // original engine gates this via the response priority; we
                // protect all sequences.)
                if self.current_behavior.borrow().scripted_state() == ScriptedState::Running {
                    (sync_effect, Effect::NoEffect)
                } else {
                    // AIAlertLevel is repr(u32) in escalation order
                    let decayed = (new_level as u32) < (old_level as u32);

                    // Losing contact after actively tracking the player
                    // doesn't drop straight to wandering: investigate the
                    // last-known position first. Further decay during an
                    // active search leaves it running - it hands off to
                    // Wander itself.
                    let searching = self.current_behavior.borrow().name() == "Search";
                    let new_behavior: Option<Box<RefCell<dyn Behavior>>> = if decayed && searching {
                        // An active search keeps running across further decay
                        // (it hands off on its own) - unless a fresher
                        // sighting was recorded mid-search, which re-targets
                        // it. This must be checked BEFORE the decay-from-
                        // tracking branch, or a High-origin search is stomped
                        // one decay later.
                        self.last_known_player_pos.take().map(
                            |goal| -> Box<RefCell<dyn Behavior>> {
                                Box::new(RefCell::new(SearchBehavior::new(goal)))
                            },
                        )
                    } else if decayed
                        && matches!(old_level, AIAlertLevel::Moderate | AIAlertLevel::High)
                    {
                        match self.last_known_player_pos.take() {
                            Some(goal) => Some(Box::new(RefCell::new(SearchBehavior::new(goal)))),
                            // Never actually saw the player (e.g. aggroed by
                            // damage from behind) - nothing to investigate
                            None => Some(self.behavior_for_alertness(world, physics, entity_id)),
                        }
                    } else {
                        Some(self.behavior_for_alertness(world, physics, entity_id))
                    };
                    // Fully calmed: a sighting from this engagement must not
                    // trigger a cross-map search minutes later
                    if new_level == AIAlertLevel::Lowest {
                        self.last_known_player_pos = None;
                    }

                    let animation_effect = if let Some(behavior) = new_behavior {
                        self.current_behavior = behavior;
                        let is_locomotion = self.current_behavior.borrow().is_locomotion();
                        let selection_strategy = self.next_selection(is_locomotion);
                        Effect::PlayAnimationBySchema {
                            entity_id,
                            motion_queries: vec![self.current_behavior.borrow().animation()],
                            selection_strategy,
                        }
                    } else {
                        Effect::NoEffect
                    };

                    (sync_effect, animation_effect)
                }
            } else {
                (Effect::NoEffect, Effect::NoEffect)
            }
        } else {
            (Effect::NoEffect, Effect::NoEffect)
        };

        // Check our AIWatchObj status
        let ai_signal_resp =
            script_util::get_all_links_with_data(world, entity_id, |link| match link {
                Link::AIWatchObj(data) => Some(data.clone()),
                _ => None,
            });

        for (ent_id, watch_options) in ai_signal_resp {
            if self.played_ai_watch_obj.contains(&ent_id) {
                continue;
            }

            // Don't preempt a performance in progress; the watch obj stays
            // unplayed and can trigger once the current sequence finishes.
            if self.current_behavior.borrow().scripted_state() == ScriptedState::Running {
                break;
            }

            if player_is_within_watch_obj(world, ent_id, watch_options.radius) {
                // Immediately switch to Scripted sequence Behavior
                self.played_ai_watch_obj.insert(ent_id);
                self.current_behavior = Box::new(RefCell::new(ScriptedSequenceBehavior::new(
                    world,
                    entity_id,
                    None,
                    watch_options.scripted_actions.clone(),
                )));
                let is_locomotion = self.current_behavior.borrow().is_locomotion();
                let selection_strategy = self.next_selection(is_locomotion);
                return Effect::PlayAnimationBySchema {
                    entity_id,
                    motion_queries: vec![self.current_behavior.borrow().animation()],
                    selection_strategy,
                };
            }
        }

        // Temporary steering behavior
        let (steering_output, steering_effects) = self
            .current_behavior
            .borrow_mut()
            .steer(self.current_heading, world, physics, entity_id, time)
            .unwrap_or((
                Steering::from_current(self.current_heading),
                Effect::NoEffect,
            ));

        let rotation_effect = self.apply_steering_output(steering_output, time, entity_id);

        // A finished scripted sequence (its final queued effects were drained
        // by the steer above - scripted_state only reports Finished once they
        // are) hands control back to the alertness-appropriate behavior. This
        // runs every frame, so it also covers sequences ended by the
        // watchdog, where no further AnimationCompleted may ever arrive.
        let handback_effect =
            if self.current_behavior.borrow().scripted_state() == ScriptedState::Finished {
                self.current_behavior = self.behavior_for_alertness(world, physics, entity_id);
                let is_locomotion = self.current_behavior.borrow().is_locomotion();
                let selection_strategy = self.next_selection(is_locomotion);
                Effect::PlayAnimationBySchema {
                    entity_id,
                    motion_queries: vec![self.current_behavior.borrow().animation()],
                    selection_strategy,
                }
            } else {
                Effect::NoEffect
            };

        let sensor_effect = self.try_tickle_sensor(world, physics, entity_id);

        // Debug visualization - alertness bar
        let alertness_debug_effect = ai_debug_util::draw_debug_alertness(
            world,
            entity_id,
            &self.alertness,
            is_visible,
            &AlertnessDebugConfig::monster(),
        );

        // Debug visualization - FOV cone
        // Monster rotation is set directly via Effect::SetRotation, so pose.rotation
        // already contains the heading. Pass Deg(0.0) to avoid applying it twice.
        let fov_debug_effect = ai_debug_util::draw_debug_fov(
            world,
            entity_id,
            Deg(0.0),
            is_visible,
            &FovDebugConfig::monster(),
        );

        let behavior_publish_effect = self.publish_behavior(entity_id);

        // Open a door blocking the pursuit (or give up at a locked one)
        let door_effect = self.handle_doors(world, physics, entity_id, time);

        Effect::combine(vec![
            alertness_effect,
            awareness_effect,
            behavior_change_effect,
            steering_effects,
            rotation_effect,
            handback_effect,
            sensor_effect,
            alertness_debug_effect,
            fov_debug_effect,
            behavior_publish_effect,
            door_effect,
        ])
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        {
            self.current_behavior
                .borrow_mut()
                .handle_message(entity_id, world, physics, msg);
        }
        match msg {
            MessagePayload::Damage { amount, impact } => {
                // Corpses don't bleed: no HP churn, aggro, or replayed death
                // from shooting a dead monster. The world check also covers a
                // post-load corpse, whose recreated script has is_dead reset
                // while its hit points are still <= 0
                if self.is_dead || is_killed(entity_id, world) {
                    return Effect::NoEffect;
                }
                // TODO: Let behavior handle this?
                self.took_damage = true;
                let hit_points_effect = Effect::AdjustHitPoints {
                    entity_id,
                    delta: -(amount.round() as i32),
                };
                // Taking damage alerts the AI: escalate toward Moderate
                // (chase) so a shot AI aggros even when it never saw the
                // player. Escalation only - an already-alerted AI stays put.
                // The hit-point adjustment applies after this handler
                // returns, so lethality is checked against the incoming
                // amount - a killing blow must not stand the AI up to chase
                // for a frame. Note: the damage source isn't attributed, so
                // any damage aggros toward the player (chase is
                // player-centric like the other behaviors).
                let lethal = hit_points(entity_id, world)
                    .map(|hp| hp - amount.round() as i32 <= 0)
                    .unwrap_or(false);
                // A surviving hit alerts the AI to the attacker (the player):
                // a shot reveals the player's position even with no line of
                // sight. A killing blow doesn't - it goes straight to death.
                let alert_effect = if lethal {
                    Effect::NoEffect
                } else if let Ok(player) = world.borrow::<shipyard::UniqueView<PlayerInfo>>() {
                    let origin = player.pos;
                    self.alert_to_position(origin, world, physics, entity_id)
                } else {
                    Effect::NoEffect
                };
                // A killing blow reacts immediately - the death animation
                // interrupts the in-flight clip (cross-fading from its
                // current pose) instead of waiting for it to complete
                let death_effect = if lethal {
                    // Remember how the killing blow landed for the
                    // crumple->ragdoll handoff.
                    self.death_impact = *impact;
                    self.enter_death(world, entity_id)
                } else {
                    Effect::NoEffect
                };
                Effect::combine(vec![hit_points_effect, alert_effect, death_effect])
            }
            MessagePayload::HeardNoise { origin } => {
                // A gunshot (or other noise) draws the AI to investigate its
                // source, even with no line of sight - same alert path as
                // taking a hit, minus the damage. (Deaf AIs never receive
                // this message - raise_noise filters them out.)
                self.alert_to_position(*origin, world, physics, entity_id)
            }
            MessagePayload::TurnOn { from: _ } => {
                // Dead AIs stay dead - a corpse can still be a SwitchLink
                // target (e.g. a tripwire), and the scripted sequence would
                // animate it
                if self.is_dead || is_killed(entity_id, world) {
                    return Effect::NoEffect;
                }
                // A re-fired trigger must not restart a performance in
                // progress (the tripwires driving these are rarely ONCE)
                if self.current_behavior.borrow().scripted_state() == ScriptedState::Running {
                    return Effect::NoEffect;
                }
                let v_prop_sig_resp = world.borrow::<View<PropAISignalResponse>>().unwrap();

                if let Ok(prop_sig_resp) = v_prop_sig_resp.get(entity_id) {
                    // Immediately switch to Scripted sequence Behavior
                    self.current_behavior = Box::new(RefCell::new(ScriptedSequenceBehavior::new(
                        world,
                        entity_id,
                        None,
                        prop_sig_resp.actions.clone(),
                    )));
                    let is_locomotion = self.current_behavior.borrow().is_locomotion();
                    let selection_strategy = self.next_selection(is_locomotion);
                    Effect::PlayAnimationBySchema {
                        entity_id,
                        motion_queries: vec![self.current_behavior.borrow().animation()],
                        selection_strategy,
                    }
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::Signal { name } => {
                // Dead AIs stay dead - level signals reach corpses too
                if self.is_dead || is_killed(entity_id, world) {
                    return Effect::NoEffect;
                }
                // A re-fire of the signal that started the current
                // performance must not restart it. A DIFFERENT signal may
                // replace the sequence - a watch-obj sequence (origin None)
                // hands off to the real response this way: its lone action
                // sends the response's signal to itself. (A signal arriving
                // during an unrelated TurnOn/watch performance could likewise
                // interrupt it; no shipped content wires that, and the
                // engine-faithful fix is response priority - deferred, as in
                // the sequence-protection work this builds on.)
                if self.current_behavior.borrow().scripted_state() == ScriptedState::Running {
                    let same_origin = self
                        .current_behavior
                        .borrow()
                        .origin_signal()
                        .is_some_and(|origin| origin.eq_ignore_ascii_case(name));
                    if same_origin {
                        return Effect::NoEffect;
                    }
                }
                // Respond only to a signal matching this AI's authored
                // response signal name (named script messages like ApparBegin
                // also arrive as signals and must not trigger the response).
                let v_prop_sig_resp = world.borrow::<View<PropAISignalResponse>>().unwrap();

                if let Some(prop_sig_resp) = v_prop_sig_resp
                    .get(entity_id)
                    .ok()
                    .filter(|resp| resp.signal.eq_ignore_ascii_case(name))
                {
                    // Immediately switch to Scripted sequence Behavior
                    self.current_behavior = Box::new(RefCell::new(ScriptedSequenceBehavior::new(
                        world,
                        entity_id,
                        Some(name.clone()),
                        prop_sig_resp.actions.clone(),
                    )));
                    let is_locomotion = self.current_behavior.borrow().is_locomotion();
                    let selection_strategy = self.next_selection(is_locomotion);
                    Effect::PlayAnimationBySchema {
                        entity_id,
                        motion_queries: vec![self.current_behavior.borrow().animation()],
                        selection_strategy,
                    }
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::SetAlertness { level } => {
                // Dead AIs stay dead - a corpse keeps a live script, and the
                // broadcast (DebugAlertAll) reaches every creature
                if self.is_dead || is_killed(entity_id, world) {
                    return Effect::NoEffect;
                }
                // The behavior reset is unconditional, even when the level
                // didn't change - forcing is a debug reset, so it also
                // cancels scripted sequences
                self.force_alertness(*level, world, physics, entity_id)
            }
            MessagePayload::AnimationCompleted => {
                if self.is_dead {
                    // The death crumple finished: offer the corpse to physics.
                    // No-op unless the `ragdoll` experimental flag is on - the
                    // animated corpse stays otherwise. (The is_dead branch
                    // queues nothing and a successful spawn removes this
                    // entity, so this cannot double-fire.)
                    //
                    // Completions dispatched in the same tick the killing blow
                    // landed belong to the clip the crumple interrupted, not
                    // the crumple itself (which just started) - swallow those,
                    // or the corpse would ragdoll from its still-standing pose.
                    if self.updates_since_death >= 1 {
                        Effect::SpawnCorpseRagdoll {
                            entity_id,
                            impact: self.death_impact,
                        }
                    } else {
                        Effect::NoEffect
                    }
                } else if is_killed(entity_id, world) {
                    // Fallback for kills that didn't arrive as a Damage
                    // message (the lethal-damage path enters death eagerly)
                    self.enter_death(world, entity_id)
                } else if self.took_damage {
                    self.took_damage = false;
                    // Hybrid wound clips are keyed under a combat-context
                    // level (receivewound -> meleecombat -> grunt -> ...),
                    // not directly under the creature tags, so a bare
                    // receivewound query matches nothing for them. Prefer
                    // the creature's directly-keyed clips (droids, humans
                    // keep their full sets, including damage-type variants);
                    // only when there are none, retry through the
                    // combat-context level (verified across creature types
                    // with `cargo dq motion`).
                    Effect::PlayAnimationBySchema {
                        entity_id,
                        motion_queries: vec![
                            vec![MotionQueryItem::new("receivewound")],
                            vec![
                                MotionQueryItem::new("receivewound"),
                                MotionQueryItem::new("meleecombat"),
                            ],
                        ],
                        selection_strategy: dark::motion::MotionQuerySelectionStrategy::Random,
                    }
                } else {
                    // (The behavior already saw this message via the
                    // unconditional forward at the top of handle_message -
                    // that's what lets a scripted Play action mark itself
                    // complete, even when the took_damage branch detours.)
                    let next_behavior = {
                        self.current_behavior
                            .borrow_mut()
                            .next_behavior(world, physics, entity_id)
                    };

                    match next_behavior {
                        NextBehavior::NoOpinion => (),
                        NextBehavior::Stay => (),
                        NextBehavior::Next(behavior) => {
                            self.current_behavior = behavior;
                        }
                    };

                    // A sequence that just finished is handed back to the
                    // alertness behavior by update() (whose steer call drains
                    // the sequence's final effects first); don't re-queue its
                    // animation here.
                    if self.current_behavior.borrow().scripted_state() == ScriptedState::Finished {
                        return Effect::NoEffect;
                    }
                    //self.current_behavior = Rc::new(IdleBehavior);
                    let is_locomotion = self.current_behavior.borrow().is_locomotion();
                    let selection_strategy = self.next_selection(is_locomotion);
                    let motion_query_items = self.current_behavior.borrow().animation();

                    // Check if this is an attack animation and play attack sound
                    let attack_sound_effect = if is_attack_animation(&motion_query_items) {
                        if let Some(voice_index) =
                            crate::scripts::speech_util::resolve_entity_voice_index(
                                world, entity_id,
                            )
                        {
                            Effect::PlaySpeech {
                                entity_id,
                                voice_index,
                                concept: "comattack".to_string(),
                                tags: vec![],
                            }
                        } else {
                            Effect::NoEffect
                        }
                    } else {
                        Effect::NoEffect
                    };

                    let queue_animation_effect = Effect::QueueAnimationBySchema {
                        entity_id,
                        motion_queries: vec![motion_query_items],
                        selection_strategy,
                        //tag: "idlegesture".to_owned(),
                        // motion_query_items: vec![
                        //     MotionQueryItem::new("search"),
                        //     MotionQueryItem::new("scan").optional(),
                        // -- Walk around items
                        // MotionQueryItem::new("locomote").optional(),
                        // MotionQueryItem::new("search").optional(),
                        // --

                        // Die
                        // MotionQueryItem::new("crumple").optional(),
                        // MotionQueryItem::new("grunt").optional(),
                        // MotionQueryItem::new("pipe").optional(),
                        // --

                        // --- Melee attack items
                        // MotionQueryItem::new("meleecombat").optional(),
                        // MotionQueryItem::new("attack").optional(),
                        // MotionQueryItem::new("direction").optional(),
                        // ---

                        // --- Ranged combat attack items
                        // MotionQueryItem::new("rangedcombat").optional(),
                        // MotionQueryItem::new("attack").optional(),
                        // MotionQueryItem::new("direction").optional(),
                        // ---

                        //MotionQueryItem::new("search").optional(),
                        // MotionQueryItem::new("locourgent").optional(),
                        //MotionQueryItem::new("attack"),
                        // MotionQueryItem::new("stand"),
                        //MotionQueryItem::new("direction").optional(),
                        //],
                    };

                    Effect::combine(vec![attack_sound_effect, queue_animation_effect])
                }
            }
            MessagePayload::AnimationFlagTriggered { motion_flags } => {
                if motion_flags.contains(MotionFlags::FIRE) {
                    // A killed monster's in-flight attack clip keeps playing
                    // until the death is processed - don't let it fire
                    if self.is_dead || is_killed(entity_id, world) {
                        return Effect::NoEffect;
                    }
                    fire_ranged_projectile(world, entity_id)
                // } else if motion_flags.contains(MotionFlags::END) {
                //     Effect::QueueAnimationBySchema {
                //         entity_id,
                //         motion_query_items: vec![MotionQueryItem::new("rangedcombat")],
                //         //     MotionQueryItem::new("rangedcombat".to_owned())),
                //         //     // "rangedcombat".to_owned(),
                //         //     // "attack".to_owned(),
                //         //     //"direction".to_owned(),
                //         // ],
                //     }
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

fn player_is_within_watch_obj(world: &World, entity_id: EntityId, radius: f32) -> bool {
    let u_player = world.borrow::<shipyard::UniqueView<PlayerInfo>>().unwrap();
    let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

    if let Ok(ent_pos) = v_current_pos.get(entity_id) {
        return ent_pos.position.distance(u_player.pos) <= radius;
    }

    false
}

/// Checks if the motion query items represent an attack animation
fn is_attack_animation(motion_query_items: &[MotionQueryItem]) -> bool {
    for item in motion_query_items {
        let tag = item.tag_name();
        if tag == "attack" || tag == "meleecombat" || tag == "rangedcombat" {
            return true;
        }
    }
    false
}
