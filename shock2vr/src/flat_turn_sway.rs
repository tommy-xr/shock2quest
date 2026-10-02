//! Camera-speed-driven cosmetic viewmodel lean, inspired by Nightdive's
//! viewmodel.nut. The two exponential filters remain stable across frame rates.
use cgmath::{Deg, Quaternion, Rotation, Rotation3, Vector2, vec2, vec3};

pub(crate) struct FlatTurnSway {
    previous: Option<Vector2<f32>>,
    velocity: Vector2<f32>,
    lean: Vector2<f32>,
}

impl Default for FlatTurnSway {
    fn default() -> Self {
        Self {
            previous: None,
            velocity: vec2(0.0, 0.0),
            lean: vec2(0.0, 0.0),
        }
    }
}

impl FlatTurnSway {
    pub fn step(&mut self, look: Quaternion<f32>, dt: f32, scale: f32) -> Quaternion<f32> {
        // Paused redraws can carry new input. Leave the sample and filters
        // untouched until simulation advances, just like the weapon clips.
        if dt <= 0.0 {
            return self.rotation(scale);
        }
        let forward = look.rotate_vector(vec3(0.0, 0.0, -1.0));
        let angles = vec2(
            (-forward.x).atan2(-forward.z).to_degrees(),
            forward.y.clamp(-1.0, 1.0).asin().to_degrees(),
        );
        let previous = self.previous.replace(angles);
        let delta = previous.map(|old| {
            let yaw = (angles.x - old.x + 180.0).rem_euclid(360.0) - 180.0;
            vec2(yaw, angles.y - old.y)
        });
        // Resample after equip, a long frame, or a discontinuous camera cut.
        // None of those should become an enormous apparent turning velocity.
        if !dt.is_finite()
            || dt > 0.25
            || delta.is_none_or(|d| d.x.abs() > 45.0 || d.y.abs() > 45.0)
        {
            self.velocity = vec2(0.0, 0.0);
            self.lean = vec2(0.0, 0.0);
        } else if let Some(delta) = delta {
            // Nightdive: camera delta * 0.018/dt, then lean * 1.25.
            // Its LerpRot divisors 50 and 16 correspond to ~150 and 48 ms.
            let target = delta * (0.018 * 1.25 / dt);
            let target = vec2(target.x.clamp(-6.0, 6.0), target.y.clamp(-6.0, 6.0));
            self.velocity += (target - self.velocity) * (1.0 - (-dt / 0.150).exp());
            self.lean += (self.velocity - self.lean) * (1.0 - (-dt / 0.048).exp());
        }
        self.rotation(scale)
    }

    fn rotation(&self, scale: f32) -> Quaternion<f32> {
        Quaternion::from_angle_y(Deg(-self.lean.x * scale))
            * Quaternion::from_angle_x(Deg(-self.lean.y * scale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::InnerSpace;

    fn look(yaw: f32, pitch: f32) -> Quaternion<f32> {
        Quaternion::from_angle_y(Deg(yaw)) * Quaternion::from_angle_x(Deg(pitch))
    }

    fn sweep(hz: usize) -> (FlatTurnSway, Quaternion<f32>) {
        let mut sway = FlatTurnSway::default();
        let mut pose = sway.step(look(0.0, 0.0), 1.0 / hz as f32, 1.0);
        for frame in 1..=hz {
            pose = sway.step(
                look(frame as f32 * 90.0 / hz as f32, 0.0),
                1.0 / hz as f32,
                1.0,
            );
        }
        (sway, pose)
    }

    #[test]
    fn turn_leans_and_settles_at_all_frame_rates() {
        for hz in [30, 60, 120] {
            let (mut sway, pose) = sweep(hz);
            let forward = pose.rotate_vector(vec3(0.0, 0.0, -1.0));
            assert!((forward.x - 2.025_f32.to_radians().sin()).abs() < 0.001);
            for _ in 0..hz * 2 {
                sway.step(look(90.0, 0.0), 1.0 / hz as f32, 1.0);
            }
            assert!(sway.lean.magnitude() < 0.001);
        }
    }

    #[test]
    fn yaw_wrap_is_a_small_turn_and_camera_cuts_reset() {
        let mut sway = FlatTurnSway::default();
        sway.step(look(179.0, 0.0), 1.0 / 60.0, 1.0);
        sway.step(look(-179.0, 0.0), 1.0 / 60.0, 1.0);
        assert!(sway.lean.x > 0.0 && sway.lean.x < 0.2);
        sway.step(look(0.0, 0.0), 1.0 / 60.0, 1.0);
        assert_eq!(sway.lean, vec2(0.0, 0.0));
    }

    #[test]
    fn pitch_works_and_paused_redraws_preserve_samples() {
        let mut sway = FlatTurnSway::default();
        sway.step(look(0.0, 0.0), 1.0 / 60.0, 1.0);
        let pose = sway.step(look(0.0, 2.0), 1.0 / 60.0, 1.0);
        assert!(pose.rotate_vector(vec3(0.0, 0.0, -1.0)).y < 0.0);
        let previous = sway.previous;
        assert_eq!(sway.step(look(0.0, 10.0), 0.0, 1.0), pose);
        assert_eq!(sway.previous, previous);
        sway.step(look(0.0, 10.0), 0.5, 1.0);
        assert_eq!(sway.lean, vec2(0.0, 0.0));
    }
}
