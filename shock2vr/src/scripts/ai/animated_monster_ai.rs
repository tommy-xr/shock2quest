use std::{cell::RefCell, collections::HashSet};

use cgmath::{Deg, EuclideanSpace, MetricSpace, Quaternion, Rotation3, vec3, vec4};
use dark::{
    SCALE_FACTOR,
    motion::{MotionFlags, MotionQueryItem},
    properties::{
        AIAlertLevel, Link, PropAIAlertCap, PropAIAwareDelay, PropAISignalResponse, PropPosition,
    },
};
use rand::Rng;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::{GlobalPathfinding, GlobalTemplateIdMap, PlayerInfo},
    pathfinding::MovementHold,
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
    steering::{STALL_SECONDS, Steering, SteeringOutput},
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
/// A door is passable once its leaf has travelled at least this far. A leaf
/// that slides sideways never gains height, so height clearance alone would
/// hold an AI at it forever.
const DOOR_OPEN_FRACTION: f32 = 0.95;
/// Ceiling on the door wait, so a leaf that stops halfway (halted, jammed,
/// or frobbed shut again) can never park an AI in a doorway. It is also
/// deliberately SHORTER than the path-follower's stall window: a held body
/// makes no progress toward its waypoint, so a longer hold would read as a
/// wedge, blacklist the crossing the AI just opened, and route it back away
/// from the door. Longer than the slowest shipped leaf's travel (~1.6 s).
const DOOR_WAIT_TIMEOUT: f32 = 2.5;
/// How long a door has to keep the AI waiting before that reads as
/// impatience. A shipped sliding leaf clears in about half a second, and an
/// AI is not thwarted by a door that opens for it - gesturing at every one
/// would also park a body in the doorway (the gesture has no root motion)
/// long enough to dam the creatures queued behind it.
const FRUSTRATION_DOOR_WAIT_SECONDS: f32 = 1.75;
/// Rate limit on the frustration gesture, jittered per play so a knot of
/// AIs blocked on the same thing doesn't gesture in unison.
const FRUSTRATION_COOLDOWN_MIN: f32 = 20.0;
const FRUSTRATION_COOLDOWN_MAX: f32 = 45.0;
/// After giving up at a locked door, wait this long before re-frustrating -
/// bounds the "thwarted" gesture for an AI whose alert cap keeps it in a
/// pursuing state even after the give-up's alertness drop.
const DOOR_GIVEUP_COOLDOWN: f32 = 8.0;
/// How far into the death crumple the corpse hands off to physics. Near-
/// instant (a few frames) so the killing-blow impulse lands AT the kill -
/// any longer and the reaction reads as delayed. The crumple still starts
/// (its first frames shape the initial pose and root velocity, both of which
/// the rig inherits - see spawn_ragdoll), but physics owns the death from
/// here; the ragdoll spawns from the current pose, so there is no snap.
/// Raise this to let more of the authored death animation play before
/// physics takes over (at 0.5 the impulse visibly lags the shot).
const CRUMPLE_HANDOFF_SECONDS: f32 = 0.05;

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

/// Whether a creature that can currently see the player should turn to face
/// it, overriding the heading its own behavior asked for.
///
/// Sight is the only thing that escalates alertness, but the behaviors an
/// unaware creature runs steer by their own agenda - wander picks a random
/// destination, patrol follows its route, search walks to a stale position -
/// so a creature that spots the player promptly turns away, loses the
/// contact before it can escalate, and decays back to calm. It then stands
/// facing wherever it stopped, permanently blind to a player outside that
/// cone: the "never re-acquires after alertness decays" loop of #791.
/// Orienting on what it can see lets the normal escalation ladder finish.
///
/// Pursuing levels (Moderate and up) are excluded because their behaviors
/// already steer at their target, a running scripted sequence owns its
/// actor's heading, and a creature with no alertness config at all (an
/// apparition - see `build_config`) must never notice the player.
fn should_orient_on_target(
    processes_alertness: bool,
    level: AIAlertLevel,
    is_visible: bool,
    scripted: ScriptedState,
) -> bool {
    processes_alertness
        && is_visible
        && matches!(level, AIAlertLevel::Lowest | AIAlertLevel::Low)
        && scripted != ScriptedState::Running
}

/// Forward-speed multiplier for a given heading error: 1.0 facing the
/// travel direction, ramping down to a third by 60 degrees of error, then
/// to a standstill by 90.
///
/// Past a right angle the body turns in PLACE. Walking a reversal at a
/// third speed is a second-long arc that carries the body sideways into
/// whatever happens to be beside it - a railing, a door frame, the AI it
/// is trying to get around - which is exactly where patrol reversals wedge.
/// Turning is unaffected by the scale, so a stopped body still pivots and
/// walks off again the moment its error is back under 90.
/// Whether a door's leaf has travelled far enough for a creature of
/// `actor_height` to walk under (or past) it: either the leaf has finished
/// its travel, or it has risen clear of the creature's head. A closed leaf
/// starts at floor level, so its rise IS the height of the gap beneath it.
pub(crate) fn door_is_passable(fraction: f32, rise: f32, actor_height: f32) -> bool {
    fraction >= DOOR_OPEN_FRACTION || rise >= actor_height
}

/// Whether to show frustration now: only while actually blocked, only once
/// the rate limit has expired, never inside melee reach of the believed
/// target (an AI in a position to swing is fighting, not thwarted), and
/// never over a performance the level authored.
pub(crate) fn should_gesture_frustration(
    door_wait_seconds: Option<f32>,
    cooldown_remaining: f32,
    target_distance: Option<f32>,
    scripted: ScriptedState,
) -> bool {
    door_wait_seconds.is_some_and(|s| s >= FRUSTRATION_DOOR_WAIT_SECONDS)
        && frustration_gesture_allowed(cooldown_remaining, target_distance, scripted)
}

/// The limits every gesture path shares, whatever it is frustrated at: the
/// rate limit, no gesturing over a scripted performance, and none in the
/// player's face (a creature in reach of its target attacks, it does not
/// mime). The locked-door give-up goes through this too - two paths playing
/// the one performance must not take turns past the limit either claims.
pub(crate) fn frustration_gesture_allowed(
    cooldown_remaining: f32,
    target_distance: Option<f32>,
    scripted: ScriptedState,
) -> bool {
    cooldown_remaining <= 0.0
        && scripted != ScriptedState::Running
        && !matches!(target_distance, Some(d) if d <= MELEE_ATTACK_RANGE)
}

/// How long to wait for the applier to report which clip it picked. The report
/// comes back on the next frame, so this only bounds a pivot that resolves to
/// no clip at all and never reports one.
const TURN_CLIP_REPORT_TIMEOUT: f32 = 0.25;

/// How long past its authored length a turn clip may run before the pivot
/// gives up on ever seeing it complete.
const TURN_CLIP_OVERRUN_MARGIN: f32 = 0.25;

/// How early a completion may arrive and still be the turn clip's own. Any
/// other clip that preempted it completes far sooner than this (a run cycle is
/// a third of a second), and settling on one of those would swing the AI
/// through the authored turn with a neutral pose.
const TURN_CLIP_COMPLETION_SLACK: f32 = 0.25;

/// The longest a creature may stand still to perform a pivot. The stock turn
/// clips run 2.4-5.6 s; one long enough to read as a wedge (the reachability
/// harness calls holding one spot for 6 s stuck, and it is right to) is worse
/// than steering the turn, so it is simply not picked.
const TURN_CLIP_MAX_SECONDS: f32 = 4.0;

/// How long a pivot is barred for. Two pivots back to back would hold the
/// creature still for longer than either of them, so a pivot has to walk (or
/// at least steer) it off before the next one - and a stall arms the same
/// window, which has to outlast the path follower's own recovery.
const TURN_CLIP_COOLDOWN: f32 = 3.0;

/// Fallback for a clip that authors no blend length. The turn clip's pose
/// swings back to neutral over the blend as the next clip fades in, so the
/// entity takes the authored facing change over exactly the same window and
/// the creature's visible facing never jumps.
const TURN_CLIP_SETTLE_SECONDS: f32 = 0.5;

/// A pivot being played as the creature's own authored turn clip. The clip's
/// POSE does the visible turning, so the script holds its heading while the
/// clip runs and only then takes the authored facing change.
enum TurnClip {
    /// Asked for; waiting for the applier to say which clip it picked.
    Requested { remaining: f32 },
    /// Playing: the pose is doing the turning, so the entity holds still.
    Playing {
        turn: Deg<f32>,
        remaining: f32,
        /// The clip's authored blend length - the window the pose takes to
        /// swing back to neutral once it ends.
        blend: f32,
    },
    /// Done: the entity takes the clip's authored facing change across the
    /// blend that swings the pose back to neutral.
    Settling {
        turn: Deg<f32>,
        remaining: f32,
        /// The blend window this is spread over.
        window: f32,
    },
}

/// The "thwarted" performance. The motion database files the tag under
/// `discover`, so the bare tag resolves to nothing at all - the query has to
/// name the branch as well as the leaf.
fn frustration_gesture(entity_id: EntityId) -> Effect {
    Effect::PlayAnimationBySchema {
        entity_id,
        motion_queries: vec![vec![
            MotionQueryItem::new("discover"),
            MotionQueryItem::new("thwarted"),
        ]],
        selection_strategy: dark::motion::MotionQuerySelectionStrategy::Random,
    }
}

/// Tell the path follower this AI is standing still on purpose, so the hold
/// suspends its no-progress accounting instead of reading as a wedge. Written
/// on the same channel the follower publishes its own state on, and BEFORE the
/// steer that reads it.
fn publish_movement_hold(world: &World, entity_id: EntityId, hold: MovementHold) {
    if let Some(service) = world
        .borrow::<UniqueView<GlobalPathfinding>>()
        .ok()
        .and_then(|g| g.0.clone())
    {
        service.record_movement_hold(entity_id.inner(), hold);
    }
}

/// How long path-following has been making no progress, as published by the
/// steering strategy - and already frozen for the frames this AI published a
/// hold for. Zero for an AI that isn't following a route.
fn path_stall_seconds(world: &World, entity_id: EntityId) -> f32 {
    world
        .borrow::<UniqueView<GlobalPathfinding>>()
        .ok()
        .and_then(|g| g.0.clone())
        .and_then(|service| service.ai_steering(entity_id.inner()))
        .map(|steering| steering.stall_seconds)
        .unwrap_or(0.0)
}

