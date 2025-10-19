//
// update_teleported_state
//
// System that updates the 'PropTeleported' property - this is used for us to know if an entity
// has just teleported (which affects triggering tripwires)

// TODO: Need to bring time as a unique view

// pub fn update_teleported_state(v_teleported: ViewMut<dark::properties::PropTeleported>) {
//     let mut ents_to_remove = Vec::new();
//     for (id, door) in (&mut v_teleported).iter().with_id() {
//         door.countdown_timer -= time.elapsed.as_secs_f32();

//         if door.countdown_timer < 0.0 {
//             ents_to_remove.push(id);
//         }
//     }

//     for id in ents_to_remove {
//         trace!("remove v_teleported for entity: {:?}", &id);
//         v_teleported.remove(id);
//     }
// }
