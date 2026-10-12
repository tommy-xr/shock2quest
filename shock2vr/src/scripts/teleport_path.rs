use std::time::Duration;

use dark::properties::{Link, PropPosition};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::get_first_link_of_type,
};

const INTERVAL: Duration = Duration::from_millis(1500);
const STATE_KEY: &str = "shock2vr.teleport_path";

/// Retail TeleportMe copies the current marker's complete pose, then advances
/// the link and schedules a new callback 1.5 seconds later. Keep the cursor
/// private rather than rewriting authored links; save hydration remaps it.
#[derive(Serialize, Deserialize)]
pub struct TeleportPath {
    remaining: Duration,
    marker: Option<u64>,
}

impl TeleportPath {
    pub fn new() -> Self {
        Self {
            remaining: INTERVAL,
            marker: None,
        }
    }
}

impl Script for TeleportPath {
    fn initialize(&mut self, entity: EntityId, world: &World) -> Effect {
        self.marker = get_first_link_of_type(world, entity, Link::TelePath).map(EntityId::inner);
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        self.remaining = self.remaining.saturating_sub(time.elapsed);
        if !self.remaining.is_zero() {
            return Effect::NoEffect;
        }
        // Retail rearms once per callback, even after a long frame.
        self.remaining = INTERVAL;
        let Some(marker) = self.marker.and_then(EntityId::from_inner) else {
            return Effect::NoEffect;
        };
        let positions = world.borrow::<View<PropPosition>>().unwrap();
        let effect = positions
            .get(marker)
            .map(|pose| Effect::SetPositionRotation {
                entity_id,
                position: pose.position,
                rotation: pose.rotation,
            })
            .unwrap_or(Effect::NoEffect);
        // An unconnected endpoint remains the destination, as in retail.
        if let Some(next) = get_first_link_of_type(world, marker, Link::TelePath) {
            self.marker = Some(next.inner());
        }
        effect
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, self, STATE_KEY)
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let mut restored: Self = state.decode(1, STATE_KEY)?;
        restored.marker = match restored.marker {
            Some(marker) => match context.remap_entity(marker) {
                Ok(marker) => Some(marker.inner()),
                Err(ScriptStateError::MissingEntityReference(_)) => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        *self = restored;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, Quaternion, Rotation3, vec3};
    use dark::properties::{Links, ToLink, WrappedEntityId};
    use std::collections::HashMap;

    fn link(target: EntityId) -> Links {
        Links {
            to_links: vec![ToLink {
                to_template_id: 1,
                to_entity_id: Some(WrappedEntityId(target)),
                link: Link::TelePath,
            }],
        }
    }

    fn step(script: &mut dyn Script, world: &World, entity: EntityId, millis: u64) -> Effect {
        script.update(
            entity,
            world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_millis(millis),
                total: Duration::ZERO,
            },
        )
    }

    #[test]
    fn registered_script_copies_current_marker_pose_before_advancing_and_wraps() {
        let mut world = World::new();
        let a = world.add_entity((PropPosition {
            position: vec3(1.0, 2.0, 3.0),
            rotation: Quaternion::from_angle_y(Deg(45.0)),
            cell: 0,
        },));
        let b = world.add_entity((
            PropPosition {
                position: vec3(4.0, 2.0, 6.0),
                rotation: Quaternion::from_angle_y(Deg(90.0)),
                cell: 0,
            },
            link(a),
        ));
        world.add_component(a, link(b));
        let entity = world.add_entity((link(a),));
        let mut script = super::super::ScriptWorld::create_script("teleportpath".to_owned());
        script.initialize(entity, &world);
        assert!(matches!(
            step(script.as_mut(), &world, entity, 1499),
            Effect::NoEffect
        ));
        for (millis, marker) in [(1, a), (1500, b), (10000, a), (1500, b)] {
            let pose = world.borrow::<View<PropPosition>>().unwrap();
            let pose = pose.get(marker).unwrap();
            match step(script.as_mut(), &world, entity, millis) {
                Effect::SetPositionRotation {
                    entity_id,
                    position,
                    rotation,
                } => {
                    assert_eq!(entity_id, entity);
                    assert_eq!(position, pose.position);
                    assert_eq!(rotation, pose.rotation);
                }
                effect => panic!("expected instantaneous marker pose, got {effect:?}"),
            }
        }
    }

    #[test]
    fn save_remaps_cursor_and_preserves_timer_without_reinitializing() {
        let mut world = World::new();
        let old = world.add_entity(());
        let new = world.add_entity((PropPosition {
            position: vec3(7.0, 8.0, 9.0),
            rotation: Quaternion::from_angle_y(Deg(135.0)),
            cell: 0,
        },));
        let entity = world.add_entity(());
        let original = TeleportPath {
            remaining: Duration::from_millis(600),
            marker: Some(old.inner()),
        };
        let state =
            serde_json::from_str(&serde_json::to_string(&original.save_state().unwrap()).unwrap())
                .unwrap();
        let mut restored = TeleportPath::new();
        restored
            .restore_state(
                &state,
                &ScriptRestoreContext::new(&HashMap::from([(old, new)])),
            )
            .unwrap();
        restored.initialize_after_hydration(entity, &world, true);
        assert!(matches!(
            step(&mut restored, &world, entity, 599),
            Effect::NoEffect
        ));
        assert!(
            matches!(step(&mut restored, &world, entity, 1), Effect::SetPositionRotation { position, .. } if position == vec3(7.0, 8.0, 9.0))
        );
        // Missing onward link keeps the endpoint; missing saved target disables it.
        assert_eq!(restored.marker, Some(new.inner()));
        restored
            .restore_state(&state, &ScriptRestoreContext::new(&HashMap::new()))
            .unwrap();
        assert!(matches!(
            step(&mut restored, &world, entity, 600),
            Effect::NoEffect
        ));
    }

    #[test]
    fn unlinked_and_deleted_markers_are_inert() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let mut script = TeleportPath::new();
        script.initialize(entity, &world);
        assert!(matches!(
            step(&mut script, &world, entity, 1500),
            Effect::NoEffect
        ));
        let marker = world.add_entity(());
        script.marker = Some(marker.inner());
        world.delete_entity(marker);
        assert!(matches!(
            step(&mut script, &world, entity, 1500),
            Effect::NoEffect
        ));
    }
}
