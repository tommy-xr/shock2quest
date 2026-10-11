//! Ring buffer of recently *dispatched* script messages, so headless tooling
//! (the debug runtime's `GET /v1/messages/recent`) can see what actually drove
//! script behavior on a given frame - e.g. which entities got a `TurnOn`
//! immediately before a burst of audio.
//!
//! Modeled on [`crate::audio_log`]: a process-global ring buffer, written from
//! the message-dispatch points in `ScriptWorld::update`, read straight out of
//! the static by the HTTP handler (no game-loop round trip). An entry means
//! the message was dispatched to the entity - an entity with no script for it
//! is still traced, which is usually what you want to see when debugging a
//! chain that went nowhere. The buffer is never cleared, so it can span a
//! level transition; `template_id` is the only identity that survives one.
//!
//! **Filtering.** Per-frame, high-frequency payloads are dropped so the buffer
//! holds useful history instead of one second of churn: `Hover`, `GUIHover`,
//! `SensorBeginIntersect`, `SensorEndIntersect`,
//! `AnimationFlagTriggered` and `AnimationCompleted` are not traced. (Idle
//! creature animation alone produces hundreds of the last two per second in a
//! populated mission.) Event-class payloads (`TurnOn`/`TurnOff`/`Frob`/
//! `Signal`/`Damage`/`Slay`/...) always are. Collisions have a separate 128-entry
//! budget, so even many distinct colliding pairs cannot evict the 256 event
//! entries. Retained collisions from the same directed pair are deduplicated
//! within 30 frames. Both histories are returned in dispatch sequence order.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;
use shipyard::{EntityId, World};

use crate::audio_log::frame_of;
use crate::scripts::MessagePayload;
use crate::util::entity_ident;

const MAX_ENTRIES: usize = 256;
const MAX_COLLISIONS: usize = 128;
const COLLISION_WINDOW_FRAMES: u64 = 30;

/// Identity of a message endpoint. `entity_id` is only meaningful within one
/// run (and matches the id space of the debug runtime's entity endpoints);
/// `template_id` is the stable handle across launches.
#[derive(Clone, Debug, Serialize)]
pub struct MessageEntity {
    pub name: String,
    pub entity_id: i32,
    pub template_id: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TracedMessage {
    /// Monotonically increasing id, so callers can diff "new since last poll".
    pub sequence: u64,
    /// Simulation time (seconds) at which the message was delivered.
    pub sim_time: f64,
    /// `sim_time` expressed in fixed 60 Hz frames.
    pub frame: u64,
    pub to: MessageEntity,
    /// Payload variant name (e.g. "TurnOn", "Frob").
    pub payload: String,
    /// Physical context carried by a `Damage` message, when its source knows
    /// where and in which direction the blow landed.
    pub impact: Option<TracedDamageImpact>,
    /// Sender, for the payloads that carry one (`TurnOn`/`TurnOff`/`Alarm`/
    /// `Reset`), or the contact partner for `Collided`; null otherwise.
    /// `Message` has no universal sender.
    pub from: Option<MessageEntity>,
}

/// Serializable subset of [`crate::scripts::DamageImpact`] exposed to
/// headless behavior tests.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TracedDamageImpact {
    pub direction: [f32; 3],
    pub point: [f32; 3],
    pub bone: Option<u32>,
}

struct MessageTrace {
    next_sequence: u64,
    last_frame: u64,
    collision_epoch_start: u64,
    events: VecDeque<TracedMessage>,
    collisions: VecDeque<TracedMessage>,
}

impl MessageTrace {
    const fn new() -> Self {
        Self {
            next_sequence: 0,
            last_frame: 0,
            collision_epoch_start: 0,
            events: VecDeque::new(),
            collisions: VecDeque::new(),
        }
    }

    fn push(&mut self, mut entry: TracedMessage) {
        if entry.frame < self.last_frame {
            // Retain history across loads, but never dedupe against contacts
            // from an earlier timeline that happens to revisit this frame.
            self.collision_epoch_start = self.next_sequence;
        }
        self.last_frame = entry.frame;
        let (entries, capacity) = if entry.payload == "Collided" {
            // Search only this timeline within the bounded collision history.
            if self.collisions.iter().rev().any(|previous| {
                previous.sequence > self.collision_epoch_start
                    && previous.to.entity_id == entry.to.entity_id
                    && previous.from.as_ref().map(|from| from.entity_id)
                        == entry.from.as_ref().map(|from| from.entity_id)
                    && entry.frame >= previous.frame
                    && entry.frame - previous.frame < COLLISION_WINDOW_FRAMES
            }) {
                return;
            }
            (&mut self.collisions, MAX_COLLISIONS)
        } else {
            (&mut self.events, MAX_ENTRIES)
        };
        self.next_sequence += 1;
        entry.sequence = self.next_sequence;
        entries.push_back(entry);
        if entries.len() > capacity {
            entries.pop_front();
        }
    }

