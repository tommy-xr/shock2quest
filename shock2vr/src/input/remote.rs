//! Remote input control: the channel vocabulary an out-of-process agent uses to
//! drive an `InputContext` (aim the head/hands, hold a trigger, push a stick).
//!
//! This lives in `shock2vr` rather than in a runtime because more than one
//! runtime speaks it: the debug runtime *owns* its `InputContext` and patches it
//! directly, while the oculus runtime rebuilds the context from OpenXR every
//! frame and layers the patched channels ON TOP (see [`InputOverrides`]) so a
//! human in the headset keeps every channel an agent has not claimed.

use std::collections::BTreeMap;

use cgmath::{InnerSpace, Quaternion, Vector2};
use serde_json::Value;

use crate::input_context::{InputContext, Pointer2D};

/// Head rotation for a yaw/pitch (degrees), matching the desktop runtime's
/// camera convention (`camera_forward` / `camera_rotation`): at `yaw=pitch=0`
/// the forward is `(1,0,0)` fed through `look_at_rh`, i.e. looking toward `-X`.
/// The SAME rotation drives both the render camera and `InputContext.head`, so
/// the flat viewmodel (placed relative to the head) and the rendered view always
/// agree - otherwise the viewmodel renders off-axis as a giant side-on slab.
pub fn head_rotation_from_yaw_pitch(yaw_deg: f32, pitch_deg: f32) -> Quaternion<f32> {
    use cgmath::{Decomposed, Rotation, Transform, Vector3, point3, vec3};
    let (yaw, pitch) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    let forward = point3(
        yaw.cos() * pitch.cos(),
        pitch.sin(),
        yaw.sin() * pitch.cos(),
    );
    let up = vec3(0.0, 1.0, 0.0);
    let decomposed: Decomposed<Vector3<f32>, Quaternion<f32>> =
        Transform::look_at_rh(forward, point3(0.0, 0.0, 0.0), up);
    decomposed.rot.invert()
}

/// One line describing every recognized input channel, used in error messages so
/// a bad request is self-documenting. Includes the locomotion semantics (which
/// stick does what) since that is the game's convention, not guessable.
pub fn input_channels_help() -> &'static str {
    "valid channels: head.rotation [x,y,z,w], head.look [yaw_deg,pitch_deg], \
     pointer.position [x,y] in [0,1] (origin top-left; null clears), pointer.pressed 0|1, \
     {left,right}_hand.{trigger,squeeze,a} <number 0..1>, \
     {left,right}_hand.thumbstick [x,y], \
     {left,right}_hand.position [x,y,z] (pawn-local), \
     {left,right}_hand.rotation [x,y,z,w], \
     crouch 0|1 (stand-up refused without headroom), \
     jump 0|1 (held; launches on the grounded rising edge); \
     locomotion: right_hand.thumbstick [strafe, forward] moves the player, \
     left_hand.thumbstick.x turns, left_hand.thumbstick.y flies up/down"
}

