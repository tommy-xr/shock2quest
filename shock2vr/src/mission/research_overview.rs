//! Read-only research journal, resolved by archetype rather than live item.

use crate::{
    quest_info::QuestInfo, research::ResearchState,
    scripts::script_util::hydrate_template_component,
};
use dark::{
    properties::{
        PropChemicalNeeded, PropObjLookString, PropObjName, PropResearchTime, PropSymName,
    },
    ss2_entity_info::SystemShock2EntityInfo,
};
use engine::assets::asset_cache::AssetCache;
use shipyard::{UniqueView, World};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct ResearchCatalog(HashMap<i32, Project>);

struct Project {
    name: String,
    seconds: f32,
    chemicals: Option<PropChemicalNeeded>,
    report: String,
}

impl ResearchCatalog {
    pub(super) fn body(
        &mut self,
        world: &World,
        assets: &mut AssetCache,
        info: &SystemShock2EntityInfo,
    ) -> String {
        let Ok(quests) = world.borrow::<UniqueView<QuestInfo>>() else {
            return "No research projects yet.".into();
        };
        for id in quests.research().project_template_ids() {
            self.0.entry(id).or_insert_with(|| {
                let name = hydrate_template_component::<PropObjName>(id, info).map(|p| p.0);
                let symbol = hydrate_template_component::<PropSymName>(id, info).map(|p| p.0);
                let look = hydrate_template_component::<PropObjLookString>(id, info).map(|p| p.0);
                let names = assets.get(&dark::importers::STRINGS_IMPORTER, "objname.str");
                let title = name
                    .as_deref()
                    .map(|name| dark::importers::resolve_localized_property_string(name, &names))
                    .or(symbol.clone())
                    .unwrap_or_else(|| "Research project".into());
                let descriptions = assets.get(&dark::importers::STRINGS_IMPORTER, "objlooks.str");
                Project {
                    name: title,
                    seconds: hydrate_template_component::<PropResearchTime>(id, info)
                        .map(|p| p.0.max(1) as f32)
                        .unwrap_or(1.0),
                    chemicals: hydrate_template_component::<PropChemicalNeeded>(id, info),
                    report: dark::importers::resolve_object_description(
                        look.as_deref(),
                        name.as_deref(),
                        symbol.as_deref(),
                        &descriptions,
                    )
                    .unwrap_or_else(|| "Research complete. No written report available.".into()),
                }
            });
        }
        self.describe(quests.research(), |name| {
            crate::scripts::gui::chemical_display_name(world, name)
        })
    }

    fn describe(&self, state: &ResearchState, chemical_name: impl Fn(&str) -> String) -> String {
        let ids = state.project_template_ids();
        if ids.is_empty() {
            return "No research projects yet. Use a researchable item to begin a project. Opening this overview does not start research.".into();
        }
        let mut body = String::new();
        for id in ids {
            let Some(project) = self.0.get(&id) else {
                continue;
            };
            let status = state.status(id, project.chemicals.as_ref());
            body.push_str(&format!("{}\n", project.name));
            if status.complete {
                body.push_str(&format!("Complete\nReport: {}\n\n", project.report));
            } else {
                let fraction =
                    (status.authored_seconds / project.seconds * 100.0).clamp(0.0, 100.0);
                body.push_str(&format!(
                    "{}: {fraction:.0} percent\n",
                    if status.active { "Active" } else { "Suspended" }
                ));
                if let Some(chemical) = status.needed_chemical {
                    body.push_str(&format!("Chemical needed: {}\n", chemical_name(&chemical)));
                }
                body.push('\n');
            }
        }
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_report_survives_without_a_live_item_and_reading_is_pure() {
        let mut state = ResearchState::default();
        state.begin(-42, 1, 1);
        state.advance(-42, 10.0, 1, 1.0, 1.0, None, 1);
        let before = serde_json::to_value(&state).unwrap();
        let catalog = ResearchCatalog(HashMap::from([(
            -42,
            Project {
                name: "Sample".into(),
                seconds: 1.0,
                chemicals: None,
                report: "Findings preserved.".into(),
            },
        )]));
        let body = catalog.describe(&state, str::to_owned);
        assert!(body.contains("Complete"));
        assert!(body.contains("Findings preserved."));
        assert_eq!(serde_json::to_value(&state).unwrap(), before);
    }
}
