///
/// creature_definitions.rs
///
/// Metadata for different creature types in SystemShock 2.
/// I'm not sure where this data actually comes from in shock2.gam / metadata...
/// Most of it is taken from Hardkern's work here:
/// https://github.com/Kernvirus/SystemShock2VR/blob/5f0f7d054e79c2e36d9661f4ca62ab95ae69de0b/Assets/Scripts/Editor/DarkEngine/Animation/CreatureDefinitions.cs#L29
///
/// If we can find where this information is available, we can slowly replace this constant data
/// with dynamically loaded data.
use std::{collections::HashMap, sync::Arc};

use cgmath::{Vector3, vec3};

use dark::{SCALE_FACTOR, properties::PropCreature};
use num_derive::{FromPrimitive, ToPrimitive};
use once_cell::sync::Lazy;
use shipyard::{EntityId, Get, View, World};

use super::HitBoxType;

#[derive(FromPrimitive, ToPrimitive, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActorType {
    Human = 0,
    PlayerLimb = 1,
    Droid = 2,
    Overlord = 3,
    Arachnid = 4,
}

/// Articulation limit for a single skeleton joint, used when building ragdolls.
/// `cone` is a symmetric half-angle (radians) applied to each angular axis of the
/// limb's ball joint, measured from the bind/rest pose.
#[derive(Clone, Copy, Debug)]
pub struct JointLimit {
    pub cone: f32,
}

impl JointLimit {
    pub fn cone(cone: f32) -> Self {
        Self { cone }
    }
}

pub struct CreatureDefinition {
    pub actor_type: ActorType,
    pub physics_offset_height: f32,
    pub bounding_size: Vector3<f32>,
    joint_map: Vec<i32>,
    pub hit_boxes: Arc<HashMap<u32, HitBoxType>>,
    pub joint_limits: Arc<HashMap<u32, JointLimit>>,
}

impl CreatureDefinition {
    pub fn get_mapped_joint(&self, joint_id: u32) -> Option<u32> {
        self.joint_map
            .get(joint_id as usize)
            .filter(|v| **v >= 0)
            .map(|v| *v as u32)
    }

    pub fn get_hitbox_type(&self, joint_id: u32) -> Option<HitBoxType> {
        self.hit_boxes.get(&joint_id).cloned()
    }
}

pub const HUMAN_HEIGHT: f32 = 6.5 / SCALE_FACTOR;
pub const HUMAN_WIDTH: f32 = 3.5 / SCALE_FACTOR;
pub const HUMAN_PHYS_OFFSET: f32 = 1.0 / SCALE_FACTOR;

pub const DROID_HEIGHT: f32 = 7.0 / SCALE_FACTOR;
pub const DROID_WIDTH: f32 = 5.0 / SCALE_FACTOR;

// An arachnid's origin sits low - the legs reach only ~0.2 (baby) / ~0.4
// (adult) below it while the body rises well above - so the capsule is raised
// (a negative offset) until its bottom meets the leg tips. Sized so the
// capsule is as tall as the mesh whether it comes from these bounds or from
// an authored sphere model; a capsule centred on the origin left the spider
// standing on air. Widths follow the authored collision (~2 ft).
pub const BABY_SPIDER_WIDTH: f32 = 2.0 / SCALE_FACTOR;
pub const BABY_SPIDER_HEIGHT: f32 = 3.0 / SCALE_FACTOR;
pub const BABY_SPIDER_PHYS_OFFSET: f32 = -1.0 / SCALE_FACTOR;

pub const SPIDER_WIDTH: f32 = 3.0 / SCALE_FACTOR;
pub const SPIDER_HEIGHT: f32 = 4.5 / SCALE_FACTOR;
pub const SPIDER_PHYS_OFFSET: f32 = -1.3 / SCALE_FACTOR;

pub const MONKEY_HEIGHT: f32 = 3.0 / SCALE_FACTOR;
pub const MONKEY_WIDTH: f32 = 3.0 / SCALE_FACTOR;

