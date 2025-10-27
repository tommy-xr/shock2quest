use serde::{Deserialize, Serialize};
use shipyard::Component;

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropService(pub u32);
