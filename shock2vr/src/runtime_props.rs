/**
 * `runtime_props.rs`
 *
 * Runtime properties are properties that are convenience properties for running the game.
 *
 * Notably:
 * - They are not part of SS2 / Dark - just convenience properties for implementing the game.
 * - They are not serialized / deserialized
 */
use cgmath::{Matrix4, Point3, Vector3};
use dark::ss2_bin_obj_loader::Vhot;
use serde::{Deserialize, Serialize};
use shipyard::Component;

// RuntimePropGazeAmount - track how much the player is gazing at a prop
#[derive(Component)]
#[allow(dead_code)]
pub struct RuntimePropGazeAmount(pub f32);

#[derive(Component)]
pub struct RuntimePropTransform(pub Matrix4<f32>);

// Name of the behavior an AI script is currently running (published by the
// script when it changes; read by debug introspection)
#[derive(Component)]
pub struct RuntimePropAIBehavior(pub String);

// What an AI knows about its target: where it last saw the player and
// whether it can see them right now. Published by the AI script each frame
// (the position tracks the player while visible and freezes when sight
// breaks); chase steering targets THIS, not the player's true position, so
// breaking line of sight actually works. Like all runtime props it is not
// serialized - after a load the AI re-learns on its first sighting.
#[derive(Component, Clone, Copy)]
pub struct RuntimePropAITargetAwareness {
    pub last_known_pos: Vector3<f32>,
    pub has_line_of_sight: bool,
}

// Horizontal locomotion speed scale for an AI, published by its steering
// each frame: full speed when facing the travel direction, ramping down to
// a floor of a third as heading error grows (never zero - see
// locomotion_scale_for_heading_error). The animation velocity write
// multiplies by this and consumes the component; absent = 1.0 (non-AI
// animation players are unaffected).
#[derive(Component, Clone, Copy)]
pub struct RuntimePropLocomotionScale(pub f32);

#[derive(Component)]
pub struct RuntimePropJointTransforms(pub [Matrix4<f32>; 40]);

/// Exact resolved death clip used to reconstruct a killed creature's
/// terminal pose.
///
/// Runtime properties are normally rebuilt rather than saved, but this one
/// is explicitly persisted by `EntitySaveData`: random motion-schema
/// resolution cannot be repeated on load without risking a different corpse
/// pose. Keeping it separate from Dark's authored `P$CretPose` also leaves
/// mission-placed corpse decorations untouched.
///
/// The marker is attached when the random crumple query resolves. A save
/// taken during that crumple therefore resumes at the already-selected
/// clip's canonical final pose without replaying events. Saves created before
/// this marker existed cannot recover which random clip had been selected.
#[derive(Component, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePropDeathPose(pub String);

#[derive(Component)]
pub struct RuntimePropSpawnTimeInSeconds(pub f32);

#[derive(Component)]
pub struct RuntimeBitmapAnimationFrameCount(pub u32);

#[derive(Component, Debug)]
pub struct RuntimePropVhots(pub Vec<Vhot>);

// RuntimePropDoNotSerialize - runtime prop to signal that this prop should not be serialized
#[derive(Component)]
pub struct RuntimePropDoNotSerialize;

// RuntimePropProxyEntity - pointer to the parent entity (for example, hitboxes use this to point to the parent entity)
#[derive(Component)]
pub struct RuntimePropProxyEntity(pub shipyard::EntityId);

// RuntimePropTransientFx - a runtime-spawned, fire-and-forget effect entity
// (e.g. an impact spang): once its one-shot particle burst expires, the entity
// is destroyed. Level-authored particle groups never get this - their one-shot
// burst just goes dormant, matching the original engine (the object persists).
#[derive(Component, Clone, Copy)]
pub struct RuntimePropTransientFx;

// RuntimePropAttachment - rigidly bolts an entity to a parent's transform. Each
// frame the child's RuntimePropTransform is recomputed as
// `parent.transform * local_transform`, so short-lived attached effects (e.g. the
// muzzle flash, and later shell ejection / smoke) track a moving parent - notably
// the flat first-person weapon, which is re-placed against the camera every frame
// - instead of staying pinned at their spawn pose. `local_transform` is captured
// at spawn as `parent.transform.invert() * child.transform`, preserving the
// child's own scale/orientation relative to the parent.
#[derive(Component, Clone, Copy)]
pub struct RuntimePropAttachment {
    pub parent: shipyard::EntityId,
    pub local_transform: Matrix4<f32>,
}

// RuntimePropReloading - an in-progress weapon reload, set on the wielded weapon
// while it reloads. Models SS2's first-person reload: the gun pitches DOWN to a
// peak angle, HOLDS while the clip is swapped, then pitches back UP. The peak
// angle and pitch speed come from the weapon's own data (`PropPlayerGun`'s
// reload pitch/rate), the hold from its reload time. Drives three things from one
// source of truth: the viewmodel tilt (render), the fire gate (you cannot fire
// mid-reload), and debug introspection. Durations are in seconds; `peak_deg` is
// the (signed) peak tilt in degrees.
//
// Like all runtime props this is not serialized: a save taken mid-reload loads
// with the reload already "finished" (no tilt, firing allowed). That is benign -
// `begin_reload` refills the clip up front, so there is no ammo inconsistency.
#[derive(Component, Clone, Copy, Debug)]
pub struct RuntimePropReloading {
    pub elapsed: f32,
    pub down: f32,
    pub hold: f32,
    pub up: f32,
    pub peak_deg: f32,
}

