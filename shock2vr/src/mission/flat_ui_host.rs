//! Flat-mode MFD panel host (projects/flat-ui.md §5.2, PR 2).
//!
//! The flat presentation of the shared `Gui` layer: default VR opens the same
//! object-bound panel as a world quad (`GuiManager` + `ProxyGuiScript`), while
//! the original flat game docks it in an MFD and drives it with the mouse
//! cursor. This host holds that flat-only state:
//!
//! - **Open**: `Effect::OpenPanel { entity }` (emitted by `GuiScript` on
//!   Frob) binds the panel to one world object - the original's single
//!   `gOverlayObj` binding.
//! - **Render**: the panel's `Effect::SetUI` component list is intercepted
//!   and drawn onto the shared 640x480 [`UiCanvas`] at the original game's
//!   left-MFD anchor `(2, 124)`, after the flat HUD, plus a host-drawn
//!   close button and the `CURSOR.PCX` pointer.
//! - **Input**: each frame the normalized 2D pointer is mapped through
//!   [`pointer_to_canvas`] into panel-local normalized coordinates and sent
//!   to the panel entity as `MessagePayload::GUIHover` - the exact contract
//!   the VR hand ray synthesizes (`virtual_hand.rs` / `ProxyGuiScript`), so
//!   every existing panel (keypad, container, elevator, ...) works unchanged.
//! - **Close**: the host close button, LMB on the bare 3D view (the
//!   original's exit gesture), or walking away from the bound object (the
//!   original's per-overlay `distance` auto-close).

use cgmath::{InnerSpace, Vector2, point2};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{EntitiesView, EntityId, Get, UniqueView, View, World};

use crate::{
    gui::GuiComponentRenderInfo,
    input_context::Pointer2D,
    mission::PlayerInfo,
    scripts::{Message, MessagePayload},
    ui::{Rect, ScaleMode, UiCanvas, pointer_to_canvas},
    vr_config::Handedness,
};

/// The shared 640x480 virtual canvas the flat HUD renders on - and, in VR, the
/// canvas the cyber-interface panel presents, so a ray is mapped onto exactly
/// the pixels the host lays its widgets out in.
pub const CANVAS_SIZE: Vector2<f32> = Vector2::new(640.0, 480.0);

/// The original game's left MFD slot anchor for world-object panels (keypad,
/// container, ...) on the 640x480 canvas: rect `(2, 124, 188x300)`.
/// The right slot `(450, 124)` is reserved for later character panels.
const LEFT_MFD_ANCHOR: Vector2<f32> = Vector2::new(2.0, 124.0);

/// The top-docked inventory strip anchor, matching the original game's
/// layout: `(2, 0)`, `inv_rect = 636x121` - horizontally centered on the 640
/// canvas, flush with the top edge.
const STRIP_ANCHOR: Vector2<f32> = Vector2::new(2.0, 0.0);

/// Walk-away auto-close distance (world units; dark units / SCALE_FACTOR).
/// ~10 feet - past normal frob range, so a panel opened up close survives
/// small repositioning but closes when the player leaves the object.
const PANEL_AUTO_CLOSE_DISTANCE: f32 = 4.0;

/// Host-drawn close button, panel-local: the original keypad overlay's
/// CloseOff/CloseOn gadget at (163, 8, 20x21) in the 188-wide panel,
/// generalized to hug the panel's top-right corner.
const CLOSE_BUTTON_SIZE: Vector2<f32> = Vector2::new(20.0, 21.0);
const CLOSE_BUTTON_MARGIN: Vector2<f32> = Vector2::new(25.0, 8.0);

/// `CURSOR.PCX` native size.
const CURSOR_SIZE: Vector2<f32> = Vector2::new(12.0, 16.0);

/// A host-side action produced by the cursor-is-the-item drag (§1.5/§2.4),
/// applied by `mission_core` because it touches physics/effects.
///
/// Lift/place/swap are *not* here: a lifted item **stays in the backpack
/// container** (the host just hides it from the strip while it rides the
/// cursor), so it is always reachable and serializes correctly on
/// save/transition. Only committing the drag reaches the world: **Throw**
/// detaches it and gives it world presence with an impulse along the view ray;
/// **Wield** equips/uses it (a double-click) via the same effect as a backpack
/// click, acting on the still-contained item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatUiDragAction {
    Throw(EntityId),
    Wield(EntityId),
    /// Cycle the wielded weapon's ammo type (the AMMOFULL cycle button
    /// was clicked). Not tied to a cursor item - the caller maps it to
    /// `Effect::CycleAmmo`, which acts on the wielded weapon.
    CycleAmmo,
}

/// Double-click window (frames at 60Hz) and radius (canvas px) for the
/// wield gesture: a second press on the just-lifted item's slot within this
/// window/radius wields it instead of placing (the original equips from the
/// inventory; with a single flat pointer button a double-click is the
/// faithful mapping - projects/flat-ui.md §1.5).
const DOUBLE_CLICK_WINDOW_FRAMES: u32 = 20;
const DOUBLE_CLICK_RADIUS: f32 = 24.0;

/// What a press that lands on neither the strip nor the MFD panel means.
///
/// The two presentations genuinely differ here, and only here. On flat the
/// canvas is drawn over the 3D view, so everything the widgets do not cover
/// *is* the world: clicking it is the original's exit gesture (close the MFD,
/// throw the held item). In VR the same canvas is the cyber-interface panel
/// hanging in front of the player - its empty pixels are still the interface,
/// and a ray off the panel entirely means that hand is not pointing at the UI
/// at all (it stays a world hand, see
/// [`crate::ui::vr_frontend_pointer_pass`]).
///
/// This is an input-semantics difference, not a layout one: nothing here moves
/// or resizes a widget, so the shared canvas still renders identically
/// (AGENTS.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BareViewPress {
    /// Flat: a press off the widgets is a click on the 3D view behind them.
    Exit,
    /// VR: a press off the widgets is not an exit gesture. (Throwing a held
    /// item into the world from the panel is a later slice.)
    Ignore,
}

/// One frame of pointing at the host's canvas, already resolved to canvas
/// pixels.
///
/// The single input contract both presentations feed: flat resolves it from the
/// mouse through [`pointer_to_canvas`], VR from the controller ray through
/// [`crate::ui::ray_to_canvas`]. Everything downstream - hover routing, the
/// close gestures, the cursor-is-the-item drag - runs on this one struct, so
/// the mouse and the VR ray cannot drift into two different interfaces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasPointer {
    /// Where the pointer is on the 640x480 canvas, or `None` when it is off it
    /// (flat: the letterbox bars; VR: the ray missed the panel).
    pub canvas_pos: Option<Vector2<f32>>,
    /// The click button: flat LMB, VR trigger.
    pub pressed: bool,
    /// The grab gesture: the VR squeeze. Flat has none, so it is always false
    /// there - the same `GUIHover` field the hand ray fills, which is what lets
    /// a squeeze on a panel item pull it into the hand.
    pub grabbing: bool,
    /// Which hand the gesture belongs to, for the panel's per-hand edge
    /// tracking and for `GrabEntity`'s destination hand. Flat reports `Right`.
    pub hand: Handedness,
    /// What a press away from the host's widgets means.
    pub bare_view: BareViewPress,
}

/// The cyber interface's pointer for one frame: a resolved VR pointer pass
/// mapped into the host's canvas-pointer contract.
///
/// The whole VR side of the bridge, in one place: the ray's canvas hit is the
/// cursor position, the trigger is the click button and the squeeze is the grab
/// - reported for the hand the pass says owns the panel, which is also the hand
/// a `GrabEntity` from a slot lands in.
///
/// A pointer is produced even when nothing is on the panel (`canvas_pos:
/// None`). The trigger must stay accounted for while it is held off-panel, or
/// sweeping a still-held press onto a slot would read as a fresh edge and lift
/// an item nobody clicked.
pub fn vr_canvas_pointer(
    pass: &crate::ui::FrontendPointerPass,
    input_context: &crate::input_context::InputContext,
) -> CanvasPointer {
    let hand = pass
        .active_ray()
        .map(|ray| ray.handedness)
        .unwrap_or(Handedness::Right);
    let input_hand = match hand {
        Handedness::Left => &input_context.left_hand,
        Handedness::Right => &input_context.right_hand,
    };
    CanvasPointer {
        canvas_pos: pass.point(),
        pressed: pass.pressed,
        grabbing: input_hand.squeeze_value > crate::ui::VR_TRIGGER_THRESHOLD,
        hand,
        bare_view: BareViewPress::Ignore,
    }
}

/// The just-lifted item and where/when it was lifted, so a quick second click
/// on the same slot reads as a double-click (wield) rather than a place.
struct LiftMark {
    entity: EntityId,
    canvas_pos: Vector2<f32>,
    frames_left: u32,
}

/// The item currently riding the cursor (the original's `drag_obj`): the
/// cursor renders this item's icon instead of the arrow, and the item has no
/// container link and no world presence until placed or thrown.
struct CursorItem {
    entity: EntityId,
    /// `objicon` art (`"<icon>.pcx"`) drawn as the cursor, if the item has one.
    icon: Option<String>,
    /// The item's symbolic name, for `/v1/ui` `cursor.label`.
    label: Option<String>,
}

/// Flat-mode MFD panel state: which world object (if any) has its panel
/// open, the panel's latest components (from `Effect::SetUI`), and the
/// cursor/pointer bookkeeping needed to render and hit-test them.
pub struct FlatUiHost {
    active_panel: Option<EntityId>,
    /// Panel size in panel-local pixels (from `SetUI.world_size`); `None`
    /// until the panel's first `SetUI` arrives (the frame after opening).
    panel_size_px: Option<Vector2<f32>>,
    /// Latest `SetUI` components for the active panel (normalized panel
    /// coordinates, as `GuiScript` emits them).
    components: Vec<GuiComponentRenderInfo>,
    /// Tab metagame ("use") mode: the top-docked inventory strip, bound to
    /// the player's `internal_inventory` entity. `Some` exactly while flat
    /// use mode is on (projects/flat-ui.md §5.2, PR 4).
    strip: Option<StripSlot>,
    /// The item lifted onto the cursor (the original's "cursor IS the item"
    /// drag, §2.4). `Some` between a lift and the place/throw that clears it.
    cursor_item: Option<CursorItem>,
    /// The most recent lift, for double-click (wield) detection. Counts down
    /// each frame and clears when the window elapses.
    last_lift: Option<LiftMark>,
    /// The AMMOFULL ammo-cycle button rect on the 640x480 canvas, set each
    /// frame by the mission when use mode has a multi-ammo weapon wielded
    /// (flat UI 5); `None` otherwise. Clicking it emits `CycleAmmo`.
    ammo_cycle_rect: Option<Rect>,
    /// The active panel was opened unbound (the automap): it has no world
    /// object, so the walk-away distance auto-close is skipped. Cleared on
    /// open/close.
    sticky_panel: bool,
    /// Pointer position on the 640x480 canvas (None: no pointer / letterbox).
    cursor_canvas: Option<Vector2<f32>>,
    hover_close: bool,
    last_pointer_pressed: bool,
    /// The pointer of the latest [`update_canvas`](Self::update_canvas), for
    /// `GET /v1/ui` - so a test can see where the VR ray actually landed on the
    /// canvas instead of inferring it from what lit up.
    last_pointer: Option<CanvasPointer>,
    /// Whether the grab gesture was down last frame, so a *held* squeeze is
    /// reported to the panel only on its rising edge. The panel's grab handler
    /// is level-triggered (`GuiComponent::get_event` reads `is_grabbed`
    /// directly), so a squeeze held while the ray swept the grid would take
    /// every slot it crossed.
    last_pointer_grabbing: bool,
    /// Last known render-target size, for pointer->canvas letterbox mapping
    /// (updated every rendered frame; 4:3 default until the first render).
    screen_size: Vector2<f32>,
}

