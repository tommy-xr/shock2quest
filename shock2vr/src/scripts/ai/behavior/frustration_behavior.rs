use super::{Behavior, NextBehavior};
use crate::physics::PhysicsWorld;
use cgmath::{InnerSpace, Vector3};
use dark::motion::MotionQueryItem;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CombatMode {
    Melee,
    Ranged,
}

/// Private AI state: no ECS mirrors, and no target entity IDs to remap.
#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct CombatFrustration {
    lockouts: [f32; 2],
    failed_mode: Option<CombatMode>,
    stalled_seconds: f32,
    progress_anchor: Option<Vector3<f32>>,
    pub gesture_remaining: f32,
}
impl CombatFrustration {
    pub fn allows(&self, mode: CombatMode) -> bool {
        self.lockouts[mode as usize] <= 0.0
    }
    pub fn tick(&mut self, dt: f32) {
        for remaining in &mut self.lockouts {
            *remaining = (*remaining - dt).max(0.0);
        }
        self.gesture_remaining = (self.gesture_remaining - dt).max(0.0);
    }
    /// A blocked ray alone is not a failure: walking toward an opening is
    /// useful progress. Wait for two seconds of actual stalled combat.
    pub fn observe(&mut self, failed: Option<CombatMode>, position: Vector3<f32>, dt: f32) -> bool {
        let failed = failed.filter(|mode| self.allows(*mode));
        let progressed = self
            .progress_anchor
            .is_none_or(|anchor| (position - anchor).magnitude2() > 0.0625);
        if failed != self.failed_mode || progressed || failed.is_none() {
            self.stalled_seconds = 0.0;
            self.progress_anchor = Some(position);
            self.failed_mode = failed;
        }
        let Some(mode) = failed else {
            return false;
        };
        self.stalled_seconds += dt;
        if self.stalled_seconds < 2.0 {
            return false;
        }
        self.lockouts[mode as usize] = 10.0;
        self.gesture_remaining = 2.0;
        self.stalled_seconds = 0.0;
        self.failed_mode = None;
        true
    }
}

pub struct FrustrationBehavior;
impl Behavior for FrustrationBehavior {
    fn name(&self) -> &'static str {
        "Frustration"
    }
    fn is_combat_frustration(&self) -> bool {
        true
    }
    fn preempted_by_alertness(&self) -> bool {
        false
    }
    fn animation(&self) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("discover"),
            MotionQueryItem::new("thwarted"),
        ]
    }
    fn animation_queries(&self) -> Vec<Vec<MotionQueryItem>> {
        // Shotgun hybrids author a challenge but no thwarted performance.
        // Keep the actual frustration gesture first for pipe hybrids and
        // other creatures that have it.
        vec![
            self.animation(),
            vec![
                MotionQueryItem::new("discover"),
                MotionQueryItem::new("challenge"),
            ],
        ]
    }
    fn next_behavior(
        &mut self,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity: EntityId,
    ) -> NextBehavior {
        // Completion from the displaced attack/turn clip can arrive on the
        // next frame. The parent owns the bounded two-second hold, including
        // the fallback when this creature has no authored gesture.
        NextBehavior::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec3;
    #[test]
    fn progress_prevents_frustration_and_lockouts_are_per_mode() {
        let mut state = CombatFrustration::default();
        for i in 0..30 {
            assert!(!state.observe(Some(CombatMode::Ranged), vec3(i as f32, 0.0, 0.0), 0.1));
        }
        for _ in 0..18 {
            assert!(!state.observe(Some(CombatMode::Ranged), vec3(29.0, 0.0, 0.0), 0.1));
        }
        assert!(state.observe(Some(CombatMode::Ranged), vec3(29.0, 0.0, 0.0), 0.2));
        assert!(!state.allows(CombatMode::Ranged));
        assert!(state.allows(CombatMode::Melee));
        state.tick(9.0);
        assert!(!state.allows(CombatMode::Ranged));
        state.tick(1.0);
        assert!(state.allows(CombatMode::Ranged));
    }
    #[test]
    fn recovering_an_attack_resets_pending_frustration() {
        let mut state = CombatFrustration::default();
        let pos = vec3(0.0, 0.0, 0.0);
        assert!(!state.observe(Some(CombatMode::Melee), pos, 1.5));
        assert!(!state.observe(None, pos, 0.1));
        assert!(!state.observe(Some(CombatMode::Melee), pos, 1.5));
    }
}
