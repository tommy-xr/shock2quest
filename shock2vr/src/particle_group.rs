//! How an authored Dark particle group (`P$ParticleGroup`) maps onto the
//! engine particle system, where the mapping is more than a unit conversion.

use cgmath::Vector3;
use dark::properties::PropParticleGroup;

use crate::palette;

const PRT_SINGLE_COLOR_DISK: u32 = 4;
const PGMT_OBJECT_TRAILING: u32 = 3;
const PAT_LAUNCH_CONTINUOUS: u32 = 1;

/// Trailing groups launch into world space, so particles stay where they
/// were emitted and streak behind a moving host. Every other motion type
/// rides its host. Dark derives this from the motion type and ignores the
/// authored world-space flag; a trailing group with a fixed (non-launch)
/// animation falls back to riding its host.
pub fn is_world_space(pg: &PropParticleGroup) -> bool {
    pg.motion_type == PGMT_OBJECT_TRAILING && pg.animation_type <= PAT_LAUNCH_CONTINUOUS
}

/// Colour over a disk particle's remaining life, as engine color keys. The
/// palette indices `g` / `b` are the fade targets: `r` until the shortest
/// lifetime remains, then to `g` at death, or to `g` at half of it and `b`
/// at death when both are set. Other render types draw a single colour.
pub fn color_fade_keys(pg: &PropParticleGroup, min_lifetime: f32) -> Vec<(f32, Vector3<f32>)> {
    if pg.render_type != PRT_SINGLE_COLOR_DISK || pg.fade_time <= 0.0 || (pg.g == 0 && pg.b == 0) {
        return Vec::new();
    }
    let rgb = palette::index_to_rgb;
    if pg.g != 0 && pg.b != 0 {
        vec![
            (min_lifetime, rgb(pg.r)),
            (min_lifetime / 2.0, rgb(pg.g)),
            (0.0, rgb(pg.b)),
        ]
    } else {
        vec![(min_lifetime, rgb(pg.r)), (0.0, rgb(pg.g))]
    }
}

#[cfg(test)]
mod tests {
    use cgmath::Vector3;
    use dark::properties::PropParticleGroup;

    use super::{color_fade_keys, is_world_space};
    use crate::palette::index_to_rgb;

    fn group(motion_type: u32, animation_type: u32, g: u8, b: u8) -> PropParticleGroup {
        let zero = Vector3::new(0.0, 0.0, 0.0);
        PropParticleGroup {
            render_type: 4,
            motion_type,
            animation_type,
            num: 10,
            velocity: zero,
            gravity: zero,
            r: 10,
            g,
            b,
            a: 255,
            spin: zero,
            is_active: true,
            is_worldspace: false,
            size: 1.0,
            scale_vel: 0.0,
            prev_loc: zero,
            bbox_min: zero,
            bbox_max: zero,
            radius: 0.0,
            launch_time: 0.1,
            fade_time: 0.5,
            model_name: String::new(),
        }
    }

    #[test]
    fn only_launching_trailing_groups_are_world_space() {
        assert!(is_world_space(&group(3, 1, 0, 0)));
        assert!(is_world_space(&group(3, 0, 0, 0)));
        assert!(
            !is_world_space(&group(3, 2, 0, 0)),
            "fixed shape rides its host"
        );
        let mut attached = group(4, 1, 0, 0);
        attached.is_worldspace = true;
        assert!(!is_world_space(&attached), "authored flag is ignored");
    }

    #[test]
    fn color_fade_keys_follow_the_set_targets() {
        assert_eq!(color_fade_keys(&group(0, 1, 0, 0), 2.0), vec![]);
        assert_eq!(
            color_fade_keys(&group(0, 1, 20, 0), 2.0),
            vec![(2.0, index_to_rgb(10)), (0.0, index_to_rgb(20))]
        );
        assert_eq!(
            color_fade_keys(&group(0, 1, 20, 30), 2.0),
            vec![
                (2.0, index_to_rgb(10)),
                (1.0, index_to_rgb(20)),
                (0.0, index_to_rgb(30)),
            ]
        );
        let mut bitmap = group(0, 1, 20, 30);
        bitmap.render_type = 5;
        assert_eq!(color_fade_keys(&bitmap, 2.0), vec![], "disks only");
        let mut no_fade = group(0, 1, 20, 30);
        no_fade.fade_time = 0.0;
        assert_eq!(color_fade_keys(&no_fade, 2.0), vec![]);
    }
}