/// The inventory strip's slot state - the same stash-latest-`SetUI` shape as
/// the MFD panel slot, docked at [`STRIP_ANCHOR`] instead of the left MFD.
struct StripSlot {
    entity: EntityId,
    /// Strip size in panel-local pixels (from `SetUI.world_size`); `None`
    /// until the first `SetUI` after entering use mode.
    size_px: Option<Vector2<f32>>,
    components: Vec<GuiComponentRenderInfo>,
}

impl FlatUiHost {
    pub fn new() -> FlatUiHost {
        FlatUiHost {
            active_panel: None,
            panel_size_px: None,
            components: Vec::new(),
            strip: None,
            cursor_item: None,
            last_lift: None,
            ammo_cycle_rect: None,
            sticky_panel: false,
            cursor_canvas: None,
            hover_close: false,
            last_pointer_pressed: false,
            last_pointer: None,
            last_pointer_grabbing: false,
            screen_size: CANVAS_SIZE,
        }
    }

    pub fn active_panel(&self) -> Option<EntityId> {
        self.active_panel
    }

    /// The entity whose inventory the top-docked strip shows, while use mode
    /// is on.
    pub fn strip_entity(&self) -> Option<EntityId> {
        self.strip.as_ref().map(|s| s.entity)
    }

    /// Whether `canvas_pos` lands on the top-docked inventory strip.
    ///
    /// The strip is the interface's "put it in the backpack" surface, so a VR
    /// hand releasing a held item over it deposits rather than drops (see
    /// `MissionCore::strip_deposit_entities`). Answered from the same
    /// [`strip_rect`](Self::strip_rect) the host hit-tests its own gestures
    /// against, so the drop target can never drift from the drawn strip.
    pub fn strip_contains(&self, canvas_pos: Vector2<f32>) -> bool {
        self.strip_rect()
            .is_some_and(|rect| rect.contains(canvas_pos))
    }

    /// The item currently held on the cursor mid-drag (for `/v1/ui` `cursor`).
    pub fn cursor_debug(&self) -> Option<crate::game_scene::DebugUiCursor> {
        self.cursor_item
            .as_ref()
            .map(|c| crate::game_scene::DebugUiCursor {
                entity_id: c.entity.inner() as i32,
                label: c.label.clone(),
            })
    }

    /// Where the pointer last landed on the canvas, for `GET /v1/ui`.
    pub fn pointer_debug(&self) -> Option<crate::game_scene::DebugUiPointer> {
        self.last_pointer
            .map(|pointer| crate::game_scene::DebugUiPointer {
                canvas: pointer.canvas_pos.map(|p| [p.x, p.y]),
                pressed: pointer.pressed,
                grabbing: pointer.grabbing,
                // Only meaningful when something is actually on the canvas: off
                // it there is no owning hand, and reporting one would invent a
                // pointer the interface does not have.
                hand: pointer.canvas_pos.map(|_| match pointer.hand {
                    Handedness::Left => "left".to_string(),
                    Handedness::Right => "right".to_string(),
                }),
            })
    }

    /// Swallow a button that is already held as the host takes over input, so
    /// it cannot read as a fresh press-edge on the next frame.
    ///
    /// [`open`](Self::open) does this for the MFD slot (the frob that opened
    /// the panel is still down); entering use mode needs the same guard, in
    /// both presentations - an LMB overlapping the Tab, or a VR trigger held
    /// while the interface opens. Rule 6 of the vr-ui-design skill: a screen
    /// entered under a held button starts "already pressed".
    pub fn guard_held_press(&mut self) {
        self.last_pointer_pressed = true;
    }

    /// Set (or clear) the AMMOFULL ammo-cycle button's canvas rect for this
    /// frame. The mission passes `Some(rect)` only in use mode with a
    /// multi-ammo weapon wielded, matching what the flat HUD draws.
    pub fn set_ammo_cycle_button(&mut self, rect: Option<Rect>) {
        self.ammo_cycle_rect = rect;
    }

    /// The ammo-cycle button as a `/v1/ui` element (so tests click it by
    /// meaning), or `None` when it is not shown.
    pub fn ammo_cycle_debug(&self) -> Option<crate::game_scene::DebugUiElement> {
        self.ammo_cycle_rect
            .map(|r| crate::game_scene::DebugUiElement {
                kind: "button".to_string(),
                texture: Some("ammoarw0.pcx".to_string()),
                text: None,
                label: Some("cycle_ammo".to_string()),
                entity_id: None,
                rect: [r.x, r.y, r.w, r.h],
                screen_rect: self.to_screen_rect(r),
            })
    }

    /// Take the item off the cursor (clearing it), returning its entity id.
    /// The caller returns it to the backpack - the original refuses to leave
    /// the metagame while `drag_obj` is set, so Tab-out re-homes the item
    /// rather than losing it (projects/flat-ui.md §1.5).
    pub fn take_cursor_item(&mut self) -> Option<EntityId> {
        self.last_lift = None;
        self.cursor_item.take().map(|c| c.entity)
    }

    /// Drop every host-side reference to a destroyed entity immediately.
    /// Normally `update` notices a dead panel on the next frame, but a cursor
    /// item and its double-click mark are pure host state and otherwise retain
    /// a recycled `EntityId`. The destruction effect pipeline calls this before
    /// deleting the world entity.
    pub fn on_entity_destroyed(&mut self, entity: EntityId) {
        if self.active_panel == Some(entity) {
            self.close();
        }
        if self
            .strip
            .as_ref()
            .is_some_and(|strip| strip.entity == entity)
        {
            self.strip = None;
        }
        self.components
            .retain(|component| component_entity(component) != Some(entity));
        if let Some(strip) = self.strip.as_mut() {
            strip
                .components
                .retain(|component| component_entity(component) != Some(entity));
        }
        if self
            .cursor_item
            .as_ref()
            .is_some_and(|item| item.entity == entity)
        {
            self.cursor_item = None;
        }
        if self
            .last_lift
            .as_ref()
            .is_some_and(|mark| mark.entity == entity)
        {
            self.last_lift = None;
        }
    }

    /// Enter/leave Tab metagame mode: bind (or drop) the top-docked
    /// inventory strip. `Some(entity)` is the player's `internal_inventory`
    /// entity, whose `GuiScript` already emits `SetUI` every frame - the
    /// strip just stashes and re-anchors it.
    pub fn set_strip(&mut self, entity: Option<EntityId>) {
        self.strip = entity.map(|entity| StripSlot {
            entity,
            size_px: None,
            components: Vec::new(),
        });
    }

    /// Bind the MFD to `entity` (the original's `gOverlayObj`). Opening a
    /// second panel replaces the first - one panel per (left) slot.
    pub fn open(&mut self, entity: EntityId) {
        if self.active_panel != Some(entity) {
            self.components.clear();
            self.panel_size_px = None;
        }
        self.active_panel = Some(entity);
        self.sticky_panel = false;
        // A button still held at open (e.g. the shift+LMB frob that opened
        // the panel on desktop) must not read as a fresh press-edge next
        // frame - it would instantly close the panel as a bare-view click.
        // Require a release to be observed first.
        self.last_pointer_pressed = true;
    }

    /// Bind the MFD to a panel with no world object behind it (the automap's
    /// synthetic player-owned entity): same as [`open`](Self::open) but the
    /// walk-away distance auto-close is skipped - the original's map overlay
    /// has no `distance` and closes only explicitly (close button / bare-view
    /// click / `ToggleMap` again).
    pub fn open_unbound(&mut self, entity: EntityId) {
        self.open(entity);
        self.sticky_panel = true;
    }

    pub fn close(&mut self) {
        self.active_panel = None;
        self.sticky_panel = false;
        self.panel_size_px = None;
        self.components.clear();
        self.hover_close = false;
    }

    /// Observe an `Effect::SetUI`: stash the component list if it belongs to
    /// the active panel or the inventory strip (the VR world-quad path is
    /// untouched by this).
    pub fn on_set_ui(
        &mut self,
        world: &World,
        parent_entity: EntityId,
        world_size: Vector2<f32>,
        components: &[GuiComponentRenderInfo],
    ) {
        let is_strip = self
            .strip
            .as_ref()
            .is_some_and(|strip| strip.entity == parent_entity);
        if !is_strip && self.active_panel != Some(parent_entity) {
            return;
        }

        // SetUI effects are snapshots produced during script update. A
        // DestroyEntity earlier in the same effect batch can invalidate one
        // of their entity-bound buttons, so never cache a dead/recycled id.
        let entities = world.borrow::<EntitiesView>().unwrap();
        let components: Vec<_> = components
            .iter()
            .filter(|component| {
                component_entity(component).is_none_or(|entity| entities.is_alive(entity))
            })
            .cloned()
            .collect();

        // `SetUI.world_size` is `screen_size_in_pixels * GUI_PIXEL_TO_WORLD_SIZE`.
        let size_px = world_size / crate::gui::GUI_PIXEL_TO_WORLD_SIZE;
        if is_strip {
            let strip = self.strip.as_mut().unwrap();
            strip.size_px = Some(size_px);
            strip.components = components;
            return;
        }
        self.panel_size_px = Some(size_px);
        self.components = components;
    }

    /// The render-target size, for the pointer->canvas letterbox mapping.
    /// Called from the flat render path each frame.
    pub fn set_screen_size(&mut self, screen_size: Vector2<f32>) {
        if screen_size.x > 0.0 && screen_size.y > 0.0 {
            self.screen_size = screen_size;
        }
    }

    /// The active panel's rect on the 640x480 canvas (None until its first
    /// `SetUI` arrives).
    fn panel_rect(&self) -> Option<Rect> {
        self.panel_size_px.map(panel_canvas_rect)
    }

    /// The inventory strip's top-docked rect on the 640x480 canvas (None
    /// outside use mode / until its first `SetUI` arrives).
    fn strip_rect(&self) -> Option<Rect> {
        self.strip
            .as_ref()
            .and_then(|s| s.size_px)
            .map(strip_canvas_rect)
    }

