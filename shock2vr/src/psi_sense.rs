//! Shared world-space presentation for sensory psi powers.
use cgmath::Vector3;
use engine::scene::{RenderLayer, SceneObject, SceneObjectDebugTag, SkinnedMaterial};
use serde::Serialize;
use shipyard::EntityId;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Debug, Serialize)]
pub struct PsiSenseContact {
    pub entity_id: u64,
    pub position: [f32; 3],
    pub distance: f32,
    pub strength: f32,
}

pub fn silhouette(
    object: &SceneObject,
    strength: f32,
    id: EntityId,
    color: Vector3<f32>,
    source: &str,
) -> SceneObject {
    let material = object.material.borrow();
    let replacement = if let Some(skinned) = material.as_any().downcast_ref::<SkinnedMaterial>() {
        skinned.silhouette(color)
    } else {
        engine::scene::color_material::create(color)
    };
    let mut echo = object.clone();
    echo.material = Rc::new(RefCell::new(replacement));
    echo.set_transparency(Some(1.0 - 0.75 * strength));
    echo.set_depth_write(false);
    // SceneOverlay has its own depth, so walls cannot hide the echo. SceneUi
    // follows it, preserving viewmodels and readable HUDs on both runtimes.
    echo.set_render_layer(RenderLayer::SceneOverlay);
    echo.set_debug_tag(Some(Rc::new(SceneObjectDebugTag {
        entity_id: Some(id.inner()),
        source: Some(source.into()),
        ..Default::default()
    })));
    echo
}
