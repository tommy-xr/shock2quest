//! A second hand on an item the first hand already holds.
//!
//! The primary hand keeps everything it owns - the item's position, its trigger,
//! its magazine anchors. What the support hand adds is an **aim**: while it is
//! attached, the item's forward runs down the line from the primary hand to the
//! support hand instead of down the primary wrist. Every consumer of the held
//! transform (muzzle, mag zone, the melee drive's kinematic target) follows for
//! free, because there is still exactly one transform.
//!
//! Where the off-hand may take hold is deliberately unrestricted: any point on
//! the item's own surface. An authored anchor ([`crate::vr_grips::SupportSeat`],
//! the pump on `sg_h`, the foregrip on `empgun_h`, the magwell on `ar15_h`) is a
//! *snap preference* when the palm comes near it, not a requirement - the
//! oversized weapons bake no support hand at all, and a basketball has no
//! authored anything.
//!
//! The latch is taken **once**, at the grip edge, in the primary hand's own
//! space. Re-picking the nearest surface point every frame would let the item
//! crawl through the off-hand as the aim swung it.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use cgmath::{
    EuclideanSpace, InnerSpace, Matrix3, Point3, Quaternion, Rotation, SquareMatrix, Transform,
    Vector3, vec3,
};
use engine::assets::asset_cache::AssetCache;
use once_cell::sync::Lazy;
use shipyard::EntityId;

use dark::importers::{VR_CONTACT_MESH_IMPORTER, VrContactMesh};

use crate::hand_fit::ContactMesh;
use crate::physics::PhysicsWorld;
use crate::util::meters;
use crate::vr_config::{self, Handedness};

/// How far the off-hand's palm may sit from the item's surface and still take
/// hold. Generous on purpose: the point is that a second hand lands on a rifle
/// wherever the player puts it, not that they hunt for a sweet spot.
const CONTACT_RADIUS: f32 = meters(0.10);

/// An authored support seat this close to the palm wins over a free latch, so
/// a hand brought to a shotgun's pump lands *on* the pump.
const ANCHOR_SNAP_RADIUS: f32 = meters(0.08);

/// Closer than this the hand line carries no direction, so the last valid one
/// stands - hands do cross, and a weapon must not spin when they do.
const MIN_HAND_SEPARATION: f32 = meters(0.10);

/// Frames the aim takes to ramp in on attach and out on release. Without it the
/// weapon snaps to the new axis in one frame, which reads as a glitch.
const BLEND_FRAMES: f32 = 6.0;

/// How long a lost support pose is held before the grip is dropped. Same window
/// as the body anchors' (`body_frame::ANCHOR_HOLD_FRAMES`): a controller that
/// blinks out for a few frames has not let go.
const TRACKING_HOLD_FRAMES: u8 = 12;

/// Latch points are cached per centimetre; finer than that is below what a
/// tracked hand resolves and would only thrash the cache.
const LATCH_QUANTUM: f32 = meters(0.01);

/// Where the off-hand took hold of the other hand's item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupportLatch {
    pub entity_id: EntityId,
    /// The hand doing the supporting.
    pub hand: Handedness,
    /// The grip point, in the **primary** hand's own space - rigid with the
    /// item, so it survives the aim swinging the item about.
    pub point: Vector3<f32>,
    /// Whether an authored support seat claimed it, rather than a free latch.
    pub snapped: bool,
}

/// What the two-hand resolve says about one hand this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct TwoHandFrame {
    /// The rotation the held item is placed with, replacing the tracked wrist
    /// while a support is attached. `None` is ordinary one-hand placement.
    ///
    /// This is a *placement* rotation only: the glove still draws at the
    /// tracked pose, because that is where the player's hand really is.
    pub aim_rotation: Option<Quaternion<f32>>,
    /// This hand holds a support grip on the other hand's item, so it must not
    /// also grab or frob whatever its ray crosses.
    pub supporting: bool,
    /// This hand is empty and on (or already holding) the other hand's item:
    /// the glove lights green for the grip it could take.
    pub offered: bool,
}

/// One hand's inputs to the resolve.
#[derive(Clone, Copy, Debug)]
pub struct HandInput {
    pub hand: Handedness,
    /// World pose of the tracked hand, as `VirtualHand` sees it.
    pub position: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub squeeze: f32,
    /// What this hand holds, from the *previous* frame's hand state.
    pub held: Option<EntityId>,
    /// The hand is already spoken for (a body anchor, a climbing hold): it
    /// cannot also take a support grip.
    pub claimed: bool,
}

