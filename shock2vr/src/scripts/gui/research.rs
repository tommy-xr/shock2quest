//! Retail-shaped research MFD for carried researchable objects.

use cgmath::{Vector2, Vector3, vec2};
use dark::properties::{
    PropChemicalNeeded, PropObjIcon, PropObjShortName, PropResearchText, PropResearchTime,
};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    gui::{self, Gui, GuiComponent, GuiConfig, GuiCursor},
    mission::GlobalEntityMetadata,
    quest_info::QuestInfo,
    research::ResearchStatus,
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

/// The Research MFD for a researchable item. `ResearchableUseScript` items
/// (INT Boost, Worm Launcher, ...) set `usable`: once researched, frobbing them
/// is a normal use, so the panel stays shut and leaves the frob to their other script.
pub struct ResearchGui {
    pub usable: bool,
}

impl ResearchGui {
    fn defers_to_use(&self, entity_id: EntityId, world: &World) -> bool {
        self.usable && !crate::research::is_unresearched(world, entity_id)
    }
}

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

        if entity_class_template_id(world, entity_id).is_none() {
            return components;
        }
        let status = research_status(world, entity_id);
        let fraction = research_fraction(world, entity_id, &status);

        if let Ok(icon) = world.borrow::<View<PropObjIcon>>().unwrap().get(entity_id) {
            components.push(
                gui::image(&format!("{}.pcx", icon.0))
                    .with_object_icon()
                    .with_rect(layout::SPECIMEN),
            );
        }
        let title = if status.complete {
            short_name(world, entity_id).unwrap_or_else(|| "Research complete".into())
        } else {
            unresearched_name(world)
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
            &percent_text(fraction),
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

        let body = research_text(world, entity_id, &status);

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
                let max_scroll = PanelText::wrap(
                    world,
                    &research_text(world, entity_id, &research_status(world, entity_id)),
                    TEXT_W,
                )
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

    fn opens_on_frob(&self, entity_id: EntityId, world: &World) -> bool {
        !self.defers_to_use(entity_id, world)
    }

    fn on_frob(&self, entity_id: EntityId, world: &World) -> Effect {
        if self.defers_to_use(entity_id, world) {
            return Effect::NoEffect;
        }
        tracing::info!(?entity_id, "researchable frob opened Research MFD");
        Effect::BeginResearch { entity_id }
    }

    /// A chemical this specimen's research waits on (VR: brought to it in the
    /// other hand) is taken as a tool; any other offer keeps the default path.
    fn accepts_tool(&self, entity_id: EntityId, world: &World, tool: EntityId) -> bool {
        crate::research::chemical_ready_for(world, tool, entity_id)
    }

    /// A chemical it does not take is refused, never deposited inside it.
    fn on_provide_for_consumption(
        &self,
        entity_id: EntityId,
        world: &World,
        provided_entity_id: EntityId,
    ) -> Option<Effect> {
        if self.accepts_tool(entity_id, world, provided_entity_id) {
            return Some(Effect::UseResearchChemical {
                entity_id: provided_entity_id,
                specimen: Some(entity_id),
                released: false,
            });
        }
        crate::scripts::script_util::entity_has_script(world, provided_entity_id, "Chemical")
            .then_some(Effect::NoEffect)
    }
}

fn research_status(world: &World, entity_id: EntityId) -> ResearchStatus {
    let template_id = entity_class_template_id(world, entity_id).unwrap_or(0);
    let chemicals_view = world.borrow::<View<PropChemicalNeeded>>().unwrap();
    let chemicals = chemicals_view.get(entity_id).ok();
    world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|quests| quests.research().status(template_id, chemicals))
        .unwrap_or_else(|_| ResearchStatus {
            authored_seconds: 0.0,
            active: false,
            complete: false,
            needed_chemical: None,
        })
}

fn research_fraction(world: &World, entity_id: EntityId, status: &ResearchStatus) -> f32 {
    let total = world
        .borrow::<View<PropResearchTime>>()
        .unwrap()
        .get(entity_id)
        .map(|time| time.0.max(1) as f32)
        .unwrap_or(1.0);
    (status.authored_seconds / total).clamp(0.0, 1.0)
}

fn percent_text(fraction: f32) -> String {
    format!("{:.1} %", fraction * 100.0)
}

fn short_name(world: &World, entity_id: EntityId) -> Option<String> {
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
}

fn unresearched_name(world: &World) -> String {
    PanelText::string(world, "research", "NameUnresearched", "Unresearched Object")
}

