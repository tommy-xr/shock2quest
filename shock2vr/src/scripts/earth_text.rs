use std::collections::HashMap;
use std::time::Duration;

use engine::assets::asset_cache::AssetCache;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Unique, UniqueView, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{Effect, Script, ScriptRestoreContext, ScriptState, ScriptStateError};

/// The CHARGEN.STR key the card's text lives under, and the shipped English
/// text, verbatim, for a data install that lacks the table. The first line is
/// padded with spaces there to fake centering; the banner centers for real, so
/// the padding is trimmed at layout.
const CARD_KEY: &str = "earthtext0";
const FALLBACK_CARD: &str = "4 Years Earlier\nRamsey Recruitment Ctr.";

/// The original schedules the card 100 ms into the sim and clears it 7 s in.
/// The card's own duration is whatever is left of that deadline once the delay
/// has actually elapsed, so an overlong first frame shortens the card instead
/// of pushing its removal past 7 s.
const SHOW_DELAY: Duration = Duration::from_millis(100);
const REMOVE_AT: Duration = Duration::from_millis(7000);

const SCRIPT_STATE_KEY: &str = "shock2vr.earth_text";

/// CHARGEN.STR, held as a world `Unique` because a script has no `AssetCache`
/// when it runs (the `UseMessageStrings` pattern).
#[derive(Unique, Clone, Debug, Default)]
pub struct CharGenStrings(pub HashMap<String, String>);

impl CharGenStrings {
    pub fn load(asset_cache: &mut AssetCache) -> CharGenStrings {
        CharGenStrings(
            asset_cache
                .get_opt(&dark::importers::STRINGS_IMPORTER, "chargen.str")
                .map(|strings| (*strings).clone())
                .unwrap_or_default(),
        )
    }

    /// The Earth mission's title card.
    fn card(&self) -> String {
        crate::ui::resolve_menu_label(Some(&self.0), CARD_KEY, FALLBACK_CARD)
    }
}

#[derive(Serialize, Deserialize)]
struct EarthTextState {
    delay_remaining: Option<f32>,
}

/// `EarthText`: the title card the Earth mission opens with.
///
/// The original's script takes no world input at all - on sim start it
/// schedules itself the card 100 ms later and its removal 7 s later. Here the
/// delay is a countdown and the removal is the banner's own expiry, so the one
/// thing that has to survive a save is whether the card has already been
/// shown: a save made inside the mission must not replay it on load.
pub struct EarthText {
    delay_remaining: Option<f32>,
}

impl EarthText {
    pub fn new() -> EarthText {
        EarthText {
            delay_remaining: None,
        }
    }
}

impl Script for EarthText {
    fn initialize(&mut self, _entity_id: EntityId, _world: &World) -> Effect {
        self.delay_remaining = Some(SHOW_DELAY.as_secs_f32());
        Effect::NoEffect
    }

    fn update(
        &mut self,
        _entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let Some(remaining) = self.delay_remaining else {
            return Effect::NoEffect;
        };
        let remaining = remaining - time.elapsed.as_secs_f32();
        if remaining > 0.0 {
            self.delay_remaining = Some(remaining);
            return Effect::NoEffect;
        }

        self.delay_remaining = None;
        // `remaining` is now zero or negative: the overshoot past the 100 ms
        // mark comes off the card, keeping its removal at the original's 7 s.
        let Some(duration) =
            (REMOVE_AT - SHOW_DELAY).checked_sub(Duration::from_secs_f32(-remaining))
        else {
            return Effect::NoEffect;
        };
        Effect::ShowBanner {
            text: world
                .borrow::<UniqueView<CharGenStrings>>()
                .map(|strings| strings.card())
                .unwrap_or_else(|_| FALLBACK_CARD.to_owned()),
            duration,
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &EarthTextState {
                delay_remaining: self.delay_remaining,
            },
            SCRIPT_STATE_KEY,
        )
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: EarthTextState = state.decode(1, SCRIPT_STATE_KEY)?;
        self.delay_remaining = restored.delay_remaining;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::PhysicsWorld;

    fn tick(script: &mut EarthText, world: &World, seconds: f32) -> Effect {
        script.update(
            EntityId::dead(),
            world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_secs_f32(seconds),
                total: Duration::ZERO,
            },
        )
    }

