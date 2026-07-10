//! Flatscreen first-person player controller.
//!
//! The dedicated flatscreen analog of `VirtualHand`: it wields a single weapon
//! as a first-person viewmodel, fires it on the trigger, and uses/frobs/picks
//! up the object under the crosshair. It produces the same `VirtualHandEffect`s
//! the VR hands do (`HoldItem`, `DropItem`, `SetPositionRotation`,
//! `OutMessage { TriggerPull/Release/Frob }`), so `mission_core` processes VR
//! and flat through one shared path, and the weapon/frob scripts run unchanged.
//!
//! See `projects/flatscreen-and-vr-architecture.md` (Slices 5-6).

use cgmath::{Deg, Point3, Quaternion, Rotation, Rotation3, Vector2, Vector3, point3, vec2, vec3};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{EntityId, Get, View, World};

use dark::{
    SCALE_FACTOR,
    properties::{
        Link, PropFrobInfo, PropInventoryDimensions, PropLimbModel, PropObjIcon, PropObjShortName,
        PropPlayerGun, PropScripts,
    },
};

use crate::{
    input_context::InputContext,
    inventory::Inventory,
    physics::{InternalCollisionGroups, PhysicsWorld},
    runtime_props::RuntimePropReloading,
    scripts::{Message, MessagePayload, script_util},
    ui::{HAlign, Rect, ScaleMode, UiCanvas, VAlign},
    util::resolve_proxy_entity,
    virtual_hand::{VirtualHandEffect, can_grab_item},
};

/// Fallback viewmodel framing offset (look space: +x right, +y up, -z forward),
/// before the world-scale divide. Used only for wielded items with no
/// `PropPlayerGun` and no `PropLimbModel` (guns and melee use authored
/// offsets). Tuned before the viewmodel-FOV scale existed, so such items now
/// read a bit smaller; retune if a real pickup ends up on this path.
const VIEWMODEL_OFFSET: Vector3<f32> = vec3(2.0, -2.5, -5.0);
/// Viewmodel offset for melee weapons (PropLimbModel, no PropPlayerGun): the
/// authored arm anchor from the gamesys `PlayerMelee` motion archetype's
/// "Arm Pos Offset" (0.2, -0.6, -2.4) - Dark camera axes (x back, y left,
/// z up), same convention as `PropPlayerGun.model_offset` - pre-converted to
/// look space. The arm ROOT sits there (below/right of and just behind the
/// eye); the melee pose (root motion cancelled) raises the hand from it. Its
/// "Arm Ang Offset" is zero, so the arm root takes the camera rotation
/// directly.
const MELEE_VIEWMODEL_OFFSET: Vector3<f32> = vec3(0.6, -2.4, 0.2);
/// Camera (eye) height above the player's feet, in SS2 units before the
/// world-scale divide. Shared with the runtimes' render-camera `head_offset` via
/// `crate::PLAYER_EYE_HEIGHT` so the shot/viewmodel origin coincides with the
/// rendered eye - otherwise horizontal shots land above/below the crosshair.
const HEAD_HEIGHT: f32 = crate::PLAYER_EYE_HEIGHT;

/// Base yaw applied to every first-person gun model before its per-weapon
/// `PropPlayerGun.heading`. The pistol (`atek_h`, heading 0) renders correctly
/// at -90deg; `heading` then corrects models authored at other angles (e.g. the
/// shotgun's 90deg). Tuned against the FP models, NOT the VR hand-model table.
const VIEWMODEL_BASE_YAW_DEG: f32 = -90.0;

/// Carried guns idle pitched down slightly; the original raises the gun to
/// level only around firing. The same pitch also swings the framing offset
/// down around the camera, so the carried gun sits lower on screen.
const GUN_CARRY_PITCH_DEG: f32 = -11.25;

const CONTAINER_CANVAS_SIZE: Vector2<f32> = vec2(640.0, 480.0);
const CONTAINER_PANEL: Rect = Rect::new(226.0, 92.0, 188.0, 296.0);
const CONTAINER_INVENTORY_OFFSET: Vector2<f32> = vec2(241.0, 252.0);
const CONTAINER_SLOT_SIZE: Vector2<f32> = vec2(35.0, 32.0);

#[derive(Clone, Copy, Debug)]
struct FlatContainerItem {
    entity_id: EntityId,
    rect: Rect,
}

#[derive(Clone, Copy, Debug)]
struct FlatContainerState {
    entity_id: EntityId,
    hovered_item: Option<EntityId>,
}