/// The HUD line for research begun with no Research MFD showing (a VR
/// specimen in hand): the panel's own name, percentage and status text.
pub(crate) fn research_message(world: &World, entity_id: EntityId) -> String {
    let status = research_status(world, entity_id);
    let name = short_name(world, entity_id).unwrap_or_else(|| unresearched_name(world));
    status_line(
        &name,
        &status,
        research_fraction(world, entity_id, &status),
        research_text(world, entity_id, &status),
    )
}

/// e.g. "Researching Grub Organ: 12.0 %"; a stalled project (skill too low,
/// chemical needed, suspended) shows the panel's body text instead.
fn status_line(name: &str, status: &ResearchStatus, fraction: f32, body: String) -> String {
    if status.complete {
        format!("{name}: Research complete")
    } else if status.active && status.needed_chemical.is_none() {
        format!("Researching {name}: {}", percent_text(fraction))
    } else {
        body
    }
}

/// HUD line for firing a weapon not yet researched.
pub(crate) fn unresearched_message(world: &World) -> String {
    PanelText::string(
        world,
        "research",
        "QueryUnresearched",
        "This item is not yet researched.",
    )
}

/// HUD line for a chemical the player used: taken by the active project or not.
pub(crate) fn chemical_message(world: &World, consumed: bool) -> String {
    if consumed {
        PanelText::string(world, "misc", "ChemUsed", "Chemical used in research.")
    } else {
        PanelText::string(
            world,
            "misc",
            "ChemNotNeeded",
            "Please insert correct chemical.",
        )
    }
}

/// HUD line when the active project stalls on a chemical, e.g.
/// "Chemical Needed: Antimony (Sb)".
pub(crate) fn chemical_needed_message(world: &World, entity_id: EntityId) -> Option<String> {
    let chemical = research_status(world, entity_id).needed_chemical?;
    Some(
        PanelText::string(world, "misc", "ResearchChemNeeded", "Chemical Needed: %s").replacen(
            "%s",
            &chemical_display_name(world, &chemical),
            1,
        ),
    )
}

/// The one HUD line `BeginResearch` reports: a skill refusal always, so flat
/// (panel up) and VR word it alike; otherwise the status, only when no
/// Research MFD shows it.
pub(crate) fn begin_research_message(
    world: &World,
    entity_id: EntityId,
    skill_required: Option<i32>,
    mfd_showing: bool,
) -> Option<String> {
    match skill_required {
        Some(required) => Some(PanelText::format(
            &PanelText::string(
                world,
                "misc",
                "ResearchSkillNeeded",
                "This item requires a research skill of %d to be researched.",
            ),
            &[required],
        )),
        None => (!mfd_showing).then(|| research_message(world, entity_id)),
    }
}

