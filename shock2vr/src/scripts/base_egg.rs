use cgmath::vec3;
use dark::properties::PropPosition;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

use crate::{
    mission::entity_creator::CreateEntityOptions, physics::PhysicsWorld, util::vec3_to_point3,
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
    /// itself. The cloud is currently decoration: it authors a Venom radius
    /// stim, but the port applies radius stims only from explosions and the
    /// radiation source, so the toxin a goo pod actually deals is the globs'
    /// contact stim.
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
        if !matches!(msg, MessagePayload::TurnOn { .. }) {
            return Effect::NoEffect;
        }
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

        let mut effects = vec![change_to_last_model(world, entity_id)];
        effects.extend(self.payload.template_names().iter().map(|template_name| {
            Effect::CreateEntityByTemplateName {
                source_entity_id: entity_id,
                template_name: (*template_name).to_owned(),
                // Lifted clear of the shell along the POD's own up, not the
                // world's: the wall-mounted variants run these same scripts,
                // and a world-space lift would push their payload into the
                // wall. Inside the shell a goo shot splashes on the pod
                // instead of on the player.
                position: vec3_to_point3(position + rotation * vec3(0.0, PAYLOAD_LIFT, 0.0)),
                // The pod's own facing: a wall pod hatches out of the wall.
                orientation: rotation,
                // The payloads carry their own motion: the emitter's tweq
                // launches the goo, and a creature walks or flies.
                initial_velocity: vec3(0.0, 0.0, 0.0),
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

/// Height above the pod's origin the payload is created at, in world units
/// (one unit is 0.76 m) - just above the shell's own sphere.
const PAYLOAD_LIFT: f32 = 1.2;