pub struct FlatPlayerController {
    wielded_entity: Option<EntityId>,
    last_fire_pressed: bool,
    last_use_pressed: bool,
    /// Camera/crosshair fire ray (world space) from the last `update`, so the
    /// firing path can spawn projectiles along the crosshair (camera-origin aim).
    last_aim: Option<(Point3<f32>, Vector3<f32>)>,
    active_container: Option<FlatContainerState>,
    last_pointer_pressed: bool,
}

impl FlatPlayerController {
    pub fn new() -> Self {
        Self {
            wielded_entity: None,
            last_fire_pressed: false,
            last_use_pressed: false,
            last_aim: None,
            active_container: None,
            last_pointer_pressed: false,
        }
    }

    pub fn is_wielding(&self) -> bool {
        self.wielded_entity.is_some()
    }

    pub fn wielded_entity(&self) -> Option<EntityId> {
        self.wielded_entity
    }

    /// The camera/crosshair fire ray (origin, forward) from the last `update`.
    pub fn aim_ray(&self) -> Option<(Point3<f32>, Vector3<f32>)> {
        self.last_aim
    }

    pub fn active_container(&self) -> Option<EntityId> {
        self.active_container.map(|state| state.entity_id)
    }

    pub fn active_container_items(&self, world: &World) -> Vec<EntityId> {
        self.active_container
            .map(|state| {
                container_items(world, state.entity_id)
                    .into_iter()
                    .map(|item| item.entity_id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Stop wielding `entity_id` if it was the held weapon (e.g. it was
    /// destroyed).
    pub fn on_entity_destroyed(&mut self, entity_id: EntityId) {
        if self.wielded_entity == Some(entity_id) {
            self.wielded_entity = None;
        }
    }

    /// If the wielded weapon was recreated as a new entity, track the new id.
    pub fn replace_wielded(&mut self, old: EntityId, new: EntityId) {
        if self.wielded_entity == Some(old) {
            self.wielded_entity = Some(new);
        }
    }

    /// Wield `entity_id` as the first-person weapon. Any previously-wielded
    /// weapon is dropped back into the world (regains physics + world model).
    pub fn wield(&mut self, entity_id: EntityId) -> Vec<VirtualHandEffect> {
        let mut effects = Vec::new();
        if let Some(prev) = self.wielded_entity {
            if prev != entity_id {
                effects.push(VirtualHandEffect::DropItem { entity_id: prev });
            }
        }
        self.wielded_entity = Some(entity_id);
        self.last_fire_pressed = false;
        effects.push(VirtualHandEffect::HoldItem { entity_id });
        effects
    }

    /// Per-frame update. Returns the effects to apply plus the entity currently
    /// under the crosshair (for highlight rendering), if any.
    pub fn update(
        &mut self,
        input_context: &InputContext,
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
        head_rotation: Quaternion<f32>,
        world: &World,
        physics: &PhysicsWorld,
    ) -> (Vec<VirtualHandEffect>, Option<EntityId>) {
        let mut effects = Vec::new();
        let input = &input_context.right_hand;

        // While a flat container is open the pointer owns interaction. A new
        // squeeze edge toggles it closed; a pointer click takes exactly the
        // chosen item through the normal wield/HoldItem path.
        if let Some(mut container) = self.active_container {
            let use_pressed = input.squeeze_value > 0.5;
            if use_pressed && !self.last_use_pressed {
                self.active_container = None;
                self.last_pointer_pressed = false;
            } else {
                let pointer = input_context.pointer;
                let canvas_pointer = pointer.map(|p| {
                    vec2(
                        p.position.x * CONTAINER_CANVAS_SIZE.x,
                        p.position.y * CONTAINER_CANVAS_SIZE.y,
                    )
                });
                let items = container_items(world, container.entity_id);
                container.hovered_item = canvas_pointer.and_then(|position| {
                    items
                        .iter()
                        .find(|item| item.rect.contains(position))
                        .map(|item| item.entity_id)
                });
                let pointer_pressed = pointer.is_some_and(|p| p.pressed);
                if pointer_pressed && !self.last_pointer_pressed {
                    if let Some(item) = container.hovered_item {
                        effects.extend(self.wield(item));
                    }
                }
                self.last_pointer_pressed = pointer_pressed;
                self.active_container = Some(container);
            }
            self.last_use_pressed = use_pressed;
            effects.extend(self.update_wielded_viewmodel(
                input.trigger_value,
                player_pos,
                player_rotation,
                head_rotation,
                world,
                false,
            ));
            return (effects, None);
        }
        self.last_pointer_pressed = false;

        let look = player_rotation * head_rotation;
        let camera_pos = player_pos + vec3(0.0, HEAD_HEIGHT / SCALE_FACTOR, 0.0);

        // Crosshair raycast: the frobbable entity under the reticle (resolving
        // hitbox proxies to their parent, and ignoring the weapon we hold).
        let forward = look.rotate_vector(vec3(0.0, 0.0, -1.0));
        self.last_aim = Some((point3(camera_pos.x, camera_pos.y, camera_pos.z), forward));
        let highlighted = physics
            .ray_cast(
                point3(camera_pos.x, camera_pos.y, camera_pos.z),
                forward,
                InternalCollisionGroups::ENTITY
                    | InternalCollisionGroups::SELECTABLE
                    | InternalCollisionGroups::WORLD
                    | InternalCollisionGroups::UI
                    | InternalCollisionGroups::RAYCAST,
            )
            .and_then(|r| r.maybe_entity_id)
            .map(|e| resolve_proxy_entity(world, e))
            .filter(|e| Some(*e) != self.wielded_entity && is_frobbable(world, *e));

        effects.extend(self.update_wielded_viewmodel(
            input.trigger_value,
            player_pos,
            player_rotation,
            head_rotation,
            world,
            true,
        ));

        // Use / frob / pickup on the use-button (squeeze) rising edge.
        let use_pressed = input.squeeze_value > 0.5;
        if use_pressed && !self.last_use_pressed {
            if let Some(target) = highlighted {
                if can_grab_item(world, target) {
                    // A pickup-able object (e.g. a weapon): wield it.
                    effects.extend(self.wield(target));
                } else {
                    // Otherwise interact with it.
                    effects.push(out_message(target, MessagePayload::Frob));
                    if is_container(world, target) {
                        self.active_container = Some(FlatContainerState {
                            entity_id: target,
                            hovered_item: None,
                        });
                    }
                }
            }
        }
        self.last_use_pressed = use_pressed;

        (effects, highlighted)
    }

    fn update_wielded_viewmodel(
        &mut self,
        trigger_value: f32,
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
        head_rotation: Quaternion<f32>,
        world: &World,
        allow_fire: bool,
    ) -> Vec<VirtualHandEffect> {
        let Some(entity_id) = self.wielded_entity else {
            self.last_fire_pressed = false;
            return Vec::new();
        };

        let look = player_rotation * head_rotation;
        let camera_pos = player_pos + vec3(0.0, HEAD_HEIGHT / SCALE_FACTOR, 0.0);
        // First-person framing: guns place at their authored per-weapon
        // `PropPlayerGun.model_offset`; melee arms use the authored PlayerMelee
        // anchor; anything else falls back to `VIEWMODEL_OFFSET`.
        let is_melee = world
            .borrow::<View<PropLimbModel>>()
            .map(|v| v.get(entity_id).is_ok())
            .unwrap_or(false);
        let gun_offset = world
            .borrow::<View<PropPlayerGun>>()
            .ok()
            .and_then(|v| v.get(entity_id).ok().map(|g| g.model_offset));
        let offset = if is_melee {
            MELEE_VIEWMODEL_OFFSET
        } else if let Some(mo) = gun_offset {
            vec3(-mo.y, mo.z, mo.x)
        } else {
            VIEWMODEL_OFFSET
        };
        let (rotation, position) = if is_melee {
            (
                look * Quaternion::from_angle_y(Deg(180.0)),
                camera_pos + look.rotate_vector(offset / SCALE_FACTOR),
            )
        } else {
            let pitch_deg = match world
                .borrow::<View<RuntimePropReloading>>()
                .ok()
                .and_then(|v| v.get(entity_id).ok().copied())
            {
                Some(r) if r.peak_deg.abs() > f32::EPSILON => {
                    let frac = r.pitch_deg() / r.peak_deg;
                    GUN_CARRY_PITCH_DEG + (r.peak_deg - GUN_CARRY_PITCH_DEG) * frac
                }
                _ => GUN_CARRY_PITCH_DEG,
            };
            let pitch = Quaternion::from_angle_x(Deg(pitch_deg));
            (
                look * pitch * Quaternion::from_angle_y(Deg(VIEWMODEL_BASE_YAW_DEG)),
                camera_pos + (look * pitch).rotate_vector(offset / SCALE_FACTOR),
            )
        };
        let mut effects = vec![VirtualHandEffect::SetPositionRotation {
            entity_id,
            position,
            rotation,
            scale: vec3(1.0, 1.0, 1.0),
        }];

        if allow_fire {
            let fire_pressed = trigger_value > 0.5;
            if fire_pressed && !self.last_fire_pressed {
                effects.push(out_message(entity_id, MessagePayload::TriggerPull));
            } else if !fire_pressed && self.last_fire_pressed {
                effects.push(out_message(entity_id, MessagePayload::TriggerRelease));
            }
            self.last_fire_pressed = fire_pressed;
        } else {
            self.last_fire_pressed = false;
        }
        effects
    }

    pub fn render_container_ui(
        &self,
        asset_cache: &mut AssetCache,
        world: &World,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        let Some(state) = self.active_container else {
            return Vec::new();
        };

        let mut canvas = UiCanvas::new(CONTAINER_CANVAS_SIZE);
        canvas.image(CONTAINER_PANEL, "contain.pcx");
        let title = world
            .borrow::<View<PropObjShortName>>()
            .ok()
            .and_then(|names| names.get(state.entity_id).ok().map(|name| name.0.clone()))
            .unwrap_or_else(|| "CONTAINER".to_string());
        canvas.text(
            Rect::new(238.0, 106.0, 164.0, 24.0),
            &title,
            "mainfont.fon",
            16.0,
            HAlign::Center,
            VAlign::Middle,
        );

        let icons = world.borrow::<View<PropObjIcon>>().unwrap();
        for item in container_items(world, state.entity_id) {
            if let Ok(icon) = icons.get(item.entity_id) {
                canvas.image(item.rect, &format!("{}.pcx", icon.0));
            }
            if state.hovered_item == Some(item.entity_id) {
                canvas.text(
                    Rect::new(item.rect.x - 14.0, item.rect.y, 12.0, item.rect.h),
                    ">",
                    "mainfont.fon",
                    16.0,
                    HAlign::Center,
                    VAlign::Middle,
                );
            }
        }
        canvas.text(
            Rect::new(238.0, 354.0, 164.0, 22.0),
            "CLICK ITEM  |  USE TO CLOSE",
            "mainfont.fon",
            9.0,
            HAlign::Center,
            VAlign::Middle,
        );
        canvas.render_screen_space(asset_cache, screen_size, ScaleMode::PreserveAspect)
    }
}

impl Default for FlatPlayerController {
    fn default() -> Self {
        Self::new()
    }
}

fn out_message(to: EntityId, payload: MessagePayload) -> VirtualHandEffect {
    VirtualHandEffect::OutMessage {
        message: Message { to, payload },
    }
}

/// Whether an entity is worth highlighting / interacting with (it has frob
/// info), which excludes plain world geometry the ray also hits.
fn is_frobbable(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropFrobInfo>>()
        .map(|v| v.get(entity_id).is_ok())
        .unwrap_or(false)
}

fn is_container(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropScripts>>()
        .map(|scripts| {
            scripts.get(entity_id).is_ok_and(|scripts| {
                // CreatureContainer belongs to living monsters too; exposing
                // it here would allow looting AI before death. Concrete loot
                // containers and spawned corpses inherit ContainerScript.
                scripts
                    .scripts
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case("containerscript"))
            })
        })
        .unwrap_or(false)
}

fn container_items(world: &World, entity_id: EntityId) -> Vec<FlatContainerItem> {
    let mut links = script_util::get_all_links_with_data(world, entity_id, |link| match link {
        Link::Contains(ordinal) => Some(*ordinal),
        _ => None,
    });
    links.sort_by_key(|(_, ordinal)| *ordinal);

    let dimensions = world.borrow::<View<PropInventoryDimensions>>().unwrap();
    let mut inventory = Inventory::new(4, 4);
    for (item, _) in links {
        let (width, height) = dimensions
            .get(item)
            .map(|dims| (dims.width as usize, dims.height as usize))
            .unwrap_or((1, 1));
        inventory.insert_first_available(item, width, height);
    }
    inventory
        .all_items()
        .map(|item| FlatContainerItem {
            entity_id: item.entity,
            rect: Rect::new(
                CONTAINER_INVENTORY_OFFSET.x + CONTAINER_SLOT_SIZE.x * item.x as f32,
                CONTAINER_INVENTORY_OFFSET.y + CONTAINER_SLOT_SIZE.y * item.y as f32,
                CONTAINER_SLOT_SIZE.x * item.width as f32,
                CONTAINER_SLOT_SIZE.y * item.height as f32,
            ),
        })
        .collect()
}