fn research_text(world: &World, entity_id: EntityId, status: &ResearchStatus) -> String {
    let (required_skill, player_skill) = crate::research::research_skill(world, entity_id);
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
        if world
            .borrow::<UniqueView<QuestInfo>>()
            .is_ok_and(|q| q.research().ignore_chemicals)
        {
            crate::research::CHEMICAL_FREE_STATUS.to_owned()
        } else {
            property_fallback(world, entity_id)
        }
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

    use dark::properties::{ObjectState, PropObjState, PropTemplateId};

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

    #[test]
    fn hud_line_reports_progress_or_the_panel_status() {
        let status = |active, complete, chemical: Option<&str>| ResearchStatus {
            authored_seconds: 3.0,
            active,
            complete,
            needed_chemical: chemical.map(str::to_owned),
        };
        let body = || "panel body".to_owned();
        assert_eq!(
            status_line("Grub Organ", &status(true, false, None), 0.125, body()),
            "Researching Grub Organ: 12.5 %"
        );
        assert_eq!(
            status_line("Grub Organ", &status(false, true, None), 1.0, body()),
            "Grub Organ: Research complete"
        );
        // Skill too low, chemical needed or suspended: the panel's own words.
        assert_eq!(
            status_line("Grub Organ", &status(true, false, Some("Sb")), 0.5, body()),
            "panel body"
        );
        assert_eq!(
            status_line("Grub Organ", &status(false, false, None), 0.0, body()),
            "panel body"
        );
    }

    fn misc_table(world: &World) {
        world.add_unique(crate::string_table::StringTable::from_table(
            "misc.str",
            HashMap::from([
                ("chemused".to_owned(), "Used.".to_owned()),
                ("chemnotneeded".to_owned(), "Wrong.".to_owned()),
                ("researchskillneeded".to_owned(), "Need %d.".to_owned()),
                ("researchchemneeded".to_owned(), "Need: %s".to_owned()),
            ]),
        ));
    }

    #[test]
    fn chemical_messages_come_from_the_misc_table() {
        let world = World::new();
        assert_eq!(chemical_message(&world, true), "Chemical used in research.");
        assert_eq!(
            chemical_message(&world, false),
            "Please insert correct chemical."
        );
        misc_table(&world);
        assert_eq!(chemical_message(&world, true), "Used.");
        assert_eq!(chemical_message(&world, false), "Wrong.");
    }

    #[test]
    fn begin_research_reports_skill_always_and_status_only_without_an_mfd() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let specimen = world.add_entity((
            PropTemplateId { template_id: -1095 },
            PropObjShortName("Organ_1: \"Grub Organ\"".to_owned()),
        ));
        let skill = Some("This item requires a research skill of 3 to be researched.".to_owned());
        assert_eq!(
            begin_research_message(&world, specimen, Some(3), true),
            skill
        );
        assert_eq!(
            begin_research_message(&world, specimen, Some(3), false),
            skill
        );
        assert_eq!(begin_research_message(&world, specimen, None, true), None);
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .research_mut()
            .begin(-1095, 0, 0);
        assert_eq!(
            begin_research_message(&world, specimen, None, false).as_deref(),
            Some("Researching Grub Organ: 0.0 %")
        );
        misc_table(&world);
        assert_eq!(
            begin_research_message(&world, specimen, Some(2), true).as_deref(),
            Some("Need 2.")
        );
    }

    #[test]
    fn chemical_due_names_the_chemical() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        world.add_unique(GlobalEntityMetadata(HashMap::from([(
            "chem #3".to_owned(),
            EntityMetadata {
                template_id: -145,
                obj_icon: None,
                obj_short_name: Some("Chem_p3: \"Antimony (Sb)\"".to_owned()),
                obj_name: None,
            },
        )])));
        let chemicals = PropChemicalNeeded {
            chemicals: std::array::from_fn(|i| {
                if i == 0 {
                    "Chem #3".into()
                } else {
                    String::new()
                }
            }),
            thresholds_secs: [5, 0, 0, 0, 0, 0, 0],
        };
        let specimen = world.add_entity((PropTemplateId { template_id: -1341 }, chemicals.clone()));
        assert_eq!(chemical_needed_message(&world, specimen), None);
        {
            let mut quests = world
                .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
                .unwrap();
            let research = quests.research_mut();
            research.begin(-1341, 0, 0);
            research.advance(-1341, 10.0, 1, 0.0, 100.0, Some(&chemicals), 0);
        }
        assert_eq!(
            chemical_needed_message(&world, specimen).as_deref(),
            Some("Chemical Needed: Antimony (Sb)")
        );
        misc_table(&world);
        assert_eq!(
            chemical_needed_message(&world, specimen).as_deref(),
            Some("Need: Antimony (Sb)")
        );
    }

    #[test]
    fn hud_line_uses_the_panel_skill_text() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let specimen = world.add_entity((
            PropTemplateId { template_id: -1095 },
            PropObjShortName("Organ_1: \"Grub Organ\"".to_owned()),
        ));
        // No trained Research at all: every project needs at least 1.
        assert_eq!(
            research_message(&world, specimen),
            "This item requires a Research skill of 1. Use a Tech Upgrade Unit to train Research."
        );
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .research_mut()
            .begin(-1095, 0, 0);
        assert_eq!(
            research_message(&world, specimen),
            "Researching Grub Organ: 0.0 %"
        );
    }

    #[test]
    fn usable_item_opens_research_only_until_researched() {
        let mut world = World::new();
        let item = world.add_entity(PropObjState(ObjectState::Unresearched));
        let usable = ResearchGui { usable: true };
        assert!(usable.opens_on_frob(item, &world));
        assert!(matches!(
            usable.on_frob(item, &world),
            Effect::BeginResearch { entity_id } if entity_id == item
        ));

        world.add_component(item, PropObjState(ObjectState::Normal));
        assert!(!usable.opens_on_frob(item, &world));
        assert!(matches!(usable.on_frob(item, &world), Effect::NoEffect));

        // Plain research specimens keep opening the Research MFD.
        let specimen = ResearchGui { usable: false };
        assert!(specimen.opens_on_frob(item, &world));
        assert!(matches!(
            specimen.on_frob(item, &world),
            Effect::BeginResearch { .. }
        ));
    }
}
