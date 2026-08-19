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
//! `SensorBeginIntersect`, `SensorEndIntersect`, `AnimationFlagTriggered` and
//! `AnimationCompleted` are not traced. (Idle creature animation alone
//! produces hundreds of the last two per second in a populated mission.)
//! Event-class payloads (`TurnOn`/`TurnOff`/`Frob`/`Signal`/`Damage`/`Slay`/
//! `Collided`/...) always are.
//!
//! `Collided` is traced despite sitting next to the sensor payloads above,
//! because it is not per-frame churn: it is dispatched only on Rapier's
//! `CollisionStarted` edge, so a resting contact produces exactly one entry
//! the way a `TurnOn` does. Excluding it made VR melee undiagnosable - a
//! swing whose contact volume never touched the target and a swing that
//! touched it but was disallowed from damaging both showed up as *nothing at
//! all* in `/v1/messages/recent`, since only the resulting `Damage` was
//! visible. Contact is the input to that decision, so it has to be
//! observable.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;
use shipyard::{EntityId, World};

use crate::audio_log::frame_of;
use crate::scripts::MessagePayload;
use crate::util::entity_ident;

const MAX_ENTRIES: usize = 256;

/// How long a `Collided` between one pair of entities suppresses further ones
/// (in 60 Hz frames). See [`record`].
const COLLIDED_DEDUP_FRAMES: u64 = 30;

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
    /// The message's *other party*, for the payloads that name one:
    /// the sender for `TurnOn`/`TurnOff`/`Alarm`/`Reset`, and the thing
    /// collided with for `Collided`. Null otherwise - `Message` has no
    /// universal sender. Without it a `Collided` entry says only "this entity
    /// touched something", which cannot answer "did the *weapon* touch it?".
    pub from: Option<MessageEntity>,
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
        // Not a sender, but the same "who was the other party" slot, and the
        // only thing that makes a contact entry identifiable.
        MessagePayload::Collided { with } => Some(*with),
        _ => None,
    }
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
        from: sender_of(payload).map(|from| describe(world, from)),
    };

    let mut guard = RECENT.lock().unwrap();
    let (next_sequence, entries) = &mut *guard;

    // `Collided` is the one traced payload that a *persisting* physical
    // situation can re-emit without limit. It is an edge, not per-frame churn
    // - but a weapon dragged along geometry, or a settling ragdoll limb, keeps
    // separating and re-touching, and every collision dispatches to *both*
    // parties. Left alone that can evict the whole buffer in a second.
    //
    // Keeping the first contact per *pair* preserves the entire diagnostic
    // value (the question is "did these two ever touch?"), so repeats within
    // half a second are dropped rather than the payload being filtered out
    // wholesale as it used to be. Keying on the pair rather than on the
    // receiver alone matters: a receiver-only key would let an unrelated
    // contact hide the one being investigated.
    //
    // This bounds a *chattering* pair, not a broad settling burst across many
    // distinct pairs, which can still churn the ring - see #1051.
    if matches!(payload, MessagePayload::Collided { .. })
        && entries.iter().rev().any(|previous| {
            previous.frame + COLLIDED_DEDUP_FRAMES >= entry.frame
                && previous.to.entity_id == entry.to.entity_id
                && previous.payload == entry.payload
                && previous.from.as_ref().map(|f| f.entity_id)
                    == entry.from.as_ref().map(|f| f.entity_id)
        })
    {
        return;
    }

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
    fn high_frequency_payloads_are_filtered() {
        assert!(is_traced(&MessagePayload::Frob));
        assert!(is_traced(&MessagePayload::TurnOn {
            from: EntityId::dead()
        }));
        // `Collided` is an edge (Rapier `CollisionStarted`), not per-frame
        // churn, and it is the only observable evidence that a melee contact
        // volume touched anything at all - so it must survive the filter.
        assert!(is_traced(&MessagePayload::Collided {
            with: EntityId::dead()
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
