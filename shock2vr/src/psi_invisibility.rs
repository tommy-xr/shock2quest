//! Photonic Redirection's local visual feedback. Gameplay sight checks remain
//! separate: this fades only the caster's hands and held items, never the HUD.
use engine::scene::SceneObject;
use shipyard::{UniqueView, World};

pub(crate) fn transparency(world: &World) -> Option<f32> {
    let active = world
        .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
        .ok()?;
    let power = active
        .0
        .iter()
        .find(|p| p.template_id == crate::psi::INVISO_TEMPLATE_ID)?;
    fade(power.remaining_secs)
}

fn fade(remaining: f32) -> Option<f32> {
    // A readable ghost while active, returning smoothly during the last 3s.
    (remaining > 0.0).then(|| 0.78 * (remaining / 3.0).clamp(0.0, 1.0))
}

pub(crate) fn apply(object: &mut SceneObject, transparency: Option<f32>) {
    if let Some(alpha) = transparency {
        let authored = object.effective_transparency().unwrap_or(0.0);
        object.set_transparency(Some(authored.max(alpha)));
        object.set_depth_write(false);
    }
}