    /// Per-frame pointer processing while in flat presentation. Returns the
    /// `GUIHover` messages to dispatch to the strip/panel entity plus any
    /// cursor-drag [`FlatUiDragAction`]s (lift/place/throw) for the caller to
    /// apply. Handles the close gestures (close button, LMB on the bare view,
    /// walk-away, entity gone) and the cursor-is-the-item drag (§1.5/§2.4):
    /// LMB on a strip item lifts it onto the cursor, LMB on another slot
    /// places/swaps, LMB on the bare view throws it into the world. A
    /// bare-view click closes the MFD panel but never the strip - use mode is
    /// left by Tab (projects/flat-ui.md §5.2).
    pub fn update(
        &mut self,
        world: &World,
        pointer: Option<Pointer2D>,
    ) -> (Vec<Message>, Vec<FlatUiDragAction>) {
        // Flat's only job is resolving the mouse onto the canvas; the gestures
        // themselves live in the shared core below.
        let pointer = pointer.map(|pointer| CanvasPointer {
            canvas_pos: pointer_to_canvas(
                CANVAS_SIZE,
                pointer.position,
                self.screen_size,
                ScaleMode::PreserveAspect,
            ),
            pressed: pointer.pressed,
            grabbing: false,
            hand: Handedness::Right,
            bare_view: BareViewPress::Exit,
        });
        self.update_canvas(world, pointer)
    }

    /// The presentation-free core of [`update`](Self::update): the same frame
    /// of pointing, expressed in canvas pixels. VR's cyber-interface panel
    /// enters here directly (its ray is already a canvas point), so both
    /// presentations run one implementation of every gesture.
    pub fn update_canvas(
        &mut self,
        world: &World,
        pointer: Option<CanvasPointer>,
    ) -> (Vec<Message>, Vec<FlatUiDragAction>) {
        // Edge-detect the grab before anything can return early, so a squeeze
        // held across frames is one gesture wherever the ray goes.
        let grabbing = pointer.map(|p| p.grabbing).unwrap_or(false);
        let grab_edge = grabbing && !self.last_pointer_grabbing;
        self.last_pointer_grabbing = grabbing;
        // `/v1/ui` reports the gesture as the player is making it (held or
        // not); only what reaches the panel is reduced to the edge.
        self.last_pointer = pointer;
        let pointer = pointer.map(|pointer| CanvasPointer {
            grabbing: grab_edge,
            ..pointer
        });
        let pressed = pointer.map(|p| p.pressed).unwrap_or(false);
        let pressed_edge = pressed && !self.last_pointer_pressed;
        self.last_pointer_pressed = pressed;

        // Age out the double-click window since the last lift.
        if let Some(mark) = self.last_lift.as_mut() {
            match mark.frames_left.checked_sub(1) {
                Some(remaining) => mark.frames_left = remaining,
                None => self.last_lift = None,
            }
        }

        // MFD-slot auto-close: the bound object is gone (destroyed / level
        // state changed), or the player walked away from it (the original's
        // per-overlay `distance` check). The strip is bound to the player's
        // own inventory entity - neither applies.
        if let Some(panel) = self.active_panel {
            let alive = world
                .borrow::<EntitiesView>()
                .map(|entities| entities.is_alive(panel))
                .unwrap_or(false);
            // Inventory-item MFDs (research reports, readable media) remain
            // bound while carried. Their inherited/last world position may be
            // on the other side of the level and is no longer meaningful.
            let carried =
                alive && crate::scripts::script_util::player_carried_items(world).contains(&panel);
            let too_far = alive
                && !self.sticky_panel
                && !carried
                && (|| {
                    let player = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
                    let v_pos = world
                        .borrow::<View<dark::properties::PropPosition>>()
                        .ok()?;
                    let pos = v_pos.get(panel).ok()?;
                    Some((pos.position - player.pos).magnitude() > PANEL_AUTO_CLOSE_DISTANCE)
                })()
                .unwrap_or(false);
            if !alive || too_far {
                self.close();
            }
        }

        if self.active_panel.is_none() && self.strip.is_none() {
            self.cursor_canvas = None;
            return (Vec::new(), Vec::new());
        }

        let Some(pointer) = pointer else {
            self.cursor_canvas = None;
            self.hover_close = false;
            return (Vec::new(), Vec::new());
        };
        let canvas_pos = pointer.canvas_pos;
        self.cursor_canvas = canvas_pos;
        if canvas_pos.is_none() {
            self.hover_close = false;
        }

        // A press fully outside the canvas (flat: the letterbox bars) is a
        // bare-view click: throw a held item, else close the panel. In VR the
        // canvas is the panel, so an off-panel press belongs to the world hand.
        let Some(canvas_pos) = canvas_pos else {
            if pressed_edge && pointer.bare_view == BareViewPress::Exit {
                if let Some(held) = self.cursor_item.take() {
                    return (Vec::new(), vec![FlatUiDragAction::Throw(held.entity)]);
                }
                self.close();
            }
            return (Vec::new(), Vec::new());
        };

        let strip_rect = self
            .strip
            .as_ref()
            .and_then(|s| s.size_px)
            .map(strip_canvas_rect);
        let over_strip = strip_rect.map(|r| r.contains(canvas_pos)).unwrap_or(false);
        let panel_rect = self.panel_rect();
        // The close button hugs the panel's corner but sits just outside the
        // panel rect; treat both as "over the panel" so a held item is never
        // thrown from there.
        let over_panel = panel_rect
            .map(|r| r.contains(canvas_pos) || close_button_canvas_rect(r).contains(canvas_pos))
            .unwrap_or(false);
        let over_ammo = self
            .ammo_cycle_rect
            .map(|r| r.contains(canvas_pos))
            .unwrap_or(false);

        // --- Cursor-is-the-item drag: while an item rides the cursor, LMB
        // places/swaps/throws it and never routes to a GuiScript (protecting
        // the held item - the original blocks losing `drag_obj`). ---
        if self.cursor_item.is_some() {
            self.hover_close = false;
            if !pressed_edge {
                return (Vec::new(), Vec::new());
            }
            if over_strip {
                let held = self.cursor_item.as_ref().unwrap().entity;
                // Double-click on the just-lifted item's slot wields it (the
                // second press of a quick same-spot double-click) rather than
                // placing - the faithful single-button equip gesture.
                if self.is_double_click(held, canvas_pos) {
                    self.cursor_item = None;
                    self.last_lift = None;
                    return (Vec::new(), vec![FlatUiDragAction::Wield(held)]);
                }
                // Place (or swap): the held item stays in the backpack the
                // whole time, so placing is just dropping it off the cursor.
                // Dropping onto another item swaps by lifting that occupant.
                self.last_lift = None;
                match self.strip_item_at(canvas_pos) {
                    // An always-collected occupant is collected rather than
                    // swapped onto the cursor - the same rule as a plain click
                    // below, so the swap can't be a back door into carrying one.
                    Some(target)
                        if target != held
                            && crate::scripts::script_util::is_always_collected(world, target) =>
                    {
                        self.cursor_item = None;
                        return (
                            vec![Message {
                                to: target,
                                payload: MessagePayload::Frob,
                            }],
                            Vec::new(),
                        );
                    }
                    Some(target) if target != held => {
                        self.cursor_item = Some(make_cursor_item(world, target));
                        self.last_lift = Some(LiftMark {
                            entity: target,
                            canvas_pos,
                            frames_left: DOUBLE_CLICK_WINDOW_FRAMES,
                        });
                    }
                    _ => self.cursor_item = None,
                }
                return (Vec::new(), Vec::new());
            }
            if over_panel || over_ammo || pointer.bare_view == BareViewPress::Ignore {
                // Escape hatch: a click on an open MFD or the AMMOFULL cycle
                // button keeps the held item (a visible button must not throw
                // the item you're carrying) - and so does empty panel space in
                // VR, where there is no 3D view behind the canvas to throw at.
                return (Vec::new(), Vec::new());
            }
            // Bare 3D view: throw the held item along the view ray.
            let held = self.cursor_item.take().unwrap();
            self.last_lift = None;
            return (Vec::new(), vec![FlatUiDragAction::Throw(held.entity)]);
        }

        // --- AMMOFULL ammo-cycle button (use mode, multi-ammo weapon): a
        // click cycles the wielded ammo type. Checked with an empty cursor
        // only (mid-drag, a bottom-right click is a throw), before strip/panel
        // routing since the button is disjoint from both. ---
        if pressed_edge {
            if let Some(rect) = self.ammo_cycle_rect {
                if rect.contains(canvas_pos) {
                    return (Vec::new(), vec![FlatUiDragAction::CycleAmmo]);
                }
            }
        }

        // --- Empty cursor over the strip: LMB on an item lifts it onto the
        // cursor (the item stays in the backpack - the host just hides it from
        // the strip - so nothing reaches the world). Hover highlights via
        // GUIHover. ---
        if over_strip {
            self.hover_close = false;
            if pressed_edge {
                if let Some(item) = self.strip_item_at(canvas_pos) {
                    // An always-collected item is never carried on the cursor:
                    // it collects, exactly as it would from the world or a loot
                    // panel. Reachable only for one already sitting in the
                    // backpack - a save made before its category collected on
                    // pickup - which is the one way it can be in the grid at
                    // all, and otherwise the one way it could never be
                    // collected.
                    if crate::scripts::script_util::is_always_collected(world, item) {
                        return (
                            vec![Message {
                                to: item,
                                payload: MessagePayload::Frob,
                            }],
                            Vec::new(),
                        );
                    }
                    self.cursor_item = Some(make_cursor_item(world, item));
                    self.last_lift = Some(LiftMark {
                        entity: item,
                        canvas_pos,
                        frames_left: DOUBLE_CLICK_WINDOW_FRAMES,
                    });
                    return (Vec::new(), Vec::new());
                }
                return (Vec::new(), Vec::new());
            }
            let rect = strip_rect.unwrap();
            let entity = self.strip.as_ref().unwrap().entity;
            // The click (`is_triggered`) is deliberately withheld: the host
            // owns strip clicks as the cursor-is-the-item drag above. The
            // squeeze is not - it is how a VR hand takes an item straight out
            // of the grid, exactly as it does from a loot panel.
            return (
                vec![gui_hover(entity, rect, canvas_pos, false, pointer)],
                Vec::new(),
            );
        }

        let Some(panel) = self.active_panel else {
            // Use mode with no MFD open: a bare-view click has nothing to
            // close (Tab leaves use mode).
            return (Vec::new(), Vec::new());
        };
        let Some(rect) = panel_rect else {
            // Panel opened this frame; no SetUI yet - nothing to hit-test.
            return (Vec::new(), Vec::new());
        };

        // Host-drawn close button (the shared guis have no close component;
        // VR uses its world-space distance close instead).
        let close_rect = close_button_canvas_rect(rect);
        self.hover_close = close_rect.contains(canvas_pos);
        if pressed_edge && self.hover_close {
            self.close();
            return (Vec::new(), Vec::new());
        }

        if rect.contains(canvas_pos) {
            (
                vec![gui_hover(panel, rect, canvas_pos, pointer.pressed, pointer)],
                Vec::new(),
            )
        } else {
            // LMB on the bare 3D view closes the panels (manual p.7).
            if pressed_edge && pointer.bare_view == BareViewPress::Exit {
                self.close();
            }
            (Vec::new(), Vec::new())
        }
    }

    /// Whether a press on `held`'s slot at `canvas_pos` is the second click of
    /// a double-click on the item just lifted (same entity, within the window
    /// and radius of the lift) - the wield gesture.
    fn is_double_click(&self, held: EntityId, canvas_pos: Vector2<f32>) -> bool {
        self.last_lift.as_ref().is_some_and(|mark| {
            mark.entity == held
                && mark.frames_left > 0
                && (mark.canvas_pos - canvas_pos).magnitude() <= DOUBLE_CLICK_RADIUS
        })
    }

