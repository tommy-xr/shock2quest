use std::sync::{Arc, Mutex};

use rapier3d::prelude::{ColliderSet, ContactPair, EventHandler, Real, RigidBodySet};
use shipyard::EntityId;

use super::util::{npoint_to_cgvec, nvec_to_cgmath};

pub struct PhysicsEvents {
    queued_events: Mutex<Vec<super::CollisionEvent>>,
    /// The level's per-triangle materials, so a contact against world geometry
    /// can name the surface it touched. `None` until a level is added (debug
    /// scenes never add one).
    level_surface_materials: Mutex<Option<Arc<super::LevelSurfaceMaterials>>>,
}

impl PhysicsEvents {
    pub fn new() -> PhysicsEvents {
        PhysicsEvents {
            queued_events: Mutex::new(vec![]),
            level_surface_materials: Mutex::new(None),
        }
    }

    pub fn set_level_surface_materials(&self, materials: Arc<super::LevelSurfaceMaterials>) {
        *self.level_surface_materials.lock().unwrap() = Some(materials);
    }

    pub fn get_and_clear_events(&self) -> Vec<super::CollisionEvent> {
        let mut events = self.queued_events.lock().unwrap();
        let ret = events.clone();
        *events = vec![];
        ret
    }
}

impl EventHandler for PhysicsEvents {
    fn handle_collision_event(
        &self,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        event: rapier3d::prelude::CollisionEvent,
        maybe_contact_pair: Option<&ContactPair>,
    ) {
        if let Some(contact_pair) = maybe_contact_pair {
            let maybe_entity1_id = _colliders
                .get(contact_pair.collider1)
                .and_then(|c| EntityId::from_inner(c.user_data as u64));
            let maybe_entity2_id = _colliders
                .get(contact_pair.collider2)
                .and_then(|c| EntityId::from_inner(c.user_data as u64));

            if maybe_entity1_id.is_none() || maybe_entity2_id.is_none() {
                return;
            }

            match &event {
                rapier3d::prelude::CollisionEvent::Started(_, _, _) => {
                    // Solver contacts already contain the midpoint in world
                    // space. Pick the deepest active contact and keep its
                    // manifold normal, which Rapier orients collider1 ->
                    // collider2. Some synthetic/degenerate contacts may have
                    // no solver point, so the payload remains optional.
                    let level_surface_materials = self.level_surface_materials.lock().unwrap();
                    let contact = contact_pair
                        .manifolds
                        .iter()
                        .flat_map(|manifold| {
                            manifold
                                .data
                                .solver_contacts
                                .iter()
                                .map(move |contact| (manifold, contact))
                        })
                        .min_by(|(_, a), (_, b)| a.dist.total_cmp(&b.dist))
                        .map(|(manifold, contact)| super::CollisionContact {
                            point: npoint_to_cgvec(contact.point),
                            normal: nvec_to_cgmath(manifold.data.normal),
                            // For a composite shape (the level trimesh is the
                            // only one either side of a contact can be), the
                            // manifold's subshape IS the triangle index - so a
                            // wrench on a carpeted floor knows it hit fabric.
                            surface_material: level_surface_materials.as_ref().and_then(
                                |materials| {
                                    materials
                                        .material_for_triangle_on(
                                            contact_pair.collider1,
                                            manifold.subshape1,
                                        )
                                        .or_else(|| {
                                            materials.material_for_triangle_on(
                                                contact_pair.collider2,
                                                manifold.subshape2,
                                            )
                                        })
                                },
                            ),
                        });
                    self.queued_events.lock().unwrap().push(
                        super::CollisionEvent::CollisionStarted {
                            entity1_id: maybe_entity1_id.unwrap(),
                            entity2_id: maybe_entity2_id.unwrap(),
                            contact,
                        },
                    )
                }
                _ => {}
            }
        }
    }

    fn handle_contact_force_event(
        &self,
        _dt: Real,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        _contact_pair: &ContactPair,
        _total_force_magnitude: Real,
    ) {
        // println!(
        //     "contact_force_event: {:?} {:?} mag: {:?}",
        //     _contact_pair.collider1, _contact_pair.collider2, _total_force_magnitude
        // );
    }
}
