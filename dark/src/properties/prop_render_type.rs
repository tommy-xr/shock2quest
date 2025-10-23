use std::io;

use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use shipyard::Component;

use crate::ss2_common::read_u32;

use serde::{Deserialize, Serialize};

#[derive(FromPrimitive, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RenderType {
    Normal = 0,
    NoRender = 1,
    FullBright = 2,
    EditorOnly = 3,
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropRenderType(pub RenderType);

impl PropRenderType {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropRenderType {
        let val = read_u32(reader);
        let render_type = RenderType::from_u32(val).unwrap();
        PropRenderType(render_type)
    }
}