pub const RUMBLER_HEIGHT: f32 = 6.25 / SCALE_FACTOR;
pub const RUMBLER_WIDTH: f32 = 5.0 / SCALE_FACTOR;

/// Which part of a humanoid each skeleton joint is, and so what a blow there
/// is worth (see [`HitBoxType::damage_multiplier`]).
///
/// `Limb` is the near half of a limb - the segment hanging off the torso - and
/// `Extremity` the far half, which a blow reaches more easily and which carries
/// less of the creature. The two used to be split by "leg or arm" instead, so
/// an elbow counted for as much as a shoulder and a foot for less than a knee.
pub const HUMANOID_HIT_BOXES: Lazy<Arc<HashMap<u32, HitBoxType>>> = Lazy::new(|| {
    Arc::new(HashMap::from_iter(vec![
        (2, HitBoxType::Extremity),  // LToe
        (3, HitBoxType::Extremity),  //Rtoe
        (4, HitBoxType::Extremity),  // LKnee - lower leg
        (5, HitBoxType::Extremity),  // RKnee - lower leg
        (6, HitBoxType::Limb),       // LThigh
        (7, HitBoxType::Limb),       // RThigh
        (8, HitBoxType::Body),       // Neck
        (9, HitBoxType::Head),       // Head
        (10, HitBoxType::Limb),      // LShoulder
        (11, HitBoxType::Limb),      // RShoulder
        (12, HitBoxType::Extremity), // LElbow - forearm
        (13, HitBoxType::Extremity), // RElbow - forearm
        // The `LWeap`/`RWeap` joints are where a weapon is *attached*; the
        // vertices skinned to them are the creature's own hands, and the pipe
        // it carries is a separate object with no hitbox at all. So these are
        // hands - far limb - and a blow on the weapon itself is not something
        // this engine can currently tell apart.
        (14, HitBoxType::Extremity), // LWeap - the hand
        (15, HitBoxType::Extremity), // RWeap
        (18, HitBoxType::Body),      // Abdomen
    ]))
});

/// Per-joint articulation limits for humanoid ragdolls, keyed by the same
/// skeleton joint ids as `HUMANOID_HIT_BOXES`. Wider cones for the big limb
/// joints (shoulders/hips/knees/elbows), tight cones for head/neck/spine and
/// extremities (toes/weapon hands) so they don't flop unrealistically.
/// (Knees/elbows are really hinges; approximated as moderate cones for now.)
pub const HUMANOID_JOINT_LIMITS: Lazy<Arc<HashMap<u32, JointLimit>>> = Lazy::new(|| {
    Arc::new(HashMap::from_iter(vec![
        (2, JointLimit::cone(0.3)),  // LToe
        (3, JointLimit::cone(0.3)),  // RToe
        (4, JointLimit::cone(1.2)),  // LKnee
        (5, JointLimit::cone(1.2)),  // RKnee
        (6, JointLimit::cone(1.2)),  // LThigh
        (7, JointLimit::cone(1.2)),  // RThigh
        (8, JointLimit::cone(0.5)),  // Neck
        (9, JointLimit::cone(0.4)),  // Head
        (10, JointLimit::cone(1.4)), // LShoulder
        (11, JointLimit::cone(1.4)), // RShoulder
        (12, JointLimit::cone(1.2)), // LElbow
        (13, JointLimit::cone(1.2)), // RElbow
        (14, JointLimit::cone(0.3)), // LWeap
        (15, JointLimit::cone(0.3)), // RWeap
        (18, JointLimit::cone(0.4)), // Abdomen
    ]))
});

pub const EMPTY_JOINT_LIMITS: Lazy<Arc<HashMap<u32, JointLimit>>> =
    Lazy::new(|| Arc::new(HashMap::new()));

