//! Retail service-tour debrief art, repurposed as a shared flat/VR battle report.
use crate::{
    horde_stats::HordeBattleStats,
    ui::{HAlign, Rect, UiCanvas, VAlign},
};
use cgmath::Vector2;

pub const LAYOUT_FILE: &str = "debriefr.bin";
// DEBRIEFR.BIN's one authored button, in the original 640x480 art coordinates.
pub const RETAIL_CONTINUE: Rect = Rect::new(425.0, 401.0, 210.0, 74.0);
// Keep visible arena around the panel in both presentations. Resolve this once
// in canvas coordinates so drawing, flat clicks and VR rays cannot drift.
pub const fn inset(rect: Rect) -> Rect {
    Rect::new(
        32.0 + rect.x * 0.9,
        24.0 + rect.y * 0.9,
        rect.w * 0.9,
        rect.h * 0.9,
    )
}
pub const CONTINUE: Rect = inset(RETAIL_CONTINUE);
pub const QUIT: Rect = inset(Rect::new(214.0, 411.0, 198.0, 62.0));

pub fn continue_at(point: Vector2<f32>, continue_rect: Rect) -> Option<bool> {
    if continue_rect.contains(point) {
        Some(true)
    } else if QUIT.contains(point) {
        Some(false)
    } else {
        None
    }
}

fn label(canvas: &mut UiCanvas, rect: Rect, text: &str, title: bool, align: HAlign) {
    canvas.text_native_fit(
        inset(rect),
        text,
        if title {
            crate::ui::TITLE_FONT
        } else {
            crate::ui::MFD_FONT
        },
        align,
        VAlign::Middle,
    );
}

pub fn draw(
    canvas: &mut UiCanvas,
    wave: u32,
    stats: &HordeBattleStats,
    pointer: Option<Vector2<f32>>,
    continue_rect: Rect,
) {
    canvas.image(inset(Rect::new(0.0, 0.0, 640.0, 480.0)), "debrief.pcx");
    // Retail shkdebrf.cpp places the mission title at (6,6), service logo at
    // (212,4), narrative at (216,92), and tour art at (4,320).
    label(
        canvas,
        Rect::new(12.0, 12.0, 190.0, 62.0),
        "SURVIVED",
        true,
        HAlign::Center,
    );
    label(
        canvas,
        Rect::new(222.0, 14.0, 326.0, 30.0),
        "EARTH HORDE",
        true,
        HAlign::Center,
    );
    label(
        canvas,
        Rect::new(222.0, 49.0, 326.0, 24.0),
        "BATTLE REPORT",
        false,
        HAlign::Center,
    );
    label(
        canvas,
        Rect::new(20.0, 154.0, 128.0, 28.0),
        "WAVE",
        false,
        HAlign::Center,
    );
    label(
        canvas,
        Rect::new(20.0, 186.0, 128.0, 40.0),
        &wave.to_string(),
        true,
        HAlign::Center,
    );
    label(
        canvas,
        Rect::new(20.0, 228.0, 128.0, 25.0),
        "CLEARED",
        false,
        HAlign::Center,
    );

    for (index, (name, value)) in [
        ("ENEMIES KILLED", u64::from(stats.enemies_killed)),
        ("DAMAGE TAKEN", stats.damage_taken),
        ("ENEMY HP LOST", stats.enemy_hp_lost),
    ]
    .iter()
    .enumerate()
    {
        let y = 108.0 + index as f32 * 76.0;
        label(
            canvas,
            Rect::new(232.0, y, 304.0, 24.0),
            name,
            false,
            HAlign::Left,
        );
        label(
            canvas,
            Rect::new(232.0, y + 25.0, 304.0, 40.0),
            &value.to_string(),
            true,
            HAlign::Right,
        );
    }
    label(
        canvas,
        Rect::new(232.0, 343.0, 304.0, 20.0),
        "Enemy HP lost includes damage",
        false,
        HAlign::Left,
    );
    label(
        canvas,
        Rect::new(232.0, 365.0, 304.0, 20.0),
        "from all sources.",
        false,
        HAlign::Left,
    );

    for (index, text) in [
        format!("NEXT: WAVE {}", wave.saturating_add(1)),
        "ENDLESS MODE".into(),
        "More enemies each wave".into(),
        "Equipment carries over".into(),
    ]
    .iter()
    .enumerate()
    {
        label(
            canvas,
            Rect::new(14.0, 337.0 + index as f32 * 30.0, 184.0, 24.0),
            text,
            false,
            HAlign::Center,
        );
    }
    for (rect, text) in [(continue_rect, "Continue"), (QUIT, "Main menu")] {
        if pointer.is_some_and(|p| rect.contains(p)) {
            canvas.fill(rect, [24, 72, 80]).opacity(0.65);
        }
        canvas.text_native(rect, text, "metafont.fon", HAlign::Center, VAlign::Middle);
    }
}
