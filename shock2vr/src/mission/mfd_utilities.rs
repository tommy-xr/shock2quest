//! Read-only cyber-interface utility panels. Input and art share canvas rects.

use cgmath::Vector2;
use engine::assets::asset_cache::AssetCache;
use shipyard::{EntitiesView, EntityId, Get, View, World};

use crate::{
    scripts::gui::wrap_text,
    ui::{HAlign, Rect, UiCanvas, VAlign},
};

const PANEL: Rect = Rect::new(450.0, 124.0, 188.0, 248.0);
const FONT: &str = "mainfont.fon";
const PAGE_LINES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Inspect,
    Close,
    Previous,
    Next,
}

#[derive(Default)]
pub(crate) struct MfdUtilities {
    inspecting: bool,
    selected: Option<EntityId>,
    title: String,
    lines: Vec<String>,
    page: usize,
}

impl MfdUtilities {
    pub(crate) fn is_inspecting(&self) -> bool {
        self.inspecting
    }
    fn controls(&self) -> Vec<(Control, Rect, &'static str)> {
        // Dedicated utility row below the reserved right MFD slot, clear of
        // the bottom ammo readout. New utilities fill the row incrementally.
        let mut controls = vec![(Control::Inspect, Rect::new(450.0, 382.0, 36.0, 26.0), "?")];
        if self.inspecting || self.selected.is_some() {
            controls.push((Control::Close, Rect::new(570.0, 346.0, 60.0, 20.0), "CLOSE"));
            if self.page > 0 {
                controls.push((Control::Previous, Rect::new(458.0, 346.0, 28.0, 20.0), "<"));
            }
            if (self.page + 1) * PAGE_LINES < self.lines.len() {
                controls.push((Control::Next, Rect::new(490.0, 346.0, 28.0, 20.0), ">"));
            }
        }
        controls
    }

    /// Returns whether this pointer belongs to utilities. Inspect mode owns
    /// selection edges everywhere, so a rejected selection cannot use an item.
    pub(crate) fn update(
        &mut self,
        point: Vector2<f32>,
        pressed: bool,
        candidate: Option<EntityId>,
    ) -> bool {
        if let Some((control, _, _)) = self
            .controls()
            .into_iter()
            .find(|(_, rect, _)| rect.contains(point))
        {
            if pressed {
                match control {
                    Control::Inspect => {
                        if self.inspecting {
                            *self = Self::default();
                        } else {
                            self.inspecting = true;
                            self.selected = None;
                            self.set_content("Item information".into(), "Select an inventory or held item to inspect. Select ? again to cancel.".into());
                        }
                    }
                    Control::Close => *self = Self::default(),
                    Control::Previous => self.page = self.page.saturating_sub(1),
                    Control::Next => self.page += 1,
                }
            }
            return true;
        }
        if self.inspecting {
            if pressed && let Some(entity) = candidate {
                self.selected = Some(entity);
                self.inspecting = false;
                self.page = 0;
            }
            return true;
        }
        self.selected.is_some() && PANEL.contains(point)
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

    pub(crate) fn refresh(&mut self, world: &World, assets: &mut AssetCache) {
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
                canvas.text_native_fit(rect, line, FONT, HAlign::Left, VAlign::Middle);
            }
        }
        for (_, rect, label) in self.controls() {
            canvas.image(rect, "IFBTN00.PCX");
            canvas.text_native_fit(rect, label, FONT, HAlign::Center, VAlign::Middle);
        }
    }

    fn visible_lines(&self) -> impl Iterator<Item = (Rect, &String)> {
        self.lines
            .iter()
            .skip(self.page * PAGE_LINES)
            .take(PAGE_LINES)
            .enumerate()
            .map(|(i, line)| (Rect::new(460.0, 152.0 + i as f32 * 11.0, 168.0, 11.0), line))
    }

    pub(crate) fn debug_elements(&self) -> Vec<crate::game_scene::DebugUiElement> {
        self.controls()
            .into_iter()
            .map(|(control, r, label)| crate::game_scene::DebugUiElement {
                kind: "button".into(),
                texture: Some("IFBTN00.PCX".into()),
                text: Some(label.into()),
                label: Some(
                    match control {
                        Control::Inspect => "inspect",
                        Control::Close => "utility_close",
                        Control::Previous => "utility_previous",
                        Control::Next => "utility_next",
                    }
                    .into(),
                ),
                entity_id: None,
                rect: [r.x, r.y, r.w, r.h],
                screen_rect: [r.x, r.y, r.w, r.h],
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
    fn pages_cover_every_line_and_clamp_when_content_shrinks() {
        let mut ui = MfdUtilities::default();
        ui.update(vec2(468.0, 395.0), true, None);
        ui.set_content(
            "Long description".into(),
            (0..40).map(|i| format!("Line {i}\n")).collect(),
        );
        let all = ui.lines.clone();
        let mut seen = Vec::new();
        loop {
            seen.extend(ui.visible_lines().map(|(_, line)| line.clone()));
            let Some((_, rect, _)) = ui
                .controls()
                .into_iter()
                .find(|(control, _, _)| *control == Control::Next)
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
        assert!(ui.update(vec2(468.0, 395.0), true, None));
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
