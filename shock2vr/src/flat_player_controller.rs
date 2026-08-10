//! Flatscreen first-person player controller.
//!
//! The dedicated flatscreen analog of `VirtualHand`: it wields a single weapon
//! as a first-person viewmodel, fires it on the trigger, and uses/frobs/picks
//! up the object under the crosshair. It produces the same `VirtualHandEffect`s
//! the VR hands do (`HoldItem`, `StoreItem`, `SetPositionRotation`,
//! `OutMessage { TriggerPull/Release/Frob }`), so `mission_core` processes VR
//! and flat through one shared path, and the weapon/frob scripts run unchanged.
//!
//! See `projects/flatscreen-and-vr-architecture.md` (Slices 5-6).

use cgmath::{Deg, Point3, Quaternion, Rotation, Rotation3, Vector3, point3, vec3};
use shipyard::{EntityId, Get, View, World};

use dark::{
    SCALE_FACTOR,
    properties::{PropFrobInfo, PropLimbModel, PropPlayerGun},
};

use crate::{
    input_context::Hand,
    physics::{InternalCollisionGroups, PhysicsWorld},
    runtime_props::RuntimePropReloading,
    scripts::{Message, MessagePayload},
    util::resolve_proxy_entity,
    virtual_hand::{
        FROB_REACH, VirtualHandEffect, can_grab_item, is_wieldable_weapon, uses_scripted_world_frob,
    },
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
    /// weapon is holstered back into the player's backpack, as the original
    /// does - a swap never costs you the weapon (#777). Only an explicit drop
    /// (the use-mode throw) puts a carried weapon back in the world.
    pub fn wield(&mut self, entity_id: EntityId) -> Vec<VirtualHandEffect> {
        let mut effects = Vec::new();
        if let Some(prev) = self.wielded_entity {
            if prev != entity_id {
                // `Drop` is what tells the weapon it left the hand: it restores
                // the world model over the first-person `_h` mesh and cancels a
                // charging psi amp. The item then goes to the backpack instead
                // of gaining physics presence at the viewmodel's position.
                effects.push(out_message(prev, MessagePayload::Drop));
                effects.push(VirtualHandEffect::StoreItem { entity_id: prev });
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
        if uses_scripted_world_frob(world, entity_id) {
            vec![out_message(entity_id, MessagePayload::Frob)]
        } else if is_wieldable_weapon(world, entity_id) {
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
        let highlighted = physics
            .ray_cast2(
                point3(camera_pos.x, camera_pos.y, camera_pos.z),
                forward,
                FROB_REACH,
                InternalCollisionGroups::ENTITIES
                    | InternalCollisionGroups::SELECTABLE
                    | InternalCollisionGroups::WORLD
                    | InternalCollisionGroups::UI
                    | InternalCollisionGroups::RAYCAST,
                None,
                true,
            )
            .and_then(|r| r.maybe_entity_id)
            .map(|e| resolve_proxy_entity(world, e))
            .filter(|e| Some(*e) != self.wielded_entity && is_frobbable(world, *e));

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

/// Whether an entity is worth highlighting / interacting with (it has frob
/// info), which excludes plain world geometry the ray also hits.
fn is_frobbable(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropFrobInfo>>()
        .map(|v| v.get(entity_id).is_ok())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{FrobFlag, KeyCard, PropFrobInfo, PropKeySrc, PropPlayerGun};

    use crate::physics::CollisionGroup;

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
        let ammo = world.add_entity(frob_info(FrobFlag::MOVE));
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

    /// #777: swapping the wielded weapon used to eject the previous one onto
    /// the floor. The original returns it to the backpack; only an explicit
    /// drop takes a weapon out of the player's possession.
    #[test]
    fn wielding_another_weapon_stores_the_displaced_one_instead_of_dropping_it() {
        let mut world = World::new();
        let wrench = world.add_entity(pistol());
        let shotgun = world.add_entity(pistol());
        let mut controller = FlatPlayerController::new();
        controller.wield(wrench);

        let effects = controller.wield(shotgun);

        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, VirtualHandEffect::DropItem { .. })),
            "a wield swap must never eject the displaced weapon into the world"
        );
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                VirtualHandEffect::StoreItem { entity_id } if *entity_id == wrench
            )),
            "the displaced weapon should go back to the backpack, got {effects:?}"
        );
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                VirtualHandEffect::OutMessage {
                    message: Message {
                        to,
                        payload: MessagePayload::Drop,
                    },
                } if *to == wrench
            )),
            "the displaced weapon must still be told it left the hand (world \
             model restore, psi-charge cancel), got {effects:?}"
        );
    }

    /// The world-pickup auto-wield is the path the campaign session lost a
    /// weapon on: it applies the controller's effects directly, with no
    /// `Effect::GrabEntity` recovery behind it.
    #[test]
    fn world_pickup_of_a_second_weapon_stores_the_wielded_one() {
        let mut world = World::new();
        let carried = world.add_entity(pistol());
        let on_the_floor = world.add_entity(pistol());
        let mut controller = FlatPlayerController::new();
        controller.wield(carried);

        let effects = controller.pick_up(&world, on_the_floor);

        assert_eq!(controller.wielded_entity(), Some(on_the_floor));
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                VirtualHandEffect::StoreItem { entity_id } if *entity_id == carried
            )),
            "picking a weapon up must holster the previous one, got {effects:?}"
        );
    }

    #[test]
    fn world_use_of_move_only_keycard_dispatches_frob_for_injected_script() {
        let mut world = World::new();
        let keycard = world.add_entity((
            frob_info(FrobFlag::MOVE),
            PropKeySrc(KeyCard {
                is_master: false,
                region_id: 8192,
                lock_id: 0,
            }),
        ));
        let mut controller = FlatPlayerController::new();

        let effects = controller.pick_up(&world, keycard);

        assert!(
            matches!(
                effects.as_slice(),
                [VirtualHandEffect::OutMessage {
                    message: Message {
                        to,
                        payload: MessagePayload::Frob,
                    },
                }] if *to == keycard
            ),
            "a MOVE-only PropKeySrc pickup must run its injected keycard Frob path"
        );
    }

    #[test]
    fn world_use_of_authored_move_and_script_dispatches_frob() {
        let mut world = World::new();
        let quest_item = world.add_entity(frob_info(FrobFlag::MOVE | FrobFlag::SCRIPT));
        let mut controller = FlatPlayerController::new();

        let effects = controller.pick_up(&world, quest_item);

        assert!(
            matches!(
                effects.as_slice(),
                [VirtualHandEffect::OutMessage {
                    message: Message {
                        to,
                        payload: MessagePayload::Frob,
                    },
                }] if *to == quest_item
            ),
            "an authored MOVE | SCRIPT pickup must run its Frob path exactly once; got {effects:?}"
        );
    }

    /// Retail `GAMEPARAM` authors `Frob Dist = 50`, which the original picker
    /// treats as squared SS2 units. After this engine's 2.5 world-scale divide,
    /// a surface farther than `sqrt(50) / 2.5` must not highlight or frob.
    #[test]
    fn crosshair_does_not_frob_a_visible_entity_beyond_retail_reach() {
        let mut world = World::new();
        let target = world.add_entity(frob_info(FrobFlag::SCRIPT));
        let mut physics = PhysicsWorld::new();
        physics.add_kinematic(
            target,
            vec3(0.0, 0.0, -4.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.2, 0.2, 0.2),
            CollisionGroup::selectable(),
            false,
        );
        let mut player = physics.create_player(
            vec3(100.0, 100.0, 100.0),
            EntityId::from_inner(1000).unwrap(),
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        let ray_hit = physics
            .ray_cast(
                point3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, -1.0),
                InternalCollisionGroups::SELECTABLE,
            )
            .expect("the fixture target must remain visible to an unbounded ray");
        assert_eq!(ray_hit.maybe_entity_id, Some(target));

        let (effects, highlighted) = FlatPlayerController::new().update(
            &Hand {
                squeeze_value: 1.0,
                ..Hand::default()
            },
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            0.0,
            &world,
            &physics,
        );

        assert_eq!(highlighted, None, "out-of-reach objects must not highlight");
        assert!(
            effects.is_empty(),
            "out-of-reach use must emit no frob effects, got {effects:?}"
        );
    }
}
