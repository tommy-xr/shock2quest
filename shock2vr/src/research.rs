//! Persistent, campaign-wide research progress.
//!
//! Research is keyed by the stable gamesys archetype rather than a mission
//! object's runtime id, so dropping an item, changing decks, or saving and
//! loading never loses work.

use std::collections::HashMap;

use dark::{
    properties::{
        PropBaseTechDesc, PropChemicalNeeded, PropObjLookString, PropObjState,
        PropRequiredTechDesc, PropResearchReport, PropResearchText, PropResearchTime,
    },
    ss2_entity_info::SystemShock2EntityInfo,
};
use serde::{Deserialize, Serialize};
use shipyard::{Component, EntityId, Get, View, World};

use crate::{
    runtime_props::RuntimePropCanonicalTemplateId, scripts::script_util::hydrate_template_component,
};

/// Older campaign saves serialized carried objects before research metadata
/// became a runtime component. Restore only the newly understood properties
/// from the stable gamesys archetype, while preserving every live serialized
/// value that was already present in the save.
pub(crate) fn backfill_legacy_held_research_components(
    world: &mut World,
    held_entities: &[EntityId],
    entity_info: &SystemShock2EntityInfo,
) {
    let researchables = {
        let canonical = world
            .borrow::<View<RuntimePropCanonicalTemplateId>>()
            .unwrap();
        held_entities
            .iter()
            .filter_map(|entity_id| {
                let template_id = canonical.get(*entity_id).ok()?.0;
                hydrate_template_component::<PropResearchTime>(template_id, entity_info)
                    .map(|_| (*entity_id, template_id))
            })
            .collect::<Vec<_>>()
    };

    for (entity_id, template_id) in researchables {
        backfill_component::<PropBaseTechDesc>(world, entity_id, template_id, entity_info);
        backfill_component::<PropChemicalNeeded>(world, entity_id, template_id, entity_info);
        backfill_component::<PropObjLookString>(world, entity_id, template_id, entity_info);
        backfill_component::<PropObjState>(world, entity_id, template_id, entity_info);
        backfill_component::<PropRequiredTechDesc>(world, entity_id, template_id, entity_info);
        backfill_component::<PropResearchReport>(world, entity_id, template_id, entity_info);
        backfill_component::<PropResearchText>(world, entity_id, template_id, entity_info);
        backfill_component::<PropResearchTime>(world, entity_id, template_id, entity_info);
    }
}

