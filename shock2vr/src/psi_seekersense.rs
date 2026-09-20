//! Remote Pattern Detection reveals world loot and searchable containers.
use crate::{mission::PlayerInfo, psi::ActivePsiPowers, psi_sense::PsiSenseContact};
use cgmath::InnerSpace;
use dark::properties::{
    FrobFlag, PropAI, PropContainDimensions, PropFrobInfo, PropHitPoints, PropInventoryDimensions,
    PropPosition, PropRenderType, RenderType,
};
use shipyard::{Get, IntoIter, IntoWithId, Unique, UniqueView, UniqueViewMut, View, World};

pub const POWER: i32 = -3158;
// Port tuning: use the same 80 Dark-unit reach as the Radar infrastructure.
const RANGE: f32 = 80.0 / dark::SCALE_FACTOR;
const MAX_CONTACTS: usize = 64;

#[derive(Default, Unique)]
pub struct Seekersense {
    phase: f32,
    pub contacts: Vec<PsiSenseContact>,
}

pub fn update(world: &World, dt: f32) {
    let active = world
        .borrow::<UniqueView<ActivePsiPowers>>()
        .is_ok_and(|p| p.is_active(POWER));
    let mut sense = world.borrow::<UniqueViewMut<Seekersense>>().unwrap();
    sense.contacts.clear();
    if !active {
        sense.phase = 0.0;
        return;
    }
    sense.phase = (sense.phase + dt).rem_euclid(2.0);
    let strength = 0.8 + 0.2 * (sense.phase * std::f32::consts::PI).sin();
    let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let (positions, frobs, inventory, containers, ais, health, render) = world
        .borrow::<(
            View<PropPosition>,
            View<PropFrobInfo>,
            View<PropInventoryDimensions>,
            View<PropContainDimensions>,
            View<PropAI>,
            View<PropHitPoints>,
            View<PropRenderType>,
        )>()
        .unwrap();
    for (id, (position, frob)) in (&positions, &frobs).iter().with_id() {
        if id == player.entity_id
            || id == player.inventory_entity_id
            || Some(id) == player.left_hand_entity_id
            || Some(id) == player.right_hand_entity_id
            || !crate::util::has_refs(world, id)
        {
            continue;
        }
        if render
            .get(id)
            .is_ok_and(|r| matches!(r.0, RenderType::NoRender | RenderType::EditorOnly))
        {
            continue;
        }
        let living_ai = ais.contains(id) && health.get(id).map_or(true, |h| h.hit_points > 0);
        let portable = inventory.get(id).is_ok_and(|d| d.width > 0 && d.height > 0);
        let searchable = containers
            .get(id)
            .is_ok_and(|d| d.width > 0 && d.height > 0);
        if !interesting(portable, searchable, living_ai, frob.world_action) {
            continue;
        }
        let distance = (position.position - player.pos).magnitude();
        if distance > RANGE {
            continue;
        }
        sense.contacts.push(PsiSenseContact {
            entity_id: id.inner(),
            position: position.position.into(),
            distance,
            strength,
        });
    }
    sense.contacts.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then(a.entity_id.cmp(&b.entity_id))
    });
    sense.contacts.truncate(MAX_CONTACTS);
}

fn interesting(portable: bool, searchable: bool, living_ai: bool, action: FrobFlag) -> bool {
    !living_ai
        && (portable || searchable)
        && !action.contains(FrobFlag::IGNORE)
        && action.intersects(FrobFlag::MOVE | FrobFlag::SCRIPT | FrobFlag::DELETE)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loot_and_searchable_corpses_exclude_live_creatures_and_world_controls() {
        assert!(interesting(true, false, false, FrobFlag::MOVE));
        assert!(interesting(true, false, false, FrobFlag::SCRIPT));
        assert!(interesting(false, true, false, FrobFlag::SCRIPT));
        assert!(!interesting(false, true, true, FrobFlag::SCRIPT));
        assert!(!interesting(false, false, false, FrobFlag::SCRIPT));
        assert!(!interesting(
            true,
            false,
            false,
            FrobFlag::MOVE | FrobFlag::IGNORE
        ));
        assert!(!interesting(true, false, false, FrobFlag::empty()));
    }
}