    fn recent(&self) -> Vec<TracedMessage> {
        let mut entries: Vec<_> = self
            .events
            .iter()
            .chain(&self.collisions)
            .cloned()
            .collect();
        // Sequence, not simulation time: time can rewind when loading a save.
        entries.sort_unstable_by_key(|entry| entry.sequence);
        entries
    }
}

static RECENT: Mutex<MessageTrace> = Mutex::new(MessageTrace::new());

/// Whether this payload is worth a trace entry (see the module's filter docs).
fn is_traced(payload: &MessagePayload) -> bool {
    !matches!(
        payload,
        MessagePayload::Hover { .. }
            | MessagePayload::GUIHover { .. }
            | MessagePayload::SensorBeginIntersect { .. }
            | MessagePayload::SensorEndIntersect { .. }
            | MessagePayload::AnimationFlagTriggered { .. }
            | MessagePayload::AnimationCompleted
            // One per frame of a pivot's handover fade; it would push the
            // damage/signal history the trace exists for out of the buffer.
            | MessagePayload::TurnClipBlend { .. }
    )
}

/// The sender carried by the payload itself, if any.
fn sender_of(payload: &MessagePayload) -> Option<EntityId> {
    match payload {
        MessagePayload::TurnOn { from }
        | MessagePayload::TurnOff { from }
        | MessagePayload::Alarm { from }
        | MessagePayload::Reset { from } => Some(*from),
        MessagePayload::Collided { with, .. } => Some(*with),
        _ => None,
    }
}

fn damage_impact_of(payload: &MessagePayload) -> Option<TracedDamageImpact> {
    let MessagePayload::Damage {
        impact: Some(impact),
        ..
    } = payload
    else {
        return None;
    };
    Some(TracedDamageImpact {
        direction: [impact.direction.x, impact.direction.y, impact.direction.z],
        point: [impact.point.x, impact.point.y, impact.point.z],
        bone: impact.bone,
    })
}

/// Short variant name of a payload, without its (often large) fields.
fn payload_name(payload: &MessagePayload) -> String {
    let debug = format!("{payload:?}");
    debug
        .split(|c: char| c == ' ' || c == '(' || c == '{')
        .next()
        .unwrap_or(&debug)
        .to_owned()
}

fn describe(world: &World, id: EntityId) -> MessageEntity {
    let (name, template_id) = entity_ident(world, id);
    MessageEntity {
        name,
        entity_id: id.inner() as i32,
        template_id,
    }
}

/// Record one dispatched script message. Filtered payloads are ignored.
pub(crate) fn record(world: &World, sim_time: f64, to: EntityId, payload: &MessagePayload) {
    if !is_traced(payload) {
        return;
    }

    let entry = TracedMessage {
        sequence: 0,
        sim_time,
        frame: frame_of(sim_time),
        to: describe(world, to),
        payload: payload_name(payload),
        impact: damage_impact_of(payload),
        from: sender_of(payload).map(|from| describe(world, from)),
    };

    RECENT.lock().unwrap().push(entry);
}

