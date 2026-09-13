//! Read-only cyber-interface utility panels. Input and art share canvas rects.

use cgmath::Vector2;
use engine::assets::asset_cache::AssetCache;
use shipyard::{EntitiesView, EntityId, Get, UniqueView, View, World};

use crate::{
    scripts::gui::wrap_text,
    ui::{HAlign, Rect, UiCanvas, VAlign},
};

const PANEL: Rect = Rect::new(450.0, 124.0, 188.0, 248.0);
use super::character_sheet::{CharacterSheet, PANEL as CHARACTER_PANEL};
const FONT: &str = "mainfont.fon";
const PAGE_LINES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Inspect,
    Character,
    AccessCards,
    Research,
    Map,
    Close,
    Previous,
    Next,
}

#[derive(Default)]
pub(crate) struct MfdUtilities {
    inspecting: bool,
    research: bool,
    empty_logs: bool,
    character: bool,
    character_sheet: CharacterSheet,
    access_cards: bool,
    map_requested: bool,
    research_catalog: super::research_overview::ResearchCatalog,
    selected: Option<EntityId>,
    title: String,
    lines: Vec<String>,
    page: usize,
}

impl MfdUtilities {
    pub(crate) fn has_left_panel(&self) -> bool {
        self.research || self.empty_logs || self.access_cards
    }
    pub(crate) fn show_empty_logs(&mut self) {
        *self = Self::default();
        self.empty_logs = true;
        self.set_content("LOGS".into(), "No collected logs.".into());
    }
    pub(crate) fn is_empty_logs(&self) -> bool {
        self.empty_logs
    }
    pub(crate) fn open_research(&mut self, template: Option<i32>) {
        *self = Self::default();
        self.research = true;
        if let Some(id) = template {
            self.research_catalog.select_project(id);
        }
    }
    pub(crate) fn is_open(&self) -> bool {
        self.inspecting
            || self.selected.is_some()
            || self.research
            || self.empty_logs
            || self.character
            || self.access_cards
    }
    pub(crate) fn take_map_request(&mut self) -> bool {
        std::mem::take(&mut self.map_requested)
    }
    pub(crate) fn is_inspecting(&self) -> bool {
        self.inspecting
    }
    fn controls(&self) -> Vec<(Control, Rect, &'static str, &'static str)> {
        // Retail shkiface.cpp iface_rects[2..6], on the bottom bio/ammo strips.
        // Keep art, hit testing, and debug discovery on these same canvas rects.
        let mut controls = vec![
            (
                Control::AccessCards,
                Rect::new(422.0, 432.0, 38.0, 36.0),
                "ACCESS",
            ),
            (
                Control::Character,
                Rect::new(460.0, 430.0, 32.0, 40.0),
                "MFD",
            ),
            (Control::Inspect, Rect::new(150.0, 431.0, 32.0, 18.0), "?"),
            (
                Control::Research,
                Rect::new(117.0, 431.0, 32.0, 40.0),
                "RES",
            ),
            (Control::Map, Rect::new(150.0, 451.0, 32.0, 18.0), "MAP"),
        ];
        if self.character {
            controls.push((Control::Close, Rect::new(455.0, 132.0, 20.0, 21.0), ""));
        }
        if self.empty_logs || self.access_cards {
            controls.push((Control::Close, Rect::new(165.0, 132.0, 20.0, 21.0), ""));
        }
        if self.inspecting || self.selected.is_some() {
            controls.push((Control::Close, Rect::new(570.0, 346.0, 60.0, 20.0), "CLOSE"));
            if self.page > 0 {
                controls.push((Control::Previous, Rect::new(458.0, 346.0, 28.0, 20.0), "<"));
            }
            if (self.page + 1) * PAGE_LINES < self.lines.len() {
                controls.push((Control::Next, Rect::new(490.0, 346.0, 28.0, 20.0), ">"));
            }
        }
        if self.access_cards {
            if self.page > 0 {
                controls.push((Control::Previous, Rect::new(19.0, 394.0, 28.0, 20.0), "<"));
            }
            if (self.page + 1) * PAGE_LINES < self.lines.len() {
                controls.push((Control::Next, Rect::new(130.0, 394.0, 28.0, 20.0), ">"));
            }
        }
        controls
            .into_iter()
            .map(|(control, rect, label)| {
                let texture = match control {
                    Control::AccessCards if self.access_cards => "iface/ifbtn11.pcx",
                    Control::AccessCards => "iface/ifbtn10.pcx",
                    Control::Character if self.character => "iface/ifbtn21.pcx",
                    Control::Character => "iface/ifbtn20.pcx",
                    Control::Close if self.character => "iface/closeoff.pcx",
                    Control::Inspect if self.inspecting => "iface/ifbtn31.pcx",
                    Control::Inspect => "iface/ifbtn30.pcx",
                    Control::Research if self.research => "iface/ifbtn41.pcx",
                    Control::Research => "iface/ifbtn40.pcx",
                    Control::Map => "iface/ifbtn50.pcx",
                    Control::Close if self.empty_logs || self.access_cards => "iface/closeoff.pcx",
                    _ => "IFBTN00.PCX",
                };
                (control, rect, label, texture)
            })
            .collect()
    }

