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

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &TState,
        msg: &TMsg,
    ) -> (TState, crate::Effect);
}