/// The most recently dispatched messages, oldest first.
pub fn recent() -> Vec<TracedMessage> {
    RECENT.lock().unwrap().recent()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Point2, vec3};

    fn entry(payload: &str, to: i32, from: i32, frame: u64) -> TracedMessage {
        let endpoint = |entity_id| MessageEntity {
            name: format!("entity-{entity_id}"),
            entity_id,
            template_id: Some(entity_id),
        };
        TracedMessage {
            sequence: 0,
            sim_time: frame as f64 / 60.0,
            frame,
            to: endpoint(to),
            payload: payload.to_owned(),
            impact: None,
            from: Some(endpoint(from)),
        }
    }

    #[test]
    fn many_pair_collision_burst_preserves_event_history() {
        let mut trace = MessageTrace::new();
        for payload in ["TurnOn", "Frob", "Damage"] {
            trace.push(entry(payload, 1, 2, 0));
        }
        for pair in 0..256 {
            // Match the two directed messages delivered for CollisionStarted.
            trace.push(entry("Collided", pair, pair + 1000, 1));
            trace.push(entry("Collided", pair + 1000, pair, 1));
        }
        let recent = trace.recent();
        assert_eq!(recent.len(), 3 + MAX_COLLISIONS);
        assert_eq!(
            recent[..3]
                .iter()
                .map(|m| m.payload.as_str())
                .collect::<Vec<_>>(),
            ["TurnOn", "Frob", "Damage"]
        );
        assert!(recent[3..].iter().all(|m| m.payload == "Collided"));
        assert!(
            recent
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        assert_eq!(recent.last().unwrap().sequence, 515);
    }

    #[test]
    fn event_burst_keeps_its_full_budget_and_collision_history() {
        let mut trace = MessageTrace::new();
        trace.push(entry("Collided", 1, 2, 10));
        for _ in 0..MAX_ENTRIES + 1 {
            trace.push(entry("Frob", 3, 4, 0));
        }
        let recent = trace.recent();
        assert_eq!(recent.len(), MAX_ENTRIES + 1);
        // Dispatch sequence remains chronological even when simulation rewinds.
        assert_eq!(recent[0].payload, "Collided");
        assert_eq!(recent[1].sequence, 3);
        assert_eq!(recent.last().unwrap().sequence, 258);
        // A polling client's sequence cursor finds just the new dispatch.
        let cursor = recent.last().unwrap().sequence;
        trace.push(entry("Damage", 3, 4, 1));
        let new: Vec<_> = trace
            .recent()
            .into_iter()
            .filter(|m| m.sequence > cursor)
            .collect();
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].payload, "Damage");
    }

    #[test]
    fn collisions_dedupe_directed_pairs_only_within_window() {
        let mut trace = MessageTrace::new();
        trace.push(entry("Collided", 1, 2, 100));
        trace.push(entry("Collided", 1, 2, 129)); // suppressed
        trace.push(entry("Collided", 2, 1, 129)); // reverse dispatch matters
        trace.push(entry("Collided", 1, 3, 129)); // different partner
        trace.push(entry("Collided", 1, 2, 130)); // exact window boundary
        trace.push(entry("Collided", 1, 2, 0)); // clock rewind
        trace.push(entry("Frob", 1, 2, 0));
        trace.push(entry("Frob", 1, 2, 0)); // events never deduplicated
        let recent = trace.recent();
        assert_eq!(recent.len(), 7);
        assert_eq!(recent.last().unwrap().sequence, 7);
        assert_eq!(
            recent.iter().map(|m| m.frame).collect::<Vec<_>>(),
            [100, 129, 129, 130, 0, 0, 0]
        );
    }

    #[test]
    fn collision_dedupe_does_not_cross_a_clock_rewind() {
        let mut trace = MessageTrace::new();
        trace.push(entry("Collided", 1, 2, 100));
        trace.push(entry("Frob", 3, 4, 0)); // load, keeping diagnostic history
        trace.push(entry("Collided", 1, 2, 0));
        trace.push(entry("Collided", 1, 2, 100));
        let recent = trace.recent();
        assert_eq!(recent.len(), 4);
        assert_eq!(recent[1].payload, "Frob");
        assert_eq!(recent[3].sequence, 4);
    }

    #[test]
    fn collision_partner_is_exposed_in_existing_endpoint_shape() {
        let mut world = World::new();
        let partner = world.add_entity(());
        let payload = MessagePayload::Collided {
            with: partner,
            contact: None,
        };
        assert_eq!(sender_of(&payload), Some(partner));
        let mut collision = entry("Collided", 1, 2, 0);
        collision.from = sender_of(&payload).map(|id| describe(&world, id));
        let value = serde_json::to_value(collision).unwrap();
        assert_eq!(value["payload"], "Collided");
        assert_eq!(value["from"]["entity_id"], partner.inner() as i32);
        assert!(value["impact"].is_null());
    }

    #[test]
    fn payload_names_drop_fields() {
        assert_eq!(payload_name(&MessagePayload::Frob), "Frob");
        assert_eq!(
            payload_name(&MessagePayload::TurnOn {
                from: EntityId::dead()
            }),
            "TurnOn"
        );
        assert_eq!(
            payload_name(&MessagePayload::Signal {
                name: "test".to_owned()
            }),
            "Signal"
        );
    }

    #[test]
    fn damage_trace_preserves_impact_geometry() {
        let payload = MessagePayload::Damage {
            amount: 9.0,
            impact: Some(crate::scripts::DamageImpact {
                direction: vec3(1.0, 0.0, 0.0),
                point: vec3(2.0, 3.0, 4.0),
                bone: Some(7),
            }),
        };

        assert_eq!(
            damage_impact_of(&payload),
            Some(TracedDamageImpact {
                direction: [1.0, 0.0, 0.0],
                point: [2.0, 3.0, 4.0],
                bone: Some(7),
            })
        );
        assert_eq!(
            damage_impact_of(&MessagePayload::Damage {
                amount: 3.0,
                impact: None,
            }),
            None
        );
    }

    #[test]
    fn high_frequency_payloads_are_filtered() {
        assert!(is_traced(&MessagePayload::Frob));
        assert!(is_traced(&MessagePayload::TurnOn {
            from: EntityId::dead()
        }));
        assert!(is_traced(&MessagePayload::Collided {
            with: EntityId::dead(),
            contact: None,
        }));
        assert!(!is_traced(&MessagePayload::GUIHover {
            held_entity_id: None,
            screen_coordinates: Point2::new(0.0, 0.0),
            is_triggered: false,
            is_grabbing: false,
            hand: crate::vr_config::Handedness::Left,
        }));
        assert!(!is_traced(&MessagePayload::SensorBeginIntersect {
            with: EntityId::dead()
        }));
        assert!(!is_traced(&MessagePayload::AnimationCompleted));
        assert!(is_traced(&MessagePayload::HeardNoise {
            origin: vec3(0.0, 0.0, 0.0)
        }));
    }
}