fn backfill_component<T>(
    world: &mut World,
    entity_id: EntityId,
    template_id: i32,
    entity_info: &SystemShock2EntityInfo,
) where
    T: Component + Clone + Send + Sync,
{
    let is_missing = world
        .borrow::<View<T>>()
        .map(|components| components.get(entity_id).is_err())
        .unwrap_or(true);
    if is_missing && let Some(component) = hydrate_template_component::<T>(template_id, entity_info)
    {
        world.add_component(entity_id, component);
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ResearchState {
    active_template_id: Option<i32>,
    progress: HashMap<i32, ResearchProgress>,
    reports: u32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct ResearchProgress {
    authored_seconds: f32,
    next_chemical: usize,
    complete: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResearchStatus {
    pub authored_seconds: f32,
    pub active: bool,
    pub complete: bool,
    pub needed_chemical: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BeginResearchResult {
    Started,
    SkillRequired(i32),
    AlreadyComplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdvanceResearchResult {
    InProgress,
    ChemicalRequired,
    Completed,
}

impl ResearchState {
    /// Stable identities of projects the player has begun, including finished
    /// and suspended work whose original inventory instance no longer exists.
    pub fn project_template_ids(&self) -> Vec<i32> {
        let mut ids: Vec<_> = self.progress.keys().copied().collect();
        ids.sort_by_key(|id| (Some(*id) != self.active_template_id, *id));
        ids
    }
    pub fn active_template_id(&self) -> Option<i32> {
        self.active_template_id
    }

    pub fn begin(
        &mut self,
        template_id: i32,
        required_skill: i32,
        player_skill: i32,
    ) -> BeginResearchResult {
        if self.is_complete(template_id) {
            return BeginResearchResult::AlreadyComplete;
        }
        if player_skill < required_skill {
            return BeginResearchResult::SkillRequired(required_skill);
        }
        self.progress.entry(template_id).or_default();
        self.active_template_id = Some(template_id);
        BeginResearchResult::Started
    }

    pub fn suspend(&mut self) {
        self.active_template_id = None;
    }

    pub fn is_complete(&self, template_id: i32) -> bool {
        self.progress
            .get(&template_id)
            .map(|progress| progress.complete)
            .unwrap_or(false)
    }

    pub fn has_report(&self, report_mask: u32) -> bool {
        self.reports & report_mask != 0
    }

    pub fn status(
        &self,
        template_id: i32,
        chemicals: Option<&PropChemicalNeeded>,
    ) -> ResearchStatus {
        let progress = self.progress.get(&template_id).cloned().unwrap_or_default();
        ResearchStatus {
            authored_seconds: progress.authored_seconds,
            active: self.active_template_id == Some(template_id),
            complete: progress.complete,
            needed_chemical: chemicals.and_then(|needed| {
                chemical_due(&progress, needed).map(|chemical| chemical.to_owned())
            }),
        }
    }

    pub fn advance(
        &mut self,
        template_id: i32,
        real_seconds: f32,
        skill: i32,
        research_factor: f32,
        total_authored_seconds: f32,
        chemicals: Option<&PropChemicalNeeded>,
        report_mask: u32,
    ) -> AdvanceResearchResult {
        if self.active_template_id != Some(template_id) {
            return AdvanceResearchResult::InProgress;
        }
        let progress = self.progress.entry(template_id).or_default();
        if progress.complete {
            return AdvanceResearchResult::Completed;
        }
        if chemicals
            .and_then(|needed| chemical_due(progress, needed))
            .is_some()
        {
            return AdvanceResearchResult::ChemicalRequired;
        }

        let skill_above_one = (skill.max(1) - 1) as f32;
        let multiplier = 1.0 + research_factor * skill_above_one * skill_above_one;
        progress.authored_seconds += real_seconds.max(0.0) * multiplier.max(0.0);

        if let Some(threshold) = chemicals.and_then(|needed| next_threshold(progress, needed)) {
            if progress.authored_seconds >= threshold {
                progress.authored_seconds = threshold;
                return AdvanceResearchResult::ChemicalRequired;
            }
        }

        if progress.authored_seconds >= total_authored_seconds.max(0.0) {
            progress.authored_seconds = total_authored_seconds.max(0.0);
            progress.complete = true;
            self.active_template_id = None;
            self.reports |= report_mask;
            return AdvanceResearchResult::Completed;
        }
        AdvanceResearchResult::InProgress
    }

    /// Consume the currently requested chemical. Wrong chemicals and uses
    /// before the authored threshold are harmless and remain in inventory.
    pub fn provide_chemical(
        &mut self,
        chemical_sym_name: &str,
        chemicals: &PropChemicalNeeded,
    ) -> bool {
        let Some(template_id) = self.active_template_id else {
            return false;
        };
        if !self.accepts_chemical(chemical_sym_name, chemicals) {
            return false;
        }
        let Some(progress) = self.progress.get_mut(&template_id) else {
            return false;
        };
        progress.next_chemical += 1;
        true
    }

    pub fn accepts_chemical(
        &self,
        chemical_sym_name: &str,
        chemicals: &PropChemicalNeeded,
    ) -> bool {
        let Some(template_id) = self.active_template_id else {
            return false;
        };
        let Some(progress) = self.progress.get(&template_id) else {
            return false;
        };
        chemical_due(progress, chemicals)
            .map(|needed| needed.eq_ignore_ascii_case(chemical_sym_name))
            .unwrap_or(false)
    }
}

fn next_threshold(progress: &ResearchProgress, needed: &PropChemicalNeeded) -> Option<f32> {
    let name = needed.chemicals.get(progress.next_chemical)?;
    let threshold = *needed.thresholds_secs.get(progress.next_chemical)?;
    (!name.is_empty() && threshold > 0).then_some(threshold as f32)
}

fn chemical_due<'a>(
    progress: &ResearchProgress,
    needed: &'a PropChemicalNeeded,
) -> Option<&'a str> {
    let threshold = next_threshold(progress, needed)?;
    (progress.authored_seconds >= threshold)
        .then(|| needed.chemicals[progress.next_chemical].as_str())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dark::{
        properties::{ObjectState, PropObjState, PropResearchReport, PropResearchText},
        ss2_entity_info::SystemShock2EntityInfo,
    };
    use shipyard::{Get, View, World};

    use super::*;

    fn toxin_chemicals() -> PropChemicalNeeded {
        PropChemicalNeeded {
            chemicals: std::array::from_fn(|index| match index {
                0 | 2 => "Chem #4".to_owned(),
                1 => "Chem #2".to_owned(),
                _ => String::new(),
            }),
            thresholds_secs: [30, 60, 240, 0, 0, 0, 0],
        }
    }

    #[test]
    fn requires_skill_and_pauses_at_each_authored_chemical_gate() {
        let mut state = ResearchState::default();
        let chemicals = toxin_chemicals();
        assert_eq!(
            state.begin(-1341, 1, 0),
            BeginResearchResult::SkillRequired(1)
        );
        assert_eq!(state.begin(-1341, 1, 1), BeginResearchResult::Started);
        assert_eq!(
            state.advance(-1341, 31.0, 1, 1.0, 600.0, Some(&chemicals), 0x10),
            AdvanceResearchResult::ChemicalRequired
        );
        assert_eq!(state.status(-1341, Some(&chemicals)).authored_seconds, 30.0);
        assert!(!state.accepts_chemical("Chem #2", &chemicals));
        assert!(state.accepts_chemical("chem #4", &chemicals));
        assert!(!state.provide_chemical("Chem #2", &chemicals));
        assert!(state.provide_chemical("Chem #4", &chemicals));
        assert_eq!(
            state.advance(-1341, 30.0, 1, 1.0, 600.0, Some(&chemicals), 0x10),
            AdvanceResearchResult::ChemicalRequired
        );
        assert_eq!(
            state
                .status(-1341, Some(&chemicals))
                .needed_chemical
                .as_deref(),
            Some("Chem #2")
        );
    }

    #[test]
    fn skill_multiplier_completes_and_unlocks_report() {
        let mut state = ResearchState::default();
        assert_eq!(state.begin(-10, 1, 3), BeginResearchResult::Started);
        // skill 3 -> 1 + (3 - 1)^2 = 5 authored seconds / real second.
        assert_eq!(
            state.advance(-10, 20.0, 3, 1.0, 100.0, None, 0x10),
            AdvanceResearchResult::Completed
        );
        assert!(state.is_complete(-10));
        assert!(state.has_report(0x10));
        assert_eq!(state.active_template_id(), None);
    }

    #[test]
    fn serde_round_trip_keeps_partial_and_active_progress() {
        let mut state = ResearchState::default();
        state.begin(-1341, 1, 1);
        state.advance(-1341, 12.5, 1, 1.0, 600.0, None, 0x10);
        let json = serde_json::to_string(&state).unwrap();
        let loaded: ResearchState = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, state);
    }

    #[test]
    fn legacy_held_researchable_backfills_newly_parsed_template_metadata() {
        let mut world = World::new();
        let toxin = world.add_entity((
            RuntimePropCanonicalTemplateId(-1341),
            PropResearchReport(0x20),
        ));
        let mut entity_info = SystemShock2EntityInfo::empty();
        entity_info.entity_to_properties.insert(
            -1341,
            vec![
                Arc::new(Box::new(PropResearchTime(600))),
                Arc::new(Box::new(PropResearchReport(0x10))),
                Arc::new(Box::new(PropResearchText("AATText".to_owned()))),
                Arc::new(Box::new(PropObjState(ObjectState::Unresearched))),
            ],
        );

        backfill_legacy_held_research_components(&mut world, &[toxin], &entity_info);

        assert_eq!(
            world
                .borrow::<View<PropResearchTime>>()
                .unwrap()
                .get(toxin)
                .unwrap()
                .0,
            600
        );
        assert_eq!(
            world
                .borrow::<View<PropResearchReport>>()
                .unwrap()
                .get(toxin)
                .unwrap()
                .0,
            0x20,
            "serialized live values must win over template defaults"
        );
        assert_eq!(
            world
                .borrow::<View<PropObjState>>()
                .unwrap()
                .get(toxin)
                .unwrap()
                .0,
            ObjectState::Unresearched
        );
    }
}
