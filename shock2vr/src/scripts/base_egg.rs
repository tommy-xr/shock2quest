use cgmath::{Deg, InnerSpace, Quaternion, Rotation3, Vector3, vec3};
use dark::properties::{PropAI, PropCreature, PropModelName, PropPosition};
use engine::audio::AudioHandle;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::{PlayerInfo, entity_creator::CreateEntityOptions},
    physics::PhysicsWorld,
    util::vec3_to_point3,
};

use super::{
    Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::change_to_last_model,
};

const SCRIPT_STATE_KEY: &str = "shock2vr.base_egg";

#[derive(Serialize, Deserialize)]
struct BaseEggState {
    hatched: bool,
}

/// What a pod puts into the world when it opens. Retail models this as three
/// `BaseEgg` subclasses that share the opening and differ only in the objects
/// they create.
pub enum EggPayload {
    /// `GooEgg` - "a toxic egg". The emitter is a `TweqEmitterConfig` object
    /// that lobs four venom-stimmed "Goo Projectile"s and then destroys
    /// itself. EggGooCloud adds its authored one-shot Venom radius pulse
    /// and expires after its particle burst.
    Goo,
    /// `GrubEgg` - "a crawling-annelid egg".
    Grub,
    /// `SwarmerEgg` - "a flying-annelid egg".
    Swarmer,
}

impl EggPayload {
    /// Authored object names, exactly as the retail scripts name them. Looked
    /// up by name because that is the identity the scripts carry; template ids
    /// are an implementation detail of the gamesys.
    fn template_names(&self) -> &'static [&'static str] {
        match self {
            EggPayload::Goo => &["Egg Goo Emitter", "EggGooCloud"],
            EggPayload::Grub => &["Grub"],
            EggPayload::Swarmer => &["Swarm"],
        }
    }

    /// Clearance from the pod origin along its mouth direction. Goo needs
    /// extra clearance so emitted globs do not immediately strike the shell.
    fn lift(&self) -> f32 {
        match self {
            EggPayload::Goo => GOO_MUZZLE_CLEARANCE,
            EggPayload::Grub | EggPayload::Swarmer => SHELL_MOUTH,
        }
    }
}

/// An annelid egg pod: retail `BaseEgg` plus one of its three payload
/// subclasses (`GooEgg` / `GrubEgg` / `SwarmerEgg`).
///
/// Missions arm a pod with a once, player-enter tripwire SwitchLinked to it,
/// so the pod's whole contract is a single `TurnOn`: open (the authored model
/// tweq swaps `eggcl` for `eggop`) and put the payload into the world at the
/// pod. Opening alone was already implemented; the payload was not, which is
/// why every pod in the game hatched into nothing.
pub struct BaseEgg {
    payload: EggPayload,
    hatched: bool,
}

impl BaseEgg {
    pub fn new(payload: EggPayload) -> Self {
        Self {
            payload,
            hatched: false,
        }
    }
}

impl Script for BaseEgg {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let MessagePayload::TurnOn { from } = msg else {
            return Effect::NoEffect;
        };
        // A pod hatches once. Its tripwire is authored ONCE, but a pod can
        // also be reached by a relayed TurnOn, and a second clutch of grubs
        // from a shell that is already open is not a thing the game does.
        if self.hatched {
            return Effect::NoEffect;
        }

        let (position, rotation) = {
            let v_position = world.borrow::<View<PropPosition>>().unwrap();
            let Ok(position) = v_position.get(entity_id) else {
                // Nowhere to put a payload. Leave the pod unhatched rather
                // than latching a shell that produced nothing.
                return Effect::NoEffect;
            };
            (position.position, position.rotation)
        };
        self.hatched = true;