/// Why this AI is standing still this frame, if it is. A door wait outranks a
/// pivot: the AI can be doing both, and the door is what it is waiting on.
fn movement_hold(door_wait_seconds: Option<f32>, pivoting: bool) -> MovementHold {
    if door_wait_seconds.is_some() {
        MovementHold::DoorWait
    } else if pivoting {
        MovementHold::Pivot
    } else {
        MovementHold::None
    }
}

pub(crate) fn locomotion_scale_for_heading_error(delta: Deg<f32>) -> f32 {
    const TURN_SLOW_ANGLE: f32 = 60.0;
    const TURN_IN_PLACE_ANGLE: f32 = 90.0;
    const TURN_SLOW_SCALE: f32 = 0.33;
    let error = delta.0.abs();
    if error >= TURN_IN_PLACE_ANGLE {
        0.0
    } else if error >= TURN_SLOW_ANGLE {
        let past_slow = (error - TURN_SLOW_ANGLE) / (TURN_IN_PLACE_ANGLE - TURN_SLOW_ANGLE);
        TURN_SLOW_SCALE * (1.0 - past_slow)
    } else {
        1.0 - (error / TURN_SLOW_ANGLE) * (1.0 - TURN_SLOW_SCALE)
    }
}

pub struct AnimatedMonsterAI {
    last_hit_sensor: Option<EntityId>,
    current_behavior: Box<RefCell<dyn Behavior>>,
    current_heading: Deg<f32>,
    is_dead: bool,
    /// Seconds of sim time since entering death; drives the timed
    /// crumple->ragdoll handoff.
    death_elapsed: f32,
    /// The handoff effect is emitted exactly once (a failed spawn - e.g. a
    /// non-ragdollable model - must not re-emit every frame).
    handoff_emitted: bool,
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
    /// The door this AI is standing off from, and how long it has waited, while
    /// the leaf travels clear (see `update_door_wait`)
    door_wait: Option<(EntityId, f32)>,
    /// Seconds left before this AI may show frustration again
    frustration_cooldown: f32,
    /// Pinned alertness (DebugForceChase): treated as permanent sight of the
    /// player - no decay, live target position - until cleared
    alertness_pinned: bool,
    /// The authored turn clip playing for the current pivot, if any
    turn_clip: Option<TurnClip>,
    /// Seconds left before this AI may perform another pivot
    turn_cooldown: f32,
}

impl AnimatedMonsterAI {
    pub fn idle() -> AnimatedMonsterAI {
        AnimatedMonsterAI {
            is_dead: false,
            alertness_pinned: false,
            death_elapsed: 0.0,
            handoff_emitted: false,
            death_impact: None,
            took_damage: false,
            current_behavior: Box::new(RefCell::new(IdleBehavior::new())),
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
            door_wait: None,
            frustration_cooldown: 0.0,
            turn_clip: None,
            turn_cooldown: 0.0,
        }
    }

    pub fn new() -> AnimatedMonsterAI {
        AnimatedMonsterAI {
            is_dead: false,
            alertness_pinned: false,
            death_elapsed: 0.0,
            handoff_emitted: false,
            death_impact: None,
            took_damage: false,
            // Start with IdleBehavior - alertness will drive behavior changes
            current_behavior: Box::new(RefCell::new(IdleBehavior::new())),
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
            door_wait: None,
            frustration_cooldown: 0.0,
            turn_clip: None,
            turn_cooldown: 0.0,
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
            to_two: (DEFAULT_ESCALATE_SECONDS * 1000.0) as i32,
            to_three: (DEFAULT_ESCALATE_SECONDS * 1000.0) as i32,
            two_reuse: (DEFAULT_DECAY_SECONDS * 1000.0) as i32,
            three_reuse: (DEFAULT_DECAY_SECONDS * 1000.0) as i32,
            ignore_range: (DEFAULT_DECAY_SECONDS * 1000.0) as i32,
        };

        let aware_delay = v_aware_delay
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(default_aware_delay);

        let timings = AlertnessTimings::from_aware_delay(&aware_delay);

        Some(MonsterConfig { alert_cap, timings })
    }

