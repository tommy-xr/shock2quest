use dark::properties::{PropFrobSound, PropUseMsg};
use shipyard::{EntityId, Get, UniqueView, View, World};

use engine::audio::AudioHandle;

use super::{Effect, script_util::localized_property_text};

/// An entity's `P$UseMsg` text, resolved against USEMSG.STR.
pub fn use_message_text(world: &World, entity_id: EntityId) -> Option<String> {
    let v_use_msg = world.borrow::<View<PropUseMsg>>().ok()?;
    let raw = v_use_msg.get(entity_id).ok()?;
    localized_property_text(world, &raw.0, "usemsg.str")
}

/// What the player sees and hears for frobbing `entity_id` in the world,
/// before its own frob runs - retail `ShockDoFrob`: the object's
/// `P$FrobSound` schema, then its `P$UseMsg` on the status line (the
/// trainers' and keypads' descriptions, the Art pictures' static). Only world
/// frobs do this, so an item the player carries gets neither. Plain pickups
/// (`StoreWorldPickup`/`HoldItem`) never reach here; no shipped pickup
/// authors either property.
pub fn world_frob_feedback(world: &World, entity_id: EntityId) -> Vec<Effect> {
    let frob_sound = world
        .borrow::<View<PropFrobSound>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|sound| sound.0.clone()));
    let raw_use_message = world
        .borrow::<View<PropUseMsg>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|raw| raw.0.clone()));
    // Earth's training log says "Press the "U" key..."; VR plays logs from a
    // hand's upper button, so there the line would be a wrong instruction.
    let vr = world
        .borrow::<UniqueView<crate::mission::GlobalPresentationMode>>()
        .is_ok_and(|mode| mode.0 == crate::PresentationMode::Vr);
    let use_message = raw_use_message
        .filter(|raw| {
            !(vr && raw
                .split(':')
                .next()
                .unwrap_or_default()
                .trim()
                .eq_ignore_ascii_case("traininglog"))
        })
        .and_then(|raw| localized_property_text(world, &raw, "usemsg.str"));
    if (frob_sound.is_none() && use_message.is_none())
        || super::script_util::player_carried_items(world).contains(&entity_id)
    {
        return Vec::new();
    }

    frob_sound
        .map(|name| Effect::PlaySound {
            handle: AudioHandle::new(),
            name,
            source: Some(entity_id),
            spatial: false,
        })
        .into_iter()
        .chain(use_message.map(|text| Effect::ShowMessage { text }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string_table::StringTable;

    fn world_with_strings() -> World {
        let mut world = World::new();
        world.add_unique(StringTable::from_table(
            "usemsg.str",
            [(
                "psi1".to_owned(),
                "Cybernetic Upgrade Unit for Psionic Projection".to_owned(),
            )]
            .into_iter()
            .collect(),
        ));
        world
    }

    /// medsci1's Psi trainer: its UseMsg key resolves through USEMSG.STR.
    #[test]
    fn frobbing_an_object_with_a_use_message_shows_it() {
        let mut world = world_with_strings();
        let trainer = world.add_entity((PropUseMsg(
            "Psi1: \"Psionic Upgrade Implementation\"".to_owned(),
        ),));

        match world_frob_feedback(&world, trainer).as_slice() {
            [Effect::ShowMessage { text }] => {
                assert_eq!(text, "Cybernetic Upgrade Unit for Psionic Projection")
            }
            effects => panic!("expected one message, got {effects:?}"),
        }
    }

    /// rec's Code Art pictures play `ART_STATIC` on every frob.
    #[test]
    fn frobbing_an_object_with_a_frob_sound_plays_it() {
        let mut world = world_with_strings();
        let picture = world.add_entity((PropFrobSound("ART_STATIC".to_owned()),));

        match world_frob_feedback(&world, picture).as_slice() {
            [Effect::PlaySound { name, spatial, .. }] => {
                assert_eq!(name, "ART_STATIC");
                assert!(!spatial);
            }
            effects => panic!("expected one sound, got {effects:?}"),
        }
    }

    /// Earth's training log tells a desktop player to press "U"; VR has no
    /// such key, so the line is shown flat only.
    #[test]
    fn the_training_logs_keyboard_hint_is_flat_only() {
        for (mode, shown) in [
            (crate::PresentationMode::Flat, true),
            (crate::PresentationMode::Vr, false),
        ] {
            let mut world = World::new();
            world.add_unique(StringTable::from_table(
                "usemsg.str",
                [(
                    "traininglog".to_owned(),
                    "Press the \"U\" key to play the log you just picked up.".to_owned(),
                )]
                .into_iter()
                .collect(),
            ));
            world.add_unique(crate::mission::GlobalPresentationMode(mode));
            let log = world.add_entity((PropUseMsg("TrainingLog".to_owned()),));

            assert_eq!(
                !world_frob_feedback(&world, log).is_empty(),
                shown,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn frobbing_a_plain_object_does_nothing_extra() {
        let mut world = world_with_strings();
        let crate_entity = world.add_entity(());

        assert!(world_frob_feedback(&world, crate_entity).is_empty());
    }
}
