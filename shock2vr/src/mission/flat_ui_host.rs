//! Flat-mode MFD panel host (projects/flat-ui.md §5.2, PR 2).
//!
//! The flat presentation of the shared `Gui` layer: where VR shows every
//! panel as an always-on world quad (`GuiManager` + `ProxyGuiScript`), the
//! original flat game *opens* an object-bound MFD overlay on frob and drives
//! it with the mouse cursor. This host holds that flat-only state:
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
    ui::{HAlign, Rect, ScaleMode, UiCanvas, VAlign, pointer_to_canvas},
    vr_config::Handedness,
};

/// The shared 640x480 virtual canvas the flat HUD renders on.
const CANVAS_SIZE: Vector2<f32> = Vector2::new(640.0, 480.0);

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

/// Size of the item icon drawn when the cursor "is" a lifted item (the
/// original replaces the arrow with the object's `objicon` art). One
/// inventory slot (35x32, matching `ContainerGui`'s slot pixels).
const CURSOR_ITEM_SIZE: Vector2<f32> = Vector2::new(35.0, 32.0);

/// A host-side action produced by the cursor-is-the-item drag (§1.5/§2.4),
/// applied by `mission_core` because it touches physics/effects.
///
/// Lift/place/swap are *not* here: a lifted item **stays in the backpack
/// container** (the host just hides it from the strip while it rides the
/// cursor), so it is always reachable and serializes correctly on
/// save/transition. Only committing the drag reaches the world: **Throw**
/// detaches it and gives it world presence with an impulse along the view ray;
/// **Apply** offers it to an explicitly accepting crosshair target;
/// **Wield** equips/uses it (a double-click) via the same effect as a backpack
/// click, acting on the still-contained item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatUiDragAction {
    Throw(EntityId),
    Apply(EntityId),
    Wield(EntityId),
    /// Cycle the empty wielded weapon's ammo type (the AMMOFULL cycle button
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
    last_secondary_pointer_pressed: bool,
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
            last_secondary_pointer_pressed: false,
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

    /// The item currently held on the cursor mid-drag (for `/v1/ui` `cursor`).
    pub fn cursor_debug(&self) -> Option<crate::game_scene::DebugUiCursor> {
        self.cursor_item
            .as_ref()
            .map(|c| crate::game_scene::DebugUiCursor {
                entity_id: c.entity.inner() as i32,
                label: c.label.clone(),
            })
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
    /// cursor-drag [`FlatUiDragAction`]s (lift/place/apply/throw) for the caller to
    /// apply. Handles the close gestures (close button, LMB on the bare view,
    /// walk-away, entity gone) and the cursor-is-the-item drag (§1.5/§2.4):
    /// LMB on a strip item lifts it onto the cursor, LMB on another slot
    /// places/swaps, LMB on the bare view throws it into the world, and RMB
    /// on the bare view offers it to the crosshair target. A
    /// bare-view click closes the MFD panel but never the strip - use mode is
    /// left by Tab (projects/flat-ui.md §5.2).
    pub fn update(
        &mut self,
        world: &World,
        pointer: Option<Pointer2D>,
    ) -> (Vec<Message>, Vec<FlatUiDragAction>) {
        let pressed = pointer.map(|p| p.pressed).unwrap_or(false);
        let pressed_edge = pressed && !self.last_pointer_pressed;
        self.last_pointer_pressed = pressed;
        let secondary_pressed = pointer.map(|p| p.secondary_pressed).unwrap_or(false);
        let secondary_pressed_edge = secondary_pressed && !self.last_secondary_pointer_pressed;
        self.last_secondary_pointer_pressed = secondary_pressed;

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
        let canvas_pos = pointer_to_canvas(
            CANVAS_SIZE,
            pointer.position,
            self.screen_size,
            ScaleMode::PreserveAspect,
        );
        self.cursor_canvas = canvas_pos;
        if canvas_pos.is_none() {
            self.hover_close = false;
        }

        // A press fully outside the canvas (letterbox bars) is a bare-view
        // click: RMB applies a held item, LMB throws it (or closes the panel).
        let Some(canvas_pos) = canvas_pos else {
            if secondary_pressed_edge {
                if let Some(held) = self.cursor_item.as_ref() {
                    return (Vec::new(), vec![FlatUiDragAction::Apply(held.entity)]);
                }
            }
            if pressed_edge {
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
        // places/swaps/throws it. RMB over the bare view asks mission_core to
        // apply it to the crosshair target, but deliberately keeps it on the
        // cursor until the target actually consumes/destroys it. ---
        if self.cursor_item.is_some() {
            self.hover_close = false;
            if secondary_pressed_edge && !over_strip && !over_panel && !over_ammo {
                let held = self.cursor_item.as_ref().unwrap().entity;
                self.last_lift = None;
                return (Vec::new(), vec![FlatUiDragAction::Apply(held)]);
            }
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
            if over_panel || over_ammo {
                // Escape hatch: a click on an open MFD or the AMMOFULL cycle
                // button keeps the held item (a visible button must not throw
                // the item you're carrying).
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
            return (vec![gui_hover(entity, rect, canvas_pos, false)], Vec::new());
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

        // Host-drawn close button (the shared guis have no close component -
        // VR panels never close).
        let close_rect = close_button_canvas_rect(rect);
        self.hover_close = close_rect.contains(canvas_pos);
        if pressed_edge && self.hover_close {
            self.close();
            return (Vec::new(), Vec::new());
        }

        if rect.contains(canvas_pos) {
            (
                vec![gui_hover(panel, rect, canvas_pos, pointer.pressed)],
                Vec::new(),
            )
        } else {
            // LMB on the bare 3D view closes the panels (manual p.7).
            if pressed_edge {
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
            } if Some(*entity) != held && component_canvas_rect(c, rect).contains(canvas_pos) => {
                Some(*entity)
            }
            _ => None,
        })
    }

    /// Render the inventory strip + active panel + cursor as screen-space
    /// overlay objects on the shared 640x480 canvas (drawn after the flat
    /// HUD).
    pub fn render(
        &self,
        asset_cache: &mut AssetCache,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        let strip_rect = self.strip_rect();
        let panel_rect = self.panel_rect();
        if strip_rect.is_none() && panel_rect.is_none() {
            return Vec::new();
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
                Some(icon) => canvas.image(
                    Rect::new(cursor.x, cursor.y, CURSOR_ITEM_SIZE.x, CURSOR_ITEM_SIZE.y),
                    icon,
                ),
                None => canvas.image(
                    Rect::new(cursor.x, cursor.y, CURSOR_SIZE.x, CURSOR_SIZE.y),
                    "cursor.pcx",
                ),
            };
        }
        canvas.render_screen_space(asset_cache, screen_size, ScaleMode::PreserveAspect)
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
            let r = component_canvas_rect(component, rect);
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
/// runs unchanged. LMB maps to the right-hand trigger; flat has no grab
/// gesture yet.
fn gui_hover(to: EntityId, rect: Rect, canvas_pos: Vector2<f32>, pressed: bool) -> Message {
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
            is_grabbing: false,
            hand: Handedness::Right,
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
        let r = component_canvas_rect(component, rect);
        match component {
            // The original MFD art is opaque on screen; the render-info
            // alpha is a VR world-quad translucency, deliberately not
            // applied here.
            GuiComponentRenderInfo::Image { texture, .. } => {
                canvas.image(r, texture);
            }
            GuiComponentRenderInfo::Text { text, font, .. } => {
                // Render at the font's native pixel height (the Dark engine
                // draws its bitmap fonts 1:1), not the component's box height -
                // the `size` on a GUI text component is its bounding box, not a
                // font size. Vertically center the native-height text in that box.
                canvas.text_native(r, text, font, HAlign::Left, VAlign::Middle);
            }
        }
    }
}

/// Map one `SetUI` component (normalized panel coordinates) to canvas pixels.
fn component_canvas_rect(info: &GuiComponentRenderInfo, panel: Rect) -> Rect {
    let position = info.position();
    let size = info.size();
    // Text render-info positions carry a negated y (a VR world-quad
    // convention baked into `GuiComponent::to_render_info`); undo it here.
    let y = match info {
        GuiComponentRenderInfo::Text { .. } => -position.y,
        _ => position.y,
    };
    Rect::new(
        panel.x + position.x * panel.w,
        panel.y + y * panel.h,
        size.x * panel.w,
        size.y * panel.h,
    )
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
        };
        let r = component_canvas_rect(&info, panel);
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
        let r = component_canvas_rect(&info, panel);
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
            secondary_pressed: false,
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
                secondary_pressed: false,
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
            secondary_pressed: false,
        };
        let (msgs, _) = host.update(&world, Some(over_strip));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].to, inventory, "top dock hover goes to the strip");

        // Canvas (96, 272) - the middle of the left MFD - is (0.15, ~0.567).
        let over_panel = Pointer2D {
            position: vec2(96.0 / 640.0, 272.0 / 480.0),
            pressed: false,
            secondary_pressed: false,
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
                secondary_pressed: false,
            }),
        );
        host.update(
            &world,
            Some(Pointer2D {
                position: bare,
                pressed: true,
                secondary_pressed: false,
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
                secondary_pressed: false,
            }),
        );
        host.update(
            &world,
            Some(Pointer2D {
                position: bare,
                pressed: true,
                secondary_pressed: false,
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
                },
                GuiComponentRenderInfo::Image {
                    position: vec2(4.0 / 635.0, 18.0 / 120.0),
                    size: vec2(35.0 / 635.0, 32.0 / 120.0),
                    texture: "icn_wrench.pcx".to_owned(),
                    alpha: 0.5,
                    interactive: true,
                    entity: Some(wrench),
                    label: None,
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
        // ContainerGui strip slots: 4px inset, 35px slot pitch, 32px tall
        // (container.rs inv_container), emitted normalized by 635x120.
        let x = 4.0 + 35.0 * slot_x as f32;
        GuiComponentRenderInfo::Image {
            position: vec2(x / 635.0, 18.0 / 120.0),
            size: vec2(35.0 / 635.0, 32.0 / 120.0),
            texture: "icn_x.pcx".to_owned(),
            alpha: 0.5,
            interactive: true,
            entity: Some(entity),
            label: None,
        }
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
                secondary_pressed: false,
            }),
        );
        let (_msgs, actions) = host.update(
            world,
            Some(Pointer2D {
                position: norm(canvas.0, canvas.1),
                pressed: true,
                secondary_pressed: false,
            }),
        );
        actions
    }

    fn secondary_press_edge(
        host: &mut FlatUiHost,
        world: &World,
        canvas: (f32, f32),
    ) -> Vec<FlatUiDragAction> {
        host.update(
            world,
            Some(Pointer2D {
                position: norm(canvas.0, canvas.1),
                pressed: false,
                secondary_pressed: false,
            }),
        );
        let (_msgs, actions) = host.update(
            world,
            Some(Pointer2D {
                position: norm(canvas.0, canvas.1),
                pressed: false,
                secondary_pressed: true,
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
    fn secondary_clicking_the_bare_view_offers_but_keeps_the_item() {
        let (world, mut host, wrench, _inv) = drag_world();
        press_edge(&mut host, &world, (23.5, 34.0)); // lift

        let actions = secondary_press_edge(&mut host, &world, (320.0, 300.0));

        assert_eq!(actions, vec![FlatUiDragAction::Apply(wrench)]);
        assert_eq!(
            host.cursor_debug().map(|cursor| cursor.entity_id),
            Some(wrench.inner() as i32),
            "application keeps the item until a target actually consumes it",
        );
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
}
