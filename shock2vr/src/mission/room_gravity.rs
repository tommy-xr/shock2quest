use std::collections::HashMap;

use shipyard::EntityId;

/// Room sensors overlap at boundaries, and several sensors can share the same
/// authored room archetype. Keep their contributions distinct so an old room's
/// exit cannot cancel the room just entered. Recreated sensors rebuild this
/// transient occupancy after loading a mission or save.
#[derive(Default)]
pub(super) struct RoomGravity {
    active: HashMap<EntityId, Vec<(EntityId, f32)>>,
}

impl RoomGravity {
    pub(super) fn update(
        &mut self,
        entity: EntityId,
        sensor: EntityId,
        gravity: Option<f32>,
    ) -> Option<f32> {
        if let Some(gravity) = gravity {
            let rooms = self.active.entry(entity).or_default();
            // Duplicate notifications do not add a second contribution.
            if !rooms.iter().any(|(source, _)| *source == sensor) {
                rooms.push((sensor, gravity));
            }
            return rooms.last().map(|(_, gravity)| *gravity);
        }

        let rooms = self.active.get_mut(&entity)?;
        let index = rooms.iter().position(|(source, _)| *source == sensor)?;
        rooms.remove(index);
        let gravity = rooms.last().map_or(1.0, |(_, gravity)| *gravity);
        if rooms.is_empty() {
            self.active.remove(&entity);
        }
        Some(gravity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> EntityId {
        EntityId::from_inner(value).unwrap()
    }

    #[test]
    fn room_gravity_exit_preserves_the_newly_entered_room() {
        let mut gravity = RoomGravity::default();
        assert_eq!(gravity.update(id(1), id(10), Some(0.05)), Some(0.05));
        assert_eq!(gravity.update(id(1), id(11), Some(0.01)), Some(0.01));
        assert_eq!(gravity.update(id(1), id(10), None), Some(0.01));
        assert_eq!(gravity.update(id(1), id(11), None), Some(1.0));
        assert!(gravity.active.is_empty());
    }

    #[test]
    fn room_gravity_shared_archetypes_and_backtracking_keep_each_sensor() {
        let mut gravity = RoomGravity::default();
        assert_eq!(gravity.update(id(1), id(10), Some(0.05)), Some(0.05));
        assert_eq!(gravity.update(id(1), id(11), Some(0.01)), Some(0.01));
        // Another volume using the same 1% room archetype is independent.
        assert_eq!(gravity.update(id(1), id(12), Some(0.01)), Some(0.01));
        assert_eq!(gravity.update(id(1), id(11), None), Some(0.01));
        assert_eq!(gravity.update(id(1), id(12), None), Some(0.05));
        assert_eq!(gravity.update(id(1), id(10), None), Some(1.0));
    }

    #[test]
    fn room_gravity_duplicate_edges_and_other_entities_do_not_cancel_occupancy() {
        let mut gravity = RoomGravity::default();
        gravity.update(id(1), id(10), Some(-0.3));
        gravity.update(id(1), id(10), Some(-0.3));
        assert_eq!(gravity.update(id(2), id(10), Some(0.0)), Some(0.0));
        assert_eq!(gravity.update(id(1), id(99), None), None);
        assert_eq!(gravity.update(id(1), id(10), None), Some(1.0));
        assert_eq!(gravity.update(id(1), id(10), None), None);
        assert_eq!(gravity.update(id(2), id(10), None), Some(1.0));
    }
}