    fn world_with_strings(card: Option<&str>) -> World {
        let world = World::new();
        let mut table = HashMap::new();
        if let Some(card) = card {
            table.insert(CARD_KEY.to_owned(), card.to_owned());
        }
        world.add_unique(CharGenStrings(table));
        world
    }

    #[test]
    fn the_card_is_shown_once_the_start_delay_elapses() {
        let world = world_with_strings(Some("         4 Years Earlier\nRamsey Recruitment Ctr."));
        let mut script = EarthText::new();
        script.initialize(EntityId::dead(), &world);

        assert!(matches!(tick(&mut script, &world, 0.05), Effect::NoEffect));
        let Effect::ShowBanner { text, duration } = tick(&mut script, &world, 0.05) else {
            panic!("expected the card");
        };
        assert_eq!(text, "         4 Years Earlier\nRamsey Recruitment Ctr.");
        // The delay elapsed exactly on time, so the card gets the whole
        // remainder of the original's 7 s removal deadline.
        assert_eq!(duration, REMOVE_AT - SHOW_DELAY);
    }

    /// The card is a one-shot: nothing re-shows it for the rest of the mission.
    #[test]
    fn the_card_is_shown_only_once() {
        let world = world_with_strings(Some("card"));
        let mut script = EarthText::new();
        script.initialize(EntityId::dead(), &world);
        assert!(matches!(
            tick(&mut script, &world, 1.0),
            Effect::ShowBanner { .. }
        ));
        assert!(matches!(tick(&mut script, &world, 1.0), Effect::NoEffect));
    }

    /// A save made after the card has played must not replay it on load.
    #[test]
    fn a_restored_script_does_not_replay_a_card_it_already_showed() {
        let world = world_with_strings(Some("card"));
        let mut script = EarthText::new();
        script.initialize(EntityId::dead(), &world);
        let _ = tick(&mut script, &world, 1.0);
        let saved = script.save_state().unwrap();

        let mut restored = EarthText::new();
        restored.initialize(EntityId::dead(), &world);
        restored
            .restore_state(&saved, &ScriptRestoreContext::new(&HashMap::new()))
            .unwrap();
        assert!(matches!(tick(&mut restored, &world, 1.0), Effect::NoEffect));
    }

    /// A frame long enough to overshoot the 100 ms mark shortens the card by
    /// the overshoot, so its removal still lands on the original's 7 s.
    #[test]
    fn a_late_first_frame_comes_off_the_card_not_off_its_deadline() {
        let world = world_with_strings(Some("card"));
        let mut script = EarthText::new();
        script.initialize(EntityId::dead(), &world);
        let Effect::ShowBanner { duration, .. } = tick(&mut script, &world, 1.0) else {
            panic!("expected the card");
        };
        // The countdown is in f32 seconds, so compare to the millisecond.
        assert_eq!(duration.as_millis(), (REMOVE_AT.as_millis()) - 1000);
    }

    /// A first frame past the whole 7 s deadline shows nothing at all.
    #[test]
    fn a_frame_past_the_removal_deadline_shows_no_card() {
        let world = world_with_strings(Some("card"));
        let mut script = EarthText::new();
        script.initialize(EntityId::dead(), &world);
        assert!(matches!(tick(&mut script, &world, 9.0), Effect::NoEffect));
    }

    /// A data install with no CHARGEN.STR still gets the shipped English card.
    #[test]
    fn a_missing_string_table_falls_back_to_the_shipped_text() {
        let world = world_with_strings(None);
        let mut script = EarthText::new();
        script.initialize(EntityId::dead(), &world);
        let Effect::ShowBanner { text, .. } = tick(&mut script, &world, 1.0) else {
            panic!("expected the card");
        };
        assert_eq!(text, FALLBACK_CARD);
    }
}
