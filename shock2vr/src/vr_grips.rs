//! Where each model sits in a VR hand, as authored data.
//!
//! `assets/vr_grips.json` maps a lowercased `PropModelName` to a **grip
//! profile**: how the model is turned in the hand, optionally where its origin
//! goes, and optionally the grip family, per-finger curls and render scale to
//! hold it at. Anything the file leaves out is measured off the model's own box
//! by [`crate::hand_seat`], so a pickup nobody has tuned is still held rather
//! than skewered on the wrist.
//!
//! Hand space is [`crate::hand_seat`]'s: +X thumb side, +Y out of the back of
//! the hand, -Z along the fingers, world units.
//!
//! The registry is global and live: the debug runtime's `/v1/vr/grip` writes
//! into it and bumps [`generation`], which is how a tuner re-seats a held item
//! without a reload.

use std::collections::BTreeMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use cgmath::{Deg, Quaternion, Rotation3, Vector3, Zero, vec3};
use engine::assets::{asset_cache::AssetCache, text_importer::TEXT_IMPORTER};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use crate::hand_fit::GripFamily;
use crate::hand_pose::FingerAmounts;
use crate::hand_seat::{self, Seat};
use crate::vr_config::Handedness;

/// The authored profile file, resolved through the asset mounts like any other
/// bundle asset so it loads on device as well as off it.
pub const GRIP_PROFILES_ASSET: &str = "vr_grips.json";

/// How a left hand derives its grip from the authored (right-hand) one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mirror {
    /// The model is drawn unmirrored in either hand, so both hands use the
    /// authored grip as-is - every world model.
    #[default]
    Same,
    /// The model is drawn reflected for the left hand
    /// ([`Handedness::gun_mirror`]), so the thumb-side component of the offset
    /// reflects with it and the reflected model seats on the left palm exactly
    /// where the authored one seats on the right - the 25AE `_h` set.
    FlipX,
}

/// One model's authored grip. Every field but `rotation_deg` is optional; what
/// is missing is measured off the model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GripProfile {
    /// How the model is turned in the hand, as XYZ Euler degrees applied
    /// Z * Y * X. Omitted means "lay the long axis across the palm".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_deg: Option<[f32; 3]>,
    /// Where the model's origin goes in hand space. Omitted means "drop it onto
    /// the palm", which is what an untuned pickup wants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<[f32; 3]>,
    /// The grip family to hold it in, overriding what the box measures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Per-finger curls (0 open, 1 fist) that replace the fit's answer outright
    /// - the escape hatch for a shape the contact fit reads wrong.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingers: Option<[f32; 5]>,
    /// A uniform scale applied to the held geometry (not to the dropped item).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f32>,
    #[serde(default, skip_serializing_if = "is_default_mirror")]
    pub mirror: Mirror,
}

fn is_default_mirror(mirror: &Mirror) -> bool {
    *mirror == Mirror::Same
}

/// Where a resolved grip came from - what a tuner needs to know before it
/// starts nudging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GripSource {
    /// Straight out of the profile file (or a live tuner edit of it).
    Profile,
    /// The file turned the model but left it to the palm, or said nothing at
    /// all: the seat was measured off the model's box.
    Heuristic,
    /// Nothing is authored and the model's box has not been measured yet - the
    /// model is drawn on the wrist, as it was before there were profiles.
    Unseated,
}

/// A model's grip, everything resolved.
#[derive(Clone, Copy, Debug)]
pub struct ResolvedGrip {
    pub offset: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    /// The uniform scale the held geometry is drawn at.
    pub scale: f32,
    pub family: Option<GripFamily>,
    pub fingers: Option<FingerAmounts>,
    pub source: GripSource,
}

static PROFILES: Lazy<RwLock<BTreeMap<String, GripProfile>>> =
    Lazy::new(|| RwLock::new(BTreeMap::new()));

/// Seats measured off a model's own box, keyed by model name. Filled in by
/// [`remember_measured_seat`] the first time a hand can see the geometry; read
/// by every later placement, which has no asset cache of its own.
/// `None` records a model with no mesh to measure (a skinned melee rig, a
/// missing `.BIN`): remembering the miss is what keeps a held item off the
/// asset cache's miss path, which memoizes only successes.
static MEASURED: Lazy<RwLock<BTreeMap<String, Option<(Seat, GripFamily)>>>> =
    Lazy::new(|| RwLock::new(BTreeMap::new()));

