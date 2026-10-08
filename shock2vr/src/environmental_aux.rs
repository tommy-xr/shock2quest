//! The two non-spatial auxiliary schemas belonging to the selected ambient
//! region. Retail ambient.c starts them alongside the main bed; schema.cpp
//! schedules polyphonic intervals from sample start and mono intervals from end.
use std::{collections::VecDeque, time::Duration};

use dark::{
    gamesys::sound_schema::SoundSchema, importers::AUDIO_IMPORTER, properties::PropSchemaLoopParams,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::{AudioContext, AudioHandle, AudioPlaybackSettings, stop_audio},
};
use rand::Rng;
use shipyard::EntityId;

use crate::{
    audio_log::{self, PlayOptions, PlayRecord},
    game_scene::EnvironmentalCue,
};

#[derive(Default)]
pub(crate) struct EnvironmentalAuxAudio {
    region: Option<EntityId>,
    layers: [Layer; 2],
}

#[derive(Default)]
struct Layer {
    schema: String,
    remaining: Option<Duration>,
    plays: u16,
    handles: VecDeque<AudioHandle>,
}

impl Layer {
    fn clear(&mut self, audio: &mut AudioContext<EntityId, String>) {
        for handle in self.handles.drain(..) {
            audio_log::record_stop(handle.id());
            stop_audio(audio, handle);
        }
        *self = Self::default();
    }
}

impl EnvironmentalAuxAudio {
    pub fn clear(&mut self, audio: &mut AudioContext<EntityId, String>) {
        for layer in &mut self.layers {
            layer.clear(audio);
        }
        self.region = None;
    }

    pub fn update(
        &mut self,
        cue: Option<&EnvironmentalCue>,
        elapsed: Duration,
        schemas: &SoundSchema,
        assets: &mut AssetCache,
        audio: &mut AudioContext<EntityId, String>,
    ) {
        if self.region != cue.map(|cue| cue.entity) {
            self.clear(audio);
            self.region = cue.map(|cue| cue.entity);
        }
        let Some(cue) = cue else { return };
        for (layer, name) in self.layers.iter_mut().zip(&cue.auxiliary) {
            if layer.schema != *name {
                layer.clear(audio);
                layer.schema = name.clone();
                layer.remaining = (!name.is_empty()).then_some(Duration::ZERO);
            }
            let Some(remaining) = layer.remaining.as_mut() else {
                continue;
            };
            *remaining = remaining.saturating_sub(elapsed);
            if !remaining.is_zero() {
                continue;
            }
            // Resolve only when a sample is due, so random variants/pan do not
            // restart every frame. An unavailable layer cannot silence the bed.
            layer.remaining = None;
            let Some(resolved) = schemas.resolve(name) else {
                continue;
            };
            let Some(clip) =
                assets.get_opt(&AUDIO_IMPORTER, &format!("{}.wav", resolved.sample_name))
            else {
                continue;
            };
            let params = schemas.loop_params(name);
            let limit = params.map_or(1, |p| p.max_samples.max(1)) as usize;
            while layer.handles.len() >= limit {
                let handle = layer.handles.pop_front().unwrap();
                audio_log::record_stop(handle.id());
                stop_audio(audio, handle);
            }
            let handle = AudioHandle::new();
            let settings = AudioPlaybackSettings {
                looping: resolved.looping,
                ..AudioPlaybackSettings::listener_relative(
                    resolved.linear_gain(),
                    resolved.channel_gains(),
                )
            };
            audio_log::play_and_record(
                audio,
                handle.clone(),
                None,
                clip.clone(),
                PlayOptions::ListenerRelative(settings),
                PlayRecord {
                    sample: &resolved.sample_name,
                    volume_millibels: Some(resolved.volume_millibels),
                    pan_millibels: Some(resolved.pan_millibels),
                    tags: vec![("environmental_aux".into(), name.clone())],
                    source_entity: None,
                },
            );
            layer.handles.push_back(handle);
            layer.plays = layer.plays.saturating_add(1);
            layer.remaining = params.and_then(|p| {
                next_delay(p, layer.plays, clip.total_duration().unwrap_or_default())
            });
        }
    }
}

fn next_delay(params: &PropSchemaLoopParams, plays: u16, duration: Duration) -> Option<Duration> {
    if params.is_seamless()
        || (params.flags & PropSchemaLoopParams::COUNT != 0 && plays >= params.count)
    {
        return None;
    }
    let min = params.interval_min;
    let max = params.interval_max.max(min);
    let interval = Duration::from_millis(rand::thread_rng().gen_range(min..=max) as u64);
    // SCHEMA_LOOP_POLY is bit 0. Monophonic loops wait for sample end.
    Some(
        interval
            + if params.flags & 1 != 0 {
                Duration::ZERO
            } else {
                duration
            },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_timing_and_finite_loops_follow_authored_flags() {
        let mut p = PropSchemaLoopParams {
            flags: 1,
            max_samples: 2,
            interval_min: 6000,
            interval_max: 10000,
            ..Default::default()
        };
        for _ in 0..32 {
            let delay = next_delay(&p, 1, Duration::from_secs(3)).unwrap();
            assert!((Duration::from_secs(6)..=Duration::from_secs(10)).contains(&delay));
        }
        p.flags = 0;
        p.interval_max = 6000;
        assert_eq!(
            next_delay(&p, 1, Duration::from_secs(3)),
            Some(Duration::from_secs(9))
        );
        p.flags = PropSchemaLoopParams::COUNT;
        p.count = 2;
        assert!(next_delay(&p, 1, Duration::ZERO).is_some());
        assert!(next_delay(&p, 2, Duration::ZERO).is_none());
        assert!(next_delay(&PropSchemaLoopParams::default(), 1, Duration::ZERO).is_none());
    }
}
