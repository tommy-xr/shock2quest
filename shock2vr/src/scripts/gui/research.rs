//! Retail-shaped research MFD for carried researchable objects.

use cgmath::{Vector2, Vector3, vec2};
use dark::properties::{
    PropBaseTechDesc, PropChemicalNeeded, PropObjIcon, PropObjShortName, PropResearchText,
    PropResearchTime,
};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    gui::{self, Gui, GuiComponent, GuiConfig, GuiCursor},
    mission::GlobalEntityMetadata,
    player_stats::Skill,
    quest_info::QuestInfo,
    scripts::{Effect, script_util::entity_class_template_id},
};

use super::PanelText;
#[cfg(test)]
use super::media::wrap_text;

const PANEL_W: f32 = 188.0;
const PANEL_H: f32 = 296.0;
const TEXT_X: f32 = 15.0;
const TEXT_TOP: f32 = 153.0;
// shkrsrch text_rect has right edge 138, so its width is 138 - 15.
const TEXT_W: f32 = 123.0;
// Stop at the report button; use the same measured line pitch as rendering.
fn page_lines(world: &World) -> usize {
    ((layout::REPORTS.y - TEXT_TOP) / PanelText::line_height(world))
        .floor()
        .max(1.0) as usize
}
const SCROLL_X: f32 = 159.0;
const PGUP_Y: f32 = 174.0;
const PGDN_Y: f32 = 203.0;

/// Authored RESEARCH.PCX slots shared by the live item and journal views.
pub(crate) mod layout {
    use crate::ui::Rect;
    pub const SPECIMEN: Rect = Rect::new(15.0, 14.0, 138.0, 109.0);
    pub const TITLE: Rect = Rect::new(24.0, 133.0, 129.0, 12.0);
    pub const PROGRESS: Rect = Rect::new(15.0, 267.0, 138.0, 17.0);
    pub const PERCENT: Rect = Rect::new(15.0, 270.0, 138.0, 14.0);
    pub const REPORTS: Rect = Rect::new(13.0, 243.0, 142.0, 22.0);
}

pub struct ResearchGui;

#[derive(Clone, Debug, Default)]
pub struct ResearchGuiState {
    scroll: usize,
}

#[derive(Clone)]
pub enum ResearchGuiMsg {
    ToggleReport,
    Suspend,
    PageUp,
    PageDown,
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

        if let Ok(icon) = world.borrow::<View<PropObjIcon>>().unwrap().get(entity_id) {
            components.push(
                gui::image(&format!("{}.pcx", icon.0))
                    .with_object_icon()
                    .with_rect(layout::SPECIMEN),
            );
        }
        let title = if status.complete {
            world
                .borrow::<View<PropObjShortName>>()
                .ok()
                .and_then(|names| {
                    names.get(entity_id).ok().map(|name| {
                        PanelText::string(
                            world,
                            "objshort",
                            name.0.split(':').next().unwrap_or(&name.0).trim(),
                            &localized_fallback(&name.0),
                        )
                    })
                })
                .unwrap_or_else(|| "Research complete".into())
        } else {
            PanelText::string(world, "research", "NameUnresearched", "Unresearched Object")
        };
        components.push(PanelText::text(&title, layout::TITLE));
        // shkrsrch.cpp: progress well at (15,267), not the name at y=133.
        // RESPROG is a solid fill, so sizing it preserves its authored pixels.
        if fraction > 0.0 {
            components.push(gui::image("iface/resprog.pcx").with_rect(crate::ui::Rect {
                w: layout::PROGRESS.w * fraction,
                ..layout::PROGRESS
            }));
        }
        components.push(PanelText::centered(
            &format!("{:.1} %", fraction * 100.0),
            layout::PERCENT,
        ));
        components.push(
            gui::button(ResearchGuiMsg::ToggleReport)
                .with_image("iface/report0.pcx")
                .with_hover(gui::ButtonHoverBehavior::Texture(
                    "iface/report1.pcx".into(),
                ))
                .with_label("Research reports")
                .with_rect(layout::REPORTS),
        );
        if status.active && !status.complete {
            components.push(
                gui::button(ResearchGuiMsg::Suspend)
                    .with_image("iface/sus0.pcx")
                    .with_hover(gui::ButtonHoverBehavior::Texture("iface/sus1.pcx".into()))
                    .with_label("Suspend research")
                    .with_position(vec2(157.0, 152.0))
                    .with_size(vec2(18.0, 134.0)),
            );
        }

        let body = research_text(world, entity_id);

