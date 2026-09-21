//! Retail right MFD: shared Stats/Tech/Combat/Psi layout, after shkstats,
//! shktechs, shkskill and shkiftul.cpp. Browsing never trains the character.
use crate::{
    game_scene::DebugUiElement,
    gui::{GuiComponent, GuiCursor},
    player_stats::{PlayerStats, Skill, Stat},
    scripts::gui::{PsiPowersGuiMsg, psi_panel_components},
    ui::{HAlign, MFD_FONT, Rect, UiCanvas, VAlign},
};
use cgmath::{Vector2, point2, vec2};
use engine::assets::asset_cache::AssetCache;
use shipyard::World;

pub(super) const PANEL: Rect = Rect::new(450.0, 124.0, 188.0, 296.0);
const SIZE: Vector2<f32> = vec2(188.0, 296.0);
const TABS: [(&str, &str); 4] = [
    ("STATS", "estats"),
    ("TECH", "etech"),
    ("CMBT", "ecmbt"),
    ("PSI", "epsi"),
];

#[derive(Default)]
pub(super) struct CharacterSheet {
    tab: usize,
    tier: i32,
    point: Option<Vector2<f32>>,
    stats: PlayerStats,
    help: String,
    help_lines: Vec<String>,
    psi: Vec<GuiComponent<PsiPowersGuiMsg>>,
}
impl CharacterSheet {
    pub(super) fn update(&mut self, point: Vector2<f32>, pressed: bool) {
        self.point = Some(point - vec2(PANEL.x, PANEL.y));
        if !pressed {
            return;
        }
        if let Some(tab) = (0..4).find(|i| tab_rect(*i).contains(point)) {
            self.tab = tab;
            self.help.clear();
        } else if self.tab == 3 {
            let local = self.point.unwrap();
            if let Some(PsiPowersGuiMsg::BrowseTier(tier)) = self
                .psi
                .iter()
                .find(|e| e.click_event().is_some() && e.rect().contains(local))
                .and_then(|e| e.click_event())
            {
                self.tier = *tier;
            }
        }
    }
    pub(super) fn refresh(&mut self, world: &World, assets: &mut AssetCache) {
        if let Some(stats) = crate::implants::effective_stats(world) {
            self.stats = stats;
        }
        if self.tab == 3 {
            let cursor = self.point.map(|p| GuiCursor {
                position: point2(p.x, p.y),
                held_entity_id: None,
            });
            self.psi = psi_panel_components(&cursor, world, Some(self.tier.max(1)));
            return;
        }
        let rows = self.rows();
        let hovered = self.point.and_then(|p| {
            rows.iter()
                .enumerate()
                .position(|(i, _)| Rect::new(30.0, 8.0 + i as f32 * 26.0, 148.0, 26.0).contains(p))
        });
        let hovered = hovered.or_else(|| {
            if self.tab != 1 {
                return None;
            }
            let point = self.point?;
            [6, 8, 7, 9]
                .into_iter()
                .enumerate()
                .find_map(|(column, key)| {
                    Rect::new(17.0 + column as f32 * 41.0, 144.0, 33.0, 32.0)
                        .contains(point)
                        .then_some(key)
                })
        });
        let (file, prefix) = match self.tab {
            0 => ("stathelp.str", "text"),
            1 => ("skilhelp.str", "tech"),
            _ => ("skilhelp.str", "weapon"),
        };
        let strings = assets.get(&dark::importers::STRINGS_IMPORTER, file);
        self.help = hovered
            .and_then(|i| strings.get(&format!("{prefix}{i}")).cloned())
            .unwrap_or_default();
        if self.tab == 0 && self.help.is_empty() {
            self.help = if self.stats.os_traits.is_empty() {
                "No OS upgrades installed.".into()
            } else {
                self.stats
                    .os_traits
                    .iter()
                    .map(|id| crate::scripts::gui::trait_name(*id))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            if let Some(p) = self.point {
                if Rect::new(15.0, 144.0, 159.0, 40.0).contains(p) {
                    let slot = ((p.x - 37.0).max(0.0) / 35.0) as usize;
                    if let Some(id) = self.stats.os_traits.get(slot) {
                        let traits = assets.get(&dark::importers::STRINGS_IMPORTER, "traits.str");
                        if let Some(description) = traits.get(&format!("trait{id}")) {
                            self.help = description.clone();
                        }
                    }
                }
            }
        }
        let font = crate::ui::resolve_font(assets, MFD_FONT);
        self.help_lines =
            engine::wrap_text_to_width(&**font, &self.help, font.base_height(), 159.0);
    }
    fn rows(&self) -> Vec<(&'static str, i32)> {
        match self.tab {
            0 => [
                ("STRENGTH", Stat::Strength),
                ("ENDURANCE", Stat::Endurance),
                ("PSIONICS", Stat::PsionicAbility),
                ("AGILITY", Stat::Agility),
                ("CYBER", Stat::CyberAffinity),
            ]
            .into_iter()
            .map(|(n, s)| (n, self.stats.stat_level(s)))
            .collect(),
            1 => [
                ("HACK", Skill::Hack),
                ("REPAIR", Skill::Repair),
                ("MODIFY", Skill::Modify),
                ("MAINTAIN", Skill::Maintenance),
                ("RESEARCH", Skill::Research),
            ]
            .into_iter()
            .map(|(n, s)| (n, self.stats.skill_level(s)))
            .collect(),
            2 => [
                ("STANDARD", Skill::StandardWeapons),
                ("ENERGY", Skill::EnergyWeapons),
                ("HEAVY", Skill::HeavyWeapons),
                ("EXOTIC", Skill::ExoticWeapons),
            ]
            .into_iter()
            .map(|(n, s)| (n, self.stats.skill_level(s)))
            .collect(),
            _ => Vec::new(),
        }
    }
    pub(super) fn draw(&self, canvas: &mut UiCanvas) {
        if self.tab == 3 {
            let cursor = self
                .point
                .map(|p| point2(p.x, p.y))
                .unwrap_or(point2(-1.0, -1.0));
            for component in &self.psi {
                canvas.push(component.to_render_info(SIZE, cursor).to_ui_element(PANEL));
            }
        } else {
            let art = ["iface/stats.pcx", "iface/technic.pcx", "iface/combat.pcx"][self.tab];
            canvas.image(PANEL, art);
            for (i, (name, level)) in self.rows().iter().enumerate() {
                let r = Rect::new(480.0, 132.0 + i as f32 * 26.0, 148.0, 14.0);
                // Classic art bakes labels; 25AE can remove them. Cover only
                // their band with the empty help area's matching background.
                canvas.cropped_image(
                    r,
                    "iface/stats.pcx",
                    Rect::new(40.0, 134.0, 80.0, 6.0),
                    SIZE,
                );
                canvas.text_fit(
                    Rect { h: 12.0, ..r },
                    name,
                    crate::ui::MFD_LABEL_FONT,
                    12.0,
                    HAlign::Left,
                    VAlign::Top,
                );
                for arrow in 0..(*level).clamp(0, 6) {
                    canvas.image(
                        Rect::new(
                            483.0 + arrow as f32 * 17.0,
                            146.0 + i as f32 * 26.0,
                            20.0,
                            14.0,
                        ),
                        "iface/skilstat.pcx",
                    );
                }
            }
            if self.tab == 0 {
                for (slot, id) in self.stats.os_traits.iter().take(4).enumerate() {
                    canvas.image(
                        Rect::new(487.0 + slot as f32 * 35.0, 268.0, 32.0, 32.0),
                        &crate::scripts::gui::trait_icon(*id),
                    );
                }
            }
        }
        for e in self.elements() {
            let r = Rect::new(e.rect[0], e.rect[1], e.rect[2], e.rect[3]);
            if let Some(texture) = e.texture {
                canvas.image(r, &texture);
            }
            if let Some(text) = e.text.filter(|t| !t.is_empty()) {
                // Tabs carry their own labels; other readouts use bitmap fonts.
                if e.kind != "button" {
                    canvas.text_native_fit(r, &text, MFD_FONT, HAlign::Left, VAlign::Top);
                }
            }
        }
    }
    pub(super) fn debug_elements(&self) -> Vec<DebugUiElement> {
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        self.draw(&mut canvas);
        let mut elements = self
            .elements()
            .into_iter()
            .filter(|e| e.kind == "button")
            .collect::<Vec<_>>();
        elements.extend(canvas.elements().iter().map(|element| {
            let r = element.rect();
            let (kind, texture, text) = match element {
                crate::ui::UiElement::Text { text, .. } => ("text", None, Some(text.clone())),
                crate::ui::UiElement::Image { texture, .. } => {
                    ("image", Some(texture.clone()), None)
                }
                _ => ("image", None, None),
            };
            DebugUiElement {
                kind: kind.into(),
                texture,
                text,
                label: None,
                entity_id: None,
                rect: [r.x, r.y, r.w, r.h],
                screen_rect: [r.x, r.y, r.w, r.h],
            }
        }));
        elements
    }

    pub(super) fn elements(&self) -> Vec<DebugUiElement> {
        let mut elements = Vec::new();
        let mut add =
            |kind: &str, r: Rect, text: Option<String>, texture: Option<String>, label: &str| {
                elements.push(DebugUiElement {
                    kind: kind.into(),
                    rect: [r.x, r.y, r.w, r.h],
                    screen_rect: [r.x, r.y, r.w, r.h],
                    text,
                    texture,
                    label: Some(label.into()),
                    entity_id: None,
                });
            };
        for (i, (name, art)) in TABS.iter().enumerate() {
            add(
                "button",
                tab_rect(i),
                Some((*name).into()),
                Some(format!("iface/{art}{}.pcx", usize::from(i == self.tab))),
                &format!("character_tab_{i}"),
            );
        }
        if self.tab == 3 {
            for e in &self.psi {
                let r = e.rect();
                let r = Rect::new(PANEL.x + r.x, PANEL.y + r.y, r.w, r.h);
                match e {
                    crate::ui::UiElement::Button { label, .. } => {
                        add("button", r, None, None, label.as_deref().unwrap_or("psi"))
                    }
                    // Already emitted by the shared psi canvas; expose readouts
                    // below separately without drawing them a second time.
                    _ => (),
                }
            }
        } else {
            if self.tab == 1 {
                let software = &self.stats.software;
                for (i, level) in [
                    software.hack,
                    software.repair,
                    software.modify,
                    software.research,
                ]
                .into_iter()
                .enumerate()
                {
                    add(
                        "text",
                        Rect::new(475.0 + i as f32 * 41.0, 279.0, 24.0, 12.0),
                        Some(level.to_string()),
                        None,
                        "character_software",
                    );
                }
            }
            let top = if self.tab == 0 { 314.0 } else { 304.0 };
            for (i, line) in self
                .help_lines
                .iter()
                .take(if self.tab == 0 { 6 } else { 7 })
                .enumerate()
            {
                add(
                    "text",
                    Rect::new(465.0, top + i as f32 * 11.0, 159.0, 11.0),
                    Some(line.clone()),
                    None,
                    "character_help",
                );
            }
        }
        elements
    }
}
fn tab_rect(tab: usize) -> Rect {
    Rect::new(465.0 + tab as f32 * 40.0, 394.0, 40.0, 22.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tabs_show_every_character_skill_and_live_software_without_training() {
        let mut sheet = CharacterSheet::default();
        sheet.stats.strength = 4;
        sheet.stats.skills.hack = 3;
        sheet.stats.skills.exotic_weapons = 2;
        sheet.stats.software.repair = 3;
        let before = sheet.stats.clone();
        assert_eq!(sheet.rows()[0], ("STRENGTH", 4));
        sheet.update(tab_rect(1).center(), true);
        assert_eq!(sheet.rows().len(), 5);
        assert_eq!(sheet.rows()[0], ("HACK", 3));
        let soft: Vec<_> = sheet
            .elements()
            .into_iter()
            .filter(|e| e.label.as_deref() == Some("character_software"))
            .map(|e| e.text.unwrap())
            .collect();
        assert_eq!(soft, ["0", "3", "0", "0"]);
        sheet.update(tab_rect(2).center(), false);
        assert_eq!(sheet.tab, 1, "hover alone must not navigate");
        sheet.update(tab_rect(2).center(), true);
        assert_eq!(sheet.rows().len(), 4);
        assert_eq!(sheet.rows()[3], ("EXOTIC", 2));
        assert_eq!(
            sheet.stats, before,
            "browsing never purchases or modifies stats"
        );
    }
    #[test]
    fn native_tabs_fit_panel_and_click_the_same_rect_that_draws() {
        let mut sheet = CharacterSheet::default();
        for tab in 0..4 {
            let e = sheet
                .elements()
                .into_iter()
                .find(|e| e.label.as_deref() == Some(&format!("character_tab_{tab}")))
                .unwrap();
            let r = tab_rect(tab);
            assert_eq!(e.rect, [r.x, r.y, r.w, r.h]);
            assert!(r.x + r.w <= PANEL.x + PANEL.w && r.y + r.h <= PANEL.y + PANEL.h);
            sheet.update(r.center(), true);
            assert_eq!(sheet.tab, tab);
        }
    }
}
