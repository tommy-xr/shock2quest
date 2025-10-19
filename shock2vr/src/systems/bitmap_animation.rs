use cgmath::num_traits::ToPrimitive;
use dark::properties::{PropBitmapAnimation, PropFrameAnimConfig, PropFrameAnimState};
use shipyard::{Get, IntoIter, IntoWithId, UniqueView, UniqueViewMut, View, ViewMut};

use crate::{
    mission::EffectQueue,
    runtime_props::{RuntimeBitmapAnimationFrameCount, RuntimePropSpawnTimeInSeconds},
    scripts::Effect,
    time::Time,
};

///
/// run_tweq
///
/// Runs all tweq components
///
pub fn run_bitmap_animation(
    u_time: UniqueView<Time>,
    v_frame_anim_config: View<PropFrameAnimConfig>,
    mut v_frame_anim_state: ViewMut<PropFrameAnimState>,
    v_spawn_time: View<RuntimePropSpawnTimeInSeconds>,
    v_frame_count: View<RuntimeBitmapAnimationFrameCount>,
    v_bitmap_animation: View<PropBitmapAnimation>,
    mut effects: UniqueViewMut<EffectQueue>,
) {
    let cur_time = u_time.total.as_secs_f32();

    for (id, (frame_config, frame_state, spawn_time)) in
        (&v_frame_anim_config, &mut v_frame_anim_state, &v_spawn_time)
            .iter()
            .with_id()
    {
        let current_frame = ((cur_time - spawn_time.0) * frame_config.frames_per_second)
            .to_u32()
            .unwrap();

        let frame_count = v_frame_count.get(id).map(|c| c.0).unwrap_or(0);

        let should_kill_on_complete = v_bitmap_animation
            .get(id)
            .map(|c| c.kill_on_completion)
            .unwrap_or(false);

        if current_frame >= frame_count && should_kill_on_complete {
            effects.push(Effect::DestroyEntity { entity_id: id });
            continue;
        }

        frame_state.current_frame = current_frame;
    }
}
