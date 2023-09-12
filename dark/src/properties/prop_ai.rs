use std::io;

use bitflags::bitflags;
use num_traits::ToPrimitive;
use shipyard::Component;

use crate::ss2_common::{read_i32, read_string_with_size, read_u32};
use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropAI(pub String);
