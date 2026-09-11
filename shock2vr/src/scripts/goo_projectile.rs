use dark::properties::PropTemplateId;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, internal_collision_type::terminal_impact_effects};

/// The glob a goo egg's emitter lobs - retail `GooProjectile`, "egg splat".
///
/// Its `PropCollisionType` is a plain BOUNCE (the gamesys deliberately drops
/// the projectile archetype's `SLAY_ON_IMPACT`), because retail leaves the
/// terminal impact to this script's Slay Result instead. So the shared
/// collision handler stays inert on a goo shot and the splat is applied here:
/// slay on contact, which delivers the authored contact venom stim to whatever
/// was hit and leaves the authored corpse spang behind.
pub struct GooProjectile {
    impact_handled: bool,
}

impl GooProjectile {
    pub fn new() -> Self {
        Self {
            impact_handled: false,
        }
    }
}

impl Script for GooProjectile {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let MessagePayload::Collided { with, contact } = msg else {
            return Effect::NoEffect;
        };
        // Globs from one volley pass through each other. The emitter fires
        // all four straight up from a single point 100 ms apart, so a fresh
        // one overlaps the previous one still leaving the muzzle; without
        // this the volley annihilates itself at the pod and no venom ever
        // reaches the player. Deliberately narrow - only another glob, not
        // every projectile in the air.
        if is_same_archetype(world, entity_id, *with) {
            return Effect::NoEffect;
        }
        // One splat per glob: a single contact can queue several messages
        // before the slay is applied.
        if self.impact_handled {
            return Effect::NoEffect;
        }
        self.impact_handled = true;
        terminal_impact_effects(world, physics, entity_id, *with, *contact, true, true)
    }
}

/// Whether two entities were created from the same template - i.e. `other` is
/// another glob from this same volley.
fn is_same_archetype(world: &World, entity_id: EntityId, other: EntityId) -> bool {
    let templates = world.borrow::<View<PropTemplateId>>().unwrap();
    match (templates.get(entity_id), templates.get(other)) {
        (Ok(mine), Ok(theirs)) => mine.template_id == theirs.template_id,
        _ => false,
    }
}
