use super::{Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError};
use crate::{
    mission::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
    psi_sword,
    time::Time,
};
use cgmath::{EuclideanSpace, InnerSpace, Point3, Vector3};
use shipyard::{EntityId, Get, UniqueView, View, World};

#[derive(Default)]
pub struct PsiSwordController {
    bound: bool,
    previous: Option<(Point3<f32>, Vector3<f32>)>,
    cooldown: f32,
}
impl PsiSwordController {
    fn sample_tip(
        &mut self,
        tip: Point3<f32>,
        player: Vector3<f32>,
        dt: f32,
    ) -> Option<(Point3<f32>, Vector3<f32>)> {
        // Control/inspection requests can update the pose without advancing time.
        // Keep the last simulated sample so the next frame observes that motion.
        if dt <= 0.0 {
            return None;
        }
        self.previous.replace((tip, player))
    }
}
impl Script for PsiSwordController {
    fn handle_message(
        &mut self,
        id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        message: &MessagePayload,
    ) -> Effect {
        match message {
            MessagePayload::BeginPsiSword => {
                self.bound = true;
                Effect::SetPsiSword {
                    amp: id,
                    enabled: true,
                }
            }
            MessagePayload::TriggerPull if psi_sword::active(world, id) => {
                if world
                    .borrow::<View<crate::runtime_props::RuntimePropFlatAim>>()
                    .is_ok_and(|v| v.contains(id))
                {
                    Effect::FlatMeleeSwing { entity_id: id }
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::AnimationFlagTriggered { motion_flags }
                if psi_sword::active(world, id)
                    && motion_flags.contains(dark::motion::MotionFlags::TRIGGER1) =>
            {
                let aim = world
                    .borrow::<View<crate::runtime_props::RuntimePropFlatAim>>()
                    .ok()
                    .and_then(|v| v.get(id).ok().copied());
                aim.map(|aim| super::weapon_script::flat_melee_hit(physics, aim, world, id))
                    .unwrap_or(Effect::NoEffect)
            }
            _ => Effect::NoEffect,
        }
    }
    fn update(
        &mut self,
        id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        if !self.bound {
            return Effect::NoEffect;
        }
        let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        let held =
            player.left_hand_entity_id == Some(id) || player.right_hand_entity_id == Some(id);
        let active = world
            .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
            .is_ok_and(|p| p.is_active(psi_sword::POWER));
        if !held || !active {
            self.bound = false;
            self.previous = None;
            return Effect::SetPsiSword {
                amp: id,
                enabled: false,
            };
        }
        if !world
            .borrow::<View<psi_sword::BoundBlade>>()
            .is_ok_and(|v| v.contains(id))
        {
            return Effect::SetPsiSword {
                amp: id,
                enabled: true,
            };
        }
        if world
            .borrow::<View<crate::runtime_props::RuntimePropFlatAim>>()
            .is_ok_and(|v| v.contains(id))
        {
            return Effect::NoEffect;
        }
        let Some((base, tip)) = psi_sword::segment(world, id) else {
            return Effect::NoEffect;
        };
        let previous = self.sample_tip(tip, player.pos, time.elapsed.as_secs_f32());
        self.cooldown = (self.cooldown - time.elapsed.as_secs_f32()).max(0.0);
        let Some((last_tip, last_player)) = previous else {
            return Effect::NoEffect;
        };
        let motion = tip - last_tip - (player.pos - last_player);
        let dt = time.elapsed.as_secs_f32();
        if !swing_is_valid(motion.magnitude(), dt) || self.cooldown > 0.0 {
            return Effect::NoEffect;
        }
        let groups = InternalCollisionGroups::ENTITIES
            | InternalCollisionGroups::HITBOX
            | InternalCollisionGroups::WORLD;
        let filter = |entity| entity != id && entity != player.entity_id;
        // Test the current blade and the swept tip; world geometry blocks both rays.
        let mut hit = physics.ray_cast2_with_entity_filter(
            base,
            (tip - base).normalize(),
            psi_sword::LENGTH,
            groups,
            Some(id),
            true,
            &filter,
        );
        if hit.is_none() && (tip - last_tip).magnitude2() > 0.000001 {
            hit = physics.ray_cast2_with_entity_filter(
                last_tip,
                (tip - last_tip).normalize(),
                (tip - last_tip).magnitude(),
                groups,
                Some(id),
                true,
                &filter,
            );
        }
        let Some(hit) = hit else {
            return Effect::NoEffect;
        };
        let Some(target) = hit.maybe_entity_id else {
            return Effect::NoEffect;
        };
        let target = crate::util::resolve_proxy_entity(world, target);
        let damage =
            crate::mission::stim_response::contact_stim_damage(world, psi_sword::WEAPON, target)
                * crate::scripts::berserk::melee_damage_multiplier(world);
        if damage <= 0.0 {
            return Effect::NoEffect;
        }
        self.cooldown = 0.4;
        Effect::Send {
            msg: super::Message {
                to: target,
                payload: MessagePayload::Damage {
                    amount: damage,
                    impact: Some(super::DamageImpact {
                        direction: motion.normalize(),
                        point: hit.hit_point.to_vec(),
                        bone: None,
                    }),
                },
            },
        }
    }
    fn script_state_key(&self) -> Option<&'static str> {
        Some("shock2vr.psi_sword_amp")
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &self.bound, "shock2vr.psi_sword_amp")
    }
    fn restore_state(
        &mut self,
        state: &ScriptState,
        _ctx: &ScriptRestoreContext,
    ) -> Result<(), ScriptStateError> {
        self.bound = state.decode(1, "shock2vr.psi_sword_amp")?;
        self.previous = None;
        self.cooldown = 0.0;
        Ok(())
    }
}
fn swing_is_valid(distance: f32, dt: f32) -> bool {
    dt > 0.0 && dt < 0.25 && distance < 1.0 && distance / dt >= 1.2
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_time_pose_updates_preserve_the_next_swing() {
        let mut sword = PsiSwordController::default();
        let player = Vector3::new(0.0, 0.0, 0.0);
        let before = Point3::new(0.0, 1.0, 0.0);
        let after = Point3::new(0.04, 1.0, 0.0);
        assert!(sword.sample_tip(before, player, 1.0 / 60.0).is_none());
        assert!(sword.sample_tip(after, player, 0.0).is_none());
        let (last, _) = sword.sample_tip(after, player, 1.0 / 60.0).unwrap();
        assert_eq!(last, before);
        assert!(swing_is_valid((after - last).magnitude(), 1.0 / 60.0));
    }
    #[test]
    fn sword_rejects_still_tracking_and_teleports() {
        assert!(!swing_is_valid(0.0, 1.0 / 60.0));
        assert!(!swing_is_valid(2.0, 1.0 / 60.0));
        assert!(!swing_is_valid(0.1, 0.0));
        assert!(swing_is_valid(0.04, 1.0 / 60.0));
    }
}