/// The arachnid skeleton: joint 0 is the body, 1-4 the two mandibles (root,
/// elbow), then eight legs of three joints each - shoulder, elbow, wrist -
/// four per side. The mapping is written whole so it does not depend on
/// which joints the mesh happens to skin: in practice the mandible roots and
/// six of the eight shoulders carry no vertices (nor do the claws and
/// mandible tips, 29-38), so 21 of these 29 joints get a proxy.
pub const SPIDER_HIT_BOXES: Lazy<Arc<HashMap<u32, HitBoxType>>> = Lazy::new(|| {
    let mut hit_boxes = HashMap::from([(0, HitBoxType::Body)]);
    // The mandibles are the bite: the spider's face.
    for joint in 1..=4 {
        hit_boxes.insert(joint, HitBoxType::Head);
    }
    for shoulder in (5..=26).step_by(3) {
        hit_boxes.insert(shoulder, HitBoxType::Limb);
        hit_boxes.insert(shoulder + 1, HitBoxType::Limb);
        hit_boxes.insert(shoulder + 2, HitBoxType::Extremity);
    }
    Arc::new(hit_boxes)
});

pub const OVERLORD_HIT_BOXES: Lazy<Arc<HashMap<u32, HitBoxType>>> =
    Lazy::new(|| Arc::new(HashMap::from_iter(vec![(0, HitBoxType::Body)])));

pub const EMPTY_HIT_BOXES: Lazy<Arc<HashMap<u32, HitBoxType>>> =
    Lazy::new(|| Arc::new(HashMap::from_iter(vec![])));

pub const HUMAN: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        bounding_size: vec3(HUMAN_WIDTH, HUMAN_HEIGHT, HUMAN_WIDTH),
        physics_offset_height: HUMAN_PHYS_OFFSET,
        actor_type: ActorType::Human,
        joint_map: vec![
            -1, 19, 9, 18, 8, 10, 11, 12, 13, 14, 15, 16, 17, 6, 7, 4, 5, 2, 3, 0, 1, -1,
        ],
        hit_boxes: HUMANOID_HIT_BOXES.clone(),
        joint_limits: HUMANOID_JOINT_LIMITS.clone(),
    })
});

pub const PLAYER_LIMB: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        bounding_size: vec3(HUMAN_WIDTH, HUMAN_HEIGHT, HUMAN_WIDTH),
        physics_offset_height: HUMAN_PHYS_OFFSET,
        actor_type: ActorType::PlayerLimb,
        joint_map: vec![],
        hit_boxes: EMPTY_HIT_BOXES.clone(),
        joint_limits: EMPTY_JOINT_LIMITS.clone(),
    })
});

pub const AVATAR: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        bounding_size: vec3(HUMAN_WIDTH, HUMAN_HEIGHT, HUMAN_WIDTH),
        physics_offset_height: HUMAN_PHYS_OFFSET,
        actor_type: ActorType::Human,
        joint_map: vec![
            -1, 19, 9, 18, 8, 10, 11, 12, 13, 14, 15, 16, 17, 6, 7, 4, 5, 2, 3, 0, 1, -1,
        ],
        hit_boxes: HUMANOID_HIT_BOXES.clone(),
        joint_limits: HUMANOID_JOINT_LIMITS.clone(),
    })
});

pub const RUMBLER: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        bounding_size: vec3(RUMBLER_WIDTH, RUMBLER_HEIGHT, RUMBLER_WIDTH),
        physics_offset_height: HUMAN_PHYS_OFFSET,
        actor_type: ActorType::Human,
        joint_map: vec![
            -1, 19, 9, 18, 8, 10, 11, 12, 13, 14, 15, 16, 17, 6, 7, 4, 5, 2, 3, 0, 1, -1,
        ],
        hit_boxes: HUMANOID_HIT_BOXES.clone(),
        joint_limits: HUMANOID_JOINT_LIMITS.clone(),
    })
});

pub const DROID: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        physics_offset_height: 0.0,
        bounding_size: vec3(DROID_WIDTH, DROID_HEIGHT, DROID_WIDTH),
        actor_type: ActorType::Droid,
        joint_map: vec![
            -1, 17, 10, 9, 8, 11, 12, 13, 14, 15, 16, -1, -1, 6, 7, 4, 5, 2, 3, 0, 1, -1,
        ],
        hit_boxes: HUMANOID_HIT_BOXES.clone(),
        joint_limits: HUMANOID_JOINT_LIMITS.clone(),
    })
});

