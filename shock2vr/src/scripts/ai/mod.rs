pub mod ai_util;
use ai_util::*;

mod behavior;
pub mod steering;

mod animated_monster_ai;
pub use animated_monster_ai::*;

use std::{cell::RefCell, rc::Rc};

use cgmath::{
    point3, vec3, vec4, Deg, EuclideanSpace, InnerSpace, Point3, Quaternion, Rotation, Rotation3,
};
use dark::{
    motion::{MotionFlags, MotionQueryItem},
    properties::PropPosition,
    SCALE_FACTOR,
};
use rand::Rng;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
    time::Time,
    util::{
        get_position_from_matrix, get_position_from_transform, get_rotation_from_forward_vector,
        get_rotation_from_matrix, get_rotation_from_transform,
    },
};

use self::steering::*;

use super::{Effect, Message, MessagePayload, Script};