impl RuntimePropReloading {
    pub fn total(&self) -> f32 {
        self.down + self.hold + self.up
    }

    pub fn is_done(&self) -> bool {
        self.elapsed >= self.total()
    }

    /// Fraction complete, 0..1.
    pub fn progress(&self) -> f32 {
        let total = self.total();
        if total <= 0.0 {
            1.0
        } else {
            (self.elapsed / total).clamp(0.0, 1.0)
        }
    }

    /// Current tilt angle (degrees): ramp 0 -> peak over `down`, hold at peak for
    /// `hold`, then ramp peak -> 0 over `up`.
    pub fn pitch_deg(&self) -> f32 {
        let t = self.elapsed;
        if self.down > 0.0 && t < self.down {
            self.peak_deg * (t / self.down)
        } else if t < self.down + self.hold {
            self.peak_deg
        } else if self.up > 0.0 && t < self.total() {
            let u = (t - self.down - self.hold) / self.up;
            self.peak_deg * (1.0 - u)
        } else {
            0.0
        }
    }
}

// RuntimePropSelectedAmmo - which of the wielded weapon's Projectile links is
// selected (index into `ordered_projectile_links`). Many SS2 guns carry several
// ammo types (e.g. the pistol: standard / HE / AP); firing uses the selected
// one. Absent = index 0 (the first/default link). Ignored for weapons with no
// Projectile links (melee).
//
// Scope notes (deliberate simplifications for now):
// - Not serialized (like all runtime props), so the selection resets to the
//   first link across save/load and level transitions.
// - All ammo types share one clip (`PropGunState.ammo`); switching type does not
//   switch loaded rounds. Per-ammo-type counts need an inventory-ammo model we
//   don't have yet, so this behaves like a fire-mode selector for now.
#[derive(Component, Clone, Copy, Debug)]
pub struct RuntimePropSelectedAmmo(pub usize);

/// Which phase the psi amp's hold-to-overload meter is in. `Charging` fills
/// the bar; `Overloaded`/`Burnout` are brief result flashes after release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsiChargePhase {
    Charging,
    Overloaded,
    Burnout,
}

// RuntimePropPsiCharge - the psi amp's hold-to-overload meter state, set on the
// wielded amp while the trigger is held on an overloadable power (and briefly
// after release, to flash the result). One source of truth for the HUD meter
// and debug introspection; the cast decision itself lives in `PsiAmpScript`.
// Like all runtime props this is not serialized - a save taken mid-charge
// loads with the charge dropped (no cast, no points spent), which is benign.
#[derive(Component, Clone, Copy, Debug)]
pub struct RuntimePropPsiCharge {
    /// Charge progress 0..1 (the bar fill). Overcharging past 1.0 burns out.
    pub fraction: f32,
    pub phase: PsiChargePhase,
}

// RuntimePropFlatAim - the flatscreen camera/crosshair fire ray (world space),
// set each frame on the player's wielded weapon. When present, weapon firing
// spawns projectiles from `origin` along `forward` (camera-origin aim) instead
// of the weapon's barrel transform, so shots track the crosshair regardless of
// the viewmodel's framing. Absent on AI/VR weapons, leaving their aim unchanged.
#[derive(Component, Clone, Copy)]
pub struct RuntimePropFlatAim {
    pub origin: Point3<f32>,
    pub forward: Vector3<f32>,
}

// RuntimePropMapData - the automap page data for the current mission, attached
// to the synthetic map-panel entity at mission init: the mission's level file
// name (for the per-level `intrface/<LEVEL>/english/` art paths) and the decal
// rects from P001RA.BIN / P001XA.BIN (empty when the mission ships no automap).
// Not serialized - rebuilt from mission data on every load.
#[derive(Component, Clone, Debug)]
pub struct RuntimePropMapData {
    /// Level file name as loaded (e.g. "medsci1.mis").
    pub mission: String,
    /// Bright "revealed" decal rects (P001RA.BIN), indexed by map location.
    pub revealed_rects: Vec<dark::map::MapRect>,
    /// Dim "explored" decal rects (P001XA.BIN), indexed by map location.
    pub explored_rects: Vec<dark::map::MapRect>,
}

// RuntimePropLogData - the resolved presentation strings for an audio log,
// attached to the log-disc entity when it is frobbed (collected). The reader
// panel (`MediaGui`) reads it to render the portrait/deck-icon/name/transcript.
// Not serialized: it is a presentation cache derived from the string tables and
// is re-resolved on the next frob (the log identity itself persists in
// `QuestInfo`).
#[derive(Component, Clone, Debug)]
pub struct RuntimePropLogData {
    pub name: Option<String>,
    pub text: Option<String>,
    pub portrait: Option<String>,
    pub icon: Option<String>,
}
