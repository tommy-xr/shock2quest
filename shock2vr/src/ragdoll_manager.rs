use crate::runtime_props::RuntimePropRagdoll;
use shipyard::EntityId;
use std::collections::HashMap;

/// RagdollManager manages ragdoll entities and their lifecycle
/// Similar to HitBoxManager but for ragdoll physics systems
pub struct RagdollManager {
    /// Maps ragdoll entity ID to its ragdoll data
    id_to_ragdoll: HashMap<EntityId, RuntimePropRagdoll>,
}

impl RagdollManager {
    pub fn new() -> Self {
        Self {
            id_to_ragdoll: HashMap::new(),
        }
    }

    /// Register a new ragdoll entity
    pub fn register_ragdoll(&mut self, entity_id: EntityId, ragdoll: RuntimePropRagdoll) {
        self.id_to_ragdoll.insert(entity_id, ragdoll);
    }

    /// Remove a ragdoll entity
    pub fn remove_ragdoll(&mut self, entity_id: EntityId) -> Option<RuntimePropRagdoll> {
        self.id_to_ragdoll.remove(&entity_id)
    }

    /// Get ragdoll data for an entity
    pub fn get_ragdoll(&self, entity_id: EntityId) -> Option<&RuntimePropRagdoll> {
        self.id_to_ragdoll.get(&entity_id)
    }

    /// Get mutable ragdoll data for an entity
    pub fn get_ragdoll_mut(&mut self, entity_id: EntityId) -> Option<&mut RuntimePropRagdoll> {
        self.id_to_ragdoll.get_mut(&entity_id)
    }

    /// Get all ragdoll entities
    pub fn get_all_ragdolls(&self) -> &HashMap<EntityId, RuntimePropRagdoll> {
        &self.id_to_ragdoll
    }

    /// Get all ragdoll entities (mutable)
    pub fn get_all_ragdolls_mut(&mut self) -> &mut HashMap<EntityId, RuntimePropRagdoll> {
        &mut self.id_to_ragdoll
    }

    /// Check if an entity has a ragdoll
    pub fn has_ragdoll(&self, entity_id: EntityId) -> bool {
        self.id_to_ragdoll.contains_key(&entity_id)
    }

    /// Clear all ragdolls (for cleanup)
    pub fn clear(&mut self) {
        self.id_to_ragdoll.clear();
    }
}
