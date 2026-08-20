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
//! `SensorBeginIntersect`, `SensorEndIntersect`, `Collided`,
//! `AnimationFlagTriggered` and `AnimationCompleted` are not traced. (Idle
//! creature animation alone produces hundreds of the last two per second in a
//! populated mission.) Event-class payloads (`TurnOn`/`TurnOff`/`Frob`/
//! `Signal`/`Damage`/`Slay`/...) always are.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;
use shipyard::{EntityId, World};

use crate::audio_log::frame_of;
use crate::scripts::MessagePayload;
use crate::util::entity_ident;

const MAX_ENTRIES: usize = 256;

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
    /// `Reset`); null otherwise - `Message` has no universal sender.
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

static RECENT: Mutex<(u64, VecDeque<TracedMessage>)> = Mutex::new((0, VecDeque::new()));

/// Whether this payload is worth a trace entry (see the module's filter docs).
fn is_traced(payload: &MessagePayload) -> bool {
    !matches!(
        payload,
        MessagePayload::Hover { .. }
            | MessagePayload::GUIHover { .. }
            | MessagePayload::SensorBeginIntersect { .. }
            | MessagePayload::SensorEndIntersect { .. }
            | MessagePayload::Collided { .. }
            | MessagePayload::AnimationFlagTriggered { .. }
            | MessagePayload::AnimationCompleted
    )
}

/// The sender carried by the payload itself, if any.
fn sender_of(payload: &MessagePayload) -> Option<EntityId> {
    match payload {
        MessagePayload::TurnOn { from }
        | MessagePayload::TurnOff { from }
        | MessagePayload::Alarm { from }
        | MessagePayload::Reset { from } => Some(*from),
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

    let mut entry = TracedMessage {
        sequence: 0,
        sim_time,
        frame: frame_of(sim_time),
        to: describe(world, to),
        payload: payload_name(payload),
        impact: damage_impact_of(payload),
        from: sender_of(payload).map(|from| describe(world, from)),
    };

    let mut guard = RECENT.lock().unwrap();
    let (next_sequence, entries) = &mut *guard;
    *next_sequence += 1;
    entry.sequence = *next_sequence;
    entries.push_back(entry);
    if entries.len() > MAX_ENTRIES {
        entries.pop_front();
    }
}

/// The most recently dispatched messages, oldest first.
pub fn recent() -> Vec<TracedMessage> {
    RECENT.lock().unwrap().1.iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Point2, vec3};

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
        assert!(!is_traced(&MessagePayload::Collided {
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