    /// Returns whether this pointer belongs to utilities. Inspect mode owns
    /// selection edges everywhere, so a rejected selection cannot use an item.
    pub(crate) fn update(
        &mut self,
        point: Vector2<f32>,
        pressed: bool,
        candidate: Option<EntityId>,
    ) -> bool {
        if let Some((control, _, _, _)) = self
            .controls()
            .into_iter()
            .find(|(_, rect, _, _)| rect.contains(point))
        {
            if pressed {
                match control {
                    Control::AccessCards => {
                        let open = !self.access_cards;
                        *self = Self::default();
                        self.access_cards = open;
                    }
                    Control::Character => {
                        let open = !self.character;
                        *self = Self::default();
                        self.character = open;
                    }
                    Control::Inspect => {
                        if self.inspecting {
                            *self = Self::default();
                        } else {
                            self.research = false;
                            self.empty_logs = false;
                            self.character = false;
                            self.access_cards = false;
                            self.inspecting = true;
                            self.selected = None;
                            self.set_content("Item information".into(), "Select an inventory or held item to inspect. Select ? again to cancel.".into());
                        }
                    }
                    Control::Research => {
                        *self = Self::default();
                        self.research = true;
                    }
                    Control::Map => {
                        *self = Self::default();
                        self.map_requested = true;
                    }
                    Control::Close => *self = Self::default(),
                    Control::Previous => self.page = self.page.saturating_sub(1),
                    Control::Next => self.page += 1,
                }
            }
            return true;
        }
        if self.character {
            self.character_sheet.update(point, pressed);
            return CHARACTER_PANEL.contains(point);
        }
        if self.empty_logs || self.access_cards {
            return Rect::new(2.0, 124.0, 188.0, 296.0).contains(point);
        }
        if self.research {
            let consumed = self.research_catalog.contains(point);
            if self.research_catalog.update(point, pressed) {
                *self = Self::default();
            }
            return consumed;
        }
        if self.inspecting {
            if pressed && let Some(entity) = candidate {
                self.selected = Some(entity);
                self.inspecting = false;
                self.page = 0;
            }
            return true;
        }
        (self.selected.is_some() || self.research) && PANEL.contains(point)
    }

    fn set_content(&mut self, title: String, body: String) {
        let lines = wrap_text(&body, 23);
        if self.title != title || self.lines != lines {
            self.title = title;
            self.lines = lines;
            self.page = self
                .page
                .min(self.lines.len().saturating_sub(1) / PAGE_LINES);
        }
    }