pub const OVERLORD: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        physics_offset_height: 0.0,
        bounding_size: vec3(HUMAN_WIDTH, HUMAN_HEIGHT, HUMAN_WIDTH),
        actor_type: ActorType::Overlord,
        joint_map: vec![],
        hit_boxes: OVERLORD_HIT_BOXES.clone(),
        joint_limits: EMPTY_JOINT_LIMITS.clone(),
    })
});

pub const ARACHNID: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        physics_offset_height: SPIDER_PHYS_OFFSET,
        bounding_size: vec3(SPIDER_WIDTH, SPIDER_HEIGHT, SPIDER_WIDTH),
        actor_type: ActorType::Arachnid,
        joint_map: vec![
            -1, 0, -1, -1, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
            -1, -1, -1,
        ],
        hit_boxes: SPIDER_HIT_BOXES.clone(),
        joint_limits: EMPTY_JOINT_LIMITS.clone(),
    })
});

pub const MONKEY: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        physics_offset_height: 0.0,
        bounding_size: vec3(MONKEY_WIDTH, MONKEY_HEIGHT, MONKEY_WIDTH),
        actor_type: ActorType::Human,
        joint_map: vec![
            -1, 19, 9, 18, 8, 10, 11, 12, 13, 14, 15, 16, 17, 6, 7, 4, 5, 2, 3, 0, 1, -1,
        ],
        hit_boxes: HUMANOID_HIT_BOXES.clone(),
        joint_limits: HUMANOID_JOINT_LIMITS.clone(),
    })
});

pub const BABY_ARACHNID: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        physics_offset_height: BABY_SPIDER_PHYS_OFFSET,
        bounding_size: vec3(BABY_SPIDER_WIDTH, BABY_SPIDER_HEIGHT, BABY_SPIDER_WIDTH),
        actor_type: ActorType::Arachnid,
        joint_map: vec![
            -1, 0, -1, -1, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
            -1, -1, -1,
        ],
        hit_boxes: SPIDER_HIT_BOXES.clone(),
        joint_limits: EMPTY_JOINT_LIMITS.clone(),
    })
});

pub const SHODAN: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        bounding_size: vec3(HUMAN_WIDTH, HUMAN_HEIGHT, HUMAN_WIDTH),
        physics_offset_height: HUMAN_PHYS_OFFSET,
        actor_type: ActorType::Human,
        joint_map: vec![
            -1, 19, 9, 18, 8, 10, 11, 12, 13, 14, 15, 16, 17, 6, 7, 4, 5, 2, 3, 0, 1, -1,
        ],
        hit_boxes: HUMANOID_HIT_BOXES.clone(),
        joint_limits: HUMANOID_JOINT_LIMITS.clone(),
    })
});

const CREATURES: [Lazy<Arc<CreatureDefinition>>; 10] = [
    HUMAN,
    PLAYER_LIMB,
    AVATAR,
    RUMBLER,
    DROID,
    OVERLORD,
    ARACHNID,
    MONKEY,
    BABY_ARACHNID,
    SHODAN,
];

pub fn get_creature_definition(creature_type: u32) -> Option<Arc<CreatureDefinition>> {
    let binding = CREATURES;
    let item = binding.get(creature_type as usize);

    item.map(|c| Lazy::force(c).clone())
}

pub fn get_entity_creature(world: &World, entity_id: EntityId) -> Option<Arc<CreatureDefinition>> {
    let v_creature = world.borrow::<View<PropCreature>>().ok()?;
    let creature_type = v_creature.get(entity_id).ok()?;
    get_creature_definition(creature_type.0)
}

/// Where a creature senses from, relative to its origin. A creature whose
/// collider is raised above the origin (the arachnids: a negative
/// `physics_offset_height`) looks, feels for door sensors and whiskers from
/// the collider's centre, not from ankle height where a ray meets the floor
/// within a stride. A collider hung below the origin (humanoids) leaves the
/// origin alone: there it is already the higher point, roughly the chest.
pub fn sense_offset(world: &World, entity_id: EntityId) -> Vector3<f32> {
    let lift = get_entity_creature(world, entity_id)
        .map(|creature| (-creature.physics_offset_height).max(0.0))
        .unwrap_or(0.0);
    vec3(0.0, lift, 0.0)
}