    /// Get the appropriate behavior for the current alertness level.
    ///
    /// `physics` is optional because `initialize` has none: without it the
    /// High arm falls back to its chase, which is where a High AI out of
    /// reach belongs anyway - the attack swap happens on arrival, through
    /// `ChaseBehavior::next_behavior`.
    fn behavior_for_alertness(
        &self,
        world: &World,
        physics: Option<&PhysicsWorld>,
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
                physics
                    .and_then(|physics| attack_behavior_for_distance(world, physics, entity_id))
                    .unwrap_or_else(|| Box::new(RefCell::new(ChaseBehavior::new())))
            }
        }
    }

    /// The behavior for a fully-calm (Lowest) AI: patrol an authored route if
    /// it is flagged to and a route exists, otherwise stand idle. Falls back to
    /// idle when the mission has no patrol network reachable from here.
    fn idle_behavior(&self, world: &World, entity_id: EntityId) -> Box<RefCell<dyn Behavior>> {
        // A creature excluded from awareness entirely (an apparition - see
        // `build_config`) can never see the player, so it has nothing to
        // look around for and no reason to leave its mark: it holds the
        // heading its authored performance was staged with. This also covers
        // the handback after a scripted sequence finishes.
        if self.config.is_none() {
            return Box::new(RefCell::new(IdleBehavior::holding_post()));
        }
        if is_patroller(world, entity_id) {
            let (position, _) = get_position_and_forward(world, entity_id);
            if let Some((point, goal)) = current_patrol_point(world, entity_id)
                .or_else(|| nearest_patrol_point(world, position.to_vec()))
            {
                return Box::new(RefCell::new(PatrolBehavior::new(point, goal)));
            }
        }
        Box::new(RefCell::new(IdleBehavior::new()))
    }

    fn apply_steering_output(
        &mut self,
        steering_output: SteeringOutput,
        time: &Time,
        entity_id: EntityId,
        hold_position: bool,
        // An authored turn clip owns the heading for the length of the pivot
        // (see `update_turn_clip`): the script neither slews it nor moves the
        // body, and re-asserting the held heading every frame keeps the clip's
        // own root rotation from turning the entity on top of its pose.
        heading_held: bool,
    ) -> Effect {
        let turn_velocity = self.current_behavior.borrow().turn_speed().0;
        let delta =
            clamp_to_minimal_delta_angle(steering_output.desired_heading - self.current_heading);

        let turn_amount = if heading_held {
            0.0
        } else if delta.0 < 0.0 {
            (-turn_velocity * time.elapsed.as_secs_f32()).max(delta.0)
        } else {
            (turn_velocity * time.elapsed.as_secs_f32()).min(delta.0)
        };

        self.current_heading = Deg(self.current_heading.0 + turn_amount);

        // Couple forward speed to heading error so the body doesn't arc at
        // full stride while the heading catches up (the cause of orbiting a
        // close target): full speed facing the travel direction, ramping to
        // a third by 60 degrees of error and to a standstill by 90.
        // Holding for a door still turns (so the AI keeps facing the way
        // through) - only the stride is cut.
        let scale = if hold_position || heading_held {
            0.0
        } else {
            locomotion_scale_for_heading_error(delta)
        };

        Effect::Multiple(vec![
            Effect::SetRotation {
                entity_id,
                rotation: Quaternion::from_angle_y(self.current_heading),
            },
            Effect::SetAIProperty {
                entity_id,
                update: crate::scripts::AIPropertyUpdate::LocomotionScale { scale },
            },
        ])
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

        let maybe_hit_result = physics.ray_cast2_as_actor(
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

        self.current_behavior = self.behavior_for_alertness(world, Some(physics), entity_id);
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
                motion_queries: self.current_behavior.borrow().animation_queries(),
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
        // Anchor the ragdoll-handoff timer to the crumple's actual start
        // (kills arriving via the AnimationCompleted fallback enter death
        // later than they were dealt).
        self.death_elapsed = 0.0;

        // A creature that authors Corpse/Flinderize links bursts instead of
        // leaving a body: droids link a `Corpse` explosion (plus `Flinderize`
        // parts), and organics that gib - the Overlord's viral explosion and
        // its limb/organ flinders - do the same. `Effect::SlayEntity` spawns
        // those links, plays the death environmental sound and removes the
        // entity (its handler skips the ragdoll for link deaths), so there is
        // no crumple animation and no death speech either. Pre-mark the
        // handoff so the removed entity is never offered to physics if it
        // survives a frame.
        if crate::scripts::script_util::has_death_links(world, entity_id) {
            self.handoff_emitted = true;
            return Effect::SlayEntity { entity_id };
        }

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

    /// Hold position while a door this AI just opened travels clear, rather
    /// than walking into a leaf still crossing the doorway. Returns whether
    /// the AI is holding this frame.
    fn update_door_wait(&mut self, world: &World, entity_id: EntityId, time: &Time) -> Option<f32> {
        let (door_ent, waited) = self.door_wait?;
        // Only a pursuing behavior opens doors, so only a pursuing behavior
        // waits at one; anything else (a scripted performance takes over, the
        // AI calms down) walks away from the doorway rather than holding.
        if !matches!(self.current_behavior.borrow().name(), "Chase" | "Search") {
            self.door_wait = None;
            return None;
        }
        let waited = waited + time.elapsed.as_secs_f32();
        // No progress to read (not a door any more, or one that cannot move)
        // means there is nothing to wait for.
        let progress = script_util::door_open_progress(world, door_ent);
        let clear = match progress {
            None => true,
            Some((fraction, rise)) => {
                door_is_passable(fraction, rise, creature_height(world, entity_id))
            }
        };
        if clear || waited >= DOOR_WAIT_TIMEOUT {
            tracing::debug!(
                "ai {:?} resumes past door {:?} after {:.2}s (clear: {}, progress: {:?})",
                entity_id,
                door_ent,
                waited,
                clear,
                progress
            );
            self.door_wait = None;
            return None;
        }
        self.door_wait = Some((door_ent, waited));
        Some(waited)
    }

    /// A pivot costs seconds of standing still, so it belongs to a creature
    /// that is getting somewhere: one scrambling out of a wedge only makes the
    /// wedge longer by standing through a turn clip. A stall therefore arms the
    /// pivot cooldown, whose window outlasts the path follower's own recovery -
    /// the re-path that asks for the sidestep, and so reads as a pivot, lands
    /// after the stall clock has already been reset.
    ///
    /// `following_a_route` gates the reading, not just the arming: the follower
    /// publishes its stall clock only while it steers, and the last value it
    /// published outlives it.
    fn bar_pivot_while_stalled(&mut self, stall_seconds: f32, following_a_route: bool) {
        if following_a_route && stall_seconds >= STALL_SECONDS {
            self.turn_cooldown = self.turn_cooldown.max(TURN_CLIP_COOLDOWN);
        }
    }

    /// Pivot with the creature's own authored turn clip rather than sliding
    /// the heading around under a walk cycle. Returns the effect to emit and
    /// whether the pivot owns the heading this frame.
    ///
    /// The clip's pose does the visible turning, so the script holds still
    /// while it plays and takes the authored facing change afterwards, spread
    /// over the blend that swings the pose back to neutral.
    fn update_turn_clip(
        &mut self,
        entity_id: EntityId,
        desired_heading: Deg<f32>,
        time: &Time,
        may_start: bool,
    ) -> (Effect, bool) {
        let elapsed = time.elapsed.as_secs_f32();
        let delta = clamp_to_minimal_delta_angle(desired_heading - self.current_heading);

        match &mut self.turn_clip {
            None => {
                self.turn_cooldown = (self.turn_cooldown - elapsed).max(0.0);
                // Only once the heading error has already stopped the body: a
                // pivot the creature can walk through needs no clip, and the
                // shortest stock turn clip is a quarter-circle anyway.
                let stopped = locomotion_scale_for_heading_error(delta) <= 0.0;
                if !(may_start && stopped && self.turn_cooldown <= 0.0) {
                    return (Effect::NoEffect, false);
                }
                tracing::debug!("ai {:?} pivots {:?}", entity_id, delta);
                self.turn_clip = Some(TurnClip::Requested {
                    remaining: TURN_CLIP_REPORT_TIMEOUT,
                });
                (
                    Effect::PlayTurnClip {
                        entity_id,
                        delta,
                        max_seconds: TURN_CLIP_MAX_SECONDS,
                    },
                    true,
                )
            }
            Some(TurnClip::Requested { remaining }) => {
                *remaining -= elapsed;
                if *remaining > 0.0 {
                    return (Effect::NoEffect, true);
                }
                // No clip covers this pivot - it is bigger than the ones the
                // creature can afford, or its schema has none. Steer the turn,
                // and let the cooldown space out the asking.
                tracing::debug!("ai {:?} has no clip for its pivot", entity_id);
                self.turn_clip = None;
                self.turn_cooldown = TURN_CLIP_COOLDOWN;
                (Effect::NoEffect, false)
            }
            Some(TurnClip::Playing { remaining, .. }) => {
                *remaining -= elapsed;
                if *remaining > 0.0 {
                    return (Effect::NoEffect, true);
                }
                // The completion never came - something preempted the clip.
                tracing::debug!("ai {:?} lost its turn clip", entity_id);
                self.turn_clip = None;
                self.turn_cooldown = TURN_CLIP_COOLDOWN;
                (Effect::NoEffect, false)
            }
            Some(TurnClip::Settling {
                turn,
                remaining,
                window,
            }) => {
                let step = elapsed.min(*remaining);
                let turn = *turn;
                let window = *window;
                *remaining -= step;
                // Rounding leaves a sliver of the last frame behind; anything
                // this short is the end of the blend, not another frame of it.
                let finished = *remaining <= 1e-4;
                self.current_heading = Deg(self.current_heading.0 + turn.0 * step / window);
                if finished {
                    tracing::debug!(
                        "ai {:?} finished its pivot facing {:?}",
                        entity_id,
                        self.current_heading
                    );
                    self.turn_clip = None;
                    self.turn_cooldown = TURN_CLIP_COOLDOWN;
                }
                (Effect::NoEffect, true)
            }
        }
    }

    /// Jittered rate limit, so a knot of AIs blocked on the same thing does
    /// not gesture in unison.
    fn next_frustration_cooldown(&self) -> f32 {
        rand::thread_rng().gen_range(FRUSTRATION_COOLDOWN_MIN..FRUSTRATION_COOLDOWN_MAX)
    }

    /// Show impatience at a door still opening. Only at a door: the gesture
    /// fits inside a hold the AI is already taking, whereas playing it on a
    /// stall would add seconds of standing still to a creature that is trying
    /// to get out of a wedge. The gesture is an animation only - it plays over
    /// the wait without extending it.
    fn update_frustration(
        &mut self,
        world: &World,
        entity_id: EntityId,
        door_wait_seconds: Option<f32>,
        started_a_clip: bool,
        time: &Time,
    ) -> Effect {
        // The rate limit ticks whether or not a gesture is due, so a
        // suppressed frame doesn't stretch it.
        self.frustration_cooldown =
            (self.frustration_cooldown - time.elapsed.as_secs_f32()).max(0.0);
        if started_a_clip {
            return Effect::NoEffect;
        }
        if !should_gesture_frustration(
            door_wait_seconds,
            self.frustration_cooldown,
            chase_target_distance(world, entity_id),
            self.current_behavior.borrow().scripted_state(),
        ) {
            return Effect::NoEffect;
        }
        self.frustration_cooldown = self.next_frustration_cooldown();
        tracing::debug!(
            "ai {:?} shows frustration (door wait: {:?}s)",
            entity_id,
            door_wait_seconds
        );
        frustration_gesture(entity_id)
    }

    /// The thwarted performance, if the shared limits allow it right now -
    /// arming the rate limit when it does. Every gesture path goes through
    /// this or `update_frustration`, which applies the same limits.
    fn try_frustration_gesture(&mut self, world: &World, entity_id: EntityId) -> Effect {
        if !frustration_gesture_allowed(
            self.frustration_cooldown,
            chase_target_distance(world, entity_id),
            self.current_behavior.borrow().scripted_state(),
        ) {
            return Effect::NoEffect;
        }
        self.frustration_cooldown = self.next_frustration_cooldown();
        frustration_gesture(entity_id)
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
                // Giving up on the door also gives up waiting for it - the
                // wander this drops to has no business standing at it.
                self.door_wait = None;
                // Asked for before the downgrade, so the limits are read
                // against the behavior that was actually thwarted.
                let gesture = self.try_frustration_gesture(world, entity_id);
                let downgrade = if matches!(gesture, Effect::NoEffect) {
                    // Nothing to carry the change: start the wander's own
                    // clip, as every other behavior change does.
                    self.force_alertness(AIAlertLevel::Low, world, physics, entity_id)
                } else {
                    // State-only downgrade: the thwarted gesture replaces the
                    // queue this frame (a second Play here would blend from an
                    // unseen frame-0 pose and waste a clip load); the
                    // completion handler starts the Low behavior's clip after
                    // the gesture.
                    self.force_alertness_state(AIAlertLevel::Low, world, physics, entity_id)
                };
                return Effect::combine(vec![downgrade, gesture]);
            }
            // Unlocked: open it and keep chasing through. The longer cooldown
            // avoids re-sending TurnOn (and replaying the open sound) while
            // the door is still swinging.
            self.door_cooldown = DOOR_INTERACT_COOLDOWN;
            // Stand off until the leaf has travelled clear (#1255): the AI
            // used to keep walking into a leaf still crossing the doorway.
            // Re-sending TurnOn to a leaf that has not left its closed half
            // is idempotent, but restarting the clock here is not: the wait
            // would never age out and a jammed door could hold the AI for
            // good. Only a DIFFERENT door starts a fresh wait.
            if !matches!(self.door_wait, Some((waiting_on, _)) if waiting_on == door_ent) {
                tracing::debug!("ai {:?} waits for door {:?} to clear", entity_id, door_ent);
                self.door_wait = Some((door_ent, 0.0));
            }
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

        // Save/load rebuilds scripts after restoring serialized hit points. A
        // killed monster must start in its inert state: queueing the fresh
        // script's idle clip would stand the corpse up, then its completion
        // would enter the normal death path and replay the crumple + speech.
        if is_killed(entity_id, world) {
            self.current_behavior = Box::new(RefCell::new(DeadBehavior {}));
            self.is_dead = true;
            // This corpse did not enter death in this runtime, so it must not
            // run the timed animated-crumple -> ragdoll handoff either.
            self.handoff_emitted = true;
            return self.publish_behavior(entity_id);
        }

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

        // Pick the behavior matching that starting alertness now, rather than
        // only on the first transition. A behavior is otherwise chosen only
        // when alertness CHANGES level, and both constructors seed a plain
        // `IdleBehavior`, so a creature that spawns calm and is never alerted
        // keeps that idle for the whole mission - including one flagged to
        // patrol, which then never takes a step of its authored route (#807).
        // (Selecting AFTER seeding alertness also keeps the two in step for a
        // `P$AI_AlertC` whose `min_level` is above `Lowest`; every shipped
        // entry has min_level = Lowest, so in practice every creature still
        // starts on the calm arm.)
        //
        // This also holds a creature excluded from awareness entirely (an
        // apparition - see `build_config`) still, which the previous
        // `config.is_none()` special case did: `idle_behavior` gives it the
        // non-scanning idle rather than the constructor's scanning one.
        self.current_behavior = self.behavior_for_alertness(world, None, entity_id);

        let is_locomotion = self.current_behavior.borrow().is_locomotion();
        let selection_strategy = self.next_selection(is_locomotion);
        let animation_effect = Effect::QueueAnimationBySchema {
            entity_id,
            motion_queries: self.current_behavior.borrow().animation_queries(),
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
            // A corpse recreated by save/load also latches is_dead so stale
            // animation completions stay inert, but initialize() pre-marks
            // its handoff as emitted: it must not ragdoll-ify from whatever
            // pose the save restored.
            if self.is_dead {
                self.death_elapsed += time.elapsed.as_secs_f32();
            }
            // Near-instant handoff: once the death animation has had a few
            // frames, offer the corpse to physics. With ragdolls disabled the
            // handler makes the animated capsule non-blocking; a successful
            // ragdoll spawn removes this entity, so at most one emission ever
            // matters.
            let handoff_effect = if self.is_dead
                && !self.handoff_emitted
                && self.death_elapsed >= CRUMPLE_HANDOFF_SECONDS
            {
                self.handoff_emitted = true;
                Effect::SpawnCorpseRagdoll {
                    entity_id,
                    impact: self.death_impact,
                }
            } else {
                Effect::NoEffect
            };
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
                handoff_effect,
                sensor_release_effect,
                self.publish_behavior(entity_id),
            ]);
        }

        let delta = time.elapsed.as_secs_f32();

        // Monster rotation is set directly via Effect::SetRotation, so pose.rotation
        // already contains the heading. Pass Deg(0.0) to avoid applying it twice.
        use super::ai_util::MONSTER_FOV_HALF_ANGLE;
        // A pinned alertness (DebugForceChase) acts as permanent sight of
        // the player: no decay, and the last-known position tracks them live
        let is_visible = self.alertness_pinned
            || is_player_visible_in_fov(
                entity_id,
                world,
                physics,
                Deg(0.0),
                MONSTER_FOV_HALF_ANGLE,
            );

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
                // protect all sequences.) A behavior mid-commitment (a
                // protocol droid's lit fuse) declines preemption for the same
                // reason: re-selecting it would restart the timer it is
                // counting down.
                let holds_against_alertness = self.current_behavior.borrow().scripted_state()
                    == ScriptedState::Running
                    || !self.current_behavior.borrow().preempted_by_alertness();
                if holds_against_alertness {
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
                            None => {
                                Some(self.behavior_for_alertness(world, Some(physics), entity_id))
                            }
                        }
                    } else {
                        Some(self.behavior_for_alertness(world, Some(physics), entity_id))
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
                            motion_queries: self.current_behavior.borrow().animation_queries(),
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
                    motion_queries: self.current_behavior.borrow().animation_queries(),
                    selection_strategy,
                };
            }
        }

        // Both deliberate standstills are decided BEFORE the steer that
        // reads them: a hold published afterwards would leave the frame it
        // covers counted as a frame of no progress. The hold suspends the
        // follower's progress accounting only - the behavior still steers
        // through it, so heading, whiskers, crowd repel and the route itself
        // stay live while the creature stands.
        let door_wait_seconds = self.update_door_wait(world, entity_id, time);
        let pivoting = self.turn_clip.is_some();
        publish_movement_hold(world, entity_id, movement_hold(door_wait_seconds, pivoting));
        let (steering_output, steering_effects) = self
            .current_behavior
            .borrow_mut()
            .steer(self.current_heading, world, physics, entity_id, time)
            .unwrap_or((
                Steering::from_current(self.current_heading),
                Effect::NoEffect,
            ));

        // Keep looking at a player this creature can actually see, so the
        // sighting survives long enough to escalate (see
        // `should_orient_on_target`). The behavior's own steering effects
        // still apply - only the heading is overridden.
        let steering_output = if !pivoting
            && should_orient_on_target(
                self.config.is_some(),
                self.alertness.current_level,
                is_visible,
                self.current_behavior.borrow().scripted_state(),
            ) {
            orient_toward_player(world, entity_id).unwrap_or(steering_output)
        } else {
            steering_output
        };

        // A pivot big enough to stop the body is played as an authored turn
        // clip. Same gate as the frustration gesture: never over a scripted
        // performance, and never while standing off from a door.
        let may_turn = self.current_behavior.borrow().is_locomotion()
            && self.current_behavior.borrow().scripted_state() != ScriptedState::Running
            && door_wait_seconds.is_none();
        // `is_locomotion` is exactly the set of behaviors that steer with the
        // path follower, so under `may_turn` the stall clock this reads was
        // published by the steer above.
        let stall_seconds = path_stall_seconds(world, entity_id);
        self.bar_pivot_while_stalled(stall_seconds, may_turn);
        let (turn_clip_effect, heading_held) =
            self.update_turn_clip(entity_id, steering_output.desired_heading, time, may_turn);
        let rotation_effect = self.apply_steering_output(
            steering_output,
            time,
            entity_id,
            door_wait_seconds.is_some(),
            heading_held,
        );

        // A finished scripted sequence (its final queued effects were drained
        // by the steer above - scripted_state only reports Finished once they
        // are) hands control back to the alertness-appropriate behavior. This
        // runs every frame, so it also covers sequences ended by the
        // watchdog, where no further AnimationCompleted may ever arrive.
        let handback_effect = if self.current_behavior.borrow().scripted_state()
            == ScriptedState::Finished
        {
            self.current_behavior = self.behavior_for_alertness(world, Some(physics), entity_id);
            let is_locomotion = self.current_behavior.borrow().is_locomotion();
            let selection_strategy = self.next_selection(is_locomotion);
            Effect::PlayAnimationBySchema {
                entity_id,
                motion_queries: self.current_behavior.borrow().animation_queries(),
                selection_strategy,
            }
        } else {
            Effect::NoEffect
        };

        // Emitted last of the animation effects, so it is checked against
        // every clip this frame may already have started: the gesture PLAYS
        // (it does not queue), so firing it on the same frame as a behavior's
        // own clip would drop that clip for a frame. Suppressing rather than
        // reordering also leaves the rate limit unarmed, so the gesture
        // simply comes on a later frame.
        // A pivot is a clip too, for its whole length - not just the frame it
        // was asked for: gesturing over it would preempt the turn.
        let started_a_clip = !matches!(behavior_change_effect, Effect::NoEffect)
            || !matches!(handback_effect, Effect::NoEffect)
            || !matches!(turn_clip_effect, Effect::NoEffect)
            || self.turn_clip.is_some();
        let frustration_effect =
            self.update_frustration(world, entity_id, door_wait_seconds, started_a_clip, time);

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
            frustration_effect,
            turn_clip_effect,
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
                        motion_queries: self.current_behavior.borrow().animation_queries(),
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
                        motion_queries: self.current_behavior.borrow().animation_queries(),
                        selection_strategy,
                    }
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::SetAlertness { level, pin } => {
                // Dead AIs stay dead - a corpse keeps a live script, and the
                // broadcast (DebugAlertAll) reaches every creature
                if self.is_dead || is_killed(entity_id, world) {
                    return Effect::NoEffect;
                }
                // Pinned (DebugForceChase): the level never decays and the
                // AI hunts the player's live position until a non-pinned
                // SetAlertness (DebugCalmAll) clears it
                self.alertness_pinned = *pin;
                // The behavior reset is unconditional, even when the level
                // didn't change - forcing is a debug reset, so it also
                // cancels scripted sequences
                self.force_alertness(*level, world, physics, entity_id)
            }
            MessagePayload::TurnClipStarted {
                turn,
                duration,
                blend,
            } => {
                if matches!(self.turn_clip, Some(TurnClip::Requested { .. })) {
                    // Hold the heading for as long as the clip actually runs,
                    // plus a margin: the completion is what normally ends the
                    // pivot, this only bounds a clip that is preempted.
                    self.turn_clip = Some(TurnClip::Playing {
                        turn: *turn,
                        remaining: duration + TURN_CLIP_OVERRUN_MARGIN,
                        blend: if *blend > 0.0 {
                            *blend
                        } else {
                            TURN_CLIP_SETTLE_SECONDS
                        },
                    });
                }
                Effect::NoEffect
            }
            MessagePayload::AnimationCompleted => {
                // The turn clip is done: take its authored facing change as
                // the pose blends back to neutral.
                //
                // Only the turn clip's OWN completion may do that. The
                // behavior's clip is re-queued the moment the pivot ends and
                // completes a fraction of a second later, and anything that
                // preempts the turn clip (a wound reaction, an alertness
                // change) completes long before it would have - settling on
                // one of those swings the AI through the authored turn with a
                // neutral pose. `remaining` counts down the clip's own
                // runtime, so only a completion arriving near the end of it is
                // the clip's own; anything earlier means the pivot's clip is
                // gone.
                if let Some(TurnClip::Playing {
                    turn,
                    remaining,
                    blend,
                }) = self.turn_clip
                {
                    if remaining <= TURN_CLIP_OVERRUN_MARGIN + TURN_CLIP_COMPLETION_SLACK {
                        self.turn_clip = Some(TurnClip::Settling {
                            turn,
                            remaining: blend,
                            window: blend,
                        });
                    } else {
                        tracing::debug!("ai {:?} lost its turn clip to another", entity_id);
                        self.turn_clip = None;
                        self.turn_cooldown = TURN_CLIP_COOLDOWN;
                    }
                }
                if self.is_dead {
                    // The crumple->ragdoll handoff is timed from update()
                    // (CRUMPLE_HANDOFF_SECONDS), not completion-driven - a
                    // stale completion from the clip the crumple interrupted
                    // could otherwise hand off from a still-standing pose.
                    Effect::NoEffect
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
                    let motion_queries = self.current_behavior.borrow().animation_queries();

                    // Check if this is an attack animation and play attack sound
                    let attack_sound_effect = if motion_queries
                        .iter()
                        .any(|query| is_attack_animation(query))
                    {
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
                        motion_queries,
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
                // A foot reached its authored plant frame. Resolved outside the
                // chain below because `AnimationPlayer::update` unions every
                // flag frame crossed in the tick: on a hitch one message can
                // carry `FIRE | LEFT_FOOT_STEP`, and an `else if` would drop
                // the step - or, worse, suppress it via the dead-creature guard
                // on an arm that has nothing to do with footsteps.
                //
                // Deliberately gated on neither locomotion nor life: the
                // shipped clips author foot plants on idle weight-shifts,
                // turns, staggers and death collapses too, and each is a real
                // foot hitting the deck. The walk/run clips are per-half-step
                // (`ogpwlklt` / `ogpwlkrt`), one plant each, so a walk cycle
                // produces exactly two footsteps.
                let footstep = if motion_flags
                    .intersects(MotionFlags::LEFT_FOOT_STEP | MotionFlags::RIGHT_FOOT_STEP)
                {
                    script_util::play_footstep_sound(world, entity_id)
                } else {
                    Effect::NoEffect
                };

                // A killed monster's in-flight attack clip keeps playing until
                // the death is processed - don't let it fire or connect.
                let can_act = !(self.is_dead || is_killed(entity_id, world));

                let acted = if motion_flags.contains(MotionFlags::FIRE) && can_act {
                    fire_ranged_projectile(world, physics, entity_id)
                } else if motion_flags.contains(MotionFlags::MELEE_CONTACT_START) && can_act {
                    // The swing reached its authored contact frame - resolve
                    // the hit through the attacker's melee weapon archetype.
                    super::ai_util::melee_contact_attack(world, entity_id, physics)
                } else {
                    Effect::NoEffect
                };

                Effect::combine(vec![acted, footstep])
            }
            _ => Effect::NoEffect,
        }
    }
}

