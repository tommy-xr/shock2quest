use dark::properties::{PropPhysDimensions, PropPhysType};
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
        // The original's point-vs-not-special rule: two models that both set
        // `point_vs_not_special` and are both non-special never collide with
        // each other. The Projectile archetype sets both bits, which is what
        // stops a stream of emitted globs from detonating on one another
        // before it has cleared the muzzle.
        if passes_through(world, entity_id, *with) {
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

/// The original's point-vs-not-special collision filter, for one pair.
fn passes_through(world: &World, entity_id: EntityId, other: EntityId) -> bool {
    let dimensions = world.borrow::<View<PropPhysDimensions>>().unwrap();
    let phys_types = world.borrow::<View<PropPhysType>>().unwrap();
    let point_vs_not_special = |entity| {
        dimensions
            .get(entity)
            .is_ok_and(|dimensions| dimensions.point_vs_not_special != 0)
    };
    let special = |entity| {
        phys_types
            .get(entity)
            .is_ok_and(|phys_type| phys_type.is_special)
    };
    point_vs_not_special(entity_id)
        && point_vs_not_special(other)
        && !special(entity_id)
        && !special(other)
}