/// Patch a single channel of an `InputContext`, returning an actionable error for
/// an unrecognized channel or a value of the wrong shape. These are level-held
/// values (a held trigger, a fixed aim), unlike the edge-triggered discrete
/// `InputAction`s.
///
/// Recognized channels:
/// - `head.rotation`                : `[x, y, z, w]` quaternion
/// - `head.look`                    : `[yaw_deg, pitch_deg]` convenience (forward = -Z).
///   **Pitch is POSITIVE DOWN** - `+60` looks at the floor, `-60` at the ceiling
///   - matching the desktop runtime, where mouse-down increments pitch. The same
///   rotation drives movement, so on a ladder `+pitch` descends and `-pitch`
///   ascends - a test that pitches `-60` to "look down" at a ladder climbs UP,
///   which is how issue #603's repro was written.
/// - `{left,right}_hand.trigger`    : number in [0, 1] (alias `trigger_value`)
/// - `{left,right}_hand.squeeze`    : number in [0, 1] (alias `squeeze_value`)
/// - `{left,right}_hand.a`          : number in [0, 1] (alias `a_value`)
/// - `{left,right}_hand.thumbstick` : `[x, y]`
/// - `jump`                         : number `0` or `1`
pub fn apply_input_patch(
    input: &mut InputContext,
    channel: &str,
    value: &Value,
) -> Result<(), String> {
    // Parse a scalar/array value for `channel`, attributing a clear error to the
    // channel and value shape when it doesn't match.
    // Non-finite values are rejected here rather than downstream: an override is
    // re-applied every frame, so one NaN would poison aim/locomotion until it is
    // released, with no clue where it came from.
    fn num(channel: &str, v: &Value) -> Result<f32, String> {
        let n = v
            .as_f64()
            .map(|f| f as f32)
            .ok_or_else(|| format!("channel '{channel}' expects a number, got {v}"))?;
        if !n.is_finite() {
            return Err(format!(
                "channel '{channel}' expects a finite number, got {v}"
            ));
        }
        Ok(n)
    }
    fn arr(channel: &str, v: &Value, n: usize) -> Result<Vec<f32>, String> {
        let a = v.as_array().ok_or_else(|| {
            format!("channel '{channel}' expects an array of {n} numbers, got {v}")
        })?;
        if a.len() != n {
            return Err(format!(
                "channel '{channel}' expects {n} numbers, got {} ({v})",
                a.len()
            ));
        }
        a.iter()
            .map(|e| num(channel, e))
            .collect::<Result<Vec<_>, _>>()
    }

    // Hand channels: "<left|right>_hand.<field>"
    if let Some((side, field)) = channel.split_once("_hand.") {
        let hand = match side {
            "left" => &mut input.left_hand,
            "right" => &mut input.right_hand,
            _ => {
                return Err(format!(
                    "unknown input channel '{channel}'; {}",
                    input_channels_help()
                ));
            }
        };
        return match field {
            "trigger" | "trigger_value" => {
                hand.trigger_value = num(channel, value)?;
                Ok(())
            }
            "squeeze" | "squeeze_value" => {
                hand.squeeze_value = num(channel, value)?;
                Ok(())
            }
            "a" | "a_value" => {
                hand.a_value = num(channel, value)?;
                Ok(())
            }
            "thumbstick" => {
                let a = arr(channel, value, 2)?;
                hand.thumbstick = Vector2::new(a[0], a[1]);
                Ok(())
            }
            "position" => {
                let p = arr(channel, value, 3)?;
                hand.position = cgmath::vec3(p[0], p[1], p[2]);
                Ok(())
            }
            "rotation" => {
                let q = arr(channel, value, 4)?;
                let rotation = Quaternion::new(q[3], q[0], q[1], q[2]);
                // A zero quaternion normalizes to NaN, which would silently
                // corrupt the hand transform every frame.
                if rotation.magnitude2() <= 0.0 {
                    return Err(format!(
                        "channel '{channel}' expects a non-zero quaternion, got {value}"
                    ));
                }
                hand.rotation = rotation.normalize();
                Ok(())
            }
            _ => Err(format!(
                "unknown input channel '{channel}'; {}",
                input_channels_help()
            )),
        };
    }

    match channel {
        "head.rotation" => {
            let q = arr(channel, value, 4)?;
            input.head.rotation = Quaternion::new(q[3], q[0], q[1], q[2]);
            Ok(())
        }
        // Desktop camera convention (yaw=pitch=0 looks toward -X); drives both
        // the render camera and the viewmodel. Convenient for pointing the
        // camera without hand-authoring a quaternion.
        "head.look" => {
            let yp = arr(channel, value, 2)?;
            input.head.rotation = head_rotation_from_yaw_pitch(yp[0], yp[1]);
            Ok(())
        }
        // Flat-mode 2D pointer (cursor). Position is normalized [0,1] per
        // axis, origin top-left; setting either channel materializes the
        // pointer (it is `None` until first set). `pointer.position: null`
        // clears the pointer back to `None` (scenes branch on Some/None, so
        // the cleared state must be reachable for testing).
        "pointer.position" => {
            if value.is_null() {
                input.pointer = None;
                return Ok(());
            }
            let xy = arr(channel, value, 2)?;
            if !(xy[0].is_finite() && xy[1].is_finite())
                || !(0.0..=1.0).contains(&xy[0])
                || !(0.0..=1.0).contains(&xy[1])
            {
                return Err(format!(
                    "channel '{channel}' expects normalized coordinates in [0,1], got [{}, {}]",
                    xy[0], xy[1]
                ));
            }
            let pointer = input.pointer.get_or_insert(Pointer2D {
                position: cgmath::vec2(0.0, 0.0),
                pressed: false,
            });
            pointer.position = cgmath::vec2(xy[0], xy[1]);
            Ok(())
        }
        "pointer.pressed" => {
            let pressed = match num(channel, value)? {
                v if v == 0.0 => false,
                v if v == 1.0 => true,
                v => {
                    return Err(format!("channel '{channel}' expects 0 or 1, got {v}"));
                }
            };
            let pointer = input.pointer.get_or_insert(Pointer2D {
                position: cgmath::vec2(0.0, 0.0),
                pressed: false,
            });
            pointer.pressed = pressed;
            Ok(())
        }
        // Crouch request: like the desktop LeftControl hold. The ACTUAL state
        // can lag (standing up is refused without headroom); observe it via
        // the player's body y (the collider center drops when crouched).
        "crouch" => {
            input.crouch = match num(channel, value)? {
                v if v == 0.0 => false,
                v if v == 1.0 => true,
                v => {
                    return Err(format!("channel '{channel}' expects 0 or 1, got {v}"));
                }
            };
            Ok(())
        }
        "jump" => {
            input.jump = match num(channel, value)? {
                v if v == 0.0 => false,
                v if v == 1.0 => true,
                v => {
                    return Err(format!("channel '{channel}' expects 0 or 1, got {v}"));
                }
            };
            Ok(())
        }
        _ => Err(format!(
            "unknown input channel '{channel}'; {}",
            input_channels_help()
        )),
    }
}