    pub(crate) fn refresh(
        &mut self,
        world: &World,
        assets: &mut AssetCache,
        info: &dark::ss2_entity_info::SystemShock2EntityInfo,
    ) {
        if self.character {
            self.character_sheet.refresh(world, assets);
            return;
        }
        if self.access_cards {
            let strings = assets.get(&dark::importers::STRINGS_IMPORTER, "misc.str");
            self.title = "ACCESS CARDS".into();
            self.lines = world
                .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
                .map(|quests| access_card_names(quests.key_cards(), &strings))
                .unwrap_or_else(|_| vec!["No access cards collected.".into()]);
            self.page = self
                .page
                .min(self.lines.len().saturating_sub(1) / PAGE_LINES);
            return;
        }
        if self.research {
            self.research_catalog.refresh(world, assets, info);
            return;
        }
        let Some(entity) = self.selected else {
            return;
        };
        if !world
            .borrow::<EntitiesView>()
            .is_ok_and(|entities| entities.is_alive(entity))
        {
            *self = Self::default();
            return;
        }
        let title = crate::hud::resolve_item_name(assets, world, entity)
            .unwrap_or_else(|| "Item information".into());
        let strings = assets.get(&dark::importers::STRINGS_IMPORTER, "objlooks.str");
        self.set_content(title, item_description(world, entity, &strings));
    }

    pub(crate) fn draw(&self, canvas: &mut UiCanvas) {
        if self.character {
            self.character_sheet.draw(canvas);
        }
        if self.empty_logs || self.access_cards {
            canvas.image(
                Rect::new(2.0, 124.0, 188.0, 296.0),
                if self.access_cards {
                    "iface/security.pcx"
                } else {
                    "iface/pda.pcx"
                },
            );
            canvas.text_native_fit(
                Rect::new(17.0, 136.0, 139.0, 12.0),
                &self.title,
                crate::ui::MFD_FONT,
                HAlign::Left,
                VAlign::Top,
            );
            for (rect, line) in self.visible_lines() {
                canvas.text_native_fit(rect, line, crate::ui::MFD_FONT, HAlign::Left, VAlign::Top);
            }
        }
        if self.research {
            self.research_catalog.draw(canvas);
        }
        if self.inspecting || self.selected.is_some() {
            canvas.image(PANEL, "IFBTN00.PCX");
            canvas.text_native_fit(
                Rect::new(458.0, 130.0, 172.0, 16.0),
                &self.title,
                FONT,
                HAlign::Left,
                VAlign::Middle,
            );
            for (rect, line) in self.visible_lines() {
                // Paragraph gaps occupy a line but have no glyph mesh.
                if line.trim().is_empty() {
                    continue;
                }
                canvas.text_native_fit(rect, line, FONT, HAlign::Left, VAlign::Middle);
            }
        }
        for (control, rect, label, texture) in self.controls() {
            canvas.image(rect, texture);
            if control == Control::AccessCards {
                // Gamesys fakekeys (-77) inherits PropObjIcon=passkey.
                canvas.fitted_object_icon(
                    Rect::new(rect.x + 3.0, rect.y + 2.0, 32.0, 32.0),
                    "objicon/passkey.pcx",
                );
            }
            // Native navigation art contains its own glyphs (including the vial).
            if !label.is_empty()
                && matches!(control, Control::Close | Control::Previous | Control::Next)
            {
                canvas.text_native_fit(rect, label, FONT, HAlign::Center, VAlign::Middle);
            }
        }
    }

    fn visible_lines(&self) -> impl Iterator<Item = (Rect, &String)> {
        self.lines
            .iter()
            .skip(self.page * PAGE_LINES)
            .take(PAGE_LINES)
            .enumerate()
            .map(|(i, line)| {
                let rect = if self.empty_logs || self.access_cards {
                    Rect::new(17.0, 170.0 + i as f32 * 12.0, 139.0, 12.0)
                } else {
                    Rect::new(460.0, 152.0 + i as f32 * 11.0, 168.0, 11.0)
                };
                (rect, line)
            })
    }

