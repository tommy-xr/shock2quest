//! Persistent, campaign-wide research progress.
//!
//! Research is keyed by the stable gamesys archetype rather than a mission
//! object's runtime id, so dropping an item, changing decks, or saving and
//! loading never loses work.

use std::collections::HashMap;

use dark::{
    properties::{
        Link, Links, PropBaseTechDesc, PropChemicalNeeded, PropObjLookString, PropObjState,
        PropRequiredTechDesc, PropResearchReport, PropResearchText, PropResearchTime,
    },
    ss2_entity_info::SystemShock2EntityInfo,
};
use serde::{Deserialize, Serialize};
use shipyard::{Component, EntityId, Get, UniqueView, View, World};

use crate::{
    mission::{PlayerInfo, mission_core::GlobalSkillParams},
    quest_info::QuestInfo,
    runtime_props::RuntimePropCanonicalTemplateId,
    scripts::script_util::hydrate_template_component,
};

/// Retail's damage filter checks the victim's inherited Organ relation against
/// campaign research, independently of the damage source. Run on delivery to
/// the creature, after a limb proxy forwards its location-scaled hit, so neither
/// the bonus nor the damage readout is applied twice. Healing and player damage
/// do not receive the bonus.
pub(crate) fn damage_with_research_bonus(world: &World, victim: EntityId, amount: f32) -> f32 {
    if amount <= 0.0
        || crate::creature::is_hit_box(world, victim)
        || world
            .borrow::<UniqueView<PlayerInfo>>()
            .is_ok_and(|player| player.entity_id == victim)
    {
        return amount;
    }
    let organ = world.borrow::<View<Links>>().ok().and_then(|links| {
        links
            .get(victim)
            .ok()?
            .to_links
            .iter()
            .rev()
            .find(|link| matches!(link.link, Link::Organ))
            .map(|link| link.to_template_id)
    });
    let researched = organ.is_some_and(|organ| {
        world
            .borrow::<UniqueView<QuestInfo>>()
            .is_ok_and(|quests| quests.research().is_complete(organ))
    });
    if !researched {
        return amount;
    }
    let multiplier = world
        .borrow::<UniqueView<GlobalSkillParams>>()
        .ok()
        .and_then(|params| params.0.as_ref().map(|params| params.organ_damage))
        .filter(|factor| factor.is_finite() && *factor > 0.0)
        .unwrap_or(1.0);
    amount * multiplier
}

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
    /// Run rule for Survive: skill and time still apply, but chemicals do not.
    pub(crate) ignore_chemicals: bool,
    active_template_id: Option<i32>,
    progress: HashMap<i32, ResearchProgress>,
    reports: u32,
}

pub(crate) const CHEMICAL_FREE_STATUS: &str =
    "Research in progress. Chemicals are not required in Survive.";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct ResearchProgress {
    authored_seconds: f32,
    next_chemical: usize,
    chemical_announced: bool,
    complete: bool,
}

