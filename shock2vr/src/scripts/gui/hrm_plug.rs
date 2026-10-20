//! Retail's HRM plug: a 73x194 companion raised beside an MFD panel, dropped
//! 96px from its top, with its 52x74 button at plug-local (16, 114). Retail
//! puts it at screen x 181 beside an MFD at x 2, so it overlaps the body's
//! right edge by 9px.

use crate::gui::{self, ButtonHoverBehavior, GuiComponent, PanelSidecar};
use crate::ui::Rect;

/// The MFD body the plug sits beside.
const BODY_W: f32 = 188.0;
pub(crate) const PLUG_RECT: Rect = Rect::new(179.0, 96.0, 73.0, 194.0);
/// The plug's button, panel-local.
pub(crate) const BUTTON_RECT: Rect = Rect::new(PLUG_RECT.x + 16.0, PLUG_RECT.y + 114.0, 52.0, 74.0);
/// Canvas width of a panel that keeps room for a plug.
pub(crate) const CANVAS_W: f32 = PLUG_RECT.x + PLUG_RECT.w;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PlugKind {
    Hack,
    Repair,
    Modify,
}

impl PlugKind {
    /// Backdrop, then the button's rest and lit art.
    pub(crate) fn art(self) -> (&'static str, &'static str, &'static str) {
        match self {
            PlugKind::Hack => ("plughack.pcx", "plugh0.pcx", "plugh1.pcx"),
            PlugKind::Repair => ("plugrep.pcx", "plugr0.pcx", "plugr1.pcx"),
            PlugKind::Modify => ("plugmod.pcx", "plugm0.pcx", "plugm1.pcx"),
        }
    }
}

/// The plug's art, and its button when `button` gives it a message and label.
pub(crate) fn draw_plug<T: Clone>(
    kind: PlugKind,
    button: Option<(T, &str)>,
) -> Vec<GuiComponent<T>> {
    let (backdrop, rest, lit) = kind.art();
    let mut components = vec![gui::image(backdrop).with_rect(PLUG_RECT)];
    if let Some((msg, label)) = button {
        components.push(
            gui::button(msg)
                .with_rect(BUTTON_RECT)
                .with_image(rest)
                .with_hover(ButtonHoverBehavior::Texture(lit.to_owned()))
                .with_label(label),
        );
    }
    components
}

/// The sidecar of a panel keeping room for a plug; `shown` while it is drawn.
pub(crate) fn plug_sidecar(shown: bool) -> PanelSidecar {
    PanelSidecar {
        body_width: BODY_W,
        rect: shown.then_some(PLUG_RECT),
    }
}
