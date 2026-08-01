//! Retail-shaped research MFD for carried researchable objects.

use cgmath::{Vector2, Vector3, vec2};
use dark::properties::{
    PropBaseTechDesc, PropChemicalNeeded, PropObjLookString, PropResearchReport, PropResearchText,
    PropResearchTime,
};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    gui::{self, Gui, GuiComponent, GuiConfig, GuiCursor},
    player_stats::Skill,
    quest_info::QuestInfo,
    scripts::{Effect, script_util::entity_class_template_id},
};

const PANEL_W: f32 = 188.0;
const PANEL_H: f32 = 296.0;
const TEXT_X: f32 = 15.0;
const TEXT_TOP: f32 = 153.0;
const TEXT_W: f32 = 138.0;
const LINE_H: f32 = 11.0;
const PAGE_LINES: usize = 10;
const WRAP_CHARS: usize = 27;

pub struct ResearchGui;

#[derive(Clone, Debug, Default)]
pub struct ResearchGuiState {
    show_report: bool,
}

#[derive(Clone)]
pub enum ResearchGuiMsg {
    ToggleReport,
}

impl Gui<ResearchGuiState, ResearchGuiMsg> for ResearchGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &ResearchGuiState,
    ) -> Vec<GuiComponent<ResearchGuiMsg>> {
        let mut components = vec![
            gui::image("iface/research.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(PANEL_W, PANEL_H)),
        ];

        let Some(template_id) = entity_class_template_id(world, entity_id) else {
            return components;
        };
        let chemicals_view = world.borrow::<View<PropChemicalNeeded>>().unwrap();
        let chemicals = chemicals_view.get(entity_id).ok();
        let required_skill = world
            .borrow::<View<PropBaseTechDesc>>()
            .unwrap()
            .get(entity_id)
            .map(|required| required.0.research().max(1))
            .unwrap_or(1);
        let player_skill = world
            .borrow::<UniqueView<QuestInfo>>()
            .map(|quests| quests.player_stats().skill_level(Skill::Research))
            .unwrap_or(0);
        let status = world
            .borrow::<UniqueView<QuestInfo>>()
            .map(|quests| quests.research().status(template_id, chemicals))
            .unwrap_or_else(|_| crate::research::ResearchStatus {
                authored_seconds: 0.0,
                active: false,
                complete: false,
                needed_chemical: None,
            });
        let total = world
            .borrow::<View<PropResearchTime>>()
            .unwrap()
            .get(entity_id)
            .map(|time| time.0.max(1) as f32)
            .unwrap_or(1.0);
        let fraction = (status.authored_seconds / total).clamp(0.0, 1.0);

        // The original overlays RESPROG on this slot; cropping is unavailable
        // in the generic GUI primitive, so retain the authored bar art and add
        // an exact numeric percentage for unambiguous progress feedback.
        components.push(
            gui::image("iface/resprog.pcx")
                .with_position(vec2(15.0, 131.0))
                .with_size(vec2(138.0 * fraction.max(0.01), 17.0)),
        );
        components.push(
            gui::text(&format!("Research: {:.0}%", fraction * 100.0))
                .with_position(vec2(18.0, 134.0))
                .with_size(vec2(132.0, LINE_H)),
        );

        let report_mask = world
            .borrow::<View<PropResearchReport>>()
            .unwrap()
            .get(entity_id)
            .map(|report| report.0)
            .unwrap_or(0);
        if status.complete && report_mask != 0 {
            components.push(
                gui::button(ResearchGuiMsg::ToggleReport)
                    .with_image("iface/report0.pcx")
                    .with_label("Research report")
                    .with_position(vec2(13.0, 243.0))
                    .with_size(vec2(142.0, 22.0)),
            );
        }

        let body = if player_skill < required_skill && status.authored_seconds == 0.0 {
            format!(
                "This item requires a Research skill of {required_skill}. Use a Tech Upgrade Unit to train Research."
            )
        } else if let Some(chemical) = &status.needed_chemical {
            format!(
                "Research paused. Required chemical: {}.",
                chemical_display_name(chemical)
            )
        } else if status.complete && state.show_report {
            property_fallback(world, entity_id, true)
        } else if status.complete {
            "Research complete. The item is now ready for use. Select the report button to review your findings.".to_owned()
        } else if status.active {
            property_fallback(world, entity_id, false)
        } else {
            "Research suspended. Double-click this item to resume.".to_owned()
        };

        for (index, line) in wrap_text(&body, WRAP_CHARS)
            .into_iter()
            .take(PAGE_LINES)
            .enumerate()
        {
            if !line.is_empty() {
                components.push(
                    gui::text(&line)
                        .with_position(vec2(TEXT_X, TEXT_TOP + index as f32 * LINE_H))
                        .with_size(vec2(TEXT_W, LINE_H)),
                );
            }
        }
        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.1),
            screen_size_in_pixels: Vector2::new(PANEL_W, PANEL_H),
        }
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        _world: &World,
        state: &ResearchGuiState,
        _msg: &ResearchGuiMsg,
    ) -> (ResearchGuiState, Effect) {
        (
            ResearchGuiState {
                show_report: !state.show_report,
            },
            Effect::NoEffect,
        )
    }

    fn on_frob(&self, entity_id: EntityId, _world: &World) -> Effect {
        tracing::info!(?entity_id, "researchable frob opened Research MFD");
        Effect::BeginResearch { entity_id }
    }
}

fn property_fallback(world: &World, entity_id: EntityId, report: bool) -> String {
    if report {
        world
            .borrow::<View<PropObjLookString>>()
            .ok()
            .and_then(|view| {
                view.get(entity_id)
                    .ok()
                    .map(|text| localized_fallback(&text.0))
            })
            .unwrap_or_else(|| "Research report available.".to_owned())
    } else {
        world
            .borrow::<View<PropResearchText>>()
            .ok()
            .and_then(|view| {
                view.get(entity_id)
                    .ok()
                    .map(|text| localized_fallback(&text.0))
            })
            .unwrap_or_else(|| "Research in progress.".to_owned())
    }
}

fn localized_fallback(value: &str) -> String {
    value
        .split_once('"')
        .and_then(|(_, tail)| tail.rsplit_once('"').map(|(text, _)| text))
        .unwrap_or(value)
        .replace("\\n", "\n")
}

fn chemical_display_name(sym_name: &str) -> String {
    match sym_name.to_ascii_lowercase().as_str() {
        "chem #2" => "Vanadium (V)".to_owned(),
        "chem #4" => "Antimony (Sb)".to_owned(),
        _ => sym_name.to_owned(),
    }
}

fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current = word.to_owned();
            } else if current.len() + word.len() < max_chars {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_owned();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}