/// Parse a remote-control request body into `(channel, value)` patches,
/// accepting either shape:
/// - explicit:  `{"channel": "right_hand.trigger", "value": 1.0}`
/// - map form:  `{"right_hand.trigger": 1.0, "head.look": [30, 0]}`
///
/// The map form is detected when the object does NOT have both `channel` and
/// `value` keys. Returns an actionable error (not a silent empty patch) when the
/// body isn't a usable object, so a malformed request fails loudly.
pub fn parse_input_patches(body: &Value) -> Result<Vec<(String, Value)>, String> {
    let obj = body.as_object().ok_or_else(|| {
        format!(
            "request body must be a JSON object, e.g. {{\"channel\":\"right_hand.trigger\",\"value\":1.0}} \
             or {{\"right_hand.trigger\":1.0}}; {}",
            input_channels_help()
        )
    })?;

    // Explicit {channel, value} form.
    if obj.contains_key("channel") && obj.contains_key("value") {
        let channel = obj["channel"]
            .as_str()
            .ok_or_else(|| "\"channel\" must be a string".to_string())?
            .to_string();
        return Ok(vec![(channel, obj["value"].clone())]);
    }

    if obj.is_empty() {
        return Err(format!(
            "no input channels in request; {}",
            input_channels_help()
        ));
    }

    // Map form: every key is a channel name.
    Ok(obj
        .iter()
        .map(|(channel, value)| (channel.clone(), value.clone()))
        .collect())
}

fn unknown_channel(channel: &str) -> String {
    format!(
        "unknown input channel '{channel}'; {}",
        input_channels_help()
    )
}

/// The canonical name for a channel, plus whether it was the `head.look`
/// spelling (which needs its value converted, see `canonical_channel_value`).
/// Aliases collapse - `right_hand.trigger_value` is `right_hand.trigger`, and
/// `head.look` is `head.rotation` - so two spellings of one field can never be
/// held as two independent claims.
fn canonical_channel(channel: &str) -> Result<(String, bool), String> {
    if let Some((side, field)) = channel.split_once("_hand.") {
        if side != "left" && side != "right" {
            return Err(unknown_channel(channel));
        }
        let field = match field {
            "trigger" | "trigger_value" => "trigger",
            "squeeze" | "squeeze_value" => "squeeze",
            "a" | "a_value" => "a",
            "thumbstick" | "position" | "rotation" => field,
            _ => return Err(unknown_channel(channel)),
        };
        return Ok((format!("{side}_hand.{field}"), false));
    }
    match channel {
        "head.look" => Ok(("head.rotation".to_owned(), true)),
        "head.rotation" | "pointer.position" | "pointer.pressed" | "crouch" | "jump" => {
            Ok((channel.to_owned(), false))
        }
        _ => Err(unknown_channel(channel)),
    }
}

/// Canonicalize a channel *and* its value: `head.look` is resolved to the
/// quaternion it means, so it is stored as the same claim `head.rotation` would
/// make.
fn canonical_channel_value(channel: &str, value: Value) -> Result<(String, Value), String> {
    let (canonical, is_look) = canonical_channel(channel)?;
    if is_look {
        let mut scratch = InputContext::default();
        apply_input_patch(&mut scratch, channel, &value)?;
        let r = scratch.head.rotation;
        return Ok((canonical, serde_json::json!([r.v.x, r.v.y, r.v.z, r.s])));
    }
    Ok((canonical, value))
}

