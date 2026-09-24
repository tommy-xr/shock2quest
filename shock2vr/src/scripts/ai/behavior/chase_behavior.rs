use cgmath::Deg;
use dark::motion::MotionQueryItem;
use shipyard::*;

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect,
        ai::steering::{
            self, ChasePlayerSteeringStrategy, CollisionAvoidanceSteeringStrategy,
            PathFollowSteeringStrategy, SteeringOutput, SteeringStrategy,
        },
    },
    time::Time,
};

use super::{Behavior, NextBehavior};

pub struct ChaseBehavior {
    steering_strategy: Box<dyn SteeringStrategy>,
}

impl ChaseBehavior {
    pub fn new() -> ChaseBehavior {
        ChaseBehavior {
            steering_strategy: steering::chained(vec![
                // Path steering leads: the route already avoids static
                // geometry (nav-mesh + edge clearance), and letting whisker
                // avoidance preempt it deadlocks AIs against walls the path
                // was about to turn away from (issue #481). Avoidance guards
                // only the direct-chase fallback below.
                Box::new(PathFollowSteeringStrategy::chase_player()),
                Box::new(
                    CollisionAvoidanceSteeringStrategy::conservative(), /* conservative so we can focus on the chase */
                ),
                Box::new(ChasePlayerSteeringStrategy),
            ]),
        }
    }
}

impl Behavior for ChaseBehavior {
    fn name(&self) -> &'static str {
        "Chase"
    }

    fn turn_speed(&self) -> Deg<f32> {
        Deg(360.0)
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        self.steering_strategy
            .steer(current_heading, world, physics, entity_id, time)
    }

    fn animation(self: &ChaseBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("locomote"),
            MotionQueryItem::new("locourgent").optional(),
            // A value-less direction also matches backward (4), selecting
            // mbtback for droids even while steering toward the target.
            MotionQueryItem::with_value("direction", 0).optional(),
        ]
    }

    fn is_locomotion(&self) -> bool {
        true
    }

    fn next_behavior(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> NextBehavior {
        // Reaching a heard/remembered location ends pursuit even before
        // alertness decays. Otherwise root motion walks past the empty goal.
        let remembered = world
            .borrow::<View<crate::runtime_props::RuntimePropAITargetAwareness>>()
            .ok()
            .and_then(|v| v.get(entity_id).ok().copied());
        if let Some(awareness) = remembered {
            if !awareness.has_line_of_sight
                && super::SearchBehavior::at_goal(world, entity_id, awareness.last_known_pos)
            {
                return NextBehavior::Next(Box::new(std::cell::RefCell::new(
                    super::SearchBehavior::new(awareness.last_known_pos),
                )));
            }
        }
        match super::attack_behavior_for_distance(world, physics, entity_id) {
            Some(behavior) => NextBehavior::Next(behavior),
            None => NextBehavior::Stay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::{
        motion::{MotionClip, MotionDB, MotionQuery},
        properties::{PropCreature, PropMotionActorTags, PropSymName},
    };

    #[test]
    #[ignore = "requires System Shock 2 game assets"]
    fn chase_clip_selection_across_creature_families() {
        let mut reader = crate::data_files::open_data_file("motiondb.bin").unwrap();
        let db = MotionDB::read(&mut reader);
        let mut reader = crate::data_files::open_data_file("shock2.gam").unwrap();
        let (properties, links, links_with_data) = dark::properties::get();
        let gamesys = dark::gamesys::read(&mut reader, &links, &links_with_data, &properties);
        let mut world = World::new();
        gamesys
            .entity_info
            .initialize_world_with_entities(&mut world, Default::default(), |_| true);
        let (names, creatures, actor_tags) = world
            .borrow::<(
                View<PropSymName>,
                View<PropCreature>,
                View<PropMotionActorTags>,
            )>()
            .unwrap();
        for name in [
            "OG-Pipe",
            "OG-Shotgun",
            "OG-Grenade",
            "Blue Monkey",
            "Red Monkey",
            "Arachnid",
            "Baby Arachnid",
            "Overlord",
            "Greater Over.",
            "Midwife",
            "Assassin",
            "Rumbler",
            "Maintenance",
            "Security",
            "Assault",
            "Protocol Droid",
        ] {
            let entity = names.iter().with_id().find(|(_, n)| n.0 == name).unwrap().0;
            let actor = crate::creature::get_creature_definition(creatures.get(entity).unwrap().0)
                .unwrap()
                .actor_type
                .clone() as u32;
            let authored_tags = &actor_tags.get(entity).unwrap().tags;
            let mut tags = ChaseBehavior::new().animation();
            // Match apply_animation_by_schema, including inherited actor tags.
            tags.extend(
                authored_tags
                    .iter()
                    .map(|tag| MotionQueryItem::new(tag).optional()),
            );
            let mut after = db.query_all(MotionQuery::new(actor, tags.clone()));
            let mut before_tags = tags;
            *before_tags
                .iter_mut()
                .find(|tag| tag.tag_name() == "direction")
                .unwrap() = MotionQueryItem::new("direction").optional();
            let mut before = db.query_all(MotionQuery::new(actor, before_tags));
            // Branch traversal order is unspecified. Preserve duplicate
            // candidates when comparing, since they affect selection frequency.
            before.sort();
            after.sort();
            println!("{name} ({authored_tags:?}): {before:?} -> {after:?}");
            assert!(!after.is_empty(), "{name} chase must resolve a motion");
            let mut distinct = after.clone();
            distinct.dedup();
            match actor {
                2 => assert_eq!(distinct, ["mbtwlkls", "mbtwlkrs"]),
                4 => assert_eq!(distinct, ["bs111010", "bs111011"]),
                _ => assert_eq!(before, after, "{name} chase selection must be preserved"),
            }
            for clip_name in after {
                let mut reader =
                    crate::data_files::open_data_file(&format!("res/motions/{clip_name}_.mc"))
                        .unwrap();
                let clip = MotionClip::read(&mut reader, db.get_mps_motions(clip_name.clone()));
                let displacement =
                    clip.root_positions.last().unwrap() - clip.root_positions.first().unwrap();
                // ovlmove is authored nearly in place in both queries; this
                // change preserves its selection, not its locomotion speed.
                if actor == 3 {
                    continue;
                }
                // The mission maps local -X root motion to facing-forward +Z.
                assert!(
                    displacement.x < -0.1,
                    "{name} chase selected {clip_name}, which moves {displacement:?}"
                );
            }
        }
    }
}