impl ResearchProgress {
    fn request_chemical(&mut self) -> AdvanceResearchResult {
        if std::mem::replace(&mut self.chemical_announced, true) {
            AdvanceResearchResult::WaitingForChemical
        } else {
            AdvanceResearchResult::ChemicalRequired
        }
    }
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
    WaitingForChemical,
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
        let chemicals = chemicals.filter(|_| !self.ignore_chemicals);
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
        let chemicals = chemicals.filter(|_| !self.ignore_chemicals);
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
            return progress.request_chemical();
        }

        let skill_above_one = (skill.max(1) - 1) as f32;
        let multiplier = 1.0 + research_factor * skill_above_one * skill_above_one;
        progress.authored_seconds += real_seconds.max(0.0) * multiplier.max(0.0);

        if let Some(threshold) = chemicals.and_then(|needed| next_threshold(progress, needed)) {
            if progress.authored_seconds >= threshold {
                progress.authored_seconds = threshold;
                return progress.request_chemical();
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
        progress.chemical_announced = false;
        true
    }

    pub fn accepts_chemical(
        &self,
        chemical_sym_name: &str,
        chemicals: &PropChemicalNeeded,
    ) -> bool {
        if self.ignore_chemicals {
            return false;
        }
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

    #[test]
    fn organ_bonus_requires_matching_completion_and_skips_player_proxy_and_healing() {
        use crate::creature::{HitBoxType, RuntimePropHitBox};
        use dark::properties::ToLink;
        use shipyard::UniqueViewMut;

        let mut world = World::new();
        let links = Links {
            to_links: vec![ToLink {
                to_template_id: -1095,
                to_entity_id: None,
                link: Link::Organ,
            }],
        };
        let victim = world.add_entity(links.clone());
        let unrelated = world.add_entity(Links::empty());
        let player = world.add_entity(links.clone());
        let proxy = world.add_entity((
            links,
            RuntimePropHitBox {
                parent_entity_id: victim,
                hit_box_type: HitBoxType::Head,
                joint_id: 9,
            },
        ));
        world.add_unique(PlayerInfo {
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: unrelated,
        });
        world.add_unique(GlobalSkillParams(Some(dark::gamesys::SkillParams {
            inaccuracy_degrees: 0.0,
            weapon_break_factor: 0.0,
            research_factor: 1.0,
            damage_modifier: 0.0,
            organ_damage: 1.25,
        })));
        world.add_unique(QuestInfo::new());
        assert_eq!(damage_with_research_bonus(&world, victim, 4.0), 4.0);
        {
            let mut quests = world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
            let research = quests.research_mut();
            research.begin(-229, 1, 1);
            research.advance(-229, 10.0, 1, 1.0, 10.0, None, 1);
            research.begin(-1095, 1, 1);
            research.advance(-1095, 5.0, 1, 1.0, 10.0, None, 1);
        }
        assert_eq!(
            damage_with_research_bonus(&world, victim, 4.0),
            4.0,
            "a report bit or unrelated completed organ is not matching completion"
        );
        world
            .borrow::<UniqueViewMut<QuestInfo>>()
            .unwrap()
            .research_mut()
            .advance(-1095, 5.0, 1, 1.0, 10.0, None, 1);
        assert_eq!(damage_with_research_bonus(&world, victim, 4.0), 5.0);
        for target in [unrelated, player, proxy] {
            assert_eq!(damage_with_research_bonus(&world, target, 4.0), 4.0);
        }
        for amount in [0.0, -4.0] {
            assert_eq!(damage_with_research_bonus(&world, victim, amount), amount);
        }
        world
            .borrow::<UniqueViewMut<GlobalSkillParams>>()
            .unwrap()
            .0 = None;
        assert_eq!(
            damage_with_research_bonus(&world, victim, 4.0),
            4.0,
            "missing gamesys parameters leave damage unchanged"
        );
    }

    #[test]
    fn most_specific_organ_link_overrides_inherited_research() {
        use dark::properties::ToLink;

        let mut world = World::new();
        let victim = world.add_entity(Links {
            to_links: [-1095, -229]
                .map(|to_template_id| ToLink {
                    to_template_id,
                    to_entity_id: None,
                    link: Link::Organ,
                })
                .to_vec(),
        });
        let mut quests = QuestInfo::new();
        quests.research_mut().begin(-1095, 1, 1);
        quests
            .research_mut()
            .advance(-1095, 10.0, 1, 1.0, 10.0, None, 0);
        world.add_unique(quests);
        world.add_unique(GlobalSkillParams(Some(dark::gamesys::SkillParams {
            inaccuracy_degrees: 0.0,
            weapon_break_factor: 0.0,
            research_factor: 1.0,
            damage_modifier: 0.0,
            organ_damage: 1.25,
        })));
        assert_eq!(damage_with_research_bonus(&world, victim, 4.0), 4.0);
    }

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
    fn chemical_free_runs_keep_skill_timing_reports_and_saved_progress() {
        let mut state = ResearchState {
            ignore_chemicals: true,
            ..Default::default()
        };
        let chemicals = toxin_chemicals();
        assert_eq!(
            state.begin(-1341, 2, 1),
            BeginResearchResult::SkillRequired(2)
        );
        assert_eq!(state.begin(-1341, 2, 2), BeginResearchResult::Started);
        // Research 2 advances two authored seconds per real second.
        assert_eq!(
            state.advance(-1341, 150.0, 2, 1.0, 600.0, Some(&chemicals), 0x10),
            AdvanceResearchResult::InProgress
        );
        let status = state.status(-1341, Some(&chemicals));
        assert_eq!(status.authored_seconds, 300.0);
        assert_eq!(status.needed_chemical, None);
        assert!(!state.provide_chemical("Chem #4", &chemicals));
        let mut loaded: ResearchState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(
            loaded.advance(-1341, 150.0, 2, 1.0, 600.0, Some(&chemicals), 0x10),
            AdvanceResearchResult::Completed
        );
        assert!(loaded.has_report(0x10));
        assert!(loaded.is_complete(-1341));
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
    fn chemical_requests_are_one_shot_across_wait_resume_and_save() {
        let mut state = ResearchState::default();
        let mut chemicals = toxin_chemicals();
        chemicals.thresholds_secs[0] = 1;
        state.begin(-1341, 1, 1);
        assert_eq!(
            state.advance(-1341, 1.0, 1, 1.0, 600.0, Some(&chemicals), 0x10),
            AdvanceResearchResult::ChemicalRequired
        );
        state.suspend();
        let mut loaded: ResearchState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        loaded.begin(-1341, 1, 1);
        assert!(!loaded.provide_chemical("Chem #2", &chemicals));
        assert_eq!(
            loaded.advance(-1341, 90.0, 1, 1.0, 600.0, Some(&chemicals), 0x10),
            AdvanceResearchResult::WaitingForChemical
        );
        assert!(loaded.provide_chemical("Chem #4", &chemicals));
        assert_eq!(
            loaded.advance(-1341, 90.0, 1, 1.0, 600.0, Some(&chemicals), 0x10),
            AdvanceResearchResult::ChemicalRequired
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
        assert_eq!(
            state.advance(-10, 20.0, 3, 1.0, 100.0, None, 0x10),
            AdvanceResearchResult::InProgress,
            "completion is a transition, not a per-frame notification"
        );
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
