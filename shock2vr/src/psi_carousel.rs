//! Lightweight amp-local browsing. Navigation previews; closing commits once.
use crate::{
    Handedness,
    psi::{GlobalPsiPowers, PlayerPsiKnownPowers},
    ui::{HAlign, Rect, UiCanvas, VAlign},
};
use cgmath::vec2;
use shipyard::{EntityId, UniqueView, World};

/// Reject button gestures that started before a selector opened/closed.
#[derive(shipyard::Component, Default)]
pub(crate) struct InputEpoch(pub u64);
pub(crate) fn input_epoch(world: &World, amp: EntityId) -> u64 {
    use shipyard::Get;
    world
        .borrow::<shipyard::View<InputEpoch>>()
        .ok()
        .and_then(|v| v.get(amp).ok().map(|e| e.0))
        .unwrap_or(0)
}
pub(crate) fn advance_input_epoch(world: &mut World, amp: EntityId) {
    let next = input_epoch(world, amp).wrapping_add(1);
    world.add_component(amp, InputEpoch(next));
}

pub(crate) struct Carousel {
    pub amp: EntityId,
    pub hand: Handedness,
    pub index: usize,
    pub stick_latched: bool,
    pub trigger_armed: bool,
    pub anchor: crate::ui::FrontendPanelAnchor,
}
impl Carousel {
    pub fn new(world: &World, amp: EntityId, hand: Handedness) -> Option<Self> {
        let selected = crate::psi_amp_selection::selection(world, amp)?;
        let powers = world.borrow::<UniqueView<GlobalPsiPowers>>().ok()?;
        let index = powers
            .0
            .iter()
            .position(|p| p.template_id == selected.current)?;
        Some(Self {
            amp,
            hand,
            index,
            stick_latched: true,
            trigger_armed: false,
            anchor: crate::ui::FrontendPanelAnchor::new(),
        })
    }
    pub fn canvas(
        &self,
        world: &World,
        assets: &mut engine::assets::asset_cache::AssetCache,
    ) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(480.0, 340.0));
        canvas
            .fill(Rect::new(0.0, 0.0, 480.0, 340.0), [6, 19, 26])
            .opacity(0.94);
        let Ok(powers) = world.borrow::<UniqueView<GlobalPsiPowers>>() else {
            return canvas;
        };
        let Ok(known) = world.borrow::<UniqueView<PlayerPsiKnownPowers>>() else {
            return canvas;
        };
        let Some(selected) = powers.0.get(self.index) else {
            return canvas;
        };
        let strings = world
            .borrow::<UniqueView<crate::scripts::gui::GlobalPsiStrings>>()
            .ok();
        let empty = std::collections::HashMap::new();
        let strings = strings.as_ref().map_or(&empty, |s| &s.0);
        let icon = |id: i32, variant: u8| {
            crate::scripts::gui::icon_texture(
                &crate::scripts::gui::icon_basename(strings, id),
                variant,
            )
        };
        const FONT: &str = crate::ui::MFD_FONT;
        canvas.text(
            Rect::new(16.0, 8.0, 230.0, 26.0),
            "PSI AMPLIFIER",
            FONT,
            20.0,
            HAlign::Left,
            VAlign::Middle,
        );
        if let Some(pair) = crate::psi_amp_selection::selection(world, self.amp) {
            for (i, template) in [Some(pair.current), pair.alternate].into_iter().enumerate() {
                let x = 286.0 + i as f32 * 96.0;
                canvas.text(
                    Rect::new(x, 5.0, 88.0, 12.0),
                    if i == 0 { "CURRENT" } else { "ALTERNATE" },
                    FONT,
                    10.0,
                    HAlign::Center,
                    VAlign::Middle,
                );
                if let Some(p) = powers.0.iter().find(|p| Some(p.template_id) == template) {
                    canvas.image(
                        Rect::new(x + 28.0, 19.0, 32.0, 24.0),
                        &icon(p.power.power_id, if i == 0 { 2 } else { 1 }),
                    );
                } else {
                    canvas.text(
                        Rect::new(x, 19.0, 88.0, 24.0),
                        "NONE",
                        FONT,
                        12.0,
                        HAlign::Center,
                        VAlign::Middle,
                    );
                }
            }
        }
        for row in 0..3 {
            let tier = (selected.tier() - 1 + row - 1).rem_euclid(5) + 1;
            let y = 52.0 + row as f32 * 56.0;
            if row == 1 {
                canvas.fill(Rect::new(8.0, y - 3.0, 464.0, 52.0), [16, 55, 62]);
            }
            canvas.text(
                Rect::new(15.0, y, 64.0, 42.0),
                &format!("LEVEL {tier}"),
                FONT,
                12.0,
                HAlign::Left,
                VAlign::Middle,
            );
            for (column, p) in powers.0.iter().filter(|p| p.tier() == tier).enumerate() {
                let x = 87.0 + column as f32 * 53.0;
                let trained = known.0.contains(&p.template_id);
                let active = p.template_id == selected.template_id;
                if active {
                    canvas.fill(Rect::new(x - 2.0, y - 2.0, 50.0, 46.0), [20, 218, 193]);
                }
                canvas
                    .image(
                        Rect::new(x, y, 46.0, 42.0),
                        &icon(
                            p.power.power_id,
                            crate::scripts::gui::icon_kind(trained, active),
                        ),
                    )
                    .opacity(if row == 1 { 1.0 } else { 0.55 });
            }
        }
        let title = selected.display_name.as_deref().unwrap_or(&selected.name);
        canvas.text_fit(
            Rect::new(18.0, 222.0, 444.0, 24.0),
            &format!("{title}  |  {} PSI", selected.power.psi_cost),
            FONT,
            18.0,
            HAlign::Center,
            VAlign::Middle,
        );
        let help = strings
            .get(&format!("psi{}", selected.power.power_id))
            .map(String::as_str)
            .unwrap_or("");
        let help = help
            .lines()
            .filter(|line| !line.trim().eq_ignore_ascii_case(title))
            .collect::<Vec<_>>()
            .join("\n");
        let font = crate::ui::resolve_font(assets, FONT);
        let lines = engine::wrap_text_to_width(&**font, &help, 12.0, 440.0);
        for (i, line) in lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .take(4)
            .enumerate()
        {
            canvas.text_fit(
                Rect::new(20.0, 250.0 + i as f32 * 13.0, 440.0, 13.0),
                line,
                FONT,
                12.0,
                HAlign::Center,
                VAlign::Top,
            );
        }
        canvas.text(
            Rect::new(10.0, 313.0, 460.0, 20.0),
            "STICK: LEVEL / POWER     B / Y OR TRIGGER: SELECT",
            FONT,
            11.0,
            HAlign::Center,
            VAlign::Middle,
        );
        canvas
    }
}