        // The wall shell is different art, not a pitched floor shell: wpod
        // opens along local -Z while eggcl opens along local +Y.
        let wall_pod = world
            .borrow::<View<PropModelName>>()
            .ok()
            .is_some_and(|models| {
                models.get(entity_id).is_ok_and(|model| {
                    model.0.eq_ignore_ascii_case("wpod") || model.0.eq_ignore_ascii_case("wpodopen")
                })
            });
        let (offset, payload_rotation) = hatch_transform(&self.payload, rotation, wall_pod);
        let fallback_rotation = if wall_pod {
            rotation * Quaternion::from_angle_y(Deg(180.0))
        } else {
            rotation
        };
        let initial_velocity = if matches!(self.payload, EggPayload::Grub) {
            grub_emergence_velocity(position, fallback_rotation, opener_position(world, *from))
        } else {
            vec3(0.0, 0.0, 0.0)
        };
        let mut effects = vec![
            change_to_last_model(world, entity_id),
            Effect::PlaySound {
                handle: AudioHandle::new(),
                name: "pod_exp".into(),
                source: Some(entity_id),
                spatial: true,
            },
        ];
        effects.extend(self.payload.template_names().iter().map(|template_name| {
            Effect::CreateEntityByTemplateName {
                source_entity_id: entity_id,
                template_name: (*template_name).to_owned(),
                position: vec3_to_point3(position + offset),
                orientation: payload_rotation,
                // GrubAI preserves this launch until landing. Goo and swarm
                // motion remains owned by their emitter / flight controller.
                initial_velocity,
                // Ordinary creation, NOT the emitter's launched-projectile
                // mode: that path installs the template's authored projectile
                // sphere, and the Swarm's is zero-radius, so the visible
                // particle cloud would have nothing to shoot at.
                options: CreateEntityOptions::default(),
            }
        }));
        Effect::Multiple(effects)
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &BaseEggState {
                hatched: self.hatched,
            },
            SCRIPT_STATE_KEY,
        )
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: BaseEggState = state.decode(1, SCRIPT_STATE_KEY)?;
        self.hatched = restored.hatched;
        Ok(())
    }
}

/// How far above a goo pod's origin its emitter stands, in world units (one
/// unit is 0.76 m) - clear of the shell's own sphere, so the globs leave the
/// muzzle instead of splashing on the pod that fired them.
const GOO_MUZZLE_CLEARANCE: f32 = 1.2;

/// The lip of an open pod, measured off the shell's own bounds (its top sits
/// 0.78 above the origin), so a hatched creature sits in the shell's mouth.
const SHELL_MOUTH: f32 = 0.8;

/// Place payloads outside the actual model mouth, respecting mission yaw.
/// Goo's projectile-local +Z launch frame must face out of the wall.
/// Its Tweq's additional +Y velocity is authored world-relative and stays up.
fn hatch_transform(
    payload: &EggPayload,
    rotation: Quaternion<f32>,
    wall: bool,
) -> (Vector3<f32>, Quaternion<f32>) {
    let direction = if wall {
        -Vector3::unit_z()
    } else {
        Vector3::unit_y()
    };
    let orientation = if wall && matches!(payload, EggPayload::Goo) {
        rotation * Quaternion::from_angle_y(Deg(180.0))
    } else {
        rotation
    };
    (rotation * direction * payload.lift(), orientation)
}

/// TurnOn identifies the sender, which is usually a relay/tripwire rather
/// than its activator. Honor a direct actor sender; use the live player for
/// relayed mission triggers, whose messages do not preserve that provenance.
fn opener_position(world: &World, from: EntityId) -> Option<Vector3<f32>> {
    let player = world.borrow::<UniqueView<PlayerInfo>>().ok();
    if let Some(player) = &player {
        if from == player.entity_id {
            return Some(player.pos);
        }
    }
    let actor = world
        .borrow::<View<PropAI>>()
        .ok()
        .is_some_and(|v| v.get(from).is_ok())
        || world
            .borrow::<View<PropCreature>>()
            .ok()
            .is_some_and(|v| v.get(from).is_ok());
    if actor {
        if let Some(position) = world
            .borrow::<View<PropPosition>>()
            .ok()
            .and_then(|v| v.get(from).ok().map(|p| p.position))
        {
            return Some(position);
        }
    }
    player.map(|p| p.pos)
}