/// Channels an agent has claimed, layered over a runtime-built `InputContext`.
///
/// This is the *override* half of remote control, used where the runtime cannot
/// simply own the context: the oculus runtime rebuilds `InputContext` from
/// OpenXR every frame, so a patch has to be re-applied after that rebuild. Only
/// the claimed channels are overwritten - everything else keeps the live
/// controller value, so a human can still be in the headset while an agent
/// nudges one channel.
///
/// A patch of JSON `null` *releases* the channel back to the controller (the one
/// exception to [`apply_input_patch`]'s value shapes, where `pointer.position:
/// null` means "no pointer" - the override layer has no way to express "hold the
/// pointer cleared", and releasing is what a remote driver actually wants).
#[derive(Debug, Default, Clone)]
pub struct InputOverrides {
    channels: BTreeMap<String, Value>,
}

impl InputOverrides {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim (or, with `null`, release) a channel. The value is validated
    /// eagerly against a scratch context so a bad request is rejected at the
    /// call rather than silently mis-driving the game every frame after.
    ///
    /// Channels are stored under a canonical name, so an alias (`trigger_value`)
    /// or an equivalent spelling (`head.look` for `head.rotation`) replaces the
    /// claim it duplicates instead of latching a second, unreleasable copy of the
    /// same field.
    pub fn set(&mut self, channel: &str, value: Value) -> Result<(), String> {
        if value.is_null() {
            // A release names a channel too, and a typo'd one ("thumstick")
            // would otherwise report success while the real override stayed
            // latched - the exact failure a remote driver cannot see.
            let canonical = canonical_channel(channel)?.0;
            return match self.channels.remove(&canonical) {
                Some(_) => Ok(()),
                None => Err(format!("channel '{channel}' is not currently overridden")),
            };
        }
        let (canonical, value) = canonical_channel_value(channel, value)?;
        apply_input_patch(&mut InputContext::default(), &canonical, &value)?;
        self.channels.insert(canonical, value);
        Ok(())
    }