    pub(crate) fn debug_elements(&self) -> Vec<crate::game_scene::DebugUiElement> {
        self.controls()
            .into_iter()
            .map(
                |(control, r, label, texture)| crate::game_scene::DebugUiElement {
                    kind: "button".into(),
                    texture: Some(texture.into()),
                    text: Some(label.into()),
                    label: Some(
                        match control {
                            Control::Character => "character_stats",
                            Control::AccessCards => "access_cards",
                            Control::Inspect => "inspect",
                            Control::Research => "research_overview",
                            Control::Map => "map",
                            Control::Close => "utility_close",
                            Control::Previous => "utility_previous",
                            Control::Next => "utility_next",
                        }
                        .into(),
                    ),
                    entity_id: None,
                    rect: [r.x, r.y, r.w, r.h],
                    screen_rect: [r.x, r.y, r.w, r.h],
                },
            )
            .chain(if self.research {
                self.research_catalog.elements()
            } else {
                Vec::new()
            })
            .chain(if self.character {
                self.character_sheet.debug_elements()
            } else {
                Vec::new()
            })
            .chain(self.visible_lines().map(|(rect, line)| {
                let r = [rect.x, rect.y, rect.w, rect.h];
                crate::game_scene::DebugUiElement {
                    kind: "text".into(),
                    texture: None,
                    text: Some(line.clone()),
                    label: Some("utility_text".into()),
                    entity_id: self.selected.map(|e| e.inner() as i32),
                    rect: r,
                    screen_rect: r,
                }
            }))
            .collect()
    }
}

/// Each collected region appears once, ordered like retail shksecur.cpp.
fn access_card_names(
    cards: &[dark::properties::KeyCard],
    strings: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let access = cards.iter().fold(0u32, |mask, key| mask | key.region_id);
    const ORDER: [u32; 32] = [
        10, 1, 0, 3, 2, 11, 12, 4, 14, 6, 7, 8, 5, 15, 13, 19, 9, 17, 18, 16, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31,
    ];
    let mut names: Vec<String> = ORDER
        .into_iter()
        .filter(|bit| access & (1u32 << bit) != 0)
        .map(|bit| {
            strings
                .get(&format!("access{bit}"))
                .cloned()
                .unwrap_or_else(|| "Unknown access card".into())
        })
        .collect();
    if names.is_empty() {
        names.push("No access cards collected.".into());
    }
    names
}