    /// The entity currently hidden from the strip because it is on the cursor.
    fn held_entity(&self) -> Option<EntityId> {
        self.cursor_item.as_ref().map(|c| c.entity)
    }

    /// The interactive strip item whose canvas rect contains `canvas_pos`
    /// (loot/backpack item buttons carry their entity), for lift/swap
    /// hit-testing. The item on the cursor is hidden, so it never hit-tests.
    fn strip_item_at(&self, canvas_pos: Vector2<f32>) -> Option<EntityId> {
        let strip = self.strip.as_ref()?;
        let rect = strip.size_px.map(strip_canvas_rect)?;
        let held = self.held_entity();
        strip.components.iter().find_map(|c| match c {
            GuiComponentRenderInfo::Image {
                interactive: true,
                entity: Some(entity),
                ..
            } if Some(*entity) != held && c.canvas_rect(rect).contains(canvas_pos) => Some(*entity),
            _ => None,
        })
    }

    /// Render the inventory strip + active panel + cursor as screen-space
    /// overlay objects on the shared 640x480 canvas (drawn after the flat
    /// HUD).
    /// The host's slots (strip, MFD panel, cursor) described once on the
    /// shared 640x480 canvas, or `None` when nothing is bound. Both
    /// presentations render exactly this canvas - flat on screen
    /// ([`Self::render`]), VR on the cyber-interface world panel
    /// ([`Self::render_world_space`]) - so their layout cannot drift apart
    /// (AGENTS.md §3).
    fn build_canvas(&self) -> Option<UiCanvas> {
        let strip_rect = self.strip_rect();
        let panel_rect = self.panel_rect();
        if strip_rect.is_none() && panel_rect.is_none() {
            return None;
        }
        let mut canvas = UiCanvas::new(CANVAS_SIZE);
        if let (Some(strip), Some(rect)) = (self.strip.as_ref(), strip_rect) {
            // Hide the item riding the cursor from the strip grid (it is drawn
            // as the cursor instead).
            draw_components(&mut canvas, &strip.components, rect, self.held_entity());
        }
        if let Some(rect) = panel_rect {
            draw_components(&mut canvas, &self.components, rect, None);
            canvas.image(
                close_button_canvas_rect(rect),
                if self.hover_close {
                    "closeon.pcx"
                } else {
                    "closeoff.pcx"
                },
            );
        }
        if let Some(cursor) = self.cursor_canvas {
            // The cursor IS the lifted item: draw its icon in place of the
            // arrow (the original's `SCM_DRAGOBJ`, §2.4). Fall back to the
            // arrow when the held item has no icon or nothing is held.
            match self.cursor_item.as_ref().and_then(|c| c.icon.as_deref()) {
                // No slot rect: object icons draw at their authored size, and
                // any rect bigger than the art would only center the icon
                // inside it - i.e. slide it off the pointer.
                Some(icon) => canvas.object_icon(Rect::new(cursor.x, cursor.y, 0.0, 0.0), icon),
                None => canvas.image(
                    Rect::new(cursor.x, cursor.y, CURSOR_SIZE.x, CURSOR_SIZE.y),
                    "cursor.pcx",
                ),
            };
        }
        Some(canvas)
    }

    /// Screen-space presentation (flat): the canvas letterboxed onto the
    /// render target. Empty when nothing is bound.
    pub fn render(
        &self,
        asset_cache: &mut AssetCache,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        match self.build_canvas() {
            Some(canvas) => {
                canvas.render_screen_space(asset_cache, screen_size, ScaleMode::PreserveAspect)
            }
            None => Vec::new(),
        }
    }

    /// World-space presentation (the VR cyber interface): the same canvas on
    /// an already-placed panel. The panel transform is the only
    /// per-presentation input - placement stays with the caller's
    /// `FrontendPanelAnchor`, and no layout decision is made here.
    pub fn render_world_space(
        &self,
        asset_cache: &mut AssetCache,
        panel: &crate::ui::WorldPanel,
    ) -> Vec<SceneObject> {
        match self.build_canvas() {
            Some(canvas) => canvas.render_world_space(
                asset_cache,
                panel.transform(),
                None,
                None,
                crate::ui::VR_COMPONENT_Z_STEP,
            ),
            None => Vec::new(),
        }
    }

    /// Introspection snapshot of the active panel's elements for `GET /v1/ui`:
    /// canvas + normalized-screen rects and semantic labels, so clients click
    /// widgets by meaning instead of hardcoded pixels. Entity-bound buttons
    /// (loot-panel items) label as the item's name and carry its entity id;
    /// other clickables fall back to art-derived labels (keypad digits).
    pub fn debug_elements(&self, world: &World) -> Vec<crate::game_scene::DebugUiElement> {
        let Some(rect) = self.panel_rect() else {
            return Vec::new();
        };
        let mut out = self.elements_for(world, &self.components, rect, None);
        // The host-drawn close button is clickable too.
        let close = close_button_canvas_rect(rect);
        out.push(crate::game_scene::DebugUiElement {
            kind: "button".to_string(),
            texture: Some("closeoff.pcx".to_string()),
            text: None,
            label: Some("close".to_string()),
            entity_id: None,
            rect: [close.x, close.y, close.w, close.h],
            screen_rect: self.to_screen_rect(close),
        });
        out
    }

    /// Introspection snapshot of the inventory strip's elements for
    /// `GET /v1/ui` (`strip`) - the same element contract as `debug_elements`
    /// (carried items label as their name + entity id). Empty outside use
    /// mode. The strip has no close element: use mode is left by Tab.
    pub fn strip_debug_elements(&self, world: &World) -> Vec<crate::game_scene::DebugUiElement> {
        match (self.strip.as_ref(), self.strip_rect()) {
            // Hide the item on the cursor: it left the grid for the drag.
            (Some(strip), Some(rect)) => {
                self.elements_for(world, &strip.components, rect, self.held_entity())
            }
            _ => Vec::new(),
        }
    }

    fn to_screen_rect(&self, r: Rect) -> [f32; 4] {
        let s = crate::ui::canvas_rect_to_screen(
            r,
            CANVAS_SIZE,
            self.screen_size,
            ScaleMode::PreserveAspect,
        );
        [s.x, s.y, s.w, s.h]
    }

    fn elements_for(
        &self,
        world: &World,
        components: &[GuiComponentRenderInfo],
        rect: Rect,
        hide: Option<EntityId>,
    ) -> Vec<crate::game_scene::DebugUiElement> {
        let mut out = Vec::new();
        for component in components {
            if is_gui_cursor(component) {
                continue;
            }
            if let GuiComponentRenderInfo::Image {
                entity: Some(entity),
                ..
            } = component
            {
                if Some(*entity) == hide {
                    continue;
                }
            }
            let r = component.canvas_rect(rect);
            let (kind, texture, text, label, entity_id) = match component {
                GuiComponentRenderInfo::Image {
                    texture,
                    interactive,
                    entity,
                    label,
                    ..
                } => (
                    if *interactive { "button" } else { "image" },
                    Some(texture.clone()),
                    None,
                    // Label precedence: an explicit panel-supplied label (e.g.
                    // an elevator floor name) wins, then the bound entity's
                    // symbolic name (loot items), then the art-derived fallback
                    // for clickables (keypad digits). This keeps every
                    // interactive element addressable by meaning in `/v1/ui`.
                    label
                        .clone()
                        .or_else(|| (*entity).and_then(|e| entity_label(world, e)))
                        .or_else(|| {
                            if *interactive {
                                semantic_label(texture)
                            } else {
                                None
                            }
                        }),
                    entity.map(|e| e.inner() as i32),
                ),
                GuiComponentRenderInfo::Text { text, .. } => {
                    ("text", None, Some(text.clone()), None, None)
                }
            };
            out.push(crate::game_scene::DebugUiElement {
                kind: kind.to_string(),
                texture,
                text,
                label,
                entity_id,
                rect: [r.x, r.y, r.w, r.h],
                screen_rect: self.to_screen_rect(r),
            });
        }
        out
    }
}

impl Default for FlatUiHost {
    fn default() -> Self {
        Self::new()
    }
}

/// The active panel's rect on the canvas: panel-local pixels anchored at the
/// original left-MFD slot.
fn panel_canvas_rect(panel_size_px: Vector2<f32>) -> Rect {
    Rect::new(
        LEFT_MFD_ANCHOR.x,
        LEFT_MFD_ANCHOR.y,
        panel_size_px.x,
        panel_size_px.y,
    )
}

/// The inventory strip's rect on the canvas: panel-local pixels docked at
/// the original top-of-screen inventory anchor.
fn strip_canvas_rect(strip_size_px: Vector2<f32>) -> Rect {
    Rect::new(
        STRIP_ANCHOR.x,
        STRIP_ANCHOR.y,
        strip_size_px.x,
        strip_size_px.y,
    )
}

/// A `GUIHover` for `to`, in panel-local normalized coordinates - the same
/// message the VR hand ray produces, so `GuiScript`'s hit-test/edge-detection
/// runs unchanged. The click button maps to LMB on flat and the trigger in VR;
/// the grab gesture is the VR squeeze (flat has none), and the hand it is
/// reported for is where a `GrabEntity` from the panel lands.
fn gui_hover(
    to: EntityId,
    rect: Rect,
    canvas_pos: Vector2<f32>,
    pressed: bool,
    pointer: CanvasPointer,
) -> Message {
    let local = point2(
        (canvas_pos.x - rect.x) / rect.w,
        (canvas_pos.y - rect.y) / rect.h,
    );
    Message {
        to,
        payload: MessagePayload::GUIHover {
            held_entity_id: None,
            screen_coordinates: local,
            is_triggered: pressed,
            is_grabbing: pointer.grabbing,
            hand: pointer.hand,
        },
    }
}

/// Draw one slot's `SetUI` components into its canvas rect. `hide` is the
/// entity riding the cursor (if any) - its component is skipped so it does not
/// also render in the grid.
fn draw_components(
    canvas: &mut UiCanvas,
    components: &[GuiComponentRenderInfo],
    rect: Rect,
    hide: Option<EntityId>,
) {
    for component in components {
        if is_gui_cursor(component) {
            // GuiScript appends its own panel-local cursor image for the
            // VR quads; the host draws the real screen cursor instead.
            continue;
        }
        // A zero-alpha button is a hit target over art already baked into the
        // panel backdrop (the HRM board's unlit node boxes). Keep it in input
        // routing and debug introspection, but do not paint its placeholder
        // texture over the backdrop.
        if matches!(
            component,
            GuiComponentRenderInfo::Image { alpha, .. } if *alpha <= 0.0
        ) {
            continue;
        }
        if let GuiComponentRenderInfo::Image {
            entity: Some(entity),
            ..
        } = component
        {
            if Some(*entity) == hide {
                continue;
            }
        }
        // The same component -> element conversion the VR world panel uses,
        // so the two presentations cannot lay the panel out differently.
        // Only the opacity differs: the original MFD art is opaque on screen,
        // while the component alphas (the elevator's 0.7 floor labels, say) are
        // a VR world-quad translucency, deliberately not applied here.
        canvas.push(component.to_ui_element(rect)).opacity(1.0);
    }
}

