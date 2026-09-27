//! Quantum Relocation uses the authored world marker, not an aim-ray destination.
//! Keeping its position in a real entity gives ordinary same-level saves their
//! existing entity/particle persistence; EndLevel removes it before the snapshot.
use cgmath::Vector3;
use dark::properties::{PropPosition, PropTemplateId};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};

pub const POWER: i32 = -1018;
/// The Teleport link on power -1018 targets this gamesys archetype.
pub const MARKER: i32 = -1109;

pub fn marker(world: &World) -> Option<(EntityId, Vector3<f32>)> {
    let (templates, positions) = world
        .borrow::<(View<PropTemplateId>, View<PropPosition>)>()
        .ok()?;
    templates.iter().with_id().find_map(|(entity, template)| {
        (template.template_id == MARKER)
            .then(|| positions.get(entity).ok().map(|p| (entity, p.position)))
            .flatten()
    })
}
