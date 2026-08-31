//! Floating damage readouts: a short-lived "-7 HP" at the point a blow landed,
//! labeled with the hitbox it struck.
//!
//! This is the instrument for the hitbox work: without it, "did that shot hit
//! the head or the arm, and for how much?" can only be answered by reading the
//! victim's hit points before and after. Off by default, behind the
//! `damage_numbers` dev param, so a headset can turn it on without a rebuild.
//!
//! Recorded from the one place every script message is dispatched
//! (`ScriptWorld::update`), so it sees melee, projectiles, hitbox-forwarded
//! damage and script-injected damage alike - anything that reaches a script as
//! `MessagePayload::Damage` with an impact point. The readouts live on the
//! `ScriptWorld` that records them, so they die with their scene.

use cgmath::{Deg, InnerSpace, Matrix4, SquareMatrix, Vector3, vec3};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{EntityId, World};

use dark::importers::FONT_IMPORTER;

use crate::{creature::HitBoxType, scripts::MessagePayload};

/// How long a readout stays up.
const LIFETIME_SECS: f64 = 1.6;
/// How far it drifts upward over that life, in world units.
const RISE: f32 = 0.6;
/// World-space height of a line of text.
const TEXT_HEIGHT: f32 = 0.22;
/// Older readouts are dropped rather than accumulating in a firefight.
const MAX_POPUPS: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DamagePopup {
    pub text: String,
    pub point: Vector3<f32>,
    pub spawned_at: f64,
}

/// The readout for one blow: how much, and where it landed. The hitbox name
/// comes from the struck joint, which only a hitbox-forwarded hit carries - a
/// blow that landed on the entity's own collider says nothing about limbs
/// rather than guessing "Body".
pub(crate) fn popup_text(amount: f32, hit_box: Option<HitBoxType>) -> String {
    let amount = format!("-{} HP", format_amount(amount));
    match hit_box {
        Some(hit_box) => format!("{amount}  {}", hit_box_label(hit_box)),
        None => amount,
    }
}

/// Damage is authored in whole points but arrives scaled (armor, difficulty),
/// so a fractional value keeps one decimal instead of reading as an integer it
/// is not.
fn format_amount(amount: f32) -> String {
    if (amount - amount.round()).abs() < 0.05 {
        format!("{}", amount.round() as i64)
    } else {
        format!("{amount:.1}")
    }
}

fn hit_box_label(hit_box: HitBoxType) -> &'static str {
    match hit_box {
        HitBoxType::Head => "head",
        HitBoxType::Body => "body",
        HitBoxType::Limb => "limb",
        HitBoxType::Extremity => "extremity",
        HitBoxType::NoDamage => "no-damage",
    }
}

/// Remaining life of a readout as a 0..1 ramp, `None` once it has expired.
pub(crate) fn fade(spawned_at: f64, now: f64) -> Option<f32> {
    let age = now - spawned_at;
    (age >= 0.0 && age < LIFETIME_SECS).then(|| 1.0 - (age / LIFETIME_SECS) as f32)
}

/// The readout a dispatched message earns, if any: damage that knows where it
/// landed, while the overlay is on.
///
/// A hit forwarded by a hitbox is dispatched twice - once to the hitbox entity
/// and once to the creature it belongs to, both carrying the same impact point
/// - so the proxy delivery is skipped. Recording both would stack an unlabeled
/// readout under the labeled one on every real limb hit, which is exactly the
/// ambiguity this overlay exists to remove.
pub(crate) fn popup_for(
    world: &World,
    sim_time: f64,
    target: EntityId,
    payload: &MessagePayload,
) -> Option<DamagePopup> {
    if !crate::dev_params::get_bool(crate::dev_params::DAMAGE_NUMBERS) {
        return None;
    }
    let MessagePayload::Damage {
        amount,
        impact: Some(impact),
    } = payload
    else {
        return None;
    };
    if crate::creature::is_hit_box(world, target) {
        return None;
    }

    let hit_box = impact.bone.and_then(|bone| {
        crate::creature::get_entity_creature(world, target)
            .and_then(|creature| creature.get_hitbox_type(bone))
    });

    Some(DamagePopup {
        text: popup_text(*amount, hit_box),
        point: impact.point,
        spawned_at: sim_time,
    })
}

/// Cap the buffer, oldest dropped first, so a firefight cannot grow it without
/// bound before the next render ages it out.
pub(crate) fn trim(popups: &mut Vec<DamagePopup>) {
    if popups.len() > MAX_POPUPS {
        popups.drain(..popups.len() - MAX_POPUPS);
    }
}

/// Drop expired readouts, and report those still alive with their remaining
/// life as a 0..1 ramp.
pub(crate) fn live(popups: &mut Vec<DamagePopup>, now: f64) -> Vec<(DamagePopup, f32)> {
    popups.retain(|popup| fade(popup.spawned_at, now).is_some());
    popups
        .iter()
        .filter_map(|popup| fade(popup.spawned_at, now).map(|fade| (popup.clone(), fade)))
        .collect()
}