    /// Release every claimed channel.
    pub fn clear(&mut self) {
        self.channels.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    /// The currently claimed channels, for reporting back to the caller.
    pub fn channels(&self) -> &BTreeMap<String, Value> {
        &self.channels
    }

    /// Overwrite the claimed channels on a freshly built context. Values were
    /// validated in `set`, so failures here are impossible in practice and are
    /// dropped rather than propagated (there is no caller to tell mid-frame).
    pub fn apply(&self, input: &mut InputContext) {
        for (channel, value) in &self.channels {
            let _ = apply_input_patch(input, channel, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patch_sets_channels() {
        let mut input = InputContext::default();
        apply_input_patch(&mut input, "right_hand.trigger", &json!(1.0)).unwrap();
        apply_input_patch(&mut input, "left_hand.thumbstick", &json!([0.5, -0.25])).unwrap();
        assert_eq!(input.right_hand.trigger_value, 1.0);
        assert_eq!(input.left_hand.thumbstick, Vector2::new(0.5, -0.25));
    }

    #[test]
    fn patch_rejects_unknown_channel_and_bad_shape() {
        let mut input = InputContext::default();
        let err = apply_input_patch(&mut input, "right_hand.nope", &json!(1.0)).unwrap_err();
        assert!(err.contains("unknown input channel"));
        let err = apply_input_patch(&mut input, "head.look", &json!(1.0)).unwrap_err();
        assert!(err.contains("expects an array"));
    }

    #[test]
    fn parse_accepts_explicit_and_map_forms() {
        assert_eq!(
            parse_input_patches(&json!({"channel": "jump", "value": 1.0})).unwrap(),
            vec![("jump".to_owned(), json!(1.0))]
        );
        let map = parse_input_patches(&json!({"jump": 1.0, "crouch": 0.0})).unwrap();
        assert_eq!(map.len(), 2);
        assert!(parse_input_patches(&json!([])).is_err());
        assert!(parse_input_patches(&json!({})).is_err());
    }

    /// The override layer must be additive: a channel nobody claimed keeps the
    /// value the runtime built from the real controllers this frame.
    #[test]
    fn overrides_leave_unclaimed_channels_alone() {
        let mut overrides = InputOverrides::new();
        overrides.set("right_hand.trigger", json!(1.0)).unwrap();

        let mut input = InputContext::default();
        input.right_hand.trigger_value = 0.0;
        input.left_hand.trigger_value = 0.42;
        input.left_hand.thumbstick = Vector2::new(0.1, 0.2);
        overrides.apply(&mut input);

        assert_eq!(input.right_hand.trigger_value, 1.0);
        assert_eq!(input.left_hand.trigger_value, 0.42);
        assert_eq!(input.left_hand.thumbstick, Vector2::new(0.1, 0.2));
    }

    /// A claimed channel wins every frame, even as the live value changes...
    #[test]
    fn overrides_win_until_released() {
        let mut overrides = InputOverrides::new();
        overrides
            .set("left_hand.thumbstick", json!([0.0, 1.0]))
            .unwrap();

        let mut input = InputContext::default();
        input.left_hand.thumbstick = Vector2::new(-1.0, -1.0);
        overrides.apply(&mut input);
        assert_eq!(input.left_hand.thumbstick, Vector2::new(0.0, 1.0));

        // ...and `null` releases it back to the live controller value.
        overrides.set("left_hand.thumbstick", Value::Null).unwrap();
        assert!(overrides.is_empty());
        let mut input = InputContext::default();
        input.left_hand.thumbstick = Vector2::new(-1.0, -1.0);
        overrides.apply(&mut input);
        assert_eq!(input.left_hand.thumbstick, Vector2::new(-1.0, -1.0));
    }

    /// Two spellings of one field must be one claim, or releasing the spelling
    /// you remember leaves the other latched forever.
    #[test]
    fn overrides_collapse_aliases_and_equivalent_spellings() {
        let mut overrides = InputOverrides::new();
        overrides
            .set("right_hand.trigger_value", json!(1.0))
            .unwrap();
        overrides.set("right_hand.trigger", json!(0.5)).unwrap();
        assert_eq!(overrides.channels().len(), 1);
        overrides.set("right_hand.trigger", Value::Null).unwrap();
        assert!(overrides.is_empty());

        overrides.set("head.look", json!([30.0, 0.0])).unwrap();
        overrides
            .set("head.rotation", json!([0.0, 0.0, 0.0, 1.0]))
            .unwrap();
        assert_eq!(overrides.channels().len(), 1);
        assert!(overrides.channels().contains_key("head.rotation"));
    }

    /// A misspelled release used to report success while the real override
    /// stayed latched - invisible to a remote driver.
    #[test]
    fn releasing_an_unknown_or_unclaimed_channel_is_an_error() {
        let mut overrides = InputOverrides::new();
        overrides
            .set("left_hand.thumbstick", json!([0.0, 1.0]))
            .unwrap();
        assert!(overrides.set("left_hand.thumstick", Value::Null).is_err());
        assert_eq!(overrides.channels().len(), 1);
        assert!(overrides.set("jump", Value::Null).is_err());
        overrides.set("left_hand.thumbstick", Value::Null).unwrap();
        assert!(overrides.is_empty());
    }

    /// Non-finite numbers and a degenerate quaternion would be re-applied every
    /// frame, poisoning aim with no trace of where it came from.
    #[test]
    fn patch_rejects_non_finite_and_degenerate_values() {
        let mut input = InputContext::default();
        // 1e300 is a finite f64 that overflows to +inf as an f32.
        assert!(apply_input_patch(&mut input, "right_hand.trigger", &json!(1e300)).is_err());
        assert!(
            apply_input_patch(&mut input, "left_hand.position", &json!([1.0, 1e300, 0.0])).is_err()
        );
        assert!(
            apply_input_patch(
                &mut input,
                "left_hand.rotation",
                &json!([0.0, 0.0, 0.0, 0.0])
            )
            .is_err()
        );
    }

    #[test]
    fn overrides_validate_on_set() {
        let mut overrides = InputOverrides::new();
        assert!(overrides.set("bogus.channel", json!(1.0)).is_err());
        assert!(overrides.set("jump", json!("yes")).is_err());
        assert!(overrides.is_empty());
    }

    #[test]
    fn clear_releases_everything() {
        let mut overrides = InputOverrides::new();
        overrides.set("jump", json!(1.0)).unwrap();
        overrides.set("crouch", json!(1.0)).unwrap();
        assert_eq!(overrides.channels().len(), 2);
        overrides.clear();
        assert!(overrides.is_empty());
    }
}
