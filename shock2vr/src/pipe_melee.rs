//! Physical pipe attacks in VR. Retail's humanoid weapon is four spheres
//! along wrist 15 -> tip 17 (shkcrhum.cpp), enabled by MF_TRIGGER2/3. Resolve
//! their swept contacts before billing damage, so a nearer guard wins even
//! when the same tick also reaches the player. Other melee stays unchanged.

use std::{collections::HashMap, time::Duration};

use cgmath::{EuclideanSpace, InnerSpace, Point3, Vector3, vec3};
use dark::{
    SCALE_FACTOR,
    motion::{AnimationPlayer, MotionFlags},
    properties::PropHitPoints,
};
use rapier3d::prelude::{Isometry, Real};
use shipyard::{Component, EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

use crate::{
    Handedness,
    mission::{
        PlayerInfo,
        mission_core::{GlobalTemplateClassTags, is_vr_melee_weapon},
    },
    physics::{InternalCollisionGroups, PhysicsWorld},
    runtime_props::{RuntimePropJointTransforms, RuntimePropTransform},
    scripts::{Effect, ai::ai_util, script_util},
};

const RADIUS: f32 = 0.4 / SCALE_FACTOR;
const RECOIL: Duration = Duration::from_millis(280);

#[derive(Component, Clone, Default, serde::Serialize)]
pub(crate) struct PipeAttack {
    pub active: bool,
    pub consumed: bool,
    pub recovering: bool,
    pub parries: u64,
    pub wrist: [f32; 3],
    pub tip: [f32; 3],
    #[serde(skip)]
    previous: Option<([f32; 3], [f32; 3])>,
    #[serde(skip)]
    targets: HashMap<EntityId, Isometry<Real>>,
    #[serde(skip)]
    clip: Option<String>,
    #[serde(skip)]
    frame: u32,
    #[serde(skip)]
    closing: bool,
}

pub(crate) fn uses_physical_pipe(world: &World, entity: EntityId) -> bool {
    if !crate::mission::presentation_is_vr(world)
        || ai_util::ai_team(world, entity) == dark::properties::AITeam::Good
    {
        return false;
    }
    let Some(template) = ai_util::melee_weapon_template(world, entity) else {
        return false;
    };
    world
        .borrow::<UniqueView<GlobalTemplateClassTags>>()
        .is_ok_and(|tags| {
            tags.0
                .get(&template)
                .and_then(|tags| tags.get("enemyweaptype"))
                .is_some_and(|tag| tag.eq_ignore_ascii_case("leadpipe"))
        })
}

pub(crate) fn recovering(world: &World, entity: EntityId) -> bool {
    world
        .borrow::<View<PipeAttack>>()
        .is_ok_and(|states| states.get(entity).is_ok_and(|state| state.recovering))
}

/// Called with the newly evaluated joint pose, before scripts see its flags.
pub(crate) fn observe(
    world: &mut World,
    entity: EntityId,
    player: &AnimationPlayer,
    flags: MotionFlags,
) {
    if !uses_physical_pipe(world, entity) {
        world.remove::<(PipeAttack,)>(entity);
        return;
    }
    let pose = world.run(
        |joints: View<RuntimePropJointTransforms>, transforms: View<RuntimePropTransform>| {
            let joints = joints.get(entity).ok()?;
            let transform = transforms.get(entity).ok()?.0;
            Some((
                crate::util::get_position_from_matrix(&(transform * joints.0[15]))
                    .to_vec()
                    .into(),
                crate::util::get_position_from_matrix(&(transform * joints.0[17]))
                    .to_vec()
                    .into(),
            ))
        },
    );
    let Some((wrist, tip)) = pose else {
        return;
    };
    let mut state = world
        .borrow::<View<PipeAttack>>()
        .ok()
        .and_then(|states| states.get(entity).ok().cloned())
        .unwrap_or_default();
    let snapshot = player.snapshot();
    let clip = snapshot.queue.first().and_then(|head| head.name.clone());
    // An interrupt (death/stun/etc.) or a new swing must release the previous
    // window. Recovery itself deliberately moves the frame backwards.
    if state.clip != clip || (!player.is_recoiling() && snapshot.current_frame < state.frame) {
        state.active = false;
        state.consumed = false;
        state.previous = None;
    } else {
        state.previous = Some((state.wrist, state.tip));
    }
    state.clip = clip;
    state.frame = snapshot.current_frame;
    state.wrist = wrist;
    state.tip = tip;
    state.recovering = player.is_recoiling();
    if flags.contains(MotionFlags::MELEE_CONTACT_START) {
        state.active = true;
        state.consumed = false;
        // The path before this flag is windup, not an attack sweep.
        state.previous = None;
    }
    state.closing = flags.contains(MotionFlags::MELEE_CONTACT_END) || player.is_queue_empty();
    if state.recovering {
        state.active = false;
    }
    world.add_component(entity, state);
}

fn points(wrist: [f32; 3], tip: [f32; 3]) -> [Vector3<f32>; 4] {
    let wrist = Vector3::from(wrist);
    let tip = Vector3::from(tip);
    std::array::from_fn(|index| wrist + (tip - wrist) * ((index + 1) as f32 / 4.0))
}

fn contact_time(
    state: &PipeAttack,
    physics: &PhysicsWorld,
    target: EntityId,
) -> Option<(f32, Vector3<f32>)> {
    let (previous_wrist, previous_tip) = state.previous.unwrap_or((state.wrist, state.tip));
    let previous = points(previous_wrist, previous_tip);
    let current = points(state.wrist, state.tip);
    previous
        .into_iter()
        .zip(current)
        .filter_map(|(from, to)| {
            physics
                .sweep_sphere_against_entity(
                    from,
                    to,
                    RADIUS,
                    target,
                    state.targets.get(&target).copied(),
                )
                .map(|time| (time, from + (to - from) * time))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

// The weapon segment must not reach through a wall/door to claim a contact.
// Ignore the participants, but retain opaque physical scenery. Attack
// placement is independent of the AI's vision cone and current awareness.
fn unobstructed(
    physics: &PhysicsWorld,
    state: &PipeAttack,
    point: Vector3<f32>,
    participants: &[EntityId],
) -> bool {
    let wrist = Vector3::from(state.wrist);
    let delta = point - wrist;
    let distance = delta.magnitude();
    distance <= 1e-5
        || physics
            .ray_cast2_with_entity_filter(
                Point3::from_vec(wrist),
                delta / distance,
                distance,
                InternalCollisionGroups::WORLD | InternalCollisionGroups::ENTITIES,
                None,
                true,
                &|entity| !participants.contains(&entity),
            )
            .is_none()
}

pub(crate) fn resolve(
    world: &mut World,
    physics: &mut PhysicsWorld,
    animations: &mut HashMap<EntityId, AnimationPlayer>,
) -> Vec<Effect> {
    let (player, guards) = {
        let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        (
            player.entity_id,
            [player.left_hand_entity_id, player.right_hand_entity_id],
        )
    };
    let guards = guards.map(|guard| guard.filter(|id| is_vr_melee_weapon(world, *id)));
    let attacks = world
        .borrow::<View<PipeAttack>>()
        .unwrap()
        .iter()
        .with_id()
        .map(|(id, attack)| (id, attack.clone()))
        .collect::<Vec<_>>();
    let mut effects = Vec::new();
    for (entity, mut state) in attacks {
        let alive = world
            .borrow::<View<PropHitPoints>>()
            .unwrap()
            .get(entity)
            .is_ok_and(|hp| hp.hit_points > 0);
        if state.active && !state.consumed && alive && uses_physical_pipe(world, entity) {
            let participants = [Some(entity), Some(player), guards[0], guards[1]]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            let body_hit = contact_time(&state, physics, player)
                .filter(|(_, point)| unobstructed(physics, &state, *point, &participants));
            let block = guards
                .iter()
                .enumerate()
                .filter_map(|(hand, guard)| {
                    let guard = (*guard)?;
                    let (time, point) = contact_time(&state, physics, guard)?;
                    if !unobstructed(physics, &state, point, &participants) {
                        return None;
                    }
                    Some((time, point, hand, guard))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, point, hand, guard)) =
                block.filter(|block| body_hit.is_none_or(|hit| block.0 <= hit.0 + 1e-4))
            {
                state.consumed = true;
                state.active = false;
                state.recovering = true;
                state.parries += 1;
                if let Some(animation) = animations.get_mut(&entity) {
                    *animation = animation.recoil(RECOIL);
                }
                // Keep gravity, but stop lunging through a successfully held guard.
                let velocity = physics.get_velocity(entity).unwrap_or(vec3(0.0, 0.0, 0.0));
                physics.set_velocity(entity, vec3(0.0, velocity.y, 0.0));
                effects.push(script_util::play_impact_sound(
                    world,
                    guard,
                    entity,
                    point,
                    Some("metal"),
                ));
                world
                    .borrow::<shipyard::UniqueViewMut<crate::haptics::HapticFeedback>>()
                    .unwrap()
                    .request(
                        if hand == 0 {
                            Handedness::Left
                        } else {
                            Handedness::Right
                        },
                        crate::haptics::HapticPulse {
                            amplitude: 0.85,
                            duration_ms: 65,
                        },
                    );
            } else if body_hit.is_some() {
                state.consumed = true;
                effects.push(ai_util::melee_contact_damage(world, entity, player));
            }
        }
        if state.closing || !alive {
            state.active = false;
        }
        state.targets = guards
            .into_iter()
            .flatten()
            .chain(std::iter::once(player))
            .filter_map(|entity| physics.entity_body_pose(entity).map(|pose| (entity, pose)))
            .collect();
        world.add_component(entity, state);
    }
    // Mission effects are already flat; unlike script returns they do not go
    // through ScriptWorld's flattening pass before the central applier.
    Effect::flatten(effects)
}

pub(crate) fn debug_geometry(world: &World) -> Vec<engine::scene::SceneObject> {
    let states = world.borrow::<View<PipeAttack>>().unwrap();
    states
        .iter()
        .flat_map(|state| {
            let color = if state.recovering {
                vec3(0.2, 1.0, 0.9)
            } else if state.active {
                vec3(1.0, 0.8, 0.1)
            } else {
                vec3(0.4, 0.4, 0.4)
            };
            points(state.wrist, state.tip)
                .into_iter()
                .map(move |point| dark::hit_box::draw_debug_wire_sphere(point, RADIUS, color))
        })
        .collect()
}
