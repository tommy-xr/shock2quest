use super::GamePigGuiMsg;
use crate::{
    gui::GuiComponent,
    scripts::{Effect, gui::PanelText},
    time::Time,
    ui::Rect,
};
use cgmath::vec2;
use engine::audio::AudioHandle;
use shipyard::EntityId;
use std::time::Duration;

const DESCENT: Duration = Duration::from_millis(800);
const READY: Duration = Duration::from_millis(1250);
const CHIME_GAIN: f32 = 0.18;

#[derive(Clone, Debug, Default)]
pub(super) struct Boot {
    elapsed: Duration,
    last_frame: Option<Duration>,
}

impl Boot {
    pub fn advance(&mut self, entity: EntityId, time: &Time) -> Effect {
        // Multiple layout requests in one simulation frame cannot advance or
        // replay the cue. Hidden panels never receive this visible-frame hook.
        if self.last_frame == Some(time.total) {
            return Effect::NoEffect;
        }
        self.last_frame = Some(time.total);
        let previous = self.elapsed;
        self.elapsed = self.elapsed.saturating_add(time.elapsed);
        if previous < DESCENT && self.elapsed >= DESCENT {
            let handle = AudioHandle::new();
            Effect::combine(vec![
                Effect::PlaySound {
                    handle: handle.clone(),
                    name: "ibeep1".into(),
                    source: Some(entity),
                    spatial: false,
                },
                Effect::SetSoundGain {
                    handle,
                    gain: CHIME_GAIN,
                },
            ])
        } else {
            Effect::NoEffect
        }
    }

    pub fn ready(&self) -> bool {
        self.elapsed >= READY
    }

    pub fn components(&self) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let fraction = (self.elapsed.as_secs_f32() / DESCENT.as_secs_f32()).min(1.0);
        let y = 16.0 + 96.0 * fraction;
        let text = |label: &str, rect: Rect, font_size: f32| {
            let mut text = PanelText::centered(label, rect);
            if let GuiComponent::Text {
                font_size: size,
                fit_to_rect,
                ..
            } = &mut text
            {
                *size = font_size;
                *fit_to_rect = false;
            }
            text
        };
        vec![
            GuiComponent::Fill {
                position: vec2(13.0, 11.0),
                size: vec2(140.0, 276.0),
                color: [0, 0, 0],
                alpha: 1.0,
            },
            text("GamePig", Rect::new(18.0, y, 130.0, 30.0), 24.0),
            text(
                "brought to you by",
                Rect::new(18.0, y + 37.0, 130.0, 12.0),
                9.0,
            ),
            text("TriOptimum", Rect::new(18.0, y + 49.0, 130.0, 12.0), 9.0),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn time(millis: u64) -> Time {
        Time {
            total: Duration::from_millis(millis),
            elapsed: Duration::from_millis(millis),
        }
    }
    #[test]
    fn settle_emits_one_quiet_cue_on_the_same_handle_and_then_ready() {
        let entity = EntityId::from_inner(1).unwrap();
        let mut boot = Boot::default();
        assert!(matches!(boot.advance(entity, &time(799)), Effect::NoEffect));
        assert!(!boot.ready());
        let crossing = Time {
            total: Duration::from_millis(800),
            elapsed: Duration::from_millis(1),
        };
        let effects = Effect::flatten(vec![boot.advance(entity, &crossing)]);
        let [
            Effect::PlaySound {
                handle,
                name,
                source,
                spatial,
            },
            Effect::SetSoundGain {
                handle: gain_handle,
                gain,
            },
        ] = effects.as_slice()
        else {
            panic!("one play and gain adjustment");
        };
        assert_eq!(name, "ibeep1");
        assert_eq!(*source, Some(entity));
        assert!(!spatial);
        assert_eq!(handle.id(), gain_handle.id());
        assert_eq!(*gain, CHIME_GAIN);
        assert!(matches!(boot.advance(entity, &crossing), Effect::NoEffect));
        assert!(matches!(
            boot.advance(
                entity,
                &Time {
                    total: READY,
                    elapsed: READY - DESCENT
                }
            ),
            Effect::NoEffect
        ));
        assert!(boot.ready());
    }
    #[test]
    fn long_frame_crossing_both_boundaries_still_emits_only_one_cue() {
        let entity = EntityId::from_inner(1).unwrap();
        let mut boot = Boot::default();
        assert_eq!(
            Effect::flatten(vec![boot.advance(entity, &time(2000))]).len(),
            2
        );
        assert!(boot.ready());
        assert!(matches!(
            boot.advance(entity, &time(3000)),
            Effect::NoEffect
        ));
    }
}
