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
use shipyard::Component;

// RuntimePropGazeAmount - track how much the player is gazing at a prop
#[derive(Component)]
#[allow(dead_code)]
pub struct RuntimePropGazeAmount(pub f32);

#[derive(Component)]
pub struct RuntimePropTransform(pub Matrix4<f32>);

#[derive(Component)]
pub struct RuntimePropJointTransforms(pub [Matrix4<f32>; 40]);

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
