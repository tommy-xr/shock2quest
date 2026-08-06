use std::sync::atomic::{AtomicU64, Ordering};

use cgmath::{Point2, Vector2, Vector3};
use shipyard::{EntityId, World};

mod gui_component;
mod gui_manager;
mod gui_script;
mod proxy_gui_script;
pub use gui_component::*;
pub use gui_manager::*;
pub use gui_script::*;
pub use proxy_gui_script::*;

static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct GuiHandle(u64);

impl GuiHandle {
    pub fn new() -> GuiHandle {
        let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
        GuiHandle(id)
    }
}

pub const GUI_PIXEL_TO_WORLD_SIZE: f32 = 1.0 / 250.0;

pub struct GuiConfig {
    pub world_offset: Vector3<f32>,
    pub screen_size_in_pixels: Vector2<f32>,
}

pub struct GuiCursor {
    pub position: Point2<f32>,
    pub held_entity_id: Option<EntityId>,
}

pub trait Gui<TState, TMsg>
where
    TState: Default,
    TMsg: Clone,
{
    fn get_components(
        &self,
        cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &TState,
    ) -> Vec<GuiComponent<TMsg>>;

    fn get_config(&self) -> GuiConfig;

    /// State-aware panel geometry. Most panels are fixed-size and use
    /// `get_config`; overlays with a companion plug (the retail replicator
    /// PLUGHACK sidecar) can widen only while that companion is present.
    fn get_config_for(&self, _entity_id: EntityId, _world: &World, _state: &TState) -> GuiConfig {
        self.get_config()
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &TState,
        msg: &TMsg,
    ) -> (TState, crate::Effect);

    /// Extra effect to run when this panel's entity is frobbed, in addition to
    /// opening the panel (`GuiScript` combines the two). Default: nothing. The
    /// audio-log reader (`MediaGui`) overrides it to record the log into the
    /// collection and play its audio - side effects that must fire on frob, not
    /// on a panel button.
    fn on_frob(&self, _entity_id: EntityId, _world: &World) -> crate::Effect {
        crate::Effect::NoEffect
    }

    /// Whether frobbing this panel's entity should open it as a flat-mode MFD
    /// (`GuiScript` gates `Effect::OpenPanel` on this; the `on_frob` effect
    /// runs regardless). Default: always open. The audio-log reader overrides
    /// it so a content-less disc (unset `PropLog`) doesn't open an empty
    /// backdrop with dead scroll buttons.
    fn opens_on_frob(&self, _entity_id: EntityId, _world: &World) -> bool {
        true
    }

    /// Whether the panel's transient state resets each time a frob opens it
    /// (the original re-creates its overlay state on every open). Default:
    /// keep state. The audio-log reader opts in so a reopened transcript
    /// starts back at the top instead of the last scroll position.
    fn resets_state_on_frob(&self) -> bool {
        false
    }

    /// Prepare transient state when a frob reopens this panel. The default
    /// honors `resets_state_on_frob`; stateful flows can preserve only the
    /// portion that must survive closing and reopening.
    fn prepare_state_on_frob(&self, state: &mut TState) {
        if self.resets_state_on_frob() {
            *state = TState::default();
        }
    }

    /// An item was offered to this panel's entity (the port's tool channel:
    /// releasing a held item onto a target in VR, or a `ToolConsumable`
    /// touching it). `None` - the default - takes the ordinary deposit path,
    /// dropping the item into the entity as a container. A panel returns
    /// `Some(effect)` to consume the item as a *tool* instead: the hackable
    /// crate opens itself for an ICE Pick rather than swallowing it.
    fn on_provide_for_consumption(
        &self,
        _entity_id: EntityId,
        _world: &World,
        _provided_entity_id: EntityId,
    ) -> Option<crate::Effect> {
        None
    }
}