/// Steering that faces the player's current position.
fn orient_toward_player(world: &World, entity_id: EntityId) -> Option<SteeringOutput> {
    let player_pos = world.borrow::<UniqueView<PlayerInfo>>().ok()?.pos;
    let (position, _) = get_position_and_forward(world, entity_id);
    Some(Steering::turn_to_point(
        position,
        crate::util::vec3_to_point3(player_pos),
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::Transform;
    use shipyard::{IntoIter, IntoWithId};

    /// A live monster at the origin, turned `heading` degrees off +Z, with the
    /// player standing 10 units down +Z. The physics world is empty, so
    /// line-of-sight is always clear and only the FOV cone gates visibility.
    fn world_with_monster_and_player(heading: Deg<f32>) -> (World, EntityId) {
        world_with_creature_and_player(heading, "creaturetype hybrid")
    }

    fn world_with_creature_and_player(heading: Deg<f32>, class_tag: &str) -> (World, EntityId) {
        let mut world = World::new();
        let rotation = Quaternion::from_angle_y(heading);
        let entity_id = world.add_entity((
            dark::properties::PropHitPoints { hit_points: 12 },
            dark::properties::PropClassTag::from_string(class_tag),
            // The shipped flesh-creature archetypes all carry this; the
            // footstep schema reads it as the creature's own foot material.
            dark::properties::PropMaterial("Material FleshTarget".to_owned()),
            dark::properties::PropPosition {
                position: vec3(0.0, 0.0, 0.0),
                rotation,
                cell: 0,
            },
            crate::runtime_props::RuntimePropTransform(cgmath::Matrix4::from(rotation)),
        ));
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 10.0),
            rotation: Quaternion::from_angle_y(Deg(0.0)),
            entity_id: EntityId::dead(),
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: EntityId::dead(),
        });
        (world, entity_id)
    }

    /// The yaw (degrees off +Z) the monster asked the world to set this frame.
    fn commanded_heading(effects: &[Effect]) -> Option<Deg<f32>> {
        effects.iter().find_map(|effect| match effect {
            Effect::SetRotation { rotation, .. } => {
                Some(Deg(2.0 * rotation.v.y.atan2(rotation.s).to_degrees()))
            }
            _ => None,
        })
    }

    fn step(monster: &mut AnimatedMonsterAI, world: &World, entity_id: EntityId) -> Vec<Effect> {
        let physics = PhysicsWorld::new();
        let time = Time {
            elapsed: std::time::Duration::from_millis(100),
            total: std::time::Duration::from_millis(100),
        };
        Effect::flatten(vec![monster.update(entity_id, world, &physics, &time)])
    }

    /// #791: a calm creature that can see the player must turn to look at it.
    /// Its idle/wander/patrol steering has its own agenda, so without this the
    /// sighting is dropped before alertness can escalate and the creature ends
    /// up parked facing away, unable to ever re-acquire.
    #[test]
    fn calm_monster_turns_toward_a_visible_player() {
        // 45 degrees off the player: inside the 60-degree FOV half-angle, so
        // the player is plainly visible.
        let (world, entity_id) = world_with_monster_and_player(Deg(45.0));
        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, &world);
        assert_eq!(monster.alertness.current_level, AIAlertLevel::Lowest);

        let effects = step(&mut monster, &world, entity_id);

        // 100ms at the 180 deg/s turn speed closes 18 of the 45 degrees.
        let heading = commanded_heading(&effects).expect("the AI steers every frame");
        assert!(
            (26.0..28.0).contains(&heading.0),
            "a calm AI that sees the player should turn toward it (0 degrees), got {heading:?}",
        );
    }

    /// Apparitions replay an authored performance and are excluded from
    /// alertness entirely (`build_config` returns no config for them), so the
    /// player they "see" must not turn their head either - a ghost that
    /// tracked the player would face away from its mark.
    #[test]
    fn apparition_does_not_track_a_visible_player() {
        let (world, entity_id) =
            world_with_creature_and_player(Deg(45.0), "creaturetype apparition");
        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, &world);
        assert!(monster.config.is_none(), "apparitions process no alertness");

        let effects = step(&mut monster, &world, entity_id);

        let heading = commanded_heading(&effects).expect("the AI steers every frame");
        assert!(
            (heading.0 - 45.0).abs() < 1.0,
            "an apparition must hold its authored heading, got {heading:?}",
        );
    }

    /// ...and it must keep holding it: the idle scan that lets a posted
    /// creature look around (#791) must not reach a creature excluded from
    /// awareness, or a ghost slowly swings off its authored mark.
    #[test]
    fn apparition_holds_its_heading_instead_of_scanning() {
        let (world, entity_id) =
            world_with_creature_and_player(Deg(180.0), "creaturetype apparition");
        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, &world);

        // Well past the idle scan's dwell and a full sweep.
        for _ in 0..200 {
            let effects = step(&mut monster, &world, entity_id);
            let heading = commanded_heading(&effects).expect("the AI steers every frame");
            assert!(
                (heading.0.abs() - 180.0).abs() < 1.0,
                "an apparition must hold its authored heading, got {heading:?}",
            );
        }
    }

    /// Flag `entity_id` as a patroller and (when `with_route`) lay down a
    /// two-point `AIPatrol` loop 10 units down +X for it to walk.
    fn make_patroller(world: &mut World, entity_id: EntityId, with_route: bool) {
        make_patroller_with_points(world, entity_id, with_route, [10.0, 20.0]);
    }

    /// As `make_patroller`, with the two route points at chosen distances
    /// down +X - close ones stand the AI on its own goal.
    fn make_patroller_with_points(
        world: &mut World,
        entity_id: EntityId,
        with_route: bool,
        xs: [f32; 2],
    ) {
        world.add_component(entity_id, dark::properties::PropAIPatrol(true));
        if !with_route {
            return;
        }
        let mut point = |x: f32| {
            world.add_entity((
                crate::runtime_props::RuntimePropTransform(cgmath::Matrix4::from_translation(
                    vec3(x, 0.0, 0.0),
                )),
                dark::properties::Links::empty(),
            ))
        };
        let first = point(xs[0]);
        let second = point(xs[1]);
        let mut v_links = world
            .borrow::<shipyard::ViewMut<dark::properties::Links>>()
            .unwrap();
        for (from, to) in [(first, second), (second, first)] {
            (&mut v_links)
                .get(from)
                .unwrap()
                .to_links
                .push(dark::properties::ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(dark::properties::WrappedEntityId(to)),
                    link: Link::AIPatrol,
                });
        }
    }

    /// A monster mid-pivot, its heading `heading` and its behavior asking
    /// for `desired`.
    fn pivoting_monster(heading: Deg<f32>) -> (World, EntityId, AnimatedMonsterAI) {
        let (world, entity_id) = world_with_monster_and_player(heading);
        let mut monster = AnimatedMonsterAI::new();
        monster.current_heading = heading;
        (world, entity_id, monster)
    }

    fn tick() -> Time {
        Time {
            elapsed: std::time::Duration::from_millis(100),
            total: std::time::Duration::from_millis(100),
        }
    }

    /// One frame of the pivot: the effect it emits and whether the clip owns
    /// the heading.
    fn turn_frame(
        monster: &mut AnimatedMonsterAI,
        entity_id: EntityId,
        desired: Deg<f32>,
    ) -> (Option<Deg<f32>>, bool) {
        let (effect, held) = monster.update_turn_clip(entity_id, desired, &tick(), true);
        let requested = match effect {
            Effect::PlayTurnClip { delta, .. } => Some(delta),
            _ => None,
        };
        (requested, held)
    }

    /// Report the clip the applier picked for a pivot in flight.
    fn report_clip(
        monster: &mut AnimatedMonsterAI,
        world: &World,
        entity_id: EntityId,
        turn: Deg<f32>,
        duration: f32,
    ) {
        monster.handle_message(
            entity_id,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnClipStarted {
                turn,
                duration,
                blend: TURN_CLIP_SETTLE_SECONDS,
            },
        );
    }

    fn complete_clip(monster: &mut AnimatedMonsterAI, world: &World, entity_id: EntityId) {
        monster.handle_message(
            entity_id,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::AnimationCompleted,
        );
    }

    /// Hold the pivot for `seconds` of 100ms frames, as the clip plays.
    fn play_out(monster: &mut AnimatedMonsterAI, entity_id: EntityId, seconds: f32) {
        for _ in 0..((seconds / 0.1).ceil() as usize) {
            turn_frame(monster, entity_id, Deg(90.0));
        }
    }

    fn locomotion_scale(effects: &[Effect]) -> Option<f32> {
        effects.iter().find_map(|effect| match effect {
            Effect::SetAIProperty {
                update: crate::scripts::AIPropertyUpdate::LocomotionScale { scale },
                ..
            } => Some(*scale),
            _ => None,
        })
    }

    /// The pivot the AI asks for is the heading change it actually needs; the
    /// applier is what turns that into one of the creature's authored clips.
    #[test]
    fn a_stalled_creature_steers_its_turn_instead_of_performing_it() {
        let (_world, entity_id, mut monster) = pivoting_monster(Deg(0.0));
        let reversal = Deg(180.0);

        // Getting somewhere: the reversal is performed.
        monster.bar_pivot_while_stalled(0.0, true);
        assert!(
            turn_frame(&mut monster, entity_id, reversal).0.is_some(),
            "a creature making progress performs its pivot"
        );

        // The route stops making progress. Standing still through a turn clip
        // while wedged only lengthens the wedge.
        let (_world, entity_id, mut monster) = pivoting_monster(Deg(0.0));
        monster.bar_pivot_while_stalled(STALL_SECONDS, true);
        assert!(
            turn_frame(&mut monster, entity_id, reversal).0.is_none(),
            "a stalled creature steers the turn instead"
        );

        // The stall clock resets the moment the follower recovers, but the
        // sidestep heading its re-path produces arrives a beat later - and the
        // recovery it has to outlast is itself most of a second.
        assert!(
            TURN_CLIP_COOLDOWN > crate::scripts::ai::steering::STALL_RECOVERY_SECONDS,
            "the bar must outlast the follower's recovery"
        );
        for _ in 0..(TURN_CLIP_COOLDOWN / 0.1).ceil() as u32 - 1 {
            monster.bar_pivot_while_stalled(0.0, true);
            assert!(
                turn_frame(&mut monster, entity_id, reversal).0.is_none(),
                "still barred before the window is out"
            );
        }
        monster.bar_pivot_while_stalled(0.0, true);
        assert!(
            turn_frame(&mut monster, entity_id, reversal).0.is_some(),
            "moving again for the whole window restores pivoting"
        );
    }

    #[test]
    fn a_stall_a_route_follower_did_not_publish_bars_nothing() {
        // The follower publishes its stall clock only while it steers, so the
        // last value it published outlives it - a melee or idle AI would
        // otherwise re-arm the bar off a frozen reading for the rest of its
        // life.
        let (_world, entity_id, mut monster) = pivoting_monster(Deg(0.0));
        monster.bar_pivot_while_stalled(STALL_SECONDS, false);
        assert!(
            turn_frame(&mut monster, entity_id, Deg(180.0)).0.is_some(),
            "a stall nobody is publishing must not bar a pivot"
        );
    }

    #[test]
    fn a_reversal_asks_for_an_authored_turn_clip() {
        let (_world, entity_id, mut monster) = pivoting_monster(Deg(-90.0));
        let (requested, held) = turn_frame(&mut monster, entity_id, Deg(90.0));
        assert_eq!(requested, Some(Deg(180.0)));
        assert!(
            held,
            "the clip owns the heading from the moment it is asked for"
        );
    }

    /// A pivot the creature can walk through is steered, not performed: the
    /// shortest stock turn clip is a quarter circle.
    #[test]
    fn a_shallow_pivot_is_steered_as_before() {
        let (_world, entity_id, mut monster) = pivoting_monster(Deg(0.0));
        assert_eq!(
            turn_frame(&mut monster, entity_id, Deg(40.0)),
            (None, false)
        );
    }

    /// The clip's pose does the visible turning, so the entity must hold
    /// still underneath it - rotating as well would turn the creature twice.
    #[test]
    fn the_body_holds_its_heading_and_stride_while_the_clip_plays() {
        let (world, entity_id, mut monster) = pivoting_monster(Deg(-90.0));
        turn_frame(&mut monster, entity_id, Deg(90.0));
        report_clip(&mut monster, &world, entity_id, Deg(-168.0), 4.8);

        for _ in 0..10 {
            let (requested, held) = turn_frame(&mut monster, entity_id, Deg(90.0));
            assert_eq!(requested, None, "one pivot plays one clip");
            assert!(held);
            assert_eq!(monster.current_heading, Deg(-90.0));
            let effects = Effect::flatten(vec![monster.apply_steering_output(
                Steering::from_current(Deg(90.0)),
                &tick(),
                entity_id,
                false,
                held,
            )]);
            assert_eq!(
                locomotion_scale(&effects),
                Some(0.0),
                "a pivot is performed at a standstill"
            );
            assert_eq!(commanded_heading(&effects), Some(Deg(-90.0)));
        }
    }

    /// ...and once the clip is done, the entity takes the authored facing
    /// change over the blend that swings the pose back to neutral.
    #[test]
    fn the_authored_turn_lands_on_the_entity_once_the_clip_ends() {
        let (world, entity_id, mut monster) = pivoting_monster(Deg(-90.0));
        turn_frame(&mut monster, entity_id, Deg(90.0));
        report_clip(&mut monster, &world, entity_id, Deg(-168.0), 4.8);
        play_out(&mut monster, entity_id, 4.8);
        complete_clip(&mut monster, &world, entity_id);

        // The settle is the clip's blend length, in 100ms frames.
        for _ in 0..5 {
            assert!(turn_frame(&mut monster, entity_id, Deg(90.0)).1);
        }
        assert!(
            (monster.current_heading.0 - (-90.0 - 168.0)).abs() < 1e-3,
            "expected the authored turn to land, heading is {:?}",
            monster.current_heading
        );
        assert!(
            monster.turn_clip.is_none(),
            "the pivot is over once the turn has landed"
        );
    }

    /// A pivot has a budget: it must be over, blend included, before a held
    /// spot reads as a wedge.
    #[test]
    fn a_pivot_cannot_hold_long_enough_to_look_stuck() {
        // The reachability harness treats holding one spot for 6s as wedged.
        assert!(
            TURN_CLIP_REPORT_TIMEOUT
                + TURN_CLIP_MAX_SECONDS
                + TURN_CLIP_OVERRUN_MARGIN
                + TURN_CLIP_SETTLE_SECONDS
                < 6.0
        );
    }

    /// Two pivots back to back hold the creature still for longer than either
    /// of them - which is exactly what a wedge looks like.
    #[test]
    fn a_second_pivot_waits_for_the_cooldown() {
        let (world, entity_id, mut monster) = pivoting_monster(Deg(-90.0));
        turn_frame(&mut monster, entity_id, Deg(90.0));
        report_clip(&mut monster, &world, entity_id, Deg(-100.0), 2.4);
        play_out(&mut monster, entity_id, 2.4);
        complete_clip(&mut monster, &world, entity_id);
        for _ in 0..6 {
            turn_frame(&mut monster, entity_id, Deg(90.0));
        }
        assert!(monster.turn_clip.is_none(), "the first pivot must be over");
        // Still facing a long way from where it wants to be, but it has to
        // steer out of the pivot before performing another.
        assert_eq!(
            turn_frame(&mut monster, entity_id, Deg(90.0)),
            (None, false)
        );
    }

    /// The behavior's own clip is re-queued the moment the turn clip ends,
    /// and a run cycle is a third of a second long - so its completion lands
    /// inside the blend. Restarting the blend on it left the AI turning on
    /// the spot forever, never taking another step.
    #[test]
    fn a_later_clip_completing_does_not_restart_the_blend() {
        let (world, entity_id, mut monster) = pivoting_monster(Deg(-90.0));
        turn_frame(&mut monster, entity_id, Deg(90.0));
        report_clip(&mut monster, &world, entity_id, Deg(-168.0), 4.8);
        play_out(&mut monster, entity_id, 4.8);
        for _ in 0..2 {
            complete_clip(&mut monster, &world, entity_id);
            for _ in 0..3 {
                turn_frame(&mut monster, entity_id, Deg(90.0));
            }
        }
        assert!(
            monster.turn_clip.is_none(),
            "the pivot must end, not restart with every clip that completes"
        );
        assert!(
            (monster.current_heading.0 - (-90.0 - 168.0)).abs() < 1e-3,
            "and it must apply the authored turn exactly once"
        );
    }

    /// Anything that preempts the turn clip - a wound reaction, an alertness
    /// change - completes long before the clip would have. Taking the authored
    /// turn then would swing the AI through it with a neutral pose.
    #[test]
    fn a_completion_that_arrives_early_is_not_the_turn_clip() {
        let (world, entity_id, mut monster) = pivoting_monster(Deg(-90.0));
        turn_frame(&mut monster, entity_id, Deg(90.0));
        report_clip(&mut monster, &world, entity_id, Deg(-168.0), 4.8);
        play_out(&mut monster, entity_id, 0.5);
        complete_clip(&mut monster, &world, entity_id);

        assert!(monster.turn_clip.is_none(), "the pivot is abandoned");
        assert_eq!(
            monster.current_heading,
            Deg(-90.0),
            "a turn whose pose never played must not land on the entity"
        );
    }

    /// A creature whose schema has no turn clip for the pivot never gets a
    /// report back; it must fall back to steering instead of standing there.
    #[test]
    fn an_unanswered_pivot_falls_back_to_steering() {
        let (_world, entity_id, mut monster) = pivoting_monster(Deg(-90.0));
        assert!(turn_frame(&mut monster, entity_id, Deg(90.0)).0.is_some());
        for _ in 0..10 {
            turn_frame(&mut monster, entity_id, Deg(90.0));
        }
        assert_eq!(
            turn_frame(&mut monster, entity_id, Deg(90.0)),
            (None, false),
            "an unanswered pivot must give the heading back to steering, and stop asking"
        );
    }

    /// #807: a creature flagged to patrol must walk its authored route from
    /// spawn. Behavior is otherwise only chosen on an alertness LEVEL CHANGE,
    /// so a patroller that is never alerted stood on its spawn point for the
    /// whole mission - patrol only ever started after an alert had come and
    /// gone.
    /// A pivot used to skip the behavior update entirely, purely to keep the
    /// standstill out of the stall clock - which also stopped the route, the
    /// whiskers and the crowd repel for the length of the clip. The hold now
    /// pauses the accounting alone, so the behavior is steered right through
    /// the pivot: this patroller keeps working its route while it turns.
    #[test]
    fn a_pivot_no_longer_freezes_the_behavior() {
        let (mut world, entity_id) = world_with_monster_and_player(Deg(180.0));
        // Standing on its own patrol point, so every steered frame arrives
        // and advances the route - an observable heartbeat of the steer.
        make_patroller_with_points(&mut world, entity_id, true, [0.0, 0.5]);
        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, &world);
        assert_eq!(monster.current_behavior.borrow().name(), "Patrol");
        step(&mut monster, &world, entity_id);

        monster.turn_clip = Some(TurnClip::Playing {
            turn: Deg(90.0),
            remaining: 10.0,
            blend: TURN_CLIP_SETTLE_SECONDS,
        });
        let effects = step(&mut monster, &world, entity_id);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::SetAICurrentPatrol { .. })),
            "a pivoting AI is still steered by its behavior",
        );
    }

    #[test]
    fn a_door_wait_outranks_a_pivot_as_the_hold_reason() {
        assert_eq!(movement_hold(None, false), MovementHold::None);
        assert_eq!(movement_hold(None, true), MovementHold::Pivot);
        // Both at once: the door is what the AI is actually waiting on.
        assert_eq!(movement_hold(Some(0.2), true), MovementHold::DoorWait);
    }

    #[test]
    fn patroller_starts_its_route_without_ever_being_alerted() {
        let (mut world, entity_id) = world_with_monster_and_player(Deg(180.0));
        make_patroller(&mut world, entity_id, true);

        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, &world);

        assert_eq!(
            monster.alertness.current_level,
            AIAlertLevel::Lowest,
            "the patroller must not need an alert first",
        );
        assert_eq!(
            monster.current_behavior.borrow().name(),
            "Patrol",
            "a flagged patroller with a reachable route patrols from spawn",
        );
    }

    #[test]
    fn patroller_resumes_current_target_after_alertness_interrupts_it() {
        let (mut world, entity_id) = world_with_monster_and_player(Deg(180.0));
        make_patroller(&mut world, entity_id, true);

        // The fresh nearest-source rule would target the x=20 destination.
        // Persist x=10 instead, as Dark's AICurrentPatrol relation does after
        // a route has already advanced, so the two cases are distinguishable.
        let resume_target = {
            let transforms = world
                .borrow::<shipyard::View<crate::runtime_props::RuntimePropTransform>>()
                .unwrap();
            let links = world
                .borrow::<shipyard::View<dark::properties::Links>>()
                .unwrap();
            (&links)
                .iter()
                .with_id()
                .filter(|(id, links)| {
                    *id != entity_id
                        && links
                            .to_links
                            .iter()
                            .any(|link| link.link == Link::AIPatrol)
                })
                .find_map(|(id, _)| {
                    let x = transforms
                        .get(id)
                        .ok()?
                        .0
                        .transform_point(cgmath::point3(0.0, 0.0, 0.0))
                        .x;
                    ((x - 10.0).abs() < 0.01).then_some(id)
                })
                .unwrap()
        };
        world.add_component(
            entity_id,
            dark::properties::Links {
                to_links: vec![dark::properties::ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(dark::properties::WrappedEntityId(resume_target)),
                    link: Link::AICurrentPatrol,
                }],
            },
        );

        let physics = PhysicsWorld::new();
        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, &world);
        assert_eq!(
            monster.current_behavior.borrow().patrol_target(),
            Some(resume_target),
            "fresh script initialization must hydrate the saved route target"
        );

        monster.force_alertness(AIAlertLevel::Moderate, &world, &physics, entity_id);
        assert_eq!(monster.current_behavior.borrow().name(), "Chase");
        monster.force_alertness(AIAlertLevel::Lowest, &world, &physics, entity_id);
        assert_eq!(monster.current_behavior.borrow().name(), "Patrol");
        assert_eq!(
            monster.current_behavior.borrow().patrol_target(),
            Some(resume_target),
            "calming down must resume the interrupted destination"
        );
    }

    /// ...but the flag alone is not enough: a mission with no patrol network
    /// leaves the creature on the ordinary idle (which, post-#791, scans).
    #[test]
    fn a_patroller_with_no_route_stands_idle() {
        let (mut world, entity_id) = world_with_monster_and_player(Deg(180.0));
        make_patroller(&mut world, entity_id, false);

        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, &world);

        assert_eq!(monster.current_behavior.borrow().name(), "Idle");
    }

    /// A creature excluded from awareness entirely (an apparition - see
    /// `build_config`) holds its authored mark even when it is flagged to
    /// patrol and a route exists: leaving the mark is exactly what its
    /// scripted performance must not do. It must get the STILL idle, not
    /// merely a non-patrol one, so it doesn't scan off its mark either.
    #[test]
    fn apparition_holds_its_mark_even_when_flagged_to_patrol() {
        let (mut world, entity_id) =
            world_with_creature_and_player(Deg(180.0), "creaturetype apparition");
        make_patroller(&mut world, entity_id, true);

        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, &world);

        assert!(monster.config.is_none(), "apparitions process no alertness");
        assert_eq!(monster.current_behavior.borrow().name(), "Idle");

        // Well past the idle scan's dwell and a full sweep.
        for _ in 0..200 {
            let effects = step(&mut monster, &world, entity_id);
            let heading = commanded_heading(&effects).expect("the AI steers every frame");
            assert!(
                (heading.0.abs() - 180.0).abs() < 1.0,
                "an apparition must hold its authored heading, got {heading:?}",
            );
        }
    }

    /// The counterpart: sight still gates the turn. A creature facing away has
    /// no idea the player is there and must not swivel onto them.
    #[test]
    fn calm_monster_ignores_a_player_outside_its_fov() {
        // Turned 180 degrees from the player - well outside the FOV cone.
        let (world, entity_id) = world_with_monster_and_player(Deg(180.0));
        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, &world);

        let effects = step(&mut monster, &world, entity_id);

        let heading = commanded_heading(&effects).expect("the AI steers every frame");
        assert!(
            (heading.0.abs() - 180.0).abs() < 1.0,
            "an AI that cannot see the player must hold its heading, got {heading:?}",
        );
    }

    #[test]
    fn orienting_on_target_needs_sight_and_a_non_pursuing_unscripted_ai() {
        use ScriptedState::*;
        // Sighted while unaware or merely suspicious: look at it.
        assert!(should_orient_on_target(
            true,
            AIAlertLevel::Lowest,
            true,
            NotScripted
        ));
        assert!(should_orient_on_target(
            true,
            AIAlertLevel::Low,
            true,
            NotScripted
        ));
        // No sight, no turn.
        assert!(!should_orient_on_target(
            true,
            AIAlertLevel::Lowest,
            false,
            NotScripted
        ));
        // Pursuing behaviors steer at their own target.
        assert!(!should_orient_on_target(
            true,
            AIAlertLevel::Moderate,
            true,
            NotScripted
        ));
        assert!(!should_orient_on_target(
            true,
            AIAlertLevel::High,
            true,
            NotScripted
        ));
        // A running scripted sequence owns its actor's heading.
        assert!(!should_orient_on_target(
            true,
            AIAlertLevel::Lowest,
            true,
            Running
        ));
        // A creature that processes no alertness notices nothing.
        assert!(!should_orient_on_target(
            false,
            AIAlertLevel::Lowest,
            true,
            NotScripted
        ));
    }

    /// Deal a killing blow to a fresh monster and return it with the effects
    /// its death produced.
    fn kill(world: &World, entity_id: EntityId) -> (AnimatedMonsterAI, Vec<Effect>) {
        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, world);
        let physics = PhysicsWorld::new();
        let effects = Effect::flatten(vec![monster.handle_message(
            entity_id,
            world,
            &physics,
            &MessagePayload::Damage {
                amount: 100.0,
                impact: None,
            },
        )]);
        (monster, effects)
    }

    /// A lethal blow on a creature that authors death links (droids link a
    /// `Corpse` explosion and `Flinderize` parts) must slay it into those
    /// links - no crumple animation, death speech or ragdoll handoff for a
    /// body that no longer exists.
    #[test]
    fn creature_with_death_links_slays_into_its_links() {
        let (mut world, entity_id) = world_with_monster_and_player(Deg(0.0));
        world.add_component(
            entity_id,
            dark::properties::Links {
                to_links: vec![dark::properties::ToLink {
                    to_template_id: -1425,
                    to_entity_id: None,
                    link: dark::properties::Link::Flinderize(dark::properties::FlinderizeOptions {
                        count: 1,
                        impulse: 0.0,
                        scatter: false,
                        offset: vec3(0.0, 0.0, 0.0),
                    }),
                }],
            },
        );

        let (monster, effects) = kill(&world, entity_id);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::SlayEntity { .. })),
            "expected a link death, got {effects:?}"
        );
        assert!(
            effects.iter().all(|effect| !matches!(
                effect,
                Effect::PlayAnimationBySchema { .. } | Effect::PlaySpeech { .. }
            )),
            "a link death has no crumple or death speech, got {effects:?}"
        );
        assert!(monster.handoff_emitted, "a removed body must not ragdoll");
    }

    /// Organics author no death links: they keep the crumple + death speech.
    #[test]
    fn creature_without_death_links_crumples() {
        let (world, entity_id) = world_with_monster_and_player(Deg(0.0));

        let (_monster, effects) = kill(&world, entity_id);

        assert!(
            effects
                .iter()
                .all(|effect| !matches!(effect, Effect::SlayEntity { .. })),
            "an organic must not slay into links, got {effects:?}"
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::PlayAnimationBySchema { .. })),
            "expected the crumple animation, got {effects:?}"
        );
    }

    #[test]
    fn killed_monster_initializes_inert_without_replaying_death_effects() {
        let mut world = World::new();
        let entity_id = world.add_entity((
            dark::properties::PropHitPoints { hit_points: 0 },
            crate::runtime_props::RuntimePropTransform(cgmath::Matrix4::from_scale(1.0)),
        ));
        let mut monster = AnimatedMonsterAI::new();

        let effects = Effect::flatten(vec![monster.initialize(entity_id, &world)]);

        assert!(monster.is_dead);
        assert!(monster.handoff_emitted);
        assert_eq!(monster.current_behavior.borrow().name(), "Dead");
        assert!(effects.iter().all(|effect| !matches!(
            effect,
            Effect::QueueAnimationBySchema { .. }
                | Effect::PlayAnimationBySchema { .. }
                | Effect::PlaySpeech { .. }
                | Effect::SpawnCorpseRagdoll { .. }
        )));
    }

    #[test]
    fn locomotion_scale_full_speed_when_facing_travel_direction() {
        assert_eq!(locomotion_scale_for_heading_error(Deg(0.0)), 1.0);
    }

    #[test]
    fn locomotion_scale_ramps_down_with_heading_error() {
        let at_30 = locomotion_scale_for_heading_error(Deg(30.0));
        assert!(at_30 > 0.33 && at_30 < 1.0, "got {at_30}");
        // Symmetric for left/right error
        assert_eq!(at_30, locomotion_scale_for_heading_error(Deg(-30.0)));
        // A third of full speed by 60 degrees
        assert_eq!(locomotion_scale_for_heading_error(Deg(60.0)), 0.33);
    }

    /// A hybrid is ~6.5 ft tall; the door leaf it walks under has to have
    /// risen at least that far before the gap is worth entering.
    const TEST_ACTOR_HEIGHT: f32 = 6.5 / SCALE_FACTOR;

    #[test]
    fn a_barely_open_door_is_not_passable() {
        // The leaf has just left its closed pose: the doorway is still solid
        // at head height, so the AI must wait rather than walk into it.
        assert!(!door_is_passable(
            0.05,
            0.2 / SCALE_FACTOR,
            TEST_ACTOR_HEIGHT
        ));
        assert!(!door_is_passable(
            0.5,
            3.0 / SCALE_FACTOR,
            TEST_ACTOR_HEIGHT
        ));
    }

    #[test]
    fn a_leaf_risen_clear_of_the_head_is_passable() {
        assert!(door_is_passable(0.6, 7.0 / SCALE_FACTOR, TEST_ACTOR_HEIGHT));
    }

    #[test]
    fn a_short_creature_clears_a_leaf_a_hybrid_still_waits_for() {
        let rise = 4.0 / SCALE_FACTOR;
        assert!(door_is_passable(0.5, rise, 3.0 / SCALE_FACTOR));
        assert!(!door_is_passable(0.5, rise, TEST_ACTOR_HEIGHT));
    }

    #[test]
    fn a_fully_travelled_leaf_is_passable_without_gaining_height() {
        // A door that slides sideways never rises, so height clearance alone
        // would hold an AI at it forever.
        assert!(door_is_passable(1.0, 0.0, TEST_ACTOR_HEIGHT));
    }

    /// A door wait long enough to read as impatience.
    const LONG_WAIT: Option<f32> = Some(FRUSTRATION_DOOR_WAIT_SECONDS + 1.0);

    #[test]
    fn frustration_needs_a_block() {
        assert!(!should_gesture_frustration(
            None,
            0.0,
            None,
            ScriptedState::NotScripted
        ));
        assert!(should_gesture_frustration(
            LONG_WAIT,
            0.0,
            None,
            ScriptedState::NotScripted
        ));
    }

    #[test]
    fn a_door_that_opens_promptly_draws_no_gesture() {
        // Every shipped sliding leaf clears in about half a second; an AI
        // that gestured at each one would dam the doorway behind it.
        assert!(!should_gesture_frustration(
            Some(0.5),
            0.0,
            None,
            ScriptedState::NotScripted
        ));
    }

    /// USER DECISION (2026-09-05): impatience is a door gesture only. On a
    /// stall the creature is trying to get out of a wedge, and a clip with no
    /// root motion only makes the standstill longer.
    #[test]
    fn a_sustained_stall_draws_no_gesture() {
        let mut monster = AnimatedMonsterAI::new();
        let (world, entity_id) = world_with_monster_and_player(Deg(0.0));
        // Nothing is holding this AI at a door, however long its route has
        // been going nowhere.
        assert!(matches!(
            monster.update_frustration(&world, entity_id, None, false, &tick()),
            Effect::NoEffect
        ));
    }

    #[test]
    fn the_door_wait_can_never_outlast_the_path_follower_patience() {
        // A held body makes no progress toward its waypoint. If the hold
        // could outlast the stall window, the stall system would blacklist
        // the very crossing this AI just opened and route it back away from
        // the door.
        assert!(DOOR_WAIT_TIMEOUT < crate::scripts::ai::steering::STALL_SECONDS);
    }

    #[test]
    fn impatience_is_reached_before_the_wait_times_out() {
        // A threshold above the ceiling would be a gesture that never plays.
        assert!(FRUSTRATION_DOOR_WAIT_SECONDS < DOOR_WAIT_TIMEOUT);
    }

    #[test]
    fn frustration_is_rate_limited() {
        assert!(!should_gesture_frustration(
            LONG_WAIT,
            3.0,
            None,
            ScriptedState::NotScripted
        ));
    }

    #[test]
    fn frustration_is_silent_within_reach_of_the_target() {
        let inside = Some(MELEE_ATTACK_RANGE * 0.5);
        let outside = Some(MELEE_ATTACK_RANGE * 2.0);
        assert!(!should_gesture_frustration(
            LONG_WAIT,
            0.0,
            inside,
            ScriptedState::NotScripted
        ));
        assert!(should_gesture_frustration(
            LONG_WAIT,
            0.0,
            outside,
            ScriptedState::NotScripted
        ));
    }

    /// The locked-door give-up plays the same one performance as the door
    /// wait, and used to play it whatever either path had already spent.
    #[test]
    fn the_locked_door_give_up_respects_the_shared_limit() {
        let (world, entity_id) = world_with_monster_and_player(Deg(0.0));
        let mut monster = AnimatedMonsterAI::new();
        assert!(matches!(
            monster.try_frustration_gesture(&world, entity_id),
            Effect::PlayAnimationBySchema { .. }
        ));
        assert!(monster.frustration_cooldown > 0.0, "the limit is armed");
        assert!(matches!(
            monster.try_frustration_gesture(&world, entity_id),
            Effect::NoEffect
        ));
    }

    #[test]
    fn frustration_never_interrupts_an_authored_performance() {
        assert!(!should_gesture_frustration(
            LONG_WAIT,
            0.0,
            None,
            ScriptedState::Running
        ));
    }

    #[test]
    fn locomotion_scale_stops_the_body_past_a_right_angle() {
        // A reversal pivots in place instead of arcing sideways into
        // whatever is beside the body
        assert_eq!(locomotion_scale_for_heading_error(Deg(90.0)), 0.0);
        assert_eq!(locomotion_scale_for_heading_error(Deg(180.0)), 0.0);
        assert_eq!(locomotion_scale_for_heading_error(Deg(-135.0)), 0.0);
    }

    #[test]
    fn locomotion_scale_eases_to_the_standstill_between_60_and_90() {
        // No cliff at 60: the third-speed walk fades out over the next 30
        // degrees rather than dropping to a stop in one frame
        let at_75 = locomotion_scale_for_heading_error(Deg(75.0));
        assert!((at_75 - 0.165).abs() < 1e-4, "got {at_75}");
        let at_89 = locomotion_scale_for_heading_error(Deg(89.0));
        assert!(at_89 > 0.0 && at_89 < 0.02, "got {at_89}");
        assert_eq!(at_75, locomotion_scale_for_heading_error(Deg(-75.0)));
    }

    /// Every environmental-sound query in `effect`, as its (tag, value) pairs.
    fn sound_queries(effect: &Effect) -> Vec<Vec<(String, String)>> {
        Effect::flatten(vec![effect.clone()])
            .iter()
            .filter_map(|e| match e {
                Effect::PlayEnvironmentalSound { query, .. } => Some(query.tag_values()),
                _ => None,
            })
            .collect()
    }

    /// The effect a live hybrid returns when its animation crosses `flags`.
    ///
    /// `oncegrunt`, not `hybrid`: `oncegrunt` is the value the shipped schema
    /// actually keys hybrids on, and an unknown tag *value* is silently dropped
    /// from the query rather than failing it - so a fixture using the wrong
    /// name would assert on a query that resolves to nothing in the real game.
    fn effect_of_animation_flags(flags: MotionFlags) -> Effect {
        let (world, entity_id) = world_with_creature_and_player(Deg(0.0), "creaturetype oncegrunt");
        let physics = PhysicsWorld::new();
        let mut monster = AnimatedMonsterAI::new();
        monster.initialize(entity_id, &world);

        monster.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::AnimationFlagTriggered {
                motion_flags: flags,
            },
        )
    }

    /// The shipped locomotion clips are per-half-step and author exactly one
    /// foot-plant flag each, so a creature's walk cycle should hand the schema
    /// one `event=footstep` query per foot, keyed by its creature type.
    #[test]
    fn a_foot_plant_frame_plays_a_footstep_for_the_creature_type() {
        let effect = effect_of_animation_flags(MotionFlags::LEFT_FOOT_STEP);

        let queries = sound_queries(&effect);
        assert_eq!(queries.len(), 1, "one plant, one footstep: {queries:?}");
        let query = &queries[0];
        assert!(
            query.contains(&("event".to_owned(), "footstep".to_owned())),
            "footsteps resolve on event=footstep: {query:?}"
        );
        assert!(
            query.contains(&("creaturetype".to_owned(), "oncegrunt".to_owned())),
            "a hybrid must not sound like a monkey: {query:?}"
        );
        // The hybrid branch is the one creature type that needs both
        // refinements and resolves to silence without them: `material` is the
        // creature's own foot material, `material2` the surface underfoot.
        assert!(
            query.contains(&("material".to_owned(), "fleshtarget".to_owned())),
            "material is the creature's own, not the ground's: {query:?}"
        );
        assert!(
            query.contains(&("material2".to_owned(), "metal".to_owned())),
            "material2 is the surface underfoot: {query:?}"
        );
    }

    /// The right foot is the same event - both flags land in the same schema
    /// query, they just alternate across the two half-step clips.
    #[test]
    fn the_right_foot_plants_too() {
        let effect = effect_of_animation_flags(MotionFlags::RIGHT_FOOT_STEP);

        assert_eq!(sound_queries(&effect).len(), 1);
    }

    /// `AnimationPlayer::update` unions every flag frame crossed in a tick, so
    /// on a hitch one message can carry an attack flag *and* a foot plant. The
    /// step must survive that - including when the attack half is suppressed
    /// because the creature is dying, a guard that has nothing to say about
    /// where its feet are.
    #[test]
    fn a_foot_plant_sharing_a_tick_with_an_attack_flag_still_sounds() {
        let effect = effect_of_animation_flags(MotionFlags::FIRE | MotionFlags::LEFT_FOOT_STEP);

        assert_eq!(
            sound_queries(&effect).len(),
            1,
            "the footstep must not be swallowed by the attack arm"
        );
    }

    /// Flags that are not foot plants must stay silent, or every animated
    /// frame of a creature turns into a footstep.
    #[test]
    fn a_non_foot_flag_plays_no_footstep() {
        let effect = effect_of_animation_flags(MotionFlags::INTERRUPTIBLE);

        assert!(sound_queries(&effect).is_empty());
    }
}
