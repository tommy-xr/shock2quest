use dark::properties::{Gravity, PropMapLoc, PropRoomGravity};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct CoreRoom {
    gravity_adjustment: Option<PropRoomGravity>,
    /// The room's automap location (`PropMapLoc`), if it is mapped. The player
    /// entering the room reveals this location (projects/flat-ui-panels.md §5).
    map_location: Option<i32>,
}
impl CoreRoom {
    pub fn new() -> CoreRoom {
        CoreRoom {
            gravity_adjustment: None,
            map_location: None,
        }
    }
}

/// Whether `entity` is the player (room sensors forward every intersect;
/// only the player explores the map).
fn is_player(world: &World, entity: EntityId) -> bool {
    world
        .borrow::<UniqueView<crate::mission::PlayerInfo>>()
        .map(|player| player.entity_id == entity)
        .unwrap_or(false)
}

impl Script for CoreRoom {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_prop_grav = world.borrow::<View<PropRoomGravity>>().unwrap();
        self.gravity_adjustment = v_prop_grav.get(entity_id).ok().cloned();
        let v_map_loc = world.borrow::<View<PropMapLoc>>().unwrap();
        self.map_location = v_map_loc.get(entity_id).ok().map(|loc| loc.0);

        Effect::NoEffect
    }
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let gravity_effect = if self.gravity_adjustment.is_some() {
            match msg {
                MessagePayload::SensorBeginIntersect { with } => {
                    let grav = self.gravity_adjustment.clone().unwrap();

                    match grav.0 {
                        Gravity::Reset => Effect::ResetGravity { entity_id: *with },
                        Gravity::Set(pct) => Effect::SetGravity {
                            entity_id: *with,
                            gravity_percent: pct,
                        },
                    }
                }
                MessagePayload::SensorEndIntersect { with } => {
                    Effect::ResetGravity { entity_id: *with }
                }
                _ => Effect::NoEffect,
            }
        } else {
            Effect::NoEffect
        };

        // The player entering a mapped room explores its automap location.
        // NOTE: this is a deliberate player-friendly *superset* of the
        // original engine, which marks a location explored only when the
        // automap actually draws the player's position inside it - here every
        // mapped room entered is revealed immediately. Don't "fix" this in
        // either direction without deciding that tradeoff on purpose.
        let map_effect = match (msg, self.map_location) {
            (MessagePayload::SensorBeginIntersect { with }, Some(location))
                if is_player(world, *with) =>
            {
                Effect::RevealMapLocation { location }
            }
            _ => Effect::NoEffect,
        };

        Effect::combine(vec![gravity_effect, map_effect])
    }
}
