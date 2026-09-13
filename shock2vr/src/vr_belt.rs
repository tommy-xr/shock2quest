//! Authored resting card pose shared by dark_explorer and gameplay.
use cgmath::{Deg, Matrix4, Vector3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeltCardPose {
    /// Position relative to astra-vr-belt.glb's origin, in metres.
    pub position_m: [f32; 3],
    /// Local X, Y, Z rotations composed in that order.
    pub rotation_degrees: [f32; 3],
}
impl Default for BeltCardPose {
    fn default() -> Self {
        Self {
            position_m: [-0.115, 0.0, -0.324],
            rotation_degrees: [90.0, 90.0, 0.0],
        }
    }
}
impl BeltCardPose {
    pub fn parse(text: &str) -> Result<Self, String> {
        let pose: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if !pose.is_valid() {
            return Err(
                "Card pose must contain finite, bounded position and rotation values".into(),
            );
        }
        Ok(pose)
    }
    pub fn is_valid(&self) -> bool {
        self.position_m
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1.0)
            && self
                .rotation_degrees
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 180.0)
    }
    pub fn rotation(&self) -> Matrix4<f32> {
        Matrix4::from_angle_x(Deg(self.rotation_degrees[0]))
            * Matrix4::from_angle_y(Deg(self.rotation_degrees[1]))
            * Matrix4::from_angle_z(Deg(self.rotation_degrees[2]))
    }
    pub fn transform(&self) -> Matrix4<f32> {
        Matrix4::from_translation(Vector3::from(self.position_m) / crate::METERS_PER_WORLD_UNIT)
            * self.rotation()
    }
}
/// Scale the original card model to an 8.5 cm long edge, held or resting.
pub fn card_model_scale(min: Vector3<f32>, max: Vector3<f32>) -> f32 {
    let size = max - min;
    (0.085 / crate::METERS_PER_WORLD_UNIT) / size.x.max(size.y).max(size.z).max(0.001)
}
/// Center the card for the shared belt/editor mounting transform.
pub fn card_model_transform(min: Vector3<f32>, max: Vector3<f32>) -> Matrix4<f32> {
    Matrix4::from_scale(card_model_scale(min, max)) * Matrix4::from_translation(-(min + max) * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_pose_round_trips_and_rejects_invalid_resources() {
        let pose = BeltCardPose {
            position_m: [-0.10, 0.03, -0.34],
            rotation_degrees: [80.0, 95.0, 5.0],
        };
        assert_eq!(
            BeltCardPose::parse(&serde_json::to_string(&pose).unwrap()).unwrap(),
            pose
        );
        assert!(
            BeltCardPose::parse(r#"{"position_m":[0,0,2],"rotation_degrees":[0,0,0]}"#).is_err()
        );
        assert_eq!(
            BeltCardPose::parse(include_str!("../../assets/vr-belt-card.json")).unwrap(),
            BeltCardPose::default()
        );
    }
}