/// Everything the resolve reads that is not per-hand.
pub struct ResolveContext<'a> {
    pub world: &'a shipyard::World,
    pub physics: &'a PhysicsWorld,
    pub hands: [HandInput; 2],
}

/// The two-hand grip state, owned by the VR interaction (it is the only thing
/// that sees both hands).
#[derive(Clone, Debug, Default)]
pub struct TwoHandGrip {
    latch: Option<SupportLatch>,
    /// The last valid world aim axis (primary -> support). Kept through a
    /// degenerate frame and through the whole release ramp, which is what lets
    /// the weapon ease back to the wrist instead of snapping.
    axis: Option<Vector3<f32>>,
    /// 0 = one-handed, 1 = fully aimed down the hand line.
    blend: f32,
    lost_frames: u8,
}

impl TwoHandGrip {
    /// Whether a support grip is attached right now.
    pub fn is_two_handed(&self) -> bool {
        self.latch.is_some()
    }

    pub fn latch(&self) -> Option<SupportLatch> {
        self.latch
    }

    /// Advance a frame: take, keep or drop the support grip, then hand each
    /// hand what it needs to place and light itself.
    pub fn resolve(&mut self, ctx: &ResolveContext) -> [TwoHandFrame; 2] {
        self.step_latch(ctx);

        // Ramp toward the state the latch just settled on, so attach and
        // release share one ease.
        let target = if self.latch.is_some() { 1.0 } else { 0.0 };
        let step = 1.0 / BLEND_FRAMES;
        self.blend = if self.blend < target {
            (self.blend + step).min(target)
        } else {
            (self.blend - step).max(target)
        };
        if self.blend <= 0.0 {
            self.axis = None;
        }

        let mut frames = [TwoHandFrame::default(); 2];
        let Some(latch) = self.latch else {
            // Still easing back to the wrist after a release: the primary hand
            // keeps an aim until the ramp reaches zero.
            if self.blend > 0.0 {
                if let Some(primary) = ctx
                    .hands
                    .iter()
                    .find(|hand| hand.held.is_some() && !hand.claimed)
                {
                    frames[vr_config::hand_slot(primary.hand)].aim_rotation =
                        self.aim_for(primary.rotation);
                }
            }
            // An empty hand resting on the other's item still lights green.
            if let Some(offered) = self.offer(ctx) {
                frames[vr_config::hand_slot(offered)].offered = true;
            }
            return frames;
        };

        let support = ctx.hands[vr_config::hand_slot(latch.hand)];
        let primary = ctx.hands[vr_config::hand_slot(vr_config::other_hand(latch.hand))];

        // The axis is re-read from the live hands every frame the pose is
        // good; a lost or degenerate one keeps the last.
        let delta = support.position - primary.position;
        if delta.magnitude() >= MIN_HAND_SEPARATION {
            self.axis = Some(delta.normalize());
        }

        frames[vr_config::hand_slot(support.hand)].supporting = true;
        frames[vr_config::hand_slot(support.hand)].offered = true;
        frames[vr_config::hand_slot(primary.hand)].aim_rotation = self.aim_for(primary.rotation);
        frames
    }

    /// The placement rotation for a primary wrist at `wrist`, blended over the
    /// attach/release ramp. `None` once the ramp is fully out.
    fn aim_for(&self, wrist: Quaternion<f32>) -> Option<Quaternion<f32>> {
        if self.blend <= 0.0 {
            return None;
        }
        let axis = self.axis?;
        Some(wrist.slerp(aim_rotation(wrist, axis), self.blend.min(1.0)))
    }