fn component_entity(info: &GuiComponentRenderInfo) -> Option<EntityId> {
    match info {
        GuiComponentRenderInfo::Image { entity, .. } => *entity,
        GuiComponentRenderInfo::Text { .. } => None,
    }
}

/// Host-drawn close button: top-right corner of the panel (the original
/// keypad overlay's CloseOff gadget position).
fn close_button_canvas_rect(panel: Rect) -> Rect {
    Rect::new(
        panel.x + panel.w - CLOSE_BUTTON_MARGIN.x,
        panel.y + CLOSE_BUTTON_MARGIN.y,
        CLOSE_BUTTON_SIZE.x,
        CLOSE_BUTTON_SIZE.y,
    )
}

/// Resolve a lifted item's cursor art (`objicon`) and label (`SymName`) for
/// the cursor-is-the-item drag.
fn make_cursor_item(world: &World, entity: EntityId) -> CursorItem {
    let icon = world
        .borrow::<View<dark::properties::PropObjIcon>>()
        .ok()
        .and_then(|v| v.get(entity).ok().map(|i| format!("{}.pcx", i.0)));
    CursorItem {
        entity,
        icon,
        label: entity_label(world, entity),
    }
}

/// Semantic label for an entity-bound element (a loot-panel item): the
/// entity's symbolic name (the same identity `/v1/entities` reports).
fn entity_label(world: &World, entity: EntityId) -> Option<String> {
    world
        .borrow::<View<dark::properties::PropSymName>>()
        .ok()
        .and_then(|v| v.get(entity).ok().map(|name| name.0.clone()))
}

/// Semantic label for a clickable element, derived from its art name. The
/// keypad's digit buttons use the `key<c><0|1>.pcx` convention (`c` = the
/// digit, `n` = clear; the trailing 0/1 is the normal/hover art state), so
/// both art states of a digit label identically.
fn semantic_label(texture: &str) -> Option<String> {
    let t = texture.to_ascii_lowercase();
    let rest = t.strip_prefix("key")?.strip_suffix(".pcx")?;
    let mut chars = rest.chars();
    let (c, state) = (chars.next()?, chars.next()?);
    if chars.next().is_some() || !matches!(state, '0' | '1') {
        return None;
    }
    match c {
        '0'..='9' => Some(c.to_string()),
        'n' => Some("clear".to_string()),
        _ => None,
    }
}

