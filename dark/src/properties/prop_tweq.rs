use std::{io, time::Duration};

use cgmath::Vector3;
use num_derive::FromPrimitive;
use shipyard::Component;

use crate::ss2_common::*;
use bitflags::bitflags;

use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct TweqAnimationState: u32 {
        const ON = 1 << 0;
        const REVERSE = 1 << 1;
        const RESYNCH = 1 << 2;
        const GOEDGE = 1 << 3;
        const LAPONE = 1 << 4;
    }
    #[derive(Serialize, Deserialize)]
    pub struct TweqAnimationConfig: u32 {
        const NOLIMT = 1 << 0;
        const SIM = 1 << 1;
        const WRAP = 1 << 2;
        const ONEBOUNCE = 1 << 3;
        const SIMSMALLRAD = 1 << 4;
        const SIMLARGERAD = 1 << 5;
        const OFFSCREEN = 1 << 6;
    }
}

#[derive(FromPrimitive, Clone, Debug, Deserialize, Serialize)]
pub enum TweqHalt {
    DestroyObject = 0,
    RemoveProp = 1,
    StopTweq = 2,
    Continue = 3,
    SlayObj = 4,
}

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropTweqRotateState {
    pub animation_state: TweqAnimationState,
    pub axis1_animation_state: TweqAnimationState,
    pub axis2_animation_state: TweqAnimationState,
    pub axis3_animation_state: TweqAnimationState,
}

impl PropTweqRotateState {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropTweqRotateState {
        // TODO: Look at the rotate state in earth, see if it works?
        let animation_state_bits = read_u16(reader);
        let animation_state = TweqAnimationState::from_bits(animation_state_bits.into()).unwrap();
        let _unk2 = read_u16(reader); // misc state, is this used?

        let axis1_animation_state_bits = read_u32(reader);
        let axis2_animation_state_bits = read_u32(reader);
        let axis3_animation_state_bits = read_u32(reader);

        let axis1_animation_state =
            TweqAnimationState::from_bits(axis1_animation_state_bits).unwrap();
        let axis2_animation_state =
            TweqAnimationState::from_bits(axis2_animation_state_bits).unwrap();
        let axis3_animation_state =
            TweqAnimationState::from_bits(axis3_animation_state_bits).unwrap();

        PropTweqRotateState {
            animation_state,
            axis1_animation_state,
            axis2_animation_state,
            axis3_animation_state,
        }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropTweqModelState {
    pub animation_state: TweqAnimationState,
}

impl PropTweqModelState {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropTweqModelState {
        let animation_state_bits = read_u16(reader);
        let animation_state = TweqAnimationState::from_bits(animation_state_bits.into()).unwrap();
        let _misc = read_u16(reader); // misc state, is this used?
        let _time = read_u16(reader); // misc state, is this used?
        let _frame = read_u16(reader); // misc state, is this used?

        PropTweqModelState { animation_state }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropTweqEmitterState {
    pub animation_state: TweqAnimationState,
    pub time_since_last_event: Duration,
    pub num_iterations: u32,
}

impl PropTweqEmitterState {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropTweqEmitterState {
        let animation_state_bits = read_u16(reader);
        let animation_state = TweqAnimationState::from_bits(animation_state_bits.into()).unwrap();
        let _misc = read_u16(reader); // misc state, is this used?
        let _time = read_u16(reader); // misc state, is this used?
        let _frame = read_u16(reader); // misc state, is this used?

        PropTweqEmitterState {
            animation_state,
            time_since_last_event: Duration::from_secs(0),
            num_iterations: 0,
        }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropTweqDeleteState {
    pub animation_state: TweqAnimationState,
    pub time_since_last_event: Duration,
    pub num_iterations: u32,
}

impl PropTweqDeleteState {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropTweqDeleteState {
        let animation_state_bits = read_u16(reader);
        let animation_state = TweqAnimationState::from_bits(animation_state_bits.into()).unwrap();
        let _misc = read_u16(reader); // misc state, is this used?
        let _time = read_u16(reader); // misc state, is this used?
        let _frame = read_u16(reader); // misc state, is this used?

        PropTweqDeleteState {
            animation_state,
            time_since_last_event: Duration::from_secs(0),
            num_iterations: 0,
        }
    }
}

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropTweqModelConfig {
    pub animation_config: TweqAnimationConfig,
    pub halt: TweqHalt,

    pub model_names: Vec<String>,
}

impl PropTweqModelConfig {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropTweqModelConfig {
        let _unk = read_u8(reader);
        let _curve = read_u8(reader);
        let animation_config_bits = read_u8(reader);
        let animation_config =
            TweqAnimationConfig::from_bits(animation_config_bits.into()).unwrap();
        let halt_bits = read_u8(reader);
        let halt: TweqHalt = num_traits::FromPrimitive::from_u8(halt_bits).unwrap();

        let _misc = read_u16(reader);
        let _rate = read_u16(reader);

        let mut model_names = Vec::new();
        for _ in 0..6 {
            let model_name = read_string_with_size(reader, 16);
            if !model_name.is_empty() {
                model_names.push(model_name)
            }
        }

        PropTweqModelConfig {
            animation_config,
            halt,
            model_names,
        }
    }
}

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropTweqEmitterConfig {
    pub animation_config: TweqAnimationConfig,
    pub halt: TweqHalt,

    pub rate: Duration,
    pub max_frames: u32,
    pub emit_what: String,
    pub velocity: Vector3<f32>,
    pub angle_random: Vector3<f32>,
}

impl PropTweqEmitterConfig {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropTweqEmitterConfig {
        let _unk = read_u8(reader);
        let _curve = read_u8(reader);
        let animation_config_bits = read_u8(reader);
        let animation_config =
            TweqAnimationConfig::from_bits(animation_config_bits.into()).unwrap();
        let halt_bits = read_u8(reader);
        let halt: TweqHalt = num_traits::FromPrimitive::from_u8(halt_bits).unwrap();

        let _misc = read_u16(reader);
        let rate = read_u16(reader);

        let max_frames = read_u32(reader);
        let emit_what = read_string_with_size(reader, 16);
        let velocity = read_vec3(reader);
        let angle_random = read_vec3(reader);

        PropTweqEmitterConfig {
            animation_config,
            halt,
            rate: Duration::from_millis(rate.into()),
            max_frames,
            emit_what,
            velocity,
            angle_random,
        }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]

pub struct PropTweqDeleteConfig {
    pub animation_config: TweqAnimationConfig,
    pub halt: TweqHalt,

    pub rate: Duration,
}

impl PropTweqDeleteConfig {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropTweqDeleteConfig {
        let _unk = read_u8(reader);
        let _curve = read_u8(reader);
        let animation_config_bits = read_u8(reader);
        let animation_config =
            TweqAnimationConfig::from_bits(animation_config_bits.into()).unwrap();
        let halt_bits = read_u8(reader);
        let halt: TweqHalt = num_traits::FromPrimitive::from_u8(halt_bits).unwrap();

        let _misc = read_u16(reader);
        let rate = read_u16(reader);

        PropTweqDeleteConfig {
            animation_config,
            halt,
            rate: Duration::from_millis(rate.into()),
        }
    }
}
