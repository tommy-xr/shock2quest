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

/// One axis of a rotate tweq: how fast it turns and between which angles.
///
/// The original stores this as a `rate-low-high` triple per axis, in degrees
/// (rate is degrees per second).
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct TweqAxisLimits {
    pub rate: f32,
    pub low: f32,
    pub high: f32,
}

/// `P$CfgTweqRo` - the authored spin of a rotate tweq.
///
/// Laid out as the original's vector-tweq config: the shared 8-byte base
/// config, then a `rate-low-high` triple for each of the three axes, then the
/// primary axis index and padding (48 bytes total). Without it every rotating
/// object in the port turns at one hardcoded rate, which is wrong for anything
/// whose spin has an observable consequence - a Tweq emitter's facing sets the
/// direction its emissions are launched in.
#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropTweqRotateConfig {
    pub animation_config: TweqAnimationConfig,
    pub halt: TweqHalt,
    /// Per-axis spin, in the original's axis order.
    pub axes: [TweqAxisLimits; 3],
    /// Index into [`Self::axes`] of the axis the original treats as primary.
    pub primary_axis: u8,
}

impl PropTweqRotateConfig {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropTweqRotateConfig {
        let _tweq_type = read_u8(reader);
        let _curve = read_u8(reader);
        let animation_config_bits = read_u8(reader);
        let animation_config =
            TweqAnimationConfig::from_bits(animation_config_bits.into()).unwrap();
        let halt_bits = read_u8(reader);
        let halt: TweqHalt = num_traits::FromPrimitive::from_u8(halt_bits).unwrap();
        let _misc = read_u16(reader);
        let _rate = read_u16(reader);

        let mut axes = [TweqAxisLimits {
            rate: 0.0,
            low: 0.0,
            high: 0.0,
        }; 3];
        for axis in axes.iter_mut() {
            axis.rate = read_single(reader);
            axis.low = read_single(reader);
            axis.high = read_single(reader);
        }
        let primary_axis = read_u8(reader);
        // pad[3] - the original pads the struct back to a 4-byte boundary.
        for _ in 0..3 {
            let _pad = read_u8(reader);
        }

        PropTweqRotateConfig {
            animation_config,
            halt,
            axes,
            primary_axis,
        }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropTweqModelState {
    pub animation_state: TweqAnimationState,
    /// Current slot in [`PropTweqModelConfig::model_names`].
    ///
    /// Dark persists this as the model tweq's `Frame #`. Older shock2quest
    /// saves did not serialize it, so they resume at the authored first frame.
    #[serde(default)]
    pub frame: usize,
}

impl PropTweqModelState {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropTweqModelState {
        let animation_state_bits = read_u16(reader);
        let animation_state = TweqAnimationState::from_bits(animation_state_bits.into()).unwrap();
        let _misc = read_u16(reader); // misc state, is this used?
        let _time = read_u16(reader); // misc state, is this used?
        let frame = read_u16(reader);

        PropTweqModelState {
            animation_state,
            frame: frame.into(),
        }
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

    /// Dark's `TWEQ_MC_RELVEL`: rotate the authored launch vector by the
    /// emitter object's facing before launching it.
    pub relative_velocity: bool,

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

        let misc = read_u16(reader);
        let rate = read_u16(reader);

        let max_frames = read_u32(reader);
        let emit_what = read_string_with_size(reader, 16);
        let velocity = read_vec3(reader);
        let angle_random = read_vec3(reader);

        PropTweqEmitterConfig {
            animation_config,
            halt,
            relative_velocity: misc & (1 << 8) != 0,
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{
        PropTweqEmitterConfig, PropTweqModelState, PropTweqRotateConfig, TweqAnimationState,
        TweqAxisLimits,
    };

    #[test]
    fn model_state_reads_the_authored_frame_number() {
        let mut bytes = Cursor::new([
            0x02, 0x00, // animation state: reverse
            0x00, 0x00, // misc
            0x7b, 0x00, // time
            0x04, 0x00, // frame number
        ]);

        let state = PropTweqModelState::read(&mut bytes, 8);

        assert!(state.animation_state.contains(TweqAnimationState::REVERSE));
        assert_eq!(state.frame, 4);
    }

    #[test]
    fn rotate_config_reads_the_per_axis_rate_limits_and_primary_axis() {
        // sTweqVectorConfig: an 8-byte base config, three (rate, low, high)
        // float triples, then primary_axis + 3 pad = 48 bytes.
        let mut raw = vec![
            0x00, // type
            0x00, // curve
            0x00, // anim config
            0x00, // halt action
            0x00, 0x00, // misc
            0x00, 0x00, // rate
        ];
        for (rate, low, high) in [
            (0.0f32, 0.0f32, 0.0f32),
            (0.0, 0.0, 0.0),
            (50.0, 0.0, 360.0),
        ] {
            raw.extend_from_slice(&rate.to_le_bytes());
            raw.extend_from_slice(&low.to_le_bytes());
            raw.extend_from_slice(&high.to_le_bytes());
        }
        raw.push(2); // primary_axis
        raw.extend_from_slice(&[0, 0, 0]); // pad
        assert_eq!(raw.len(), 48);

        let config = PropTweqRotateConfig::read(&mut Cursor::new(raw), 48);
        assert_eq!(config.primary_axis, 2);
        assert_eq!(
            config.axes[2],
            TweqAxisLimits {
                rate: 50.0,
                low: 0.0,
                high: 360.0
            }
        );
        assert_eq!(config.axes[0].rate, 0.0);
    }

    #[test]
    fn emitter_config_reads_relative_velocity_misc_flag() {
        let mut raw = vec![
            0x00, // type
            0x00, // curve
            0x02, // anim config: Sim
            0x00, // halt: DestroyObject
            0x00, 0x01, // misc: TWEQ_MC_RELVEL (1 << 8)
            0xf4, 0x01, // rate: 500 ms
            0x03, 0x00, 0x00, 0x00, // max frames
        ];
        raw.extend_from_slice(b"grub\0\0\0\0\0\0\0\0\0\0\0\0");
        for component in [-25.0_f32, 0.0, 0.0, 0.0, 0.0, 0.0] {
            raw.extend_from_slice(&component.to_le_bytes());
        }
        let mut bytes = Cursor::new(raw);

        let config = PropTweqEmitterConfig::read(&mut bytes, 64);

        assert!(config.relative_velocity);
        // `read_vec3` converts raw Dark (-X, Y, Z) into runtime (X, Z, Y).
        assert_eq!(config.velocity, cgmath::vec3(25.0, 0.0, 0.0));
    }
}