/// Load [`GRIP_PROFILES_ASSET`] once, through the asset mounts. The game loop
/// calls this rather than a startup path having to remember to: the profiles
/// have to be in place before the first hand places anything, and a `Once` is
/// cheaper than the bookkeeping to prove some earlier call site always ran.
pub fn ensure_loaded(asset_cache: &mut AssetCache) {
    // Latched on SUCCESS, not on the attempt: a `Once` would mark the load done
    // even when the asset could not be resolved, leaving every grip in the
    // process unseated for the rest of the run behind one warning.
    if is_loaded() {
        return;
    }
    let Some(text) = asset_cache.get_opt::<_, String, _>(&TEXT_IMPORTER, GRIP_PROFILES_ASSET)
    else {
        tracing::warn!(
            "{GRIP_PROFILES_ASSET} not found; every held model is seated from its own geometry"
        );
        return;
    };
    match parse(&text) {
        Ok(profiles) => {
            tracing::info!("loaded {} VR grip profiles", profiles.len());
            set_profiles(profiles);
            LOAD_SUCCEEDED.store(true, Ordering::Relaxed);
        }
        Err(error) => tracing::error!("{GRIP_PROFILES_ASSET} is not valid JSON: {error}"),
    }
}

/// Whether [`ensure_loaded`] has actually put the authored file in the
/// registry. Saving before it has is how an empty registry would overwrite the
/// asset with `{}`.
static LOAD_SUCCEEDED: AtomicBool = AtomicBool::new(false);

/// Whether the authored profiles are in the registry.
pub fn is_loaded() -> bool {
    LOAD_SUCCEEDED.load(Ordering::Relaxed)
}

