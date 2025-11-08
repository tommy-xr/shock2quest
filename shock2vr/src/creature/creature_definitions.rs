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

pub struct CreatureDefinition {
    pub actor_type: ActorType,
    pub physics_offset_height: f32,
    pub bounding_size: Vector3<f32>,
    joint_map: Vec<i32>,
    pub hit_boxes: Arc<HashMap<u32, HitBoxType>>,
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

pub const BABY_SPIDER_WIDTH: f32 = 3.0 / SCALE_FACTOR;
pub const BABY_SPIDER_HEIGHT: f32 = 3.0 / SCALE_FACTOR;

pub const SPIDER_WIDTH: f32 = 4.0 / SCALE_FACTOR;
pub const SPIDER_HEIGHT: f32 = 4.0 / SCALE_FACTOR;

pub const MONKEY_HEIGHT: f32 = 3.0 / SCALE_FACTOR;
pub const MONKEY_WIDTH: f32 = 3.0 / SCALE_FACTOR;

pub const RUMBLER_HEIGHT: f32 = 6.25 / SCALE_FACTOR;
pub const RUMBLER_WIDTH: f32 = 5.0 / SCALE_FACTOR;

pub const HUMANOID_HIT_BOXES: Lazy<Arc<HashMap<u32, HitBoxType>>> = Lazy::new(|| {
    Arc::new(HashMap::from_iter(vec![
        (2, HitBoxType::Extremity),  // LToe
        (3, HitBoxType::Extremity),  //Rtoe
        (4, HitBoxType::Limb),       // LKnee
        (5, HitBoxType::Limb),       // RKnee
        (6, HitBoxType::Limb),       // LThigh
        (7, HitBoxType::Limb),       // RThigh
        (8, HitBoxType::Body),       // Neck
        (9, HitBoxType::Head),       // Head
        (10, HitBoxType::Limb),      // LShoulder
        (11, HitBoxType::Limb),      // RShoulder
        (12, HitBoxType::Limb),      // LElbow
        (13, HitBoxType::Limb),      // RElbow
        (14, HitBoxType::Extremity), // LWeap
        (15, HitBoxType::Extremity), // RWeap
        (18, HitBoxType::Body),      // Abdomen
    ]))
});

pub const SPIDER_HIT_BOXES: Lazy<Arc<HashMap<u32, HitBoxType>>> =
    Lazy::new(|| Arc::new(HashMap::from_iter(vec![(0, HitBoxType::Body)])));

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
    })
});

pub const PLAYER_LIMB: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        bounding_size: vec3(HUMAN_WIDTH, HUMAN_HEIGHT, HUMAN_WIDTH),
        physics_offset_height: HUMAN_PHYS_OFFSET,
        actor_type: ActorType::PlayerLimb,
        joint_map: vec![],
        hit_boxes: EMPTY_HIT_BOXES.clone(),
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
    })
});

pub const OVERLORD: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        physics_offset_height: 0.0,
        bounding_size: vec3(HUMAN_WIDTH, HUMAN_HEIGHT, HUMAN_WIDTH),
        actor_type: ActorType::Overlord,
        joint_map: vec![],
        hit_boxes: OVERLORD_HIT_BOXES.clone(),
    })
});

pub const ARACHNID: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        physics_offset_height: 0.0,
        bounding_size: vec3(SPIDER_WIDTH, SPIDER_HEIGHT, SPIDER_WIDTH),
        actor_type: ActorType::Arachnid,
        joint_map: vec![
            -1, 0, -1, -1, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
            -1, -1, -1,
        ],
        hit_boxes: SPIDER_HIT_BOXES.clone(),
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
    })
});

pub const BABY_ARACHNID: Lazy<Arc<CreatureDefinition>> = Lazy::new(|| {
    Arc::new(CreatureDefinition {
        physics_offset_height: 0.0,
        bounding_size: vec3(BABY_SPIDER_WIDTH, BABY_SPIDER_HEIGHT, BABY_SPIDER_WIDTH),
        actor_type: ActorType::Arachnid,
        joint_map: vec![
            -1, 0, -1, -1, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
            -1, -1, -1,
        ],
        hit_boxes: SPIDER_HIT_BOXES.clone(),
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