        let lines = PanelText::wrap(world, &body, TEXT_W);
        let line_height = PanelText::line_height(world);
        let start = state
            .scroll
            .min(lines.len().saturating_sub(page_lines(world)));
        for (index, line) in lines[start..].iter().take(page_lines(world)).enumerate() {
            if !line.is_empty() {
                components.push(
                    gui::text(line)
                        .with_position(vec2(TEXT_X, TEXT_TOP + index as f32 * line_height))
                        .with_size(vec2(TEXT_W, line_height)),
                );
            }
        }
        if !status.active && lines.len() > page_lines(world) {
            components.push(
                gui::button(ResearchGuiMsg::PageUp)
                    .with_image("pgup0.pcx")
                    .with_label("Research report previous page")
                    .with_position(vec2(SCROLL_X, PGUP_Y))
                    .with_size(vec2(18.0, 26.0)),
            );
            components.push(
                gui::button(ResearchGuiMsg::PageDown)
                    .with_image("pgdn0.pcx")
                    .with_label("Research report next page")
                    .with_position(vec2(SCROLL_X, PGDN_Y))
                    .with_size(vec2(18.0, 26.0)),
            );
        }
        for component in &mut components {
            match component {
                GuiComponent::Text {
                    font, fit_to_rect, ..
                } => {
                    *font = crate::ui::MFD_FONT.into();
                    *fit_to_rect = true;
                }
                GuiComponent::Image { alpha, .. } | GuiComponent::Button { alpha, .. } => {
                    *alpha = 1.0
                }
                _ => {}
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
        entity_id: EntityId,
        world: &World,
        state: &ResearchGuiState,
        msg: &ResearchGuiMsg,
    ) -> (ResearchGuiState, Effect) {
        match msg {
            ResearchGuiMsg::ToggleReport => (state.clone(), Effect::OpenResearchReports),
            ResearchGuiMsg::Suspend => (ResearchGuiState::default(), Effect::SuspendResearch),
            ResearchGuiMsg::PageUp => (
                ResearchGuiState {
                    scroll: state.scroll.saturating_sub(page_lines(world)),
                },
                Effect::NoEffect,
            ),
            ResearchGuiMsg::PageDown => {
                let max_scroll = PanelText::wrap(world, &research_text(world, entity_id), TEXT_W)
                    .len()
                    .saturating_sub(page_lines(world));
                (
                    ResearchGuiState {
                        scroll: (state.scroll + page_lines(world)).min(max_scroll),
                    },
                    Effect::NoEffect,
                )
            }
        }
    }

    fn on_frob(&self, entity_id: EntityId, _world: &World) -> Effect {
        tracing::info!(?entity_id, "researchable frob opened Research MFD");
        Effect::BeginResearch { entity_id }
    }
}

fn research_text(world: &World, entity_id: EntityId) -> String {
    let template_id = entity_class_template_id(world, entity_id).unwrap_or(0);
    let chemicals_view = world.borrow::<View<PropChemicalNeeded>>().unwrap();
    let chemicals = chemicals_view.get(entity_id).ok();
    let status = world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|quests| quests.research().status(template_id, chemicals))
        .unwrap_or_else(|_| crate::research::ResearchStatus {
            authored_seconds: 0.0,
            active: false,
            complete: false,
            needed_chemical: None,
        });
    let required_skill = world
        .borrow::<View<PropBaseTechDesc>>()
        .unwrap()
        .get(entity_id)
        .map(|required| required.0.research().max(1))
        .unwrap_or(1);
    let player_skill = crate::scripts::script_util::player_skill_level(world, Skill::Research);
    if player_skill < required_skill && status.authored_seconds == 0.0 {
        format!(
            "This item requires a Research skill of {required_skill}. Use a Tech Upgrade Unit to train Research."
        )
    } else if let Some(chemical) = &status.needed_chemical {
        format!(
            "Research paused. Required chemical: {}.",
            chemical_display_name(world, chemical)
        )
    } else if status.complete {
        "Research complete. The item is now ready for use. Select the report button to review your findings.".to_owned()
    } else if status.active {
        property_fallback(world, entity_id)
    } else {
        "Research suspended. Double-click this item to resume.".to_owned()
    }
}

fn property_fallback(world: &World, entity_id: EntityId) -> String {
    world
        .borrow::<View<PropResearchText>>()
        .ok()
        .and_then(|view| {
            view.get(entity_id).ok().map(|text| {
                PanelText::string(
                    world,
                    "rsrchtxt",
                    text.0.split(':').next().unwrap_or(&text.0).trim(),
                    &localized_fallback(&text.0),
                )
            })
        })
        .unwrap_or_else(|| "Research in progress.".to_owned())
}

/// The English text out of an object string (`key: "text"`), for a data
/// install whose string table does not resolve the key.
pub(crate) fn localized_fallback(value: &str) -> String {
    value
        .split_once('"')
        .and_then(|(_, tail)| tail.rsplit_once('"').map(|(text, _)| text))
        .unwrap_or(value)
        .replace("\\n", "\n")
}

pub(crate) fn chemical_display_name(world: &World, sym_name: &str) -> String {
    let key = sym_name.to_ascii_lowercase();
    world
        .borrow::<UniqueView<GlobalEntityMetadata>>()
        .ok()
        .and_then(|metadata| {
            metadata
                .0
                .get(&key)
                .and_then(|chemical| chemical.obj_short_name.as_deref())
                .map(localized_fallback)
        })
        .unwrap_or_else(|| sym_name.to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::mission::EntityMetadata;

    use super::*;

    #[test]
    fn wrapping_preserves_blank_paragraphs() {
        assert_eq!(wrap_text("first\n\nsecond", 20), ["first", "", "second"]);
    }

    #[test]
    fn chemical_name_comes_from_authored_short_name() {
        let world = World::new();
        world.add_unique(GlobalEntityMetadata(HashMap::from([(
            "chem #7".to_owned(),
            EntityMetadata {
                template_id: -144,
                obj_icon: None,
                obj_short_name: Some("Chem_p7: \"Californium (Cf)\"".to_owned()),
                obj_name: None,
            },
        )])));

        assert_eq!(chemical_display_name(&world, "Chem #7"), "Californium (Cf)");
    }
}