/// `GuiScript` appends a panel-local `cursor.pcx` image for the VR quads;
/// the flat host draws its own screen-space cursor instead.
fn is_gui_cursor(info: &GuiComponentRenderInfo) -> bool {
    matches!(
        info,
        GuiComponentRenderInfo::Image { texture, .. } if texture.eq_ignore_ascii_case("cursor.pcx")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec2;

    #[test]
    fn panel_anchors_at_the_original_left_mfd_slot() {
        // Keypad panel: 188x296 at (2, 124) - the original left MFD rect.
        let rect = panel_canvas_rect(vec2(188.0, 296.0));
        assert_eq!(rect, Rect::new(2.0, 124.0, 188.0, 296.0));
        // It fits on the 640x480 canvas.
        assert!(rect.y + rect.h <= CANVAS_SIZE.y);
    }

    #[test]
    fn image_components_map_into_the_panel_rect() {
        // The keypad's digit "1" button: panel-local (15, 42, 45x60) is
        // emitted normalized by 188x296; it must land at the anchor + the
        // same panel-local pixels.
        let panel = panel_canvas_rect(vec2(188.0, 296.0));
        let info = GuiComponentRenderInfo::Image {
            position: vec2(15.0 / 188.0, 42.0 / 296.0),
            size: vec2(45.0 / 188.0, 60.0 / 296.0),
            texture: "key10.pcx".to_owned(),
            alpha: 0.5,
            interactive: true,
            entity: None,
            label: None,
            panel_size_px: vec2(188.0, 296.0),
            kind: crate::ui::ImageKind::Ui,
        };
        let r = info.canvas_rect(panel);
        assert!((r.x - 17.0).abs() < 1e-3);
        assert!((r.y - 166.0).abs() < 1e-3);
        assert!((r.w - 45.0).abs() < 1e-3);
        assert!((r.h - 60.0).abs() < 1e-3);
    }

    #[test]
    fn text_components_undo_the_negated_y_convention() {
        // to_render_info negates text y for the VR quad; canvas mapping must
        // put a panel-local y=20 text at panel top + 20, not above the panel.
        let panel = panel_canvas_rect(vec2(188.0, 296.0));
        let info = GuiComponentRenderInfo::Text {
            position: vec2(10.0 / 188.0, -20.0 / 296.0),
            size: vec2(30.0 / 188.0, 16.0 / 296.0),
            font: "mainfont.fon".to_owned(),
            text: "451".to_owned(),
            alpha: 1.0,
        };
        let r = info.canvas_rect(panel);
        assert!((r.y - (124.0 + 20.0)).abs() < 1e-3);
    }

    /// The automap panel is the wide one (MAPBACK 636x296, both MFD slots):
    /// at the left-MFD anchor it must still fit the 640x480 canvas.
    #[test]
    fn wide_map_panel_fits_the_canvas() {
        let rect = panel_canvas_rect(vec2(636.0, 296.0));
        assert!(rect.x + rect.w <= CANVAS_SIZE.x);
        assert!(rect.y + rect.h <= CANVAS_SIZE.y);
    }

    /// An unbound (sticky) panel - the automap - must NOT walk-away close,
    /// even when its entity has a position far from the player; a regular
    /// open with the same geometry does.
    #[test]
    fn unbound_panel_skips_the_walk_away_close() {
        let mut world = World::new();
        let player_entity = world.add_entity(());
        let inventory = world.add_entity(());
        // Panel entity parked at the origin; player 100 units away.
        let panel = world.add_entity(dark::properties::PropPosition {
            position: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion {
                v: cgmath::vec3(0.0, 0.0, 0.0),
                s: 1.0,
            },
            cell: 0,
        });
        world.add_unique(crate::mission::PlayerInfo {
            rotation: cgmath::Quaternion {
                v: cgmath::vec3(0.0, 0.0, 0.0),
                s: 1.0,
            },
            pos: cgmath::vec3(100.0, 0.0, 100.0),
            entity_id: player_entity,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });

        // Regular open: the distance check closes it.
        let mut host = FlatUiHost::new();
        host.open(panel);
        host.update(&world, None);
        assert!(
            host.active_panel().is_none(),
            "a bound panel far from the player must auto-close"
        );

        // Unbound open: it survives.
        host.open_unbound(panel);
        host.update(&world, None);
        assert_eq!(
            host.active_panel(),
            Some(panel),
            "an unbound (map) panel must not walk-away close"
        );

        // And a later regular open clears the stickiness.
        host.open(panel);
        host.update(&world, None);
        assert!(host.active_panel().is_none());
    }

    #[test]
    fn close_button_hugs_the_top_right_corner() {
        // Matches the original keypad overlay's CloseOff rect (163, 8, 20x21)
        // for the 188-wide panel.
        let panel = panel_canvas_rect(vec2(188.0, 296.0));
        let r = close_button_canvas_rect(panel);
        assert_eq!(r, Rect::new(2.0 + 163.0, 124.0 + 8.0, 20.0, 21.0));
    }

    #[test]
    fn gui_cursor_component_is_recognized() {
        let cursor = GuiComponentRenderInfo::Image {
            position: vec2(0.1, 0.1),
            size: vec2(0.05, 0.05),
            texture: "cursor.pcx".to_owned(),
            alpha: 0.5,
            interactive: false,
            entity: None,
            label: None,
            panel_size_px: vec2(188.0, 296.0),
            kind: crate::ui::ImageKind::Ui,
        };
        assert!(is_gui_cursor(&cursor));
        let backdrop = GuiComponentRenderInfo::Image {
            position: vec2(0.0, 0.0),
            size: vec2(1.0, 1.0),
            texture: "keypad2.pcx".to_owned(),
            alpha: 0.5,
            interactive: false,
            entity: None,
            label: None,
            panel_size_px: vec2(188.0, 296.0),
            kind: crate::ui::ImageKind::Ui,
        };
        assert!(!is_gui_cursor(&backdrop));
    }

    #[test]
    fn held_button_at_open_does_not_close_the_panel() {
        // The (shift+)LMB frob that opened the panel is typically still held
        // on the next frame - it must be swallowed, not treated as a fresh
        // bare-view click that instantly closes the panel.
        let mut world = World::new();
        let panel = world.add_entity(());
        let mut host = FlatUiHost::new();
        host.open(panel);
        host.on_set_ui(
            &world,
            panel,
            vec2(188.0, 296.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[],
        );
        let bare_view_pressed = Pointer2D {
            position: vec2(0.9, 0.9),
            pressed: true,
        };
        host.update(&world, Some(bare_view_pressed));
        assert!(
            host.active_panel().is_some(),
            "a button held since before open must not close the panel"
        );
        // After a release, a fresh bare-view click DOES close it.
        host.update(
            &world,
            Some(Pointer2D {
                position: vec2(0.9, 0.9),
                pressed: false,
            }),
        );
        host.update(&world, Some(bare_view_pressed));
        assert!(
            host.active_panel().is_none(),
            "a fresh click on the bare 3D view should close the panel"
        );
    }

    #[test]
    fn entity_bound_elements_label_with_the_item_name_and_id() {
        // A loot-panel item button carries its entity; /v1/ui must label it
        // with the item's symbolic name ("Psi Amp") and its entity id so
        // tests click loot by meaning.
        let mut world = World::new();
        let item = world.add_entity(dark::properties::PropSymName("Psi Amp".to_owned()));
        let panel = world.add_entity(());
        let mut host = FlatUiHost::new();
        host.open(panel);
        host.on_set_ui(
            &world,
            panel,
            vec2(188.0, 296.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[GuiComponentRenderInfo::Image {
                position: vec2(15.0 / 188.0, 160.0 / 296.0),
                size: vec2(35.0 / 188.0, 32.0 / 296.0),
                texture: "icn_psi.pcx".to_owned(),
                alpha: 0.5,
                interactive: true,
                entity: Some(item),
                label: None,
                panel_size_px: vec2(188.0, 296.0),
                kind: crate::ui::ImageKind::Ui,
            }],
        );
        let elements = host.debug_elements(&world);
        let el = elements
            .iter()
            .find(|e| e.entity_id == Some(item.inner() as i32))
            .expect("the item element should carry its entity id");
        assert_eq!(el.kind, "button");
        assert_eq!(el.label.as_deref(), Some("Psi Amp"));
    }

    #[test]
    fn strip_docks_at_the_top_of_the_canvas() {
        // The inventory strip: 635x120 (invback) at the original game's
        // inv_rect anchor (2, 0) - flush with the canvas top.
        let rect = strip_canvas_rect(vec2(635.0, 120.0));
        assert_eq!(rect, Rect::new(2.0, 0.0, 635.0, 120.0));
        // It fits on the canvas and clears the left-MFD slot below (y 124+).
        assert!(rect.x + rect.w <= CANVAS_SIZE.x);
        assert!(rect.y + rect.h < LEFT_MFD_ANCHOR.y);
    }

    /// Hover routing with both slots live: the strip gets the pointer when
    /// it is over the top dock, the panel when it is over the MFD slot.
    #[test]
    fn hover_routes_to_strip_or_panel_by_position() {
        let mut world = World::new();
        let inventory = world.add_entity(());
        let panel = world.add_entity(());
        let mut host = FlatUiHost::new();
        host.set_strip(Some(inventory));
        host.open(panel);
        host.on_set_ui(
            &world,
            inventory,
            vec2(635.0, 120.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[],
        );
        host.on_set_ui(
            &world,
            panel,
            vec2(188.0, 296.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[],
        );

        // Canvas (320, 60) - the middle of the strip - is normalized
        // (0.5, 0.125) at the default 640x480 screen size.
        let over_strip = Pointer2D {
            position: vec2(0.5, 0.125),
            pressed: false,
        };
        let (msgs, _) = host.update(&world, Some(over_strip));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].to, inventory, "top dock hover goes to the strip");

        // Canvas (96, 272) - the middle of the left MFD - is (0.15, ~0.567).
        let over_panel = Pointer2D {
            position: vec2(96.0 / 640.0, 272.0 / 480.0),
            pressed: false,
        };
        let (msgs, _) = host.update(&world, Some(over_panel));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].to, panel, "MFD slot hover goes to the panel");
    }

    /// A bare-view click in use mode closes the MFD panel (manual p.7) but
    /// never the strip - use mode is left by Tab.
    #[test]
    fn bare_view_click_closes_the_panel_but_keeps_the_strip() {
        let mut world = World::new();
        let inventory = world.add_entity(());
        let panel = world.add_entity(());
        let mut host = FlatUiHost::new();
        host.set_strip(Some(inventory));
        host.open(panel);
        host.on_set_ui(
            &world,
            inventory,
            vec2(635.0, 120.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[],
        );
        host.on_set_ui(
            &world,
            panel,
            vec2(188.0, 296.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[],
        );
        // Release first (open() swallows the held button), then click on the
        // bare view (bottom-right of the canvas, outside both slots).
        let bare = vec2(0.9, 0.9);
        host.update(
            &world,
            Some(Pointer2D {
                position: bare,
                pressed: false,
            }),
        );
        host.update(
            &world,
            Some(Pointer2D {
                position: bare,
                pressed: true,
            }),
        );
        assert!(
            host.active_panel().is_none(),
            "bare-view click closes the panel"
        );
        assert_eq!(
            host.strip_entity(),
            Some(inventory),
            "the strip survives a bare-view click"
        );

        // With no panel open, another bare-view click is a no-op.
        host.update(
            &world,
            Some(Pointer2D {
                position: bare,
                pressed: false,
            }),
        );
        host.update(
            &world,
            Some(Pointer2D {
                position: bare,
                pressed: true,
            }),
        );
        assert_eq!(host.strip_entity(), Some(inventory));
    }

    /// The strip's /v1/ui elements carry the carried items' names + ids -
    /// the same contract as panel elements - and leaving use mode drops them.
    #[test]
    fn strip_elements_label_carried_items() {
        let mut world = World::new();
        let wrench = world.add_entity(dark::properties::PropSymName("Wrench".to_owned()));
        let inventory = world.add_entity(());
        let mut host = FlatUiHost::new();
        host.set_strip(Some(inventory));
        host.on_set_ui(
            &world,
            inventory,
            vec2(635.0, 120.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[
                GuiComponentRenderInfo::Image {
                    position: vec2(0.0, 0.0),
                    size: vec2(1.0, 1.0),
                    texture: "invback.pcx".to_owned(),
                    alpha: 0.5,
                    interactive: false,
                    entity: None,
                    label: None,
                    panel_size_px: vec2(188.0, 296.0),
                    kind: crate::ui::ImageKind::Ui,
                },
                GuiComponentRenderInfo::Image {
                    position: vec2(4.0 / 635.0, 17.0 / 120.0),
                    size: vec2(35.0 / 635.0, 34.0 / 120.0),
                    texture: "icn_wrench.pcx".to_owned(),
                    alpha: 0.5,
                    interactive: true,
                    entity: Some(wrench),
                    label: None,
                    panel_size_px: vec2(188.0, 296.0),
                    kind: crate::ui::ImageKind::Ui,
                },
            ],
        );
        let elements = host.strip_debug_elements(&world);
        // The backdrop is anchored at the canvas top.
        let backdrop = elements
            .iter()
            .find(|e| e.texture.as_deref() == Some("invback.pcx"))
            .expect("strip should expose the INVBACK backdrop");
        assert_eq!(backdrop.rect[1], 0.0, "the strip is top-docked");
        let item = elements
            .iter()
            .find(|e| e.entity_id == Some(wrench.inner() as i32))
            .expect("the carried item should be a strip element");
        assert_eq!(item.kind, "button");
        assert_eq!(item.label.as_deref(), Some("Wrench"));
        // No close element - the strip is closed by Tab, not a click.
        assert!(elements.iter().all(|e| e.label.as_deref() != Some("close")));

        // Leaving use mode drops the strip and its elements.
        host.set_strip(None);
        assert!(host.strip_entity().is_none());
        assert!(host.strip_debug_elements(&world).is_empty());
    }

    // --- Cursor-is-the-item drag (§1.5/§2.4) ---

    /// Normalized pointer position that lands on canvas `(cx, cy)` at the
    /// default 640x480 screen size (no letterbox, so canvas = norm * size).
    fn norm(cx: f32, cy: f32) -> cgmath::Vector2<f32> {
        vec2(cx / 640.0, cy / 480.0)
    }

    fn strip_item(entity: EntityId, slot_x: usize) -> GuiComponentRenderInfo {
        // ContainerGui strip slots: BACKPACK_GRID_ORIGIN (4,17) stepping by
        // SLOT_PITCH 35x34 (container.rs inv_container), emitted normalized
        // by the 635x120 strip.
        let x = 4.0 + 35.0 * slot_x as f32;
        GuiComponentRenderInfo::Image {
            position: vec2(x / 635.0, 17.0 / 120.0),
            size: vec2(35.0 / 635.0, 34.0 / 120.0),
            texture: "icn_x.pcx".to_owned(),
            alpha: 0.5,
            interactive: true,
            entity: Some(entity),
            label: None,
            panel_size_px: vec2(188.0, 296.0),
            kind: crate::ui::ImageKind::Ui,
        }
    }

    /// [`press_edge`], keeping the messages the press emitted as well.
    fn press_edge_with_messages(
        host: &mut FlatUiHost,
        world: &World,
        canvas: (f32, f32),
    ) -> (Vec<Message>, Vec<FlatUiDragAction>) {
        host.update(
            world,
            Some(Pointer2D {
                position: norm(canvas.0, canvas.1),
                pressed: false,
            }),
        );
        host.update(
            world,
            Some(Pointer2D {
                position: norm(canvas.0, canvas.1),
                pressed: true,
            }),
        )
    }

    fn press_edge(
        host: &mut FlatUiHost,
        world: &World,
        canvas: (f32, f32),
    ) -> Vec<FlatUiDragAction> {
        // An unpressed frame first so the press is a rising edge.
        host.update(
            world,
            Some(Pointer2D {
                position: norm(canvas.0, canvas.1),
                pressed: false,
            }),
        );
        let (_msgs, actions) = host.update(
            world,
            Some(Pointer2D {
                position: norm(canvas.0, canvas.1),
                pressed: true,
            }),
        );
        actions
    }

    /// A world+host with the strip showing `Wrench` in slot 0 (icon + name so
    /// the cursor can render/label it).
    fn drag_world() -> (World, FlatUiHost, EntityId, EntityId) {
        let mut world = World::new();
        let wrench = world.add_entity((
            dark::properties::PropObjIcon("icn_wrench".to_owned()),
            dark::properties::PropSymName("Wrench".to_owned()),
        ));
        let inventory = world.add_entity(());
        let mut host = FlatUiHost::new();
        host.set_strip(Some(inventory));
        host.on_set_ui(
            &world,
            inventory,
            vec2(635.0, 120.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[strip_item(wrench, 0)],
        );
        (world, host, wrench, inventory)
    }

    #[test]
    fn lmb_on_a_strip_item_lifts_it_onto_the_cursor() {
        let (world, mut host, wrench, _inv) = drag_world();
        // Slot 0 center on the canvas: (2 + 4 + 17.5, 18 + 16) = (23.5, 34).
        // Lifting is host-internal (the item stays in the backpack) - no
        // world action is emitted.
        let actions = press_edge(&mut host, &world, (23.5, 34.0));
        assert!(actions.is_empty(), "lifting emits no world action");
        let cursor = host.cursor_debug().expect("the item is on the cursor");
        assert_eq!(cursor.entity_id, wrench.inner() as i32);
        assert_eq!(cursor.label.as_deref(), Some("Wrench"));
        // The lifted item is hidden from the strip grid and its hit-testing.
        assert!(
            host.strip_debug_elements(&world)
                .iter()
                .all(|e| e.entity_id != Some(wrench.inner() as i32)),
            "the lifted item leaves the strip grid",
        );
        assert_eq!(host.strip_item_at(vec2(23.5, 34.0)), None);
    }

    /// An always-collected item that an older save left in the backpack is the
    /// one case where the grid holds one at all - and, before this, the one
    /// place it could never be collected: the host owns strip clicks as the
    /// cursor drag, so the click never reached `ContainerGui`'s frob. Clicking
    /// one collects it instead of lifting it onto the cursor.
    #[test]
    fn clicking_an_always_collected_strip_item_collects_it() {
        use crate::test_support::{CollectedKind, spawn_collected, spawn_ordinary_loot};

        for kind in CollectedKind::ALL {
            let mut world = World::new();
            let pickup = spawn_collected(&mut world, kind);
            let inventory = world.add_entity(());
            let mut host = FlatUiHost::new();
            host.set_strip(Some(inventory));
            host.on_set_ui(
                &world,
                inventory,
                vec2(635.0, 120.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
                &[strip_item(pickup, 0)],
            );

            // Slot 0 center, as in `lmb_on_a_strip_item_lifts_it_onto_the_cursor`.
            host.update(
                &world,
                Some(Pointer2D {
                    position: norm(23.5, 34.0),
                    pressed: false,
                }),
            );
            let (msgs, actions) = host.update(
                &world,
                Some(Pointer2D {
                    position: norm(23.5, 34.0),
                    pressed: true,
                }),
            );

            assert!(
                msgs.iter()
                    .any(|msg| msg.to == pickup && matches!(msg.payload, MessagePayload::Frob)),
                "{kind:?} clicked in the strip must be frobbed, got {msgs:?}"
            );
            assert!(
                actions.is_empty(),
                "{kind:?} must not throw or wield, got {actions:?}"
            );
            assert!(
                host.cursor_debug().is_none(),
                "{kind:?} must never ride the cursor"
            );
        }

        // The control: ordinary loot still lifts onto the cursor.
        let mut world = World::new();
        let loot = spawn_ordinary_loot(&mut world);
        let inventory = world.add_entity(());
        let mut host = FlatUiHost::new();
        host.set_strip(Some(inventory));
        host.on_set_ui(
            &world,
            inventory,
            vec2(635.0, 120.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[strip_item(loot, 0)],
        );
        press_edge(&mut host, &world, (23.5, 34.0));
        assert!(
            host.cursor_debug().is_some(),
            "ordinary loot must still lift onto the cursor"
        );
    }

    /// The swap is the same gesture family as the click above: dropping a held
    /// item onto an always-collected occupant must collect that occupant, not
    /// lift it onto the cursor - otherwise the swap is a back door into
    /// carrying one.
    #[test]
    fn swapping_onto_an_always_collected_strip_item_collects_it() {
        use crate::test_support::{CollectedKind, spawn_collected, spawn_ordinary_loot};

        let mut world = World::new();
        let held = spawn_ordinary_loot(&mut world);
        let pickup = spawn_collected(&mut world, CollectedKind::KeyCard);
        let inventory = world.add_entity(());
        let mut host = FlatUiHost::new();
        host.set_strip(Some(inventory));
        host.on_set_ui(
            &world,
            inventory,
            vec2(635.0, 120.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[strip_item(held, 0), strip_item(pickup, 1)],
        );

        // Lift the ordinary item out of slot 0, then press on slot 1's card.
        press_edge(&mut host, &world, (23.5, 34.0));
        assert!(host.cursor_debug().is_some(), "slot 0 lifts as usual");
        let (msgs, actions) = press_edge_with_messages(&mut host, &world, (58.5, 34.0));

        assert!(
            msgs.iter()
                .any(|msg| msg.to == pickup && matches!(msg.payload, MessagePayload::Frob)),
            "the occupant must be collected, got {msgs:?}"
        );
        assert!(actions.is_empty(), "collecting is not a world action");
        assert!(
            host.cursor_debug().is_none(),
            "neither item may stay on the cursor"
        );
    }

    #[test]
    fn destroying_a_cursor_item_clears_host_references_immediately() {
        let (world, mut host, wrench, _inv) = drag_world();
        press_edge(&mut host, &world, (23.5, 34.0));
        assert!(host.cursor_debug().is_some());
        assert!(host.last_lift.is_some());

        host.on_entity_destroyed(wrench);

        assert!(host.cursor_debug().is_none());
        assert!(host.last_lift.is_none());
    }

    #[test]
    fn destroying_the_active_panel_closes_it_immediately() {
        let mut world = World::new();
        let panel = world.add_entity(());
        let mut host = FlatUiHost::new();
        host.open(panel);

        host.on_entity_destroyed(panel);

        assert!(host.active_panel().is_none());
    }

    #[test]
    fn destroying_the_strip_container_clears_its_binding() {
        let mut world = World::new();
        let inventory = world.add_entity(());
        let mut host = FlatUiHost::new();
        host.set_strip(Some(inventory));

        host.on_entity_destroyed(inventory);

        assert!(host.strip_entity().is_none());
    }

    #[test]
    fn destroying_an_item_prunes_cached_panel_and_strip_components() {
        let (mut world, mut host, wrench, _inventory) = drag_world();
        let panel = world.add_entity(());
        host.open(panel);
        host.on_set_ui(
            &world,
            panel,
            vec2(188.0, 296.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[strip_item(wrench, 0)],
        );
        let wrench_id = wrench.inner() as i32;
        assert!(
            host.debug_elements(&world)
                .iter()
                .any(|element| element.entity_id == Some(wrench_id))
        );
        assert!(
            host.strip_debug_elements(&world)
                .iter()
                .any(|element| element.entity_id == Some(wrench_id))
        );

        host.on_entity_destroyed(wrench);

        assert!(
            host.debug_elements(&world)
                .iter()
                .all(|element| element.entity_id != Some(wrench_id))
        );
        assert!(
            host.strip_debug_elements(&world)
                .iter()
                .all(|element| element.entity_id != Some(wrench_id))
        );
    }

    #[test]
    fn stale_set_ui_cannot_restore_an_item_destroyed_earlier_in_the_batch() {
        let (mut world, mut host, wrench, inventory) = drag_world();
        world.delete_entity(wrench);
        host.on_entity_destroyed(wrench);

        // This snapshot was produced before DestroyEntity, but is applied
        // afterwards in the same effect batch.
        host.on_set_ui(
            &world,
            inventory,
            vec2(635.0, 120.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[strip_item(wrench, 0)],
        );

        assert!(host.strip_debug_elements(&world).is_empty());
    }

    #[test]
    fn placing_over_the_strip_returns_the_item_and_clears_the_cursor() {
        let (world, mut host, wrench, _inv) = drag_world();
        press_edge(&mut host, &world, (23.5, 34.0)); // lift
        // An empty strip cell (far from slot 0) places it back: cursor clears,
        // no world action (the item never left the backpack).
        let actions = press_edge(&mut host, &world, (400.0, 60.0));
        assert!(actions.is_empty(), "placing emits no world action");
        assert!(host.cursor_debug().is_none(), "placing clears the cursor");
        // The item is visible in the strip again.
        assert_eq!(host.strip_item_at(vec2(23.5, 34.0)), Some(wrench));
    }

    #[test]
    fn clicking_the_bare_view_while_holding_throws_the_item() {
        let (world, mut host, wrench, _inv) = drag_world();
        press_edge(&mut host, &world, (23.5, 34.0)); // lift
        // Below the strip (y > 121), no panel open: the bare 3D view.
        let actions = press_edge(&mut host, &world, (320.0, 300.0));
        assert_eq!(actions, vec![FlatUiDragAction::Throw(wrench)]);
        assert!(host.cursor_debug().is_none(), "throwing clears the cursor");
    }

    #[test]
    fn dropping_onto_an_occupied_slot_swaps() {
        let (mut world, mut host, wrench, inventory) = drag_world();
        let pistol = world.add_entity((
            dark::properties::PropObjIcon("icn_pist".to_owned()),
            dark::properties::PropSymName("Pistol".to_owned()),
        ));
        host.on_set_ui(
            &world,
            inventory,
            vec2(635.0, 120.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[strip_item(wrench, 0), strip_item(pistol, 1)],
        );
        press_edge(&mut host, &world, (23.5, 34.0)); // lift the Wrench
        // Drop onto slot 1 (Pistol): center (2 + 4 + 35 + 17.5, 34) = (58.5, 34).
        // Swapping is host-internal - the Pistol lifts onto the cursor, the
        // Wrench returns to the grid; no world action.
        let actions = press_edge(&mut host, &world, (58.5, 34.0));
        assert!(actions.is_empty(), "swapping emits no world action");
        let cursor = host
            .cursor_debug()
            .expect("the swapped-in Pistol is on the cursor");
        assert_eq!(cursor.entity_id, pistol.inner() as i32);
        // The Pistol is now hidden and the Wrench visible again.
        assert_eq!(host.strip_item_at(vec2(23.5, 34.0)), Some(wrench));
        assert_eq!(host.strip_item_at(vec2(58.5, 34.0)), None);
    }

    #[test]
    fn clicking_the_ammo_cycle_button_emits_cycle_ammo() {
        let (world, mut host, _wrench, _inv) = drag_world();
        // The AMMOFULL cycle button lives at canvas (564,429,12,41).
        host.set_ammo_cycle_button(Some(Rect::new(564.0, 429.0, 12.0, 41.0)));
        // A click on its center (570, 449) cycles the ammo.
        let actions = press_edge(&mut host, &world, (570.0, 449.0));
        assert_eq!(actions, vec![FlatUiDragAction::CycleAmmo]);
        // The /v1/ui element is exposed with a clickable label.
        let el = host.ammo_cycle_debug().expect("the button is exposed");
        assert_eq!(el.label.as_deref(), Some("cycle_ammo"));
        assert_eq!(el.kind, "button");
        // A click elsewhere in the bare view does not cycle.
        let actions = press_edge(&mut host, &world, (300.0, 300.0));
        assert!(actions.is_empty());
        // Cleared when not shown.
        host.set_ammo_cycle_button(None);
        assert!(host.ammo_cycle_debug().is_none());
    }

    #[test]
    fn clicking_the_ammo_button_while_holding_keeps_the_item() {
        // A visible button must not throw the item you're carrying: clicking
        // the ammo-cycle button mid-drag protects the held item (no throw, no
        // cycle) rather than treating it as a bare-view throw.
        let (world, mut host, _wrench, _inv) = drag_world();
        host.set_ammo_cycle_button(Some(Rect::new(564.0, 429.0, 12.0, 41.0)));
        press_edge(&mut host, &world, (23.5, 34.0)); // lift the Wrench
        assert!(host.cursor_debug().is_some());
        let actions = press_edge(&mut host, &world, (570.0, 449.0)); // click the ammo button
        assert!(actions.is_empty(), "the click neither throws nor cycles");
        assert!(host.cursor_debug().is_some(), "the held item is protected");
    }

    #[test]
    fn double_click_on_a_strip_item_wields_it() {
        let (world, mut host, wrench, _inv) = drag_world();
        // First click lifts onto the cursor (no world action)...
        let first = press_edge(&mut host, &world, (23.5, 34.0));
        assert!(first.is_empty());
        assert!(host.cursor_debug().is_some());
        // ...a quick second click on the same slot wields it (double-click).
        let second = press_edge(&mut host, &world, (23.5, 34.0));
        assert_eq!(second, vec![FlatUiDragAction::Wield(wrench)]);
        assert!(host.cursor_debug().is_none(), "wielding clears the cursor");
    }

    #[test]
    fn clicking_a_panel_while_holding_keeps_the_item() {
        let (mut world, mut host, wrench, _inv) = drag_world();
        let panel = world.add_entity(());
        host.open(panel);
        host.on_set_ui(
            &world,
            panel,
            vec2(188.0, 296.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
            &[],
        );
        press_edge(&mut host, &world, (23.5, 34.0)); // lift the Wrench
        assert!(host.cursor_debug().is_some());
        // Click inside the left-MFD panel (canvas ~ (96, 272)): the held item
        // is protected (escape hatch), no drag action, cursor unchanged.
        let actions = press_edge(&mut host, &world, (96.0, 272.0));
        assert!(
            actions.is_empty(),
            "a panel click while holding does nothing"
        );
        assert_eq!(
            host.cursor_debug().map(|c| c.entity_id),
            Some(wrench.inner() as i32),
            "the item stays on the cursor",
        );
        assert_eq!(host.active_panel(), Some(panel), "the panel stays open");
    }

    #[test]
    fn semantic_labels_identify_keypad_digits() {
        // Both art states of a digit button label as the digit.
        assert_eq!(semantic_label("key40.pcx"), Some("4".to_string()));
        assert_eq!(semantic_label("key41.pcx"), Some("4".to_string()));
        assert_eq!(semantic_label("key00.pcx"), Some("0".to_string()));
        // The clear key (keyn0/keyn1).
        assert_eq!(semantic_label("keyn1.pcx"), Some("clear".to_string()));
        // Non-widget art with a "key" prefix must NOT label.
        assert_eq!(semantic_label("keypad2.pcx"), None);
        assert_eq!(semantic_label("crosshai.pcx"), None);
    }

    // --- The VR cyber interface's pointer bridge (slice 3) ---
    //
    // These drive the REAL pointer pass (`vr_frontend_pointer_pass`) against
    // the REAL anchored panel (`test_support::test_panel`), then hand the
    // result to the host through `vr_canvas_pointer` - the same three calls
    // `mission_core` makes each frame. So they cover the whole bridge,
    // ray -> canvas -> gesture, rather than a hand-rolled approximation of it.

    use crate::input_context::{Hand, InputContext};
    use crate::ui::{test_support, vr_frontend_pointer_pass};

    /// One VR frame: the given hand aimed at a canvas point (or away), with a
    /// trigger and squeeze, resolved exactly as the mission does.
    fn vr_pointer(
        hand: Handedness,
        aim: Option<(f32, f32)>,
        trigger: f32,
        squeeze: f32,
    ) -> CanvasPointer {
        let posed = match aim {
            Some((x, y)) => test_support::hand_aimed_at(CANVAS_SIZE, vec2(x, y), trigger),
            None => test_support::hand_aimed_away(trigger),
        };
        let posed = Hand {
            squeeze_value: squeeze,
            ..posed
        };
        // The other controller is untracked, so it can neither steal the pass
        // nor contribute a stray ray (the zero-quaternion guard).
        let idle = Hand {
            rotation: cgmath::Quaternion {
                v: cgmath::vec3(0.0, 0.0, 0.0),
                s: 0.0,
            },
            ..Hand::default()
        };
        let (right, left) = match hand {
            Handedness::Right => (posed, idle),
            Handedness::Left => (idle, posed),
        };
        let input = InputContext {
            right_hand: right,
            left_hand: left,
            ..InputContext::default()
        };
        let pass = vr_frontend_pointer_pass(&input, CANVAS_SIZE, &test_support::test_panel());
        vr_canvas_pointer(&pass, &input)
    }

    /// The bridge's core claim: a controller aimed at an inventory slot lands
    /// on that slot's canvas pixels, and a trigger pull there lifts the item
    /// onto the cursor - the same drag the flat mouse drives.
    #[test]
    fn a_controller_aimed_at_a_slot_lifts_its_item() {
        let (world, mut host, wrench, _inv) = drag_world();
        // Slot 0's center on the shared canvas (as the flat drag tests use).
        let slot = (23.5, 34.0);

        let idle = vr_pointer(Handedness::Right, Some(slot), 0.0, 0.0);
        let landed = idle.canvas_pos.expect("the ray should land on the panel");
        assert!(
            (landed.x - slot.0).abs() < 1.0 && (landed.y - slot.1).abs() < 1.0,
            "the ray must land on the slot it was aimed at, got {:?}",
            landed
        );
        host.update_canvas(&world, Some(idle));
        assert!(host.cursor_debug().is_none(), "hovering must not lift");

        host.update_canvas(
            &world,
            Some(vr_pointer(Handedness::Right, Some(slot), 1.0, 0.0)),
        );
        let cursor = host
            .cursor_debug()
            .expect("the trigger should lift the item");
        assert_eq!(cursor.entity_id, wrench.inner() as i32);
    }

    /// Rule 6 of the vr-ui-design skill: a trigger already held as the
    /// interface opens must not read as a click on whatever the ray first
    /// crosses. Without `guard_held_press` the very first frame lifts an item.
    #[test]
    fn a_trigger_held_when_the_interface_opens_does_not_lift() {
        let (world, mut host, _wrench, _inv) = drag_world();
        let slot = (23.5, 34.0);
        host.guard_held_press();

        host.update_canvas(
            &world,
            Some(vr_pointer(Handedness::Right, Some(slot), 1.0, 0.0)),
        );
        assert!(
            host.cursor_debug().is_none(),
            "a carried-over press must not lift an item"
        );

        // Releasing and pulling again is a real click.
        host.update_canvas(
            &world,
            Some(vr_pointer(Handedness::Right, Some(slot), 0.0, 0.0)),
        );
        host.update_canvas(
            &world,
            Some(vr_pointer(Handedness::Right, Some(slot), 1.0, 0.0)),
        );
        assert!(
            host.cursor_debug().is_some(),
            "a fresh pull after release must lift"
        );
    }

    /// A press that starts off-panel and sweeps onto a slot while still held
    /// is not a fresh edge: the pass reports the trigger even with nothing on
    /// the panel, so the host has already swallowed it.
    #[test]
    fn a_press_swept_on_from_off_panel_does_not_lift() {
        let (world, mut host, _wrench, _inv) = drag_world();
        host.update_canvas(&world, Some(vr_pointer(Handedness::Right, None, 1.0, 0.0)));
        host.update_canvas(
            &world,
            Some(vr_pointer(Handedness::Right, Some((23.5, 34.0)), 1.0, 0.0)),
        );
        assert!(
            host.cursor_debug().is_none(),
            "a press carried in from off-panel must not lift an item"
        );
    }

    /// An untracked controller (the zero quaternion) reports no point at all,
    /// so it can neither hover nor click from a hand nobody is holding.
    #[test]
    fn an_untracked_controller_never_points_at_the_interface() {
        let (world, mut host, _wrench, _inv) = drag_world();
        let untracked = InputContext::default();
        let pass = vr_frontend_pointer_pass(&untracked, CANVAS_SIZE, &test_support::test_panel());
        let pointer = vr_canvas_pointer(&pass, &untracked);
        assert_eq!(pointer.canvas_pos, None);
        host.update_canvas(&world, Some(pointer));
        assert!(host.cursor_debug().is_none());
    }

    /// In VR the empty pixels of the panel are still the interface, not the 3D
    /// view: a press there must not throw the item riding the cursor. On flat
    /// the same press IS the bare view and throws it (the contrast is the
    /// point - one `BareViewPress` decides it).
    #[test]
    fn an_off_widget_press_throws_on_flat_but_not_in_vr() {
        // Empty canvas space, below the top-docked strip and clear of the
        // (unopened) MFD slot.
        let empty = (400.0, 400.0);

        let (world, mut host, wrench, _inv) = drag_world();
        press_edge(&mut host, &world, (23.5, 34.0));
        assert!(host.cursor_debug().is_some(), "the item is on the cursor");
        let actions = press_edge(&mut host, &world, empty);
        assert_eq!(
            actions,
            vec![FlatUiDragAction::Throw(wrench)],
            "flat: a click on the bare 3D view throws the held item"
        );

        let (world, mut host, _wrench, _inv) = drag_world();
        press_edge(&mut host, &world, (23.5, 34.0));
        assert!(host.cursor_debug().is_some());
        host.update_canvas(
            &world,
            Some(vr_pointer(Handedness::Right, Some(empty), 0.0, 0.0)),
        );
        let (_msgs, actions) = host.update_canvas(
            &world,
            Some(vr_pointer(Handedness::Right, Some(empty), 1.0, 0.0)),
        );
        assert!(
            actions.is_empty() && host.cursor_debug().is_some(),
            "VR: empty panel space must keep the held item, not throw it"
        );
    }

    /// Grab-to-hand parity: a squeeze on a slot reaches the strip's GuiScript
    /// as a grab, addressed to the hand that is pointing - which is what makes
    /// the panel emit `GrabEntity` into that hand, exactly as a loot panel
    /// does. Without the squeeze/handedness passthrough the message says
    /// "not grabbing, right hand" and the item never leaves the grid.
    #[test]
    fn a_squeeze_on_a_slot_reaches_the_strip_as_a_left_hand_grab() {
        let (world, mut host, _wrench, inventory) = drag_world();
        let (msgs, _actions) = host.update_canvas(
            &world,
            Some(vr_pointer(Handedness::Left, Some((23.5, 34.0)), 0.0, 1.0)),
        );
        let hover = msgs.first().expect("hovering a slot dispatches GUIHover");
        assert_eq!(hover.to, inventory);
        match hover.payload {
            MessagePayload::GUIHover {
                is_grabbing,
                is_triggered,
                hand,
                ..
            } => {
                assert!(is_grabbing, "the squeeze must reach the panel as a grab");
                assert_eq!(hand, Handedness::Left, "grabs land in the pointing hand");
                assert!(
                    !is_triggered,
                    "the host owns strip clicks; the panel must not also see one"
                );
            }
            ref other => panic!("expected a GUIHover, got {:?}", other),
        }
    }

    /// Both controllers on the panel, only the LEFT squeezing: the squeeze is
    /// what claims the panel, so the grab must be reported for the left hand.
    /// Trigger-only arbitration would fall back to the idle right hand, read
    /// its (absent) squeeze, and take nothing - while the real left squeeze
    /// went to the world instead.
    #[test]
    fn a_squeezing_hand_wins_the_panel_over_an_idle_one() {
        let (world, mut host, _wrench, inventory) = drag_world();
        let slot = vec2(23.5, 34.0);
        let squeezing_left = Hand {
            squeeze_value: 1.0,
            ..test_support::hand_aimed_at(CANVAS_SIZE, slot, 0.0)
        };
        let idle_right = test_support::hand_aimed_at(CANVAS_SIZE, vec2(300.0, 60.0), 0.0);
        let input = InputContext {
            right_hand: idle_right,
            left_hand: squeezing_left,
            ..InputContext::default()
        };
        let pass = crate::ui::vr_pointer_pass(
            &input,
            CANVAS_SIZE,
            &test_support::test_panel(),
            crate::ui::PointerEngagement::TriggerOrGrab,
        );
        let pointer = vr_canvas_pointer(&pass, &input);
        assert_eq!(pointer.hand, Handedness::Left);
        assert!(pointer.grabbing);

        let (msgs, _) = host.update_canvas(&world, Some(pointer));
        let hover = msgs
            .first()
            .expect("the squeezing hand should hover a slot");
        assert_eq!(hover.to, inventory);
        match hover.payload {
            MessagePayload::GUIHover {
                is_grabbing, hand, ..
            } => {
                assert!(is_grabbing);
                assert_eq!(hand, Handedness::Left);
            }
            ref other => panic!("expected a GUIHover, got {:?}", other),
        }
    }

    /// The panel's grab handler is level-triggered, so a squeeze held while the
    /// ray sweeps the grid would take every slot it crossed. Only the rising
    /// edge reaches the panel.
    #[test]
    fn a_held_squeeze_grabs_once_however_far_the_ray_sweeps() {
        let (world, mut host, _wrench, _inv) = drag_world();
        let grabbing = |canvas: (f32, f32)| CanvasPointer {
            canvas_pos: Some(vec2(canvas.0, canvas.1)),
            pressed: false,
            grabbing: true,
            hand: Handedness::Right,
            bare_view: BareViewPress::Ignore,
        };
        let grabs = |msgs: Vec<Message>| {
            msgs.iter()
                .filter(|m| {
                    matches!(
                        m.payload,
                        MessagePayload::GUIHover {
                            is_grabbing: true,
                            ..
                        }
                    )
                })
                .count()
        };
        let (first, _) = host.update_canvas(&world, Some(grabbing((23.5, 34.0))));
        assert_eq!(grabs(first), 1, "the squeeze's rising edge grabs");
        let (held, _) = host.update_canvas(&world, Some(grabbing((23.5, 34.0))));
        assert_eq!(grabs(held), 0, "the same held squeeze must not grab again");
        let (swept, _) = host.update_canvas(&world, Some(grabbing((58.5, 34.0))));
        assert_eq!(
            grabs(swept),
            0,
            "sweeping a held squeeze onto another slot must not take it too"
        );
    }
}
