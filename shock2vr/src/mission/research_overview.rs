//! Research journal: retail PDA list, RESEARCH status and RESREP report reader.
//! Coordinates follow shkrsrch.cpp, shkpda.cpp and shkemail.cpp. All UI is
//! resolved once on the shared canvas; reports are collected, not live items.
use crate::scripts::gui::research::layout;
use crate::{
    game_scene::DebugUiElement,
    quest_info::QuestInfo,
    scripts::{gui::wrap_text, script_util::hydrate_template_component},
    ui::{HAlign, MFD_FONT, Rect, UiCanvas, VAlign},
};
use cgmath::Vector2;
use dark::{
    properties::{
        PropChemicalNeeded, PropObjIcon, PropObjName, PropResearchText, PropResearchTime,
        PropSymName,
    },
    ss2_entity_info::SystemShock2EntityInfo,
};
use engine::assets::asset_cache::AssetCache;
use shipyard::{UniqueView, World};
use std::collections::HashMap;

const PANEL: Rect = Rect::new(2.0, 124.0, 188.0, 296.0);
const PAGE_LINES: usize = 14;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    Project(i32),
    Report(u32),
}
#[derive(Default)]
pub(super) struct ResearchCatalog {
    projects: HashMap<i32, Project>,
    reports: HashMap<u32, Report>,
    rows: Vec<(Selection, String)>,
    selected: Option<Selection>,
    page: usize,
    font: Option<std::rc::Rc<Box<dyn engine::Font>>>,
}
struct Project {
    name: String,
    seconds: f32,
    chemicals: Option<PropChemicalNeeded>,
    icon: Option<String>,
    description: String,
    status: String,
    fraction: f32,
    complete: bool,
}
struct Report {
    name: String,
    text: String,
    header: String,
    portrait: Option<String>,
    icon: Option<String>,
}

impl ResearchCatalog {
    pub(super) fn refresh(
        &mut self,
        world: &World,
        assets: &mut AssetCache,
        info: &SystemShock2EntityInfo,
    ) {
        let Ok(quests) = world.borrow::<UniqueView<QuestInfo>>() else {
            return;
        };
        let strings = assets.get(&dark::importers::STRINGS_IMPORTER, "research.str");
        self.font = Some(crate::ui::resolve_font(assets, MFD_FONT));
        self.rows.clear();
        for id in quests.research().project_template_ids() {
            let project = self.projects.entry(id).or_insert_with(|| {
                let name = hydrate_template_component::<PropObjName>(id, info).map(|p| p.0);
                let symbol = hydrate_template_component::<PropSymName>(id, info).map(|p| p.0);
                let names = assets.get(&dark::importers::STRINGS_IMPORTER, "objname.str");
                let text = hydrate_template_component::<PropResearchText>(id, info).map(|p| p.0);
                let texts = assets.get(&dark::importers::STRINGS_IMPORTER, "rsrchtxt.str");
                Project {
                    name: name
                        .as_deref()
                        .map(|n| dark::importers::resolve_localized_property_string(n, &names))
                        .or(symbol)
                        .unwrap_or_else(|| "Research project".into()),
                    seconds: hydrate_template_component::<PropResearchTime>(id, info)
                        .map(|p| p.0.max(1) as f32)
                        .unwrap_or(1.0),
                    chemicals: hydrate_template_component::<PropChemicalNeeded>(id, info),
                    icon: hydrate_template_component::<PropObjIcon>(id, info)
                        .map(|p| format!("{}.pcx", p.0)),
                    description: text
                        .as_deref()
                        .map(|t| dark::importers::resolve_localized_property_string(t, &texts))
                        .unwrap_or_else(|| "Research in progress.".into()),
                    status: String::new(),
                    fraction: 0.0,
                    complete: false,
                }
            });
            let status = quests.research().status(id, project.chemicals.as_ref());
            project.complete = status.complete;
            project.fraction = (status.authored_seconds / project.seconds).clamp(0.0, 1.0);
            project.status = if let Some(chemical) = status.needed_chemical {
                format!(
                    "Research paused. Required chemical: {}.",
                    crate::scripts::gui::chemical_display_name(world, &chemical)
                )
            } else if status.complete {
                "Research complete. Select REPORTS to read the findings.".into()
            } else if status.active {
                project.description.clone()
            } else {
                "Research suspended. Use the specimen to resume.".into()
            };
            if !status.complete {
                self.rows.push((
                    Selection::Project(id),
                    format!(
                        "{}: {}",
                        if status.active { "Active" } else { "Suspended" },
                        "Unresearched Object"
                    ),
                ));
            }
        }
        self.refresh_reports(quests.research(), &strings);
        let (count, total) = match self.selected {
            None => (9, self.rows.len()),
            Some(Selection::Project(_)) => (7, self.lines().len()),
            Some(Selection::Report(_)) => (PAGE_LINES, self.lines().len()),
        };
        self.page = self.page.min(total.saturating_sub(1) / count);
    }