    /// Take, keep, or drop the support grip.
    fn step_latch(&mut self, ctx: &ResolveContext) {
        if let Some(latch) = self.latch {
            let support = ctx.hands[vr_config::hand_slot(latch.hand)];
            let primary = ctx.hands[vr_config::hand_slot(vr_config::other_hand(latch.hand))];

            // The primary letting go takes the support with it - the item is
            // dropped, and this slice does not hand it over.
            if primary.held != Some(latch.entity_id) {
                self.latch = None;
                return;
            }
            // A pose that blinks out is not a release: the controller keeps
            // reporting its grip, so only a real open hand detaches. The hold
            // window covers the pose, not the button.
            if crate::util::tracked_rotation(support.rotation).is_none() {
                if self.lost_frames >= TRACKING_HOLD_FRAMES {
                    self.latch = None;
                } else {
                    self.lost_frames += 1;
                }
                return;
            }
            self.lost_frames = 0;
            if support.squeeze < crate::ui::VR_TRIGGER_THRESHOLD {
                self.latch = None;
            }
            return;
        }

        self.lost_frames = 0;
        let Some(hand) = self.offer(ctx) else {
            return;
        };
        let support = ctx.hands[vr_config::hand_slot(hand)];
        // Closing on it is what takes hold, the same edge the world grab uses.
        if support.squeeze < crate::ui::VR_TRIGGER_THRESHOLD {
            return;
        }
        let primary = ctx.hands[vr_config::hand_slot(vr_config::other_hand(hand))];
        let Some(entity_id) = primary.held else {
            return;
        };
        let Some((point, snapped)) = latch_point(ctx, primary, support) else {
            return;
        };
        self.latch = Some(SupportLatch {
            entity_id,
            hand,
            point,
            snapped,
        });
    }

    /// The hand that could take a support grip on the other's item this frame,
    /// if any: empty, unclaimed, and on the item's surface.
    fn offer(&self, ctx: &ResolveContext) -> Option<Handedness> {
        for support in ctx.hands {
            if support.held.is_some() || support.claimed {
                continue;
            }
            let primary = ctx.hands[vr_config::hand_slot(vr_config::other_hand(support.hand))];
            if primary.held.is_none() || primary.claimed {
                continue;
            }
            if latch_point(ctx, primary, support).is_some() {
                return Some(support.hand);
            }
        }
        None
    }
}

/// The hand rotation that aims a held item down `axis`.
///
/// A VR hand points along its own -Z (the same forward the interaction ray
/// uses), so aiming the item at the support hand means turning the placement
/// frame until -Z lies along the hand line. Roll comes from the primary wrist's
/// own up, projected off the axis: the player still decides which way the
/// sights face, they just no longer decide where the barrel points.
pub fn aim_rotation(wrist: Quaternion<f32>, axis: Vector3<f32>) -> Quaternion<f32> {
    let z = -axis.normalize();
    let up = wrist.rotate_vector(vec3(0.0, 1.0, 0.0));
    // Hands stacked straight along the wrist's own up leave no roll reference;
    // fall back to the wrist's right, which cannot also be parallel to z.
    let mut x = up.cross(z);
    if x.magnitude2() < 1e-6 {
        x = wrist.rotate_vector(vec3(1.0, 0.0, 0.0));
        x = x - z * x.dot(z);
    }
    if x.magnitude2() < 1e-6 {
        return wrist;
    }
    let x = x.normalize();
    let y = z.cross(x);
    Quaternion::from(Matrix3::from_cols(x, y, z))
}

