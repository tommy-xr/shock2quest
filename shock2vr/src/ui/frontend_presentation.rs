//! One flat/VR presentation boundary for frontend canvases.
//!
//! Scenes describe a canvas, panel, pointer and scale policy. This presenter
//! alone decides whether that canvas belongs in the shared world render or the
//! per-eye screen overlay, and owns the fixed world-layer spacing.

use cgmath::Vector2;
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};

use crate::PresentationMode;

use super::{ScaleMode, UiCanvas, VR_COMPONENT_Z_STEP, WorldPanel};

/// Maps an already-resolved frontend canvas into exactly one presentation.
#[derive(Clone, Copy, Debug)]
pub struct FrontendCanvasPresenter {
    presentation_mode: PresentationMode,
    scale_mode: ScaleMode,
}

impl FrontendCanvasPresenter {
    pub fn new(presentation_mode: PresentationMode, scale_mode: ScaleMode) -> Self {
        Self {
            presentation_mode,
            scale_mode,
        }
    }

    /// Render a canvas on its anchored panel in VR, or emit nothing in flat
    /// mode (where the same canvas is presented by [`Self::render_screen_space`]).
    pub fn render_world_space<TEvent: Clone>(
        self,
        asset_cache: &mut AssetCache,
        canvas: &UiCanvas<TEvent>,
        panel: &WorldPanel,
        pointer: Option<Vector2<f32>>,
    ) -> Vec<SceneObject> {
        self.render_world_space_with(asset_cache, canvas, panel, pointer, |_, _| {})
    }

    /// The menu shell extends the common panel stack with pointer rays and a
    /// hit dot. Keeping that decoration inside this guarded call prevents a
    /// stale VR pointer from leaking into the flat presentation.
    pub(crate) fn render_world_space_with<TEvent: Clone>(
        self,
        asset_cache: &mut AssetCache,
        canvas: &UiCanvas<TEvent>,
        panel: &WorldPanel,
        pointer: Option<Vector2<f32>>,
        decorate: impl FnOnce(&mut AssetCache, &mut Vec<SceneObject>),
    ) -> Vec<SceneObject> {
        self.present_world_space(|| {
            let mut objects = canvas.render_world_space(
                asset_cache,
                panel.transform(),
                pointer,
                None,
                VR_COMPONENT_Z_STEP,
            );
            decorate(asset_cache, &mut objects);
            objects
        })
    }

    /// Render a canvas in the per-eye screen overlay in flat mode, or emit
    /// nothing in VR (where the panel from [`Self::render_world_space`] owns it).
    pub fn render_screen_space<TEvent: Clone>(
        self,
        asset_cache: &mut AssetCache,
        canvas: &UiCanvas<TEvent>,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        self.present_screen_space(|| {
            canvas.render_screen_space(asset_cache, screen_size, self.scale_mode)
        })
    }

    /// Run a complete world-space frontend composition only in VR. This is
    /// used by overlays whose panel stack includes objects outside the canvas
    /// itself (for example the pause comfort dim).
    pub fn present_world_space(
        self,
        render: impl FnOnce() -> Vec<SceneObject>,
    ) -> Vec<SceneObject> {
        if self.renders_world_space() {
            render()
        } else {
            Vec::new()
        }
    }

    /// Run a complete screen-space frontend composition only in flat mode.
    pub fn present_screen_space(
        self,
        render: impl FnOnce() -> Vec<SceneObject>,
    ) -> Vec<SceneObject> {
        if self.renders_screen_space() {
            render()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn renders_world_space(self) -> bool {
        self.presentation_mode == PresentationMode::Vr
    }

    pub(crate) fn renders_screen_space(self) -> bool {
        !self.renders_world_space()
    }

    #[cfg(test)]
    pub(crate) fn scale_mode(self) -> ScaleMode {
        self.scale_mode
    }

    #[cfg(test)]
    pub(crate) fn component_z_step(self) -> f32 {
        VR_COMPONENT_Z_STEP
    }
}
