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

use cgmath::{Deg, InnerSpace, Point3, Quaternion, Rotation, Rotation3, Vector3, point3, vec3};
use shipyard::{EntityId, Get, View, World};

use dark::{
    SCALE_FACTOR,
    properties::{PropFrobInfo, PropHUDSelect, PropLimbModel, PropPickBias, PropPlayerGun},
};

use crate::{
    input_context::Hand,
    physics::{InternalCollisionGroups, PhysicsWorld},
    runtime_props::RuntimePropReloading,
    scripts::{Message, MessagePayload},
    util::resolve_proxy_entity,
    virtual_hand::{VirtualHandEffect, can_grab_item, is_wieldable_weapon},
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

/// Base yaw applied to every first-person gun model before its per-weapon
/// `PropPlayerGun.heading`. The pistol (`atek_h`, heading 0) renders correctly
/// at -90deg; `heading` then corrects models authored at other angles (e.g. the
/// shotgun's 90deg). Tuned against the FP models, NOT the VR hand-model table.
const VIEWMODEL_BASE_YAW_DEG: f32 = -90.0;

/// Carried guns idle pitched down slightly; the original raises the gun to
/// level only around firing. The same pitch also swings the framing offset
/// down around the camera, so the carried gun sits lower on screen.
const GUN_CARRY_PITCH_DEG: f32 = -11.25;

pub struct FlatPlayerController {
    wielded_entity: Option<EntityId>,
    last_fire_pressed: bool,
    last_use_pressed: bool,
    /// Camera/crosshair fire ray (world space) from the last `update`, so the
    /// firing path can spawn projectiles along the crosshair (camera-origin aim).
    last_aim: Option<(Point3<f32>, Vector3<f32>)>,
}

impl FlatPlayerController {
    pub fn new() -> Self {
        Self {
            wielded_entity: None,
            last_fire_pressed: false,
            last_use_pressed: false,
            last_aim: None,
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

    /// Apply the original flat pickup split: weapons become the first-person
    /// viewmodel, while ordinary loot goes straight into the backpack.
    fn pick_up(&mut self, world: &World, entity_id: EntityId) -> Vec<VirtualHandEffect> {
        if is_wieldable_weapon(world, entity_id) {
            self.wield(entity_id)
        } else {
            vec![VirtualHandEffect::StoreItem { entity_id }]
        }
    }

    /// Per-frame update. Returns the effects to apply plus the entity currently
    /// under the crosshair (for highlight rendering), if any.
    pub fn update(
        &mut self,
        input: &Hand,
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
        head_rotation: Quaternion<f32>,
        eye_height: f32,
        world: &World,
        physics: &PhysicsWorld,
    ) -> (Vec<VirtualHandEffect>, Option<EntityId>) {
        let mut effects = Vec::new();

        let look = player_rotation * head_rotation;
        let camera_pos = player_pos + vec3(0.0, eye_height / SCALE_FACTOR, 0.0);

        // Crosshair raycast: the frobbable entity under the reticle (resolving
        // hitbox proxies to their parent, and ignoring the weapon we hold).
        let forward = look.rotate_vector(vec3(0.0, 0.0, -1.0));
        self.last_aim = Some((point3(camera_pos.x, camera_pos.y, camera_pos.z), forward));
        let highlighted = interaction_target(
            world,
            physics,
            point3(camera_pos.x, camera_pos.y, camera_pos.z),
            forward,
            self.wielded_entity,
        );

        // Place the viewmodel + fire on the trigger edge.
        if let Some(entity_id) = self.wielded_entity {
            // First-person framing: guns place at their authored per-weapon
            // `PropPlayerGun.model_offset` (plus the carry pitch below); melee
            // arms anchor at the authored PlayerMelee arm offset; anything
            // else falls back to `VIEWMODEL_OFFSET`. This intentionally
            // ignores the VR hand-model table, which is keyed inconsistently
            // and drops per-weapon heading. The first-person `hand_model`
            // meshes all share one orientation, corrected by a single base
            // yaw. NB: PropPlayerGun.heading is NOT the FP model's rotation -
            // applying it over-rotates exactly by its value (the
            // shotgun/assault 90deg, the psi-amp 180deg), so it is
            // deliberately not used here.
            let is_melee = world
                .borrow::<View<PropLimbModel>>()
                .map(|v| v.get(entity_id).is_ok())
                .unwrap_or(false);
            // Authored offsets are in Dark camera axes (x back, y left, z up);
            // look space is (+x right, +y up, -z forward), so the conversion
            // is `vec3(-o.y, o.z, o.x)` (`MELEE_VIEWMODEL_OFFSET` is stored
            // pre-converted).
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
            // Melee arms take the camera rotation directly (camSynch semantics:
            // root = ang offset * camera rotation, and the authored ang offset
            // is zero); the gun meshes share one orientation corrected by a
            // single base yaw, plus a pitch (which also swings the framing
            // offset down around the camera - the gun pivots around the EYE,
            // not around its own origin, matching the original's pitch
            // mechanism). At rest the pitch is the carry angle; a reload ramps
            // it from there to the weapon's authored peak and back, so the gun
            // dips out of view instead of spinning in place and exposing the
            // FP mesh's open rear.
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
                        let frac = r.pitch_deg() / r.peak_deg; // ramp 0..1..0
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
            effects.push(VirtualHandEffect::SetPositionRotation {
                entity_id,
                position,
                rotation,
                scale: vec3(1.0, 1.0, 1.0),
            });

            let fire_pressed = input.trigger_value > 0.5;
            if fire_pressed && !self.last_fire_pressed {
                effects.push(out_message(entity_id, MessagePayload::TriggerPull));
            } else if !fire_pressed && self.last_fire_pressed {
                effects.push(out_message(entity_id, MessagePayload::TriggerRelease));
            }
            self.last_fire_pressed = fire_pressed;
        } else {
            self.last_fire_pressed = false;
        }

        // Use / frob / pickup on the use-button (squeeze) rising edge.
        let use_pressed = input.squeeze_value > 0.5;
        if use_pressed && !self.last_use_pressed {
            if let Some(target) = highlighted {
                if can_grab_item(world, target) {
                    // Weapons become the flat viewmodel; ordinary loot goes
                    // into the backpack without displacing that viewmodel.
                    effects.extend(self.pick_up(world, target));
                } else {
                    // Otherwise interact with it.
                    effects.push(out_message(target, MessagePayload::Frob));
                }
            }
        }
        self.last_use_pressed = use_pressed;

        (effects, highlighted)
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

const INTERACTION_GROUPS: InternalCollisionGroups = InternalCollisionGroups::ENTITY
    .union(InternalCollisionGroups::SELECTABLE)
    .union(InternalCollisionGroups::WORLD)
    .union(InternalCollisionGroups::UI)
    .union(InternalCollisionGroups::RAYCAST);

/// HUD-hidden computer shells and their invisible frob overlays are authored
/// at the same origin. Keep the tolerance tight enough that a nearby control
/// cannot become the shell's accidental target.
const HUD_OVERLAY_POSITION_EPSILON: f32 = 0.05;
/// The selectable overlay may sit just inside the decorative shell's collider,
/// but it must still be part of the same visible surface rather than an object
/// farther into the room.
const HUD_OVERLAY_SURFACE_GAP: f32 = 0.5;

/// Resolve the flat crosshair's interaction target while honoring Dark's
/// authored selection priority. The first combined hit still blocks normally.
/// Only an explicitly de-prioritized entity or a tightly matched HUD-hidden
/// shell is skipped, and the second cast keeps all world/entity groups enabled,
/// so a wall or closed door behind it remains an occluder instead of allowing a
/// through-geometry Frob.
fn interaction_target(
    world: &World,
    physics: &PhysicsWorld,
    origin: Point3<f32>,
    forward: Vector3<f32>,
    wielded_entity: Option<EntityId>,
) -> Option<EntityId> {
    let first = physics.ray_cast(origin, forward, INTERACTION_GROUPS)?;
    let first_raw = first.maybe_entity_id?;
    let first_entity = resolve_proxy_entity(world, first_raw);
    let hud_hidden = has_hud_select(world, first_entity, false);
    if !hud_hidden && Some(first_entity) != wielded_entity && is_frobbable(world, first_entity) {
        return Some(first_entity);
    }

    let has_negative_pick_bias = world
        .borrow::<View<PropPickBias>>()
        .map(|v| v.get(first_entity).is_ok_and(|bias| bias.0 < 0.0))
        .unwrap_or(false);
    if !has_negative_pick_bias && !hud_hidden {
        return None;
    }

    let second = physics.ray_cast2(
        origin,
        forward,
        100.0,
        INTERACTION_GROUPS,
        Some(first_raw),
        true,
    )?;
    let second_raw = second.maybe_entity_id?;
    let second_entity = resolve_proxy_entity(world, second_raw);
    if Some(second_entity) == wielded_entity || !is_frobbable(world, second_entity) {
        return None;
    }

    // Explicit negative pick bias keeps its existing general-purpose behavior.
    if has_negative_pick_bias {
        return Some(second_entity);
    }

    // HUDSelect(false) alone is narrower: it represents a decorative shell
    // that may yield only to its co-located HUDSelect(true) frob overlay. The
    // combined recast above ensures a wall, closed door, or unrelated entity
    // between them remains the immediate hit and blocks the interaction.
    // Require direct entity colliders before comparing body origins: a damage
    // proxy's rigid body is not located at its resolved parent entity.
    if first_raw != first_entity
        || second_raw != second_entity
        || !has_hud_select(world, second_entity, true)
    {
        return None;
    }
    let first_position = physics.get_position(first.maybe_rigid_body_handle?)?;
    let second_position = physics.get_position(second.maybe_rigid_body_handle?)?;
    let co_located = (first_position - second_position).magnitude2()
        <= HUD_OVERLAY_POSITION_EPSILON * HUD_OVERLAY_POSITION_EPSILON;
    let close_surface = (first.hit_point - second.hit_point).magnitude2()
        <= HUD_OVERLAY_SURFACE_GAP * HUD_OVERLAY_SURFACE_GAP;
    (co_located && close_surface).then_some(second_entity)
}

fn has_hud_select(world: &World, entity_id: EntityId, expected: bool) -> bool {
    world
        .borrow::<View<PropHUDSelect>>()
        .map(|v| v.get(entity_id).is_ok_and(|select| select.0 == expected))
        .unwrap_or(false)
}

/// Whether an entity is worth highlighting / interacting with. Empty authored
/// FrobInfo overrides deliberately suppress an inherited world action, so
/// property presence by itself is not enough.
fn is_frobbable(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropFrobInfo>>()
        .map(|v| {
            v.get(entity_id)
                .is_ok_and(|frob| !frob.world_action.is_empty())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::CollisionGroup;
    use dark::properties::{FrobFlag, PropHUDSelect, PropPickBias, PropPlayerGun};

    fn frob_info(world_action: FrobFlag) -> PropFrobInfo {
        PropFrobInfo {
            world_action,
            inventory_action: FrobFlag::empty(),
            tool_action: FrobFlag::empty(),
        }
    }

    fn pistol() -> PropPlayerGun {
        PropPlayerGun {
            flags: 0,
            hand_model: "atek_h".to_owned(),
            icon_file: String::new(),
            model_offset: vec3(0.0, 0.0, 0.0),
            fire_offset: vec3(0.0, 0.0, 0.0),
            heading: 0,
            reload_pitch: 0,
            reload_rate: 0,
            gun_type: 0,
        }
    }

    #[test]
    fn world_use_of_nonweapon_stores_it_without_displacing_wielded_weapon() {
        let mut world = World::new();
        let pistol = world.add_entity(pistol());
        let ammo = world.add_entity(());
        let mut controller = FlatPlayerController::new();
        controller.wield(pistol);

        let effects = controller.pick_up(&world, ammo);

        assert_eq!(
            controller.wielded_entity(),
            Some(pistol),
            "ordinary loot must not replace the wielded weapon"
        );
        assert!(
            matches!(
                effects.as_slice(),
                [VirtualHandEffect::StoreItem { entity_id }] if *entity_id == ammo
            ),
            "ordinary loot should be sent to the backpack"
        );
    }

    #[test]
    fn world_use_of_weapon_still_wields_it() {
        let mut world = World::new();
        let weapon = world.add_entity(pistol());
        let mut controller = FlatPlayerController::new();

        let effects = controller.pick_up(&world, weapon);

        assert_eq!(controller.wielded_entity(), Some(weapon));
        assert!(
            matches!(
                effects.as_slice(),
                [VirtualHandEffect::HoldItem { entity_id }] if *entity_id == weapon
            ),
            "weapon pickup should preserve flat auto-wield"
        );
    }

    #[test]
    fn empty_or_inventory_only_frob_info_is_not_world_frobbable() {
        let mut world = World::new();
        let empty = world.add_entity(frob_info(FrobFlag::empty()));
        let inventory_only = world.add_entity(PropFrobInfo {
            world_action: FrobFlag::empty(),
            inventory_action: FrobFlag::SCRIPT,
            tool_action: FrobFlag::empty(),
        });

        assert!(!is_frobbable(&world, empty));
        assert!(!is_frobbable(&world, inventory_only));
    }

    #[test]
    fn negative_pick_bias_surface_yields_to_frobbable_overlay() {
        let mut world = World::new();
        let player = world.add_entity(());
        let decorative_surface = world.add_entity(PropPickBias(-2000.0));
        let frobbable_overlay = world.add_entity(frob_info(FrobFlag::SCRIPT));
        let mut physics = PhysicsWorld::new();
        let mut player_handle = physics.create_player(vec3(1000.0, 1000.0, 1000.0), player);
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        physics.add_kinematic(
            decorative_surface,
            vec3(0.0, 0.0, -1.0),
            identity,
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 1.0, 0.2),
            CollisionGroup::entity(),
            false,
        );
        physics.add_kinematic(
            frobbable_overlay,
            vec3(0.0, 0.0, -1.25),
            identity,
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 1.0, 0.1),
            CollisionGroup::selectable(),
            false,
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player_handle);

        let mut input = Hand::default();
        input.squeeze_value = 1.0;
        let (effects, highlighted) = FlatPlayerController::new().update(
            &input,
            vec3(0.0, 0.0, 0.0),
            identity,
            identity,
            0.0,
            &world,
            &physics,
        );

        assert_eq!(highlighted, Some(frobbable_overlay));
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                VirtualHandEffect::OutMessage {
                    message: Message {
                        to,
                        payload: MessagePayload::Frob
                    }
                } if *to == frobbable_overlay
            )),
            "the authored overlay should receive the production Frob"
        );
    }

    #[test]
    fn hud_hidden_surface_yields_to_colocated_selectable_overlay() {
        let mut world = World::new();
        let player = world.add_entity(());
        let decorative_surface =
            world.add_entity((PropHUDSelect(false), frob_info(FrobFlag::empty())));
        let frobbable_overlay =
            world.add_entity((PropHUDSelect(true), frob_info(FrobFlag::SCRIPT)));
        let mut physics = PhysicsWorld::new();
        let mut player_handle = physics.create_player(vec3(1000.0, 1000.0, 1000.0), player);
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        physics.add_kinematic(
            decorative_surface,
            vec3(0.0, 0.0, -1.0),
            identity,
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 1.0, 0.2),
            CollisionGroup::entity(),
            false,
        );
        physics.add_kinematic(
            frobbable_overlay,
            vec3(0.0, 0.0, -1.0),
            identity,
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 1.0, 0.1),
            CollisionGroup::selectable(),
            false,
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player_handle);

        let mut input = Hand::default();
        input.squeeze_value = 1.0;
        let (effects, highlighted) = FlatPlayerController::new().update(
            &input,
            vec3(0.0, 0.0, 0.0),
            identity,
            identity,
            0.0,
            &world,
            &physics,
        );

        assert_eq!(highlighted, Some(frobbable_overlay));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            VirtualHandEffect::OutMessage {
                message: Message {
                    to,
                    payload: MessagePayload::Frob
                }
            } if *to == frobbable_overlay
        )));
    }

    #[test]
    fn hud_hidden_surface_does_not_frob_through_an_entity_blocker() {
        let mut world = World::new();
        let player = world.add_entity(());
        let decorative_surface = world.add_entity(PropHUDSelect(false));
        let closed_door = world.add_entity(());
        let frobbable_overlay =
            world.add_entity((PropHUDSelect(true), frob_info(FrobFlag::SCRIPT)));
        let mut physics = PhysicsWorld::new();
        let mut player_handle = physics.create_player(vec3(1000.0, 1000.0, 1000.0), player);
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        for (entity, z, depth, group) in [
            (decorative_surface, -1.0, 0.2, CollisionGroup::entity()),
            (closed_door, -0.925, 0.02, CollisionGroup::entity()),
            (frobbable_overlay, -1.0, 0.1, CollisionGroup::selectable()),
        ] {
            physics.add_kinematic(
                entity,
                vec3(0.0, 0.0, z),
                identity,
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 1.0, depth),
                group,
                false,
            );
        }
        physics.update(vec3(0.0, 0.0, 0.0), &mut player_handle);

        let first = physics
            .ray_cast(
                point3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, -1.0),
                INTERACTION_GROUPS,
            )
            .expect("the hidden shell should be the first combined hit");
        assert_eq!(first.maybe_entity_id, Some(decorative_surface));
        let second = physics
            .ray_cast2(
                point3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, -1.0),
                100.0,
                INTERACTION_GROUPS,
                Some(decorative_surface),
                true,
            )
            .expect("the door should block the recast behind the shell");
        assert_eq!(second.maybe_entity_id, Some(closed_door));

        let mut input = Hand::default();
        input.squeeze_value = 1.0;
        let (effects, highlighted) = FlatPlayerController::new().update(
            &input,
            vec3(0.0, 0.0, 0.0),
            identity,
            identity,
            0.0,
            &world,
            &physics,
        );

        assert_eq!(highlighted, None);
        assert!(!effects.iter().any(|effect| matches!(
            effect,
            VirtualHandEffect::OutMessage {
                message: Message {
                    payload: MessagePayload::Frob,
                    ..
                }
            }
        )));
    }

    #[test]
    fn hud_hidden_surface_does_not_frob_an_unrelated_selectable() {
        let mut world = World::new();
        let player = world.add_entity(());
        let decorative_surface = world.add_entity(PropHUDSelect(false));
        let unrelated_control =
            world.add_entity((PropHUDSelect(true), frob_info(FrobFlag::SCRIPT)));
        let mut physics = PhysicsWorld::new();
        let mut player_handle = physics.create_player(vec3(1000.0, 1000.0, 1000.0), player);
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        physics.add_kinematic(
            decorative_surface,
            vec3(0.0, 0.0, -1.0),
            identity,
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 1.0, 0.2),
            CollisionGroup::entity(),
            false,
        );
        physics.add_kinematic(
            unrelated_control,
            vec3(0.0, 0.0, -1.3),
            identity,
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 1.0, 0.1),
            CollisionGroup::selectable(),
            false,
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player_handle);

        let mut input = Hand::default();
        input.squeeze_value = 1.0;
        let (effects, highlighted) = FlatPlayerController::new().update(
            &input,
            vec3(0.0, 0.0, 0.0),
            identity,
            identity,
            0.0,
            &world,
            &physics,
        );

        assert_eq!(highlighted, None);
        assert!(!effects.iter().any(|effect| matches!(
            effect,
            VirtualHandEffect::OutMessage {
                message: Message {
                    payload: MessagePayload::Frob,
                    ..
                }
            }
        )));
    }

    #[test]
    fn negative_pick_bias_does_not_frob_through_an_entity_blocker() {
        let mut world = World::new();
        let player = world.add_entity(());
        let decorative_surface = world.add_entity(PropPickBias(-2000.0));
        let closed_door = world.add_entity(());
        let frobbable_behind_door = world.add_entity(frob_info(FrobFlag::SCRIPT));
        let mut physics = PhysicsWorld::new();
        let mut player_handle = physics.create_player(vec3(1000.0, 1000.0, 1000.0), player);
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        for (entity, z, group) in [
            (decorative_surface, -1.0, CollisionGroup::entity()),
            (closed_door, -1.25, CollisionGroup::entity()),
            (frobbable_behind_door, -1.5, CollisionGroup::selectable()),
        ] {
            physics.add_kinematic(
                entity,
                vec3(0.0, 0.0, z),
                identity,
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 1.0, 0.1),
                group,
                false,
            );
        }
        physics.update(vec3(0.0, 0.0, 0.0), &mut player_handle);

        let mut input = Hand::default();
        input.squeeze_value = 1.0;
        let (effects, highlighted) = FlatPlayerController::new().update(
            &input,
            vec3(0.0, 0.0, 0.0),
            identity,
            identity,
            0.0,
            &world,
            &physics,
        );

        assert_eq!(highlighted, None);
        assert!(
            !effects.iter().any(|effect| matches!(
                effect,
                VirtualHandEffect::OutMessage {
                    message: Message {
                        payload: MessagePayload::Frob,
                        ..
                    }
                }
            )),
            "an ordinary entity between the biased surface and target must still occlude the Frob"
        );
    }
}
