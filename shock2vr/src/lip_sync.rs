//! Native equivalent of the 25AE engine's `Face` mouth animation, driven by the
//! authored `strings/facepos.str` sequences while a creature speaks.
//!
//! Only the archetypes Nightdive's unconditional `dbmods/ND-lipsync.dml` gives
//! a `Face` (texture to replace `*`, i.e. shape keys) take part; the base
//! gamesys `P$Face` chunk is empty, and the fingerprinted SCP patches are not
//! applied here. Each sequence character is one mouth position per 1/16 s
//! (sequence lengths match their samples' durations). The position order,
//! default shape-key map and run blend below are read from the shipped
//! `SystemShock2Remastered.exe`; the unwired `ND_SpeechShapeKeyAnimated`
//! Squirrel helper uses a different map and is not what production runs.
use crate::mission::mission_core::{GlobalEntityMetadata, GlobalTemplateHierarchy};
use dark::properties::PropTemplateId;
use engine::scene::SceneObject;
use shipyard::{EntityId, Get, UniqueView, View, World};
use std::collections::HashMap;

/// `dbmods/ND-lipsync.dml` (`MaleCrew` is commented out there).
const FACE_ARCHETYPES: [&str; 12] = [
    "Midwife",
    "og-pipe",
    "og-shotgun",
    "og-grenade",
    "MaleFlight",
    "MaleEng",
    "MaleHydro",
    "MaleMedsci",
    "MaleOps",
    "MaleRec",
    "MaleRick",
    "Male Appar.",
];

/// The Nightdive string tables are `$`-token localization stubs except this
/// animation table, so they are mounted under their own namespace only.
pub const FACEPOS_TABLE: &str = "nightdive_strings/facepos.str";

/// Mouth positions Closed, SmallOh, SmallEe, BigOh, BigEe, Shout, G, H, X, as
/// the engine's built-in `FaceNeutral` map assigns them (Closed is
/// `MouthShapeA=0`, i.e. no shape). No shipped mesh has `MouthShapeX`.
const MOUTH_SHAPES: [Option<&str>; 9] = [
    None,
    Some("MouthShapeF"),
    Some("MouthShapeB"),
    Some("MouthShapeE"),
    Some("MouthShapeC"),
    Some("MouthShapeD"),
    Some("MouthShapeG"),
    Some("MouthShapeH"),
    Some("MouthShapeX"),
];

const FRAME_MS: f32 = 1000.0 / 16.0;

/// The mouth positions authored for `sample` when `entity` has a `Face`.
pub fn sequence(
    world: &World,
    table: Option<&HashMap<String, String>>,
    entity: EntityId,
    sample: &str,
) -> Option<Vec<u8>> {
    let positions = table?.get(&sample.to_ascii_lowercase())?;
    has_face(world, entity).then(|| {
        positions
            .trim()
            .bytes()
            .map(|c| c.wrapping_sub(b'0'))
            .collect()
    })
}

fn has_face(world: &World, entity: EntityId) -> bool {
    let (Ok(templates), Ok(metadata), Ok(hierarchy)) = (
        world.borrow::<View<PropTemplateId>>(),
        world.borrow::<UniqueView<GlobalEntityMetadata>>(),
        world.borrow::<UniqueView<GlobalTemplateHierarchy>>(),
    ) else {
        return false;
    };
    let Ok(template) = templates.get(entity).map(|t| t.template_id) else {
        return false;
    };
    FACE_ARCHETYPES.iter().any(|name| {
        metadata
            .0
            .get(&name.to_ascii_lowercase())
            .is_some_and(|parent| hierarchy.is_or_descends_from(template, parent.template_id))
    })
}