/// How far the sense point sits above the bottom of the creature's collider -
/// its height above the floor when standing. Both creature shapes total
/// `bounding_size.y` tall, centred `physics_offset_height` below the origin.
pub fn sense_height(world: &World, entity_id: EntityId) -> Option<f32> {
    let creature = get_entity_creature(world, entity_id)?;
    Some(
        creature.physics_offset_height
            + creature.bounding_size.y / 2.0
            + sense_offset(world, entity_id).y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The humanoid map is what turns "which joint" into "what it is worth",
    /// so its buckets are load-bearing: near limb segments are worth more than
    /// far ones, and the weapon in a creature's hand is worth nothing.
    #[test]
    fn humanoid_joints_are_bucketed_by_what_a_blow_there_is_worth() {
        let map = HUMANOID_HIT_BOXES.clone();
        let worth = |joint: u32| map.get(&joint).copied().map(HitBoxType::damage_multiplier);

        assert_eq!(worth(9), Some(1.25), "head");
        assert_eq!(worth(8), Some(1.0), "neck");
        assert_eq!(worth(18), Some(1.0), "abdomen");
        for (joint, part) in [
            (6, "left thigh"),
            (7, "right thigh"),
            (10, "left shoulder"),
            (11, "right shoulder"),
        ] {
            assert_eq!(worth(joint), Some(0.75), "{part}");
        }
        for (joint, part) in [
            (4, "left knee"),
            (5, "right knee"),
            (12, "left elbow"),
            (13, "right elbow"),
            (2, "left toe"),
            (3, "right toe"),
        ] {
            assert_eq!(worth(joint), Some(0.5), "{part}");
        }
        for (joint, part) in [(14, "left hand"), (15, "right hand")] {
            assert_eq!(worth(joint), Some(0.5), "{part}");
        }
    }

    /// The spider map covers the whole skeleton: body, both mandibles, and
    /// every joint of all eight legs - the shoulder and elbow as near limb,
    /// the wrist as far. The claws are not mapped.
    #[test]
    fn spider_joints_are_mapped_leg_by_leg() {
        let map = SPIDER_HIT_BOXES.clone();
        let worth = |joint: u32| map.get(&joint).copied().map(HitBoxType::damage_multiplier);

        assert_eq!(map.len(), 29);
        assert_eq!(worth(0), Some(1.0), "body");
        for joint in 1..=4 {
            assert_eq!(worth(joint), Some(1.25), "mandible {joint}");
        }
        for leg in 0..8 {
            let shoulder = 5 + leg * 3;
            assert_eq!(worth(shoulder), Some(0.75), "leg {leg} shoulder");
            assert_eq!(worth(shoulder + 1), Some(0.75), "leg {leg} elbow");
            assert_eq!(worth(shoulder + 2), Some(0.5), "leg {leg} wrist");
        }
        assert_eq!(worth(29), None, "claw");
    }

    /// An arachnid senses from its raised collider's centre; a humanoid from
    /// its origin, which already sits above the collider's centre.
    #[test]
    fn a_raised_collider_lifts_the_sense_point() {
        let mut world = World::new();
        let human = world.add_entity(PropCreature(0));
        let arachnid = world.add_entity(PropCreature(6));
        let baby = world.add_entity(PropCreature(8));

        assert_eq!(sense_offset(&world, human).y, 0.0);
        assert!((sense_offset(&world, arachnid).y - 1.3 / SCALE_FACTOR).abs() < 1e-5);
        assert!((sense_offset(&world, baby).y - 1.0 / SCALE_FACTOR).abs() < 1e-5);

        // Standing heights of the sense point: human origin, spider collider centre.
        assert!((sense_height(&world, human).unwrap() - 4.25 / SCALE_FACTOR).abs() < 1e-5);
        assert!((sense_height(&world, baby).unwrap() - 1.5 / SCALE_FACTOR).abs() < 1e-5);
    }
}