    fn refresh_reports(
        &mut self,
        state: &crate::research::ResearchState,
        strings: &HashMap<String, String>,
    ) {
        for index in 1..=32 {
            if !state.has_report(1 << (index - 1)) {
                continue;
            }
            let report = self.reports.entry(index).or_insert_with(|| {
                let get = |prefix: &str| {
                    strings
                        .get(&format!("{prefix}{index}").to_ascii_lowercase())
                        .cloned()
                };
                Report {
                    header: get("ResRepName").unwrap_or_default(),
                    name: get("ReportName").unwrap_or_else(|| "Research report".into()),
                    text: get("ResRepText")
                        .unwrap_or_else(|| "No written report available.".into()),
                    portrait: get("ResRepPortrait").map(|n| format!("{n}.pcx")),
                    icon: get("ResRepIcon").map(|n| format!("{n}.pcx")),
                }
            });
            self.rows
                .push((Selection::Report(index), report.name.clone()));
        }
    }

    pub(super) fn select_project(&mut self, id: i32) {
        self.selected = Some(Selection::Project(id));
        self.page = 0;
    }
    fn lines(&self) -> Vec<String> {
        let body = match self.selected {
            Some(Selection::Project(id)) => self.projects.get(&id).map(|p| p.status.clone()),
            Some(Selection::Report(id)) => self.reports.get(&id).map(|r| {
                if r.header.is_empty() {
                    r.text.clone()
                } else {
                    format!("{}\n\n{}", r.header, r.text)
                }
            }),
            None => None,
        }
        .unwrap_or_default();
        if let Some(font) = &self.font {
            engine::wrap_text_to_width(&***font, &body, font.base_height(), 136.0)
        } else {
            wrap_text(&body, 27)
        }
    }
    /// Close is returned to the owning utility state; all other actions only
    /// navigate the journal and never start, consume or complete research.
    pub(super) fn update(&mut self, point: Vector2<f32>, pressed: bool) -> bool {
        if !pressed {
            return false;
        }
        let elements = self.elements();
        let label = elements
            .iter()
            .find(|e| {
                e.kind == "button"
                    && Rect::new(e.rect[0], e.rect[1], e.rect[2], e.rect[3]).contains(point)
            })
            .and_then(|e| e.label.as_deref());
        match label {
            Some("utility_close") => return true,
            Some("research_back") => {
                self.selected = None;
                self.page = 0;
            }
            Some("utility_previous") => self.page = self.page.saturating_sub(1),
            Some("utility_next") => self.page += 1,
            Some(label) => {
                if let Some(index) = label
                    .strip_prefix("research_entry:")
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    if let Some((selection, _)) = self.rows.get(index) {
                        self.selected = Some(*selection);
                        self.page = 0;
                    }
                }
            }
            None => {}
        }
        false
    }
    pub(super) fn contains(&self, point: Vector2<f32>) -> bool {
        PANEL.contains(point)
    }

    pub(super) fn elements(&self) -> Vec<DebugUiElement> {
        let mut out = Vec::new();
        let mut add = |kind: &str,
                       rect: Rect,
                       texture: Option<&str>,
                       text: Option<&str>,
                       label: Option<String>| {
            let r = [PANEL.x + rect.x, PANEL.y + rect.y, rect.w, rect.h];
            out.push(DebugUiElement {
                kind: kind.into(),
                texture: texture.map(str::to_owned),
                text: text.map(str::to_owned),
                label,
                entity_id: None,
                rect: r,
                screen_rect: r,
            });
        };
        let backdrop = match self.selected {
            None => "iface/pda.pcx",
            Some(Selection::Project(_)) => "iface/research.pcx",
            Some(Selection::Report(_)) => "iface/resrep.pcx",
        };
        add(
            "image",
            Rect::new(0.0, 0.0, 188.0, 296.0),
            Some(backdrop),
            None,
            None,
        );
        add(
            "button",
            Rect::new(163.0, 8.0, 20.0, 21.0),
            Some("iface/closeoff.pcx"),
            None,
            Some("utility_close".into()),
        );
        if self.selected.is_none() {
            add(
                "text",
                Rect::new(15.0, 12.0, 138.0, 15.0),
                None,
                Some("Research"),
                Some("utility_text".into()),
            );
            if self.rows.is_empty() {
                for (i, line) in wrap_text(
                    "No research projects yet. Use a researchable item to begin.",
                    27,
                )
                .iter()
                .enumerate()
                {
                    add(
                        "text",
                        Rect::new(15.0, 38.0 + i as f32 * 12.0, 138.0, 12.0),
                        None,
                        Some(line),
                        Some("utility_text".into()),
                    );
                }
            }
            for (i, (_, name)) in self.rows.iter().enumerate().skip(self.page * 9).take(9) {
                add(
                    "button",
                    Rect::new(13.0, 34.0 + (i % 9) as f32 * 24.0, 139.0, 24.0),
                    None,
                    Some(name),
                    Some(format!("research_entry:{i}")),
                );
                add(
                    "text",
                    Rect::new(22.0, 36.0 + (i % 9) as f32 * 24.0, 130.0, 24.0),
                    None,
                    Some(name),
                    Some("utility_text".into()),
                );
            }
        } else {
            let (top, count) = if let Some(Selection::Project(id)) = self.selected {
                if let Some(project) = self.projects.get(&id) {
                    if let Some(icon) = &project.icon {
                        add("object_icon", layout::SPECIMEN, Some(icon), None, None);
                    }
                    add(
                        "text",
                        layout::TITLE,
                        None,
                        Some(if project.complete {
                            &project.name
                        } else {
                            "Unresearched Object"
                        }),
                        Some("utility_text".into()),
                    );
                    if project.fraction > 0.0 {
                        add(
                            "image",
                            Rect {
                                w: layout::PROGRESS.w * project.fraction,
                                ..layout::PROGRESS
                            },
                            Some("iface/resprog.pcx"),
                            None,
                            None,
                        );
                    }
                    add(
                        "text",
                        layout::PERCENT,
                        None,
                        Some(&format!("{:.1} %", project.fraction * 100.0)),
                        Some("utility_text".into()),
                    );
                }
                add(
                    "button",
                    layout::REPORTS,
                    Some("iface/report0.pcx"),
                    None,
                    Some("research_back".into()),
                );
                (153.0, 7)
            } else {
                if let Some(Selection::Report(id)) = self.selected {
                    if let Some(report) = self.reports.get(&id) {
                        if let Some(portrait) = &report.portrait {
                            add(
                                "image",
                                Rect::new(15.0, 13.0, 58.0, 84.0),
                                Some(portrait),
                                None,
                                None,
                            );
                        }
                        if let Some(icon) = &report.icon {
                            add(
                                "image",
                                Rect::new(83.0, 13.0, 68.0, 84.0),
                                Some(icon),
                                None,
                                None,
                            );
                        }
                    }
                }
                add(
                    "button",
                    Rect::new(159.0, 252.0, 18.0, 34.0),
                    Some("iface/return0.pcx"),
                    None,
                    Some("research_back".into()),
                );
                (105.0, PAGE_LINES)
            };
            for (i, line) in self
                .lines()
                .iter()
                .skip(self.page * count)
                .take(count)
                .enumerate()
            {
                if !line.trim().is_empty() {
                    add(
                        "text",
                        Rect::new(15.0, top + i as f32 * 12.0, 136.0, 12.0),
                        None,
                        Some(line),
                        Some("utility_text".into()),
                    );
                }
            }
        }
        let (count, total) = match self.selected {
            None => (9, self.rows.len()),
            Some(Selection::Project(_)) => (7, self.lines().len()),
            _ => (PAGE_LINES, self.lines().len()),
        };
        if self.page > 0 {
            add(
                "button",
                Rect::new(159.0, 174.0, 18.0, 26.0),
                Some("iface/pgup0.pcx"),
                None,
                Some("utility_previous".into()),
            );
        }
        if (self.page + 1) * count < total {
            add(
                "button",
                Rect::new(159.0, 203.0, 18.0, 26.0),
                Some("iface/pgdn0.pcx"),
                None,
                Some("utility_next".into()),
            );
        }
        out
    }
    pub(super) fn draw(&self, canvas: &mut UiCanvas) {
        for e in self.elements() {
            let r = Rect::new(e.rect[0], e.rect[1], e.rect[2], e.rect[3]);
            if let Some(texture) = e.texture {
                if e.kind == "object_icon" {
                    canvas.fitted_object_icon(r, &texture);
                } else {
                    canvas.image(r, &texture);
                }
            }
            if let Some(text) = e.text.filter(|_| e.kind == "text") {
                canvas.text_native_fit(r, &text, MFD_FONT, HAlign::Left, VAlign::Top);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collected_reports_use_report_strings_without_a_live_specimen() {
        let mut state = crate::research::ResearchState::default();
        state.begin(-148, 1, 1);
        state.advance(-148, 10.0, 1, 1.0, 1.0, None, 1 << 21);
        let before = serde_json::to_value(&state).unwrap();
        let strings = HashMap::from([
            ("reportname22".into(), "Monkey Brain".into()),
            (
                "resreptext22".into(),
                (0..48).map(|n| format!("Report line {n}\n")).collect(),
            ),
            ("resrepportrait22".into(), "mport".into()),
            ("resrepicon22".into(), "resicon".into()),
        ]);
        let mut catalog = ResearchCatalog::default();
        catalog.refresh_reports(&state, &strings);
        assert_eq!(catalog.rows.len(), 1);
        catalog.selected = Some(catalog.rows[0].0);
        let elements = catalog.elements();
        assert!(
            elements
                .iter()
                .any(|e| e.texture.as_deref() == Some("iface/resrep.pcx"))
        );
        assert!(
            elements
                .iter()
                .any(|e| e.texture.as_deref() == Some("mport.pcx"))
        );
        assert!(
            elements
                .iter()
                .any(|e| e.texture.as_deref() == Some("resicon.pcx"))
        );
        let mut seen = Vec::new();
        loop {
            let elements = catalog.elements();
            seen.extend(
                elements
                    .iter()
                    .filter(|e| e.label.as_deref() == Some("utility_text"))
                    .filter_map(|e| e.text.clone()),
            );
            let Some(next) = elements
                .iter()
                .find(|e| e.label.as_deref() == Some("utility_next"))
            else {
                break;
            };
            catalog.update(Vector2::new(next.rect[0] + 2.0, next.rect[1] + 2.0), true);
        }
        assert_eq!(seen.len(), 48);
        assert_eq!(seen.last().unwrap(), "Report line 47");
        assert_eq!(serde_json::to_value(&state).unwrap(), before);
    }
    #[test]
    fn every_research_control_and_text_stays_in_the_authored_panel() {
        let mut catalog = ResearchCatalog::default();
        catalog.rows = (0..24)
            .map(|n| (Selection::Report(n), format!("Report {n}")))
            .collect();
        for page in 0..3 {
            catalog.page = page;
            for e in catalog.elements() {
                assert!(e.rect[0] >= PANEL.x && e.rect[1] >= PANEL.y);
                assert!(e.rect[0] + e.rect[2] <= PANEL.x + PANEL.w);
                assert!(e.rect[1] + e.rect[3] <= PANEL.y + PANEL.h);
            }
        }
    }
}