/// Shape weights `elapsed_ms` into a sequence, or `None` once it has finished
/// (the neutral face). The current position fades out across its whole run
/// while the next one fades in over the run's last frame.
pub fn mouth_weights(sequence: &[u8], elapsed_ms: f32) -> Option<Vec<(&'static str, f32)>> {
    let frame = elapsed_ms / FRAME_MS;
    if !(0.0..sequence.len() as f32).contains(&frame) {
        return None;
    }
    let index = frame as usize;
    let current = sequence[index];
    let start = (0..index)
        .rev()
        .take_while(|&i| sequence[i] == current)
        .last()
        .unwrap_or(index);
    let end = (index + 1..sequence.len())
        .take_while(|&i| sequence[i] == current)
        .last()
        .unwrap_or(index);
    let run_start = start as f32 * FRAME_MS;
    let run_end = (end + 1) as f32 * FRAME_MS;
    // Adjacent runs differ and every position has its own shape, so the two
    // weights never share a channel (the engine adds them).
    let shape = |position: u8| MOUTH_SHAPES.get(position as usize).copied().flatten();
    let mut weights = Vec::new();
    if let Some(name) = shape(current) {
        weights.push((name, (run_end - elapsed_ms) / (run_end - run_start)));
    }
    if let Some(&next) = sequence.get(end + 1) {
        let blend_start = end as f32 * FRAME_MS;
        if let Some(name) = shape(next).filter(|_| elapsed_ms >= blend_start) {
            weights.push((name, (elapsed_ms - blend_start) / (run_end - blend_start)));
        }
    }
    Some(weights)
}

/// Writes the speaking mouth shapes onto every part that has them. Scene
/// objects are rebuilt each frame, so unnamed channels are already neutral.
pub fn apply(objects: &mut [SceneObject], weights: &[(&str, f32)]) {
    for object in objects {
        let names = object.geometry.morph_names();
        for (shape, weight) in weights {
            if let Some(index) = names.iter().position(|name| name == shape) {
                object.morph_weights.resize(names.len(), 0.0);
                object.morph_weights[index] = *weight;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(sequence: &str, ms: f32) -> Option<Vec<(&'static str, f32)>> {
        let digits: Vec<u8> = sequence.bytes().map(|c| c - b'0').collect();
        mouth_weights(&digits, ms)
    }

    #[test]
    fn positions_use_the_engine_shape_map() {
        for (digit, shape) in [
            ('1', "MouthShapeF"),
            ('2', "MouthShapeB"),
            ('3', "MouthShapeE"),
            ('4', "MouthShapeC"),
            ('5', "MouthShapeD"),
            ('6', "MouthShapeG"),
            ('7', "MouthShapeH"),
        ] {
            assert_eq!(at(&digit.to_string(), 0.0), Some(vec![(shape, 1.0)]));
        }
        assert_eq!(at("0", 10.0), Some(vec![]), "closed is no shape");
    }

    #[test]
    fn runs_fade_out_while_the_next_position_fades_in_over_the_last_frame() {
        // "22" then "5": the B run spans 0..125 ms; D blends in from 62.5 ms.
        assert_eq!(at("225", 0.0), Some(vec![("MouthShapeB", 1.0)]));
        assert_eq!(at("225", 31.25), Some(vec![("MouthShapeB", 0.75)]));
        assert_eq!(
            at("225", 93.75),
            Some(vec![("MouthShapeB", 0.25), ("MouthShapeD", 0.5)])
        );
        // The next run then starts at full weight: no pop at the boundary.
        assert_eq!(at("225", 125.0), Some(vec![("MouthShapeD", 1.0)]));
        // The final run has nothing to blend into and just fades.
        assert_eq!(at("225", 156.25), Some(vec![("MouthShapeD", 0.5)]));
    }

    #[test]
    fn finished_or_unstarted_sequences_are_neutral() {
        assert_eq!(at("225", 187.5), None);
        assert_eq!(at("225", 10_000.0), None);
        assert_eq!(at("225", -1.0), None);
        assert_eq!(at("", 0.0), None);
        assert_eq!(at("9", 0.0), Some(vec![]), "unknown positions are skipped");
    }

    #[test]
    fn apply_sets_only_the_spoken_mouth_channels() {
        struct NamedMesh(Vec<String>);
        impl engine::scene::Geometry for NamedMesh {
            fn draw(&self) {}
            fn morph_names(&self) -> &[String] {
                &self.0
            }
        }
        let mut object = SceneObject::new(
            engine::scene::color_material::create(cgmath::vec3(1.0, 1.0, 1.0)),
            Box::new(NamedMesh(vec![
                "MouthShapeB".into(),
                "worm".into(),
                "MouthShapeD".into(),
            ])),
        );
        object.morph_weights = vec![0.7, 0.4, 0.9];
        apply(std::slice::from_mut(&mut object), &[("MouthShapeB", 0.25)]);
        assert_eq!(object.morph_weights, vec![0.25, 0.4, 0.9]);
    }
}