/// Bumped whenever an edit changes what [`resolve`] would answer. A cached fit
/// is stale once this moves.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// The current profile generation; caches keyed on a grip must invalidate when
/// it changes.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Replace the whole registry - what loading the asset does, and what a test
/// does to start from a known state.
pub fn set_profiles(profiles: BTreeMap<String, GripProfile>) {
    *PROFILES.write().unwrap() = profiles;
    MEASURED.write().unwrap().clear();
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Parse the profile file. Keys are lowercased on the way in so a hand-edited
/// file cannot miss by case.
pub fn parse(json: &str) -> Result<BTreeMap<String, GripProfile>, serde_json::Error> {
    let parsed: BTreeMap<String, GripProfile> = serde_json::from_str(json)?;
    Ok(parsed
        .into_iter()
        .map(|(name, profile)| (name.to_ascii_lowercase(), profile))
        .collect())
}

/// The registry as a file, keys sorted (the map is a `BTreeMap`) so saving an
/// unchanged registry reproduces the file it was loaded from.
pub fn to_json() -> String {
    let profiles = PROFILES.read().unwrap();
    serde_json::to_string_pretty(&*profiles).unwrap_or_default() + "\n"
}

/// One model's authored profile, if it has one.
pub fn profile(model_name: &str) -> Option<GripProfile> {
    PROFILES
        .read()
        .unwrap()
        .get(model_name.to_ascii_lowercase().as_str())
        .cloned()
}

/// Author (or overwrite) one model's profile - the tuner's write path.
pub fn set_profile(model_name: &str, profile: GripProfile) {
    let key = model_name.to_ascii_lowercase();
    PROFILES.write().unwrap().insert(key.clone(), profile);
    // A newly authored rotation changes what the measured drop would be.
    MEASURED.write().unwrap().remove(&key);
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Forget one model's authored profile, back to whatever its own box says.
pub fn clear_profile(model_name: &str) {
    let key = model_name.to_ascii_lowercase();
    PROFILES.write().unwrap().remove(&key);
    MEASURED.write().unwrap().remove(&key);
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Record the seat measured off `model_name`'s own box - the model-space bounds
/// of the geometry as it is drawn, at the scale its profile asks for.
///
/// Called from wherever the mesh is reachable (the wield's own asset cache);
/// every later placement reads the answer out of the registry.
pub fn remember_measured_seat(model_name: &str, min: Vector3<f32>, max: Vector3<f32>) {
    remember(model_name, Some((min, max)));
}

/// Record that `model_name` has no box to measure - a skinned rig, or a model
/// whose mesh never loaded. Remembering the miss is the point: without it every
/// frame the item is held re-walks the asset mounts looking for it.
pub fn remember_unmeasurable(model_name: &str) {
    remember(model_name, None);
}

fn remember(model_name: &str, box_of: Option<(Vector3<f32>, Vector3<f32>)>) {
    let key = model_name.to_ascii_lowercase();
    if !needs_measurement(&key) {
        return;
    }
    // Measured even for a model whose seat is fully authored: the box is also
    // where the grip *family* comes from, and an authored offset must not cost
    // the fingers their envelope.
    let solved_at = generation();
    let rotation = profile(&key).as_ref().and_then(rotation_of);
    let measured = box_of.map(|(min, max)| hand_seat::seat(min, max, rotation));

    // An edit that landed while this was solving already cleared the entry and
    // changed the turn it was solved against; drop the stale answer rather than
    // parking it where nothing would ever re-measure it.
    let mut cache = MEASURED.write().unwrap();
    if generation() != solved_at {
        return;
    }
    cache.insert(key, measured);
    drop(cache);
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Whether `model_name`'s seat still has to be measured off its geometry.
pub fn needs_measurement(model_name: &str) -> bool {
    !MEASURED
        .read()
        .unwrap()
        .contains_key(model_name.to_ascii_lowercase().as_str())
}

/// The scale a held `model_name` is drawn at, from its profile - 1.0 for
/// anything unprofiled. Independent of the gun wield's own life-size shrink,
/// which is a dev param rather than per-model data.
pub fn render_scale(model_name: &str) -> f32 {
    profile(model_name)
        .and_then(|profile| profile.scale)
        .unwrap_or(1.0)
}

/// Everything about how `handedness` holds `model_name`.
pub fn resolve(model_name: &str, handedness: Handedness) -> ResolvedGrip {
    let authored = profile(model_name);
    let measured = MEASURED
        .read()
        .unwrap()
        .get(model_name.to_ascii_lowercase().as_str())
        .copied()
        .flatten();

    // Offset and rotation resolve independently: authoring one must not throw
    // away the measurement of the other (nudging an offset would otherwise
    // reset a model's turn to no turn at all).
    let authored_offset = authored.as_ref().and_then(|p| p.offset).map(to_vec);
    let source = match (authored_offset, measured) {
        (Some(_), _) => GripSource::Profile,
        (None, Some(_)) => GripSource::Heuristic,
        (None, None) => GripSource::Unseated,
    };
    let offset = authored_offset
        .or_else(|| measured.map(|(seat, _)| seat.offset))
        .unwrap_or_else(Vector3::zero);
    let rotation = authored
        .as_ref()
        .and_then(rotation_of)
        .or_else(|| measured.map(|(seat, _)| seat.rotation))
        .unwrap_or_else(no_turn);

    // The left hand's grip on a mirrored model is the right one reflected, so
    // the reflected geometry seats where the authored one does.
    let offset = match (handedness, authored.as_ref().map(|p| p.mirror)) {
        (Handedness::Left, Some(Mirror::FlipX)) => vec3(-offset.x, offset.y, offset.z),
        _ => offset,
    };

    ResolvedGrip {
        offset,
        rotation,
        scale: authored.as_ref().and_then(|p| p.scale).unwrap_or(1.0),
        // Authored first, then the one family the code derives rather than
        // measures: a gun's grip measures like any other handle, but the index
        // belongs on the trigger.
        family: authored
            .as_ref()
            .and_then(|p| p.family.as_deref())
            .and_then(GripFamily::from_str)
            .or_else(|| {
                crate::vr_config::is_vr_gun_view_model(model_name).then_some(GripFamily::Trigger)
            })
            .or_else(|| measured.map(|(_, family)| family)),
        fingers: authored.as_ref().and_then(|p| p.fingers).map(to_fingers),
        source,
    }
}

fn no_turn() -> Quaternion<f32> {
    Quaternion::new(1.0, 0.0, 0.0, 0.0)
}

fn to_vec(values: [f32; 3]) -> Vector3<f32> {
    vec3(values[0], values[1], values[2])
}

fn to_fingers(values: [f32; 5]) -> FingerAmounts {
    FingerAmounts {
        thumb: values[0],
        index: values[1],
        middle: values[2],
        ring: values[3],
        pinky: values[4],
    }
}

/// A profile's turn as a quaternion. Euler degrees composed Z * Y * X, so a
/// yaw-only entry is exactly `Quaternion::from_angle_y`.
fn rotation_of(profile: &GripProfile) -> Option<Quaternion<f32>> {
    let [x, y, z] = profile.rotation_deg?;
    Some(
        Quaternion::from_angle_z(Deg(z))
            * Quaternion::from_angle_y(Deg(y))
            * Quaternion::from_angle_x(Deg(x)),
    )
}

/// The shipped profile file, compiled in - so a test can exercise the real
/// profiles without the asset mounts.
#[cfg(test)]
pub const SHIPPED_PROFILES: &str = include_str!("../../assets/vr_grips.json");

/// The registry is process-global, so every test that reads or writes it holds
/// this while it does.
#[cfg(test)]
pub fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Put the shipped profiles in the registry. Hold [`test_guard`] across the
/// test that calls this.
#[cfg(test)]
pub fn load_shipped_for_test() {
    set_profiles(parse(SHIPPED_PROFILES).expect("the shipped grip profiles parse"));
    LOAD_SUCCEEDED.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::InnerSpace;

    fn with_profiles<T>(json: &str, body: impl FnOnce() -> T) -> T {
        let _guard = test_guard();
        set_profiles(parse(json).expect("test profiles parse"));
        let result = body();
        set_profiles(BTreeMap::new());
        result
    }

    #[test]
    fn an_authored_offset_and_turn_resolve_verbatim() {
        with_profiles(
            r#"{"atek_h": {"rotation_deg": [0, -90, 0], "offset": [0, 0.073, 0.02], "mirror": "flip_x"}}"#,
            || {
                let right = resolve("atek_h", Handedness::Right);

                assert_eq!(right.source, GripSource::Profile);
                assert!((right.offset - vec3(0.0, 0.073, 0.02)).magnitude() < 1e-6);
                let expected = Quaternion::from_angle_y(Deg(-90.0));
                assert!((right.rotation - expected).magnitude() < 1e-6);
            },
        );
    }

    #[test]
    fn a_mirrored_models_left_grip_reflects_the_thumb_side_offset() {
        with_profiles(
            r#"{"gun": {"offset": [0.03, 0.07, 0.0], "mirror": "flip_x"},
                "prop": {"offset": [0.03, 0.07, 0.0]}}"#,
            || {
                assert_eq!(resolve("gun", Handedness::Left).offset.x, -0.03);
                assert_eq!(resolve("prop", Handedness::Left).offset.x, 0.03);
            },
        );
    }

    /// The point of the slice: a model nobody authored still lands in the palm
    /// once its geometry has been seen.
    #[test]
    fn an_unprofiled_model_is_seated_from_its_own_box() {
        with_profiles("{}", || {
            assert_eq!(
                resolve("mug", Handedness::Right).source,
                GripSource::Unseated
            );
            assert!(needs_measurement("mug"));

            remember_measured_seat("mug", vec3(-0.1, -0.05, -0.05), vec3(0.1, 0.05, 0.05));

            let grip = resolve("mug", Handedness::Right);
            assert_eq!(grip.source, GripSource::Heuristic);
            assert!(grip.offset.magnitude() > 0.0);
            assert!(grip.family.is_some());
            assert!(!needs_measurement("mug"));
        });
    }

    /// A profile that only turns the model still gets its drop measured - that
    /// is what the legacy rotation-only entries need.
    #[test]
    fn a_turn_only_profile_keeps_its_turn_and_takes_a_measured_drop() {
        with_profiles(r#"{"laser": {"rotation_deg": [0, -90, 0]}}"#, || {
            remember_measured_seat("laser", vec3(-0.3, -0.05, -0.05), vec3(0.3, 0.05, 0.05));

            let grip = resolve("laser", Handedness::Right);
            assert_eq!(grip.source, GripSource::Heuristic);
            let expected = Quaternion::from_angle_y(Deg(-90.0));
            assert!((grip.rotation - expected).magnitude() < 1e-6);
            assert!(grip.offset.magnitude() > 0.0);
        });
    }

    /// An authored offset is the last word about *placement* - but the box is
    /// still measured, because that is where the grip family comes from, and an
    /// authored seat must not cost the fingers their envelope.
    #[test]
    fn an_authored_offset_is_never_measured_over_but_still_gets_a_family() {
        with_profiles(r#"{"atek_h": {"offset": [0, 0.073, 0.02]}}"#, || {
            assert!(needs_measurement("atek_h"));
            remember_measured_seat("atek_h", vec3(-0.1, -0.1, -0.1), vec3(0.1, 0.1, 0.1));

            let grip = resolve("atek_h", Handedness::Right);
            assert_eq!(grip.source, GripSource::Profile);
            assert_eq!(grip.offset, vec3(0.0, 0.073, 0.02));
            assert!(grip.family.is_some(), "the box still names a family");
        });
    }

    #[test]
    fn a_profile_file_round_trips_through_save() {
        let json = r#"{"mug": {"rotation_deg": [0.0, 180.0, 0.0], "offset": [0.0, -0.05, -0.09], "family": "cylindrical", "fingers": [0.9, 0.8, 0.8, 0.8, 0.8], "scale": 0.7, "mirror": "flip_x"}}"#;

        with_profiles(json, || {
            let saved = to_json();
            assert_eq!(parse(&saved).unwrap(), parse(json).unwrap());
            // Keys are sorted, so a save is byte-stable across runs.
            assert_eq!(to_json(), saved);
        });
    }

    #[test]
    fn an_edit_bumps_the_generation() {
        with_profiles("{}", || {
            let before = generation();
            set_profile("mug", GripProfile::default());
            assert!(generation() > before);
        });
    }
}