/// The readouts as world-space text, turned to face the camera and drifting up
/// as they fade.
///
/// Billboarded about the vertical only, toward `camera_pos`: a readout stays
/// upright, which is what makes it readable, and it needs only the camera's
/// position - so the same call serves the flat camera and either VR eye.
///
/// `camera_pos` is the player's own eye. A *detached* free camera therefore
/// sees the readouts edge-on: the free camera's pose is resolved by `Game`
/// after this scene is built, and is deliberately not plumbed in here - a
/// spectator camera showing the body's instruments from the body's point of
/// view is the same rule the HUD already follows.
pub(crate) fn render(
    asset_cache: &mut AssetCache,
    popups: &mut Vec<DamagePopup>,
    camera_pos: Vector3<f32>,
    now: f64,
) -> Vec<SceneObject> {
    if !crate::dev_params::get_bool(crate::dev_params::DAMAGE_NUMBERS) {
        // Turning the overlay off takes the readouts with it, rather than
        // leaving the last second and a half of them hanging in the world.
        popups.clear();
        return Vec::new();
    }
    let live = live(popups, now);
    if live.is_empty() {
        return Vec::new();
    }
    let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon");

    let mut objects = live
        .into_iter()
        .map(|(popup, fade)| {
            let position = popup.point + vec3(0.0, RISE * (1.0 - fade), 0.0);
            let mut object = SceneObject::world_space_text(&popup.text, font.clone(), 1.0 - fade);
            object.set_transform(
                Matrix4::from_translation(position)
                    * facing_rotation(position, camera_pos)
                    * Matrix4::from_nonuniform_scale(
                        // `world_space_text` normalizes its glyphs into a unit
                        // square, so the caller owns the aspect: the font's own
                        // measurement of this string at unit line height.
                        TEXT_HEIGHT * engine::measure_text_width(&**font, &popup.text, 1.0),
                        TEXT_HEIGHT,
                        1.0,
                    )
                    // The glyph mesh is built in the canvas's y-down space, so
                    // world-space text is stood upright by the same half turn
                    // about x the UI panels use (`ui::world_element_transform`).
                    // Without it the readout renders upside down, which at a
                    // glance reads as mirrored.
                    * Matrix4::from_angle_x(Deg(180.0)),
            );
            object
        })
        .collect::<Vec<_>>();
    crate::util::tag_render_source(&mut objects, crate::util::render_source::DAMAGE_NUMBERS);
    objects
}

/// The rotation that stands text at `position` up facing `camera_pos`.
///
/// Built as an explicit basis rather than a yaw angle, so the glyph quad's
/// local +x is the direction the *viewer* calls right and its +z points at the
/// viewer: a yaw alone leaves the convention of the text mesh's facing to
/// chance, and getting it wrong renders every number mirrored.
fn facing_rotation(position: Vector3<f32>, camera_pos: Vector3<f32>) -> Matrix4<f32> {
    let up = vec3(0.0, 1.0, 0.0);
    let to_camera = camera_pos - position;
    // Level the text: a readout above a corpse should not pitch down at a
    // player standing over it.
    let to_camera = vec3(to_camera.x, 0.0, to_camera.z);
    if to_camera.magnitude2() < 1.0e-6 {
        // Directly overhead or underneath: any facing is as good as another.
        return Matrix4::identity();
    }
    let forward = to_camera.normalize();
    let right = up.cross(forward);
    Matrix4::from_cols(
        right.extend(0.0),
        up.extend(0.0),
        forward.extend(0.0),
        cgmath::vec4(0.0, 0.0, 0.0, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readout_names_the_hitbox_it_struck() {
        assert_eq!(popup_text(7.0, Some(HitBoxType::Head)), "-7 HP  head");
        assert_eq!(popup_text(12.0, Some(HitBoxType::Limb)), "-12 HP  limb");
    }

    /// A blow that landed on the entity's own collider carries no joint, and
    /// must not claim one.
    #[test]
    fn a_readout_without_a_hitbox_is_just_the_amount() {
        assert_eq!(popup_text(3.0, None), "-3 HP");
    }

    /// Scaled damage (armor, difficulty) is not a whole number; rounding it to
    /// one would make the overlay disagree with the hit points it explains.
    #[test]
    fn a_fractional_amount_keeps_a_decimal() {
        assert_eq!(popup_text(4.5, None), "-4.5 HP");
        assert_eq!(popup_text(6.02, None), "-6 HP");
    }

    /// The text stands up facing the camera: +z at the viewer, +x the way the
    /// viewer reads, +y up. Any other basis renders the number mirrored, or
    /// tipped over when the player stands above the body.
    #[test]
    fn a_readout_stands_up_facing_the_camera() {
        let at = vec3(0.0, 0.0, 0.0);
        let rotation = facing_rotation(at, vec3(5.0, 3.0, 0.0));

        // +z points at the camera, horizontally - the camera's height is not
        // allowed to tip the text.
        assert_eq!(rotation.z.truncate(), vec3(1.0, 0.0, 0.0));
        assert_eq!(rotation.y.truncate(), vec3(0.0, 1.0, 0.0));
        // ...and +x is the viewer's right (looking along -x, that is -z).
        assert_eq!(rotation.x.truncate(), vec3(0.0, 0.0, -1.0));
    }

    /// A camera directly overhead has no bearing to face; the text must not
    /// come out as a degenerate (zero-scaled) transform.
    #[test]
    fn a_readout_under_the_camera_keeps_a_valid_rotation() {
        let rotation = facing_rotation(vec3(0.0, 0.0, 0.0), vec3(0.0, 5.0, 0.0));

        assert_eq!(rotation, Matrix4::identity());
    }

    #[test]
    fn a_readout_fades_over_its_lifetime_and_then_expires() {
        assert_eq!(fade(10.0, 10.0), Some(1.0));
        assert!(fade(10.0, 10.0 + LIFETIME_SECS / 2.0).unwrap() < 0.55);
        // Faded out by the end of its life (the exact boundary is float
        // noise, so the expiry is checked just past it).
        assert!(fade(10.0, 10.0 + LIFETIME_SECS).unwrap_or(0.0) < 0.001);
        assert_eq!(fade(10.0, 10.0 + LIFETIME_SECS + 0.01), None);
        // A readout from a previous level (clock reset) is not live.
        assert_eq!(fade(10.0, 1.0), None);
    }
}