fn item_description(
    world: &World,
    entity: EntityId,
    strings: &std::collections::HashMap<String, String>,
) -> String {
    use dark::properties::{PropObjLookString, PropObjName, PropSymName};
    let (looks, names, symbols) = world
        .borrow::<(
            View<PropObjLookString>,
            View<PropObjName>,
            View<PropSymName>,
        )>()
        .unwrap();
    dark::importers::resolve_object_description(
        looks.get(entity).ok().map(|p| p.0.as_str()),
        names.get(entity).ok().map(|p| p.0.as_str()),
        symbols.get(entity).ok().map(|p| p.0.as_str()),
        strings,
    )
    .unwrap_or_else(|| "No description available.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec2;

    #[test]
    fn collected_credentials_show_localized_regions_once_in_deck_order_and_survive_save() {
        let mut quests = crate::quest_info::QuestInfo::new();
        let strings = std::collections::HashMap::from([
            ("access10".into(), "Cryogenics".into()),
            ("access1".into(), "Science".into()),
        ]);
        assert_eq!(
            access_card_names(quests.key_cards(), &strings),
            ["No access cards collected."]
        );
        for region_id in [2, 1026, 2] {
            quests.add_key_card(dark::properties::KeyCard {
                is_master: false,
                region_id,
                lock_id: 0,
            });
        }
        let loaded: crate::quest_info::QuestInfo =
            serde_json::from_str(&serde_json::to_string(&quests).unwrap()).unwrap();
        assert_eq!(
            access_card_names(loaded.key_cards(), &strings),
            ["Cryogenics", "Science"]
        );
        quests.add_key_card(dark::properties::KeyCard {
            is_master: false,
            region_id: 1u32 << 31,
            lock_id: 0,
        });
        assert_eq!(access_card_names(quests.key_cards(), &strings).len(), 3);
    }

    #[test]
    fn access_and_character_buttons_switch_and_close_without_selecting_inventory() {
        let mut ui = MfdUtilities::default();
        let card = ui
            .controls()
            .into_iter()
            .find(|c| c.0 == Control::AccessCards)
            .unwrap()
            .1
            .center();
        let mfd = ui
            .controls()
            .into_iter()
            .find(|c| c.0 == Control::Character)
            .unwrap()
            .1
            .center();
        assert!(ui.update(card, false, None));
        assert!(!ui.is_open());
        ui.update(card, true, None);
        assert!(ui.access_cards && ui.has_left_panel());
        assert!(ui.update(vec2(100.0, 200.0), true, None));
        assert!(ui.selected.is_none());
        ui.update(mfd, true, None);
        assert!(ui.character && !ui.has_left_panel());
        ui.update(vec2(520.0, 405.0), true, None);
        assert!(
            ui.character_sheet
                .debug_elements()
                .iter()
                .any(|e| e.texture.as_deref() == Some("iface/etech1.pcx"))
        );
        ui.update(vec2(465.0, 142.0), true, None);
        assert!(!ui.is_open());
        ui.update(card, true, None);
        ui.update(card, true, None);
        assert!(!ui.is_open());
    }

    #[test]
    fn paragraph_spacing_does_not_emit_empty_text_meshes() {
        let mut ui = MfdUtilities::default();
        ui.inspecting = true;
        ui.set_content("Title".into(), "First\n\nSecond".into());
        let mut canvas = UiCanvas::new(Vector2::new(640.0, 480.0));
        ui.draw(&mut canvas);
        assert!(canvas.elements().iter().all(|element| !matches!(element, crate::ui::UiElement::Text { text, .. } if text.trim().is_empty())));
        let lines: Vec<_> = ui.visible_lines().collect();
        assert_eq!(lines[2].0.y - lines[0].0.y, 22.0);
    }

    #[test]
    fn pages_cover_every_line_and_clamp_when_content_shrinks() {
        let mut ui = MfdUtilities::default();
        ui.update(vec2(166.0, 440.0), true, None);
        ui.set_content(
            "Long description".into(),
            (0..40).map(|i| format!("Line {i}\n")).collect(),
        );
        let all = ui.lines.clone();
        let mut seen = Vec::new();
        loop {
            seen.extend(ui.visible_lines().map(|(_, line)| line.clone()));
            let Some((_, rect, _, _)) = ui
                .controls()
                .into_iter()
                .find(|(control, _, _, _)| *control == Control::Next)
            else {
                break;
            };
            ui.update(rect.center(), true, None);
        }
        assert_eq!(seen, all);
        assert!(ui.page > 0);
        ui.set_content("Short".into(), "Only one line".into());
        assert_eq!(ui.page, 0);
        assert_eq!(ui.visible_lines().count(), 1);
    }

    #[test]
    fn inspection_selects_without_mutating_an_item_and_cancels_cleanly() {
        let mut world = World::new();
        let hypo = world.add_entity(dark::properties::PropObjLookString(
            "hypo: \"Restores health.\"".into(),
        ));
        let mut ui = MfdUtilities::default();
        assert!(ui.update(vec2(166.0, 440.0), true, None));
        assert!(ui.inspecting);
        assert!(ui.update(vec2(10.0, 40.0), false, Some(hypo)));
        assert_eq!(ui.selected, None);
        assert!(ui.update(vec2(10.0, 40.0), true, Some(hypo)));
        assert_eq!(ui.selected, Some(hypo));
        assert_eq!(
            item_description(&world, hypo, &Default::default()),
            "Restores health."
        );
        assert!(world.borrow::<EntitiesView>().unwrap().is_alive(hypo));
        assert!(ui.update(vec2(600.0, 355.0), true, None));
        assert_eq!(ui.selected, None);
        assert!(!ui.inspecting);
    }
}
