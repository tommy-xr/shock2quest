use cgmath::{Vector3, vec3};

/// CELL_MOTION stores 256 packed PortalCellMotion records (21 bytes), then
/// 256 packed sMedMoCellMotion records (14 bytes). See Dark wrloop.cpp's
/// ReadWriteWaterMotion. Translation is a velocity in original Z-up units;
/// the trailing u16 is angular texture motion, not a translational current.
pub(super) fn read_water_currents(bytes: &[u8]) -> Vec<Vector3<f32>> {
    const COUNT: usize = 256;
    const PREFIX: usize = COUNT * 21;
    let mut currents = vec![vec3(0.0, 0.0, 0.0); COUNT];
    if bytes.len() < PREFIX + COUNT * 14 {
        return currents;
    }
    for (index, current) in currents.iter_mut().enumerate().skip(1) {
        let start = PREFIX + index * 14;
        let read = |offset| {
            f32::from_le_bytes(
                bytes[start + offset..start + offset + 4]
                    .try_into()
                    .unwrap(),
            )
        };
        let (x, y, z) = (read(0), read(4), read(8));
        if x.is_finite() && y.is_finite() && z.is_finite() {
            *current = vec3(-x, z, y) / crate::SCALE_FACTOR;
        }
    }
    currents
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_motion_converts_axes_and_scale_without_texture_rotation() {
        let mut bytes = vec![0; 256 * 35];
        let offset = 256 * 21 + 14;
        for (axis, value) in [25.0_f32, 0.0, -9.0].into_iter().enumerate() {
            bytes[offset + axis * 4..offset + axis * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[offset + 12..offset + 14].copy_from_slice(&123_u16.to_le_bytes());
        let currents = read_water_currents(&bytes);
        assert_eq!(currents[1], vec3(-10.0, -3.6, 0.0));
        assert_eq!(currents[0], vec3(0.0, 0.0, 0.0));
        assert_eq!(currents[2], vec3(0.0, 0.0, 0.0));
    }

    #[test]
    fn missing_or_truncated_motion_is_still_water() {
        for bytes in [&[][..], &[0; 80][..]] {
            assert!(
                read_water_currents(bytes)
                    .iter()
                    .all(|v| *v == vec3(0.0, 0.0, 0.0))
            );
        }
    }
}