/// Deliberate fallback, not a recovered retail constant: a short arc toward
/// the opener clears the lip instead of dropping the grub inside the shell.
/// Keep a world-up component even for wall pods; gravity is world-down.
fn grub_emergence_velocity(
    position: Vector3<f32>,
    rotation: Quaternion<f32>,
    target: Option<Vector3<f32>>,
) -> Vector3<f32> {
    let horizontal = |v: Vector3<f32>| vec3(v.x, 0.0, v.z);
    let delta = target
        .map(|p| horizontal(p - position))
        .filter(|v| v.magnitude2() > 0.0001)
        .unwrap_or_else(|| {
            let forward = horizontal(rotation * Vector3::unit_z());
            if forward.magnitude2() > 0.0001 {
                forward
            } else {
                Vector3::unit_z()
            }
        });
    delta.normalize() * 2.4 + Vector3::unit_y() * 3.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, Rotation3};

    fn position(at: Vector3<f32>) -> PropPosition {
        PropPosition {
            position: at,
            cell: 0,
            rotation: Quaternion::from_angle_y(Deg(0.0)),
        }
    }

    #[test]
    fn wall_mouth_and_goo_emission_follow_authored_model_axis_and_yaw() {
        for yaw in [0.0, 90.0, 180.0] {
            let rotation = Quaternion::from_angle_y(Deg(yaw));
            let outward = rotation * -Vector3::unit_z();
            for payload in [EggPayload::Grub, EggPayload::Goo, EggPayload::Swarmer] {
                let (offset, facing) = hatch_transform(&payload, rotation, true);
                assert!((offset - outward * payload.lift()).magnitude() < 0.0001);
                if matches!(payload, EggPayload::Goo) {
                    assert!((facing * Vector3::unit_z() - outward).magnitude() < 0.0001);
                }
                let (floor_offset, floor_facing) = hatch_transform(&payload, rotation, false);
                assert!((floor_offset - Vector3::unit_y() * payload.lift()).magnitude() < 0.0001);
                assert_eq!(floor_facing, rotation);
            }
        }
    }

    #[test]
    fn emergence_aims_horizontally_with_bounded_lift_even_for_wall_pods() {
        for rotation in [
            Quaternion::from_angle_y(Deg(90.0)),
            Quaternion::from_angle_x(Deg(90.0)),
        ] {
            let v = grub_emergence_velocity(
                Vector3::new(0.0, 0.0, 0.0),
                rotation,
                Some(vec3(-10.0, -30.0, 0.0)),
            );
            assert!((v.x + 2.4).abs() < 0.001);
            assert!(v.z.abs() < 0.001);
            assert_eq!(v.y, 3.0);
            for target in [None, Some(vec3(0.0, 10.0, 0.0))] {
                let fallback = grub_emergence_velocity(vec3(0.0, 0.0, 0.0), rotation, target);
                assert!(fallback.magnitude().is_finite());
                assert_eq!(fallback.y, 3.0);
            }
        }
    }

    #[test]
    fn relays_aim_at_player_but_direct_actor_senders_are_honored() {
        let mut world = World::new();
        let player = world.add_entity(());
        let relay = world.add_entity((position(vec3(-10.0, 0.0, 0.0)),));
        let actor = world.add_entity((position(vec3(0.0, 0.0, -10.0)), PropAI("Grub".into())));
        world.add_unique(PlayerInfo {
            pos: vec3(10.0, 0.0, 0.0),
            rotation: Quaternion::from_angle_y(Deg(0.0)),
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
        });
        assert_eq!(opener_position(&world, relay), Some(vec3(10.0, 0.0, 0.0)));
        assert_eq!(opener_position(&world, actor), Some(vec3(0.0, 0.0, -10.0)));
        assert_eq!(opener_position(&world, player), Some(vec3(10.0, 0.0, 0.0)));
    }

    #[test]
    fn each_pod_plays_one_spatial_hatch_sound_and_only_grubs_launch() {
        let mut world = World::new();
        let pod = world.add_entity((position(vec3(0.0, 0.0, 0.0)),));
        let physics = PhysicsWorld::new();
        for payload in [EggPayload::Grub, EggPayload::Goo, EggPayload::Swarmer] {
            let grub = matches!(payload, EggPayload::Grub);
            let mut script = BaseEgg::new(payload);
            let msg = MessagePayload::TurnOn { from: pod };
            let Effect::Multiple(effects) = script.handle_message(pod, &world, &physics, &msg)
            else {
                panic!("hatch effects")
            };
            assert_eq!(effects.iter().filter(|e| matches!(e, Effect::PlaySound { name, spatial: true, source: Some(id), .. } if name == "pod_exp" && *id == pod)).count(), 1);
            for effect in effects {
                if let Effect::CreateEntityByTemplateName {
                    initial_velocity, ..
                } = effect
                {
                    assert_eq!(initial_velocity.magnitude2() > 0.0, grub);
                }
            }
            assert!(matches!(
                script.handle_message(pod, &world, &physics, &msg),
                Effect::NoEffect
            ));
            let state = script.save_state().unwrap();
            assert!(
                state
                    .decode::<BaseEggState>(1, SCRIPT_STATE_KEY)
                    .unwrap()
                    .hatched
            );
        }
    }
}