/// Where the off-hand's palm takes hold of the primary hand's item, in the
/// primary hand's own space, and whether an authored seat claimed it.
fn latch_point(
    ctx: &ResolveContext,
    primary: HandInput,
    support: HandInput,
) -> Option<(Vector3<f32>, bool)> {
    let entity_id = primary.held?;
    let (model_name, gun_scale) = vr_config::held_model_and_scale(ctx.world, entity_id)?;

    let to_world =
        crate::hand_glove::hand_to_world(primary.position, primary.rotation, primary.hand);
    let to_hand = to_world.invert()?;
    let palm_world =
        crate::hand_glove::hand_to_world(support.position, support.rotation, support.hand)
            .transform_point(Point3::from_vec(crate::hand_seat::PALM_CENTRE));
    let palm = to_hand.transform_point(palm_world);

    let to_model = vr_config::held_model_hand_transform(&model_name, primary.hand, gun_scale);

    // An authored seat within reach wins: a hand brought to a shotgun's pump
    // lands on the pump rather than a centimetre off it.
    let snap = crate::vr_grips::support_seats(&model_name)
        .into_iter()
        .map(|seat| to_model.transform_point(Point3::from_vec(seat.offset())))
        .map(|seat| (seat, (seat - palm).magnitude()))
        .filter(|(_, distance)| *distance <= ANCHOR_SNAP_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((seat, _)) = snap {
        return Some((seat.to_vec(), true));
    }

    on_surface(
        ctx,
        entity_id,
        &model_name,
        primary,
        gun_scale,
        palm,
        palm_world,
    )
    .then_some((palm.to_vec(), false))
}

/// Whether the palm is on the held item's own surface.
///
/// Two volumes, because the two kinds of held item report different geometry:
/// a melee wield is a *skinned* rig with no triangle soup to test, but it does
/// carry the fitted contact cuboid its damage is billed through - which is
/// exactly the volume the player sees. Everything else (the rigid gun `_h` set,
/// world pickups) tests against its own render triangles.
fn on_surface(
    ctx: &ResolveContext,
    entity_id: EntityId,
    model_name: &str,
    primary: HandInput,
    gun_scale: f32,
    palm_in_hand: Point3<f32>,
    palm_world: Point3<f32>,
) -> bool {
    if let Some((centre, rotation, half_extents)) = ctx.physics.held_melee_contact_box(entity_id) {
        let local = rotation
            .invert()
            .rotate_vector(palm_world.to_vec() - centre);
        return box_distance(local, half_extents) <= CONTACT_RADIUS;
    }

    match contact_mesh(model_name, primary.hand, gun_scale) {
        Some(mesh) => mesh.within(palm_in_hand, CONTACT_RADIUS),
        None => false,
    }
}

/// Distance from a point to an axis-aligned box centred on the origin.
fn box_distance(point: Vector3<f32>, half_extents: Vector3<f32>) -> f32 {
    let outside = vec3(
        (point.x.abs() - half_extents.x).max(0.0),
        (point.y.abs() - half_extents.y).max(0.0),
        (point.z.abs() - half_extents.z).max(0.0),
    );
    outside.magnitude()
}

/// Held items' contact meshes, in the holding hand's own space, keyed the same
/// way the finger fit keys its cache.
///
/// Filled by [`warm_contact_mesh`], where an asset cache is in reach, and read
/// by the resolve, which runs in the hand update and has none - the same split
/// [`crate::vr_grips`] uses for measured seats.
type ContactKey = (String, Handedness, u32);
static CONTACT: Lazy<RwLock<HashMap<ContactKey, Option<Arc<ContactMesh>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn contact_key(model_name: &str, handedness: Handedness, gun_scale: f32) -> ContactKey {
    (
        model_name.to_ascii_lowercase(),
        handedness,
        gun_scale.to_bits(),
    )
}

/// Load and place `model_name`'s render triangles for a hand that holds it, if
/// that has not happened yet. The miss is remembered too: a skinned melee rig
/// has no triangles, and re-walking every asset mount for it each frame is what
/// remembering avoids.
pub fn warm_contact_mesh(
    model_name: &str,
    handedness: Handedness,
    gun_scale: f32,
    asset_cache: &mut AssetCache,
) {
    let key = contact_key(model_name, handedness, gun_scale);
    if CONTACT.read().unwrap().contains_key(&key) {
        return;
    }
    let to_hand = vr_config::held_model_hand_transform(model_name, handedness, gun_scale);
    let mesh = asset_cache
        .get_opt::<_, VrContactMesh, _>(&VR_CONTACT_MESH_IMPORTER, &format!("{model_name}.BIN"))
        .map(|triangles| {
            ContactMesh::new(
                triangles
                    .0
                    .iter()
                    .map(|triangle| triangle.map(|corner| to_hand.transform_point(corner)))
                    .collect(),
            )
        })
        .filter(|mesh| !mesh.is_empty())
        .map(Arc::new);
    CONTACT.write().unwrap().insert(key, mesh);
}

fn contact_mesh(
    model_name: &str,
    handedness: Handedness,
    gun_scale: f32,
) -> Option<Arc<ContactMesh>> {
    CONTACT
        .read()
        .unwrap()
        .get(&contact_key(model_name, handedness, gun_scale))
        .cloned()
        .flatten()
}

/// The support hand's own fit key: the item, and where on it the hand latched,
/// rounded to the centimetre so a tracked hand's jitter does not re-solve it.
pub fn quantise_latch(point: Vector3<f32>) -> [i32; 3] {
    [
        (point.x / LATCH_QUANTUM).round() as i32,
        (point.y / LATCH_QUANTUM).round() as i32,
        (point.z / LATCH_QUANTUM).round() as i32,
    ]
}
