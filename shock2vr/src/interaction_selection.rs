//! Shared selection rules for authored decorative shells and control overlays.
use crate::{
    physics::{PhysicsWorld, RayCastResult},
    util::resolve_proxy_entity,
};
use cgmath::InnerSpace;
use dark::properties::{PropFrobInfo, PropHUDSelect};
use shipyard::{EntityId, Get, View, World};

/// HUD-hidden computer shells and their invisible frob overlays are authored
/// at the same origin. Keep the tolerance tight enough that a nearby control
/// cannot become the shell's accidental target.
const HUD_OVERLAY_POSITION_EPSILON: f32 = 0.05;
/// The selectable overlay may sit just inside the decorative shell's collider,
/// but it must still be part of the same visible surface rather than an object
/// farther into the room.
const HUD_OVERLAY_SURFACE_GAP: f32 = 0.5;

/// The caller supplies the immediate combined-world hit after excluding the shell.
/// Never search past another blocker or compare proxy origins with their parents.
pub(crate) fn hud_overlay_target(
    world: &World,
    physics: &PhysicsWorld,
    first: &RayCastResult,
    second: &RayCastResult,
) -> Option<EntityId> {
    let first_raw = first.maybe_entity_id?;
    let second_raw = second.maybe_entity_id?;
    let first_entity = resolve_proxy_entity(world, first_raw);
    let second_entity = resolve_proxy_entity(world, second_raw);
    if !has_hud_select(world, first_entity, false) || !is_frobbable(world, second_entity) {
        return None;
    }
    if first_raw != first_entity
        || second_raw != second_entity
        || !has_hud_select(world, second_entity, true)
    {
        return None;
    }
    let first_position = physics.get_position(first.maybe_rigid_body_handle?)?;
    let second_position = physics.get_position(second.maybe_rigid_body_handle?)?;
    let co_located = (first_position - second_position).magnitude2()
        <= HUD_OVERLAY_POSITION_EPSILON * HUD_OVERLAY_POSITION_EPSILON;
    let close_surface = (first.hit_point - second.hit_point).magnitude2()
        <= HUD_OVERLAY_SURFACE_GAP * HUD_OVERLAY_SURFACE_GAP;
    (co_located && close_surface).then_some(second_entity)
}

pub(crate) fn has_hud_select(world: &World, entity_id: EntityId, expected: bool) -> bool {
    world
        .borrow::<View<PropHUDSelect>>()
        .map(|v| v.get(entity_id).is_ok_and(|select| select.0 == expected))
        .unwrap_or(false)
}

/// Whether an entity is worth highlighting / interacting with (it has frob
/// info), which excludes plain world geometry the ray also hits. Some scripted
/// world objects (including StdDoor leaves) have an empty world-action mask but
/// still consume Frob messages, so property presence is the compatibility
/// boundary here.
pub(crate) fn is_frobbable(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropFrobInfo>>()
        .map(|v| v.get(entity_id).is_ok())
        .unwrap_or(false)
}
