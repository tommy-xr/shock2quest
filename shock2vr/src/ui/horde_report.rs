//! One shared canvas and hit map for the flat/VR survival report.
use crate::{
    horde_stats::HordeBattleStats,
    ui::{HAlign, Rect, UiCanvas, VAlign},
};
use cgmath::Vector2;

pub const CONTINUE: Rect = Rect::new(160.0, 318.0, 320.0, 48.0);
pub const QUIT: Rect = Rect::new(160.0, 380.0, 320.0, 40.0);

pub fn continue_at(point: Vector2<f32>) -> Option<bool> {
    if CONTINUE.contains(point) {
        Some(true)
    } else if QUIT.contains(point) {
        Some(false)
    } else {
        None
    }
}

pub fn draw(
    canvas: &mut UiCanvas,
    wave: u32,
    stats: &HordeBattleStats,
    pointer: Option<Vector2<f32>>,
) {
    canvas.fill(Rect::new(0.0, 0.0, 640.0, 480.0), [4, 12, 16]);
    canvas.text_native(
        Rect::new(80.0, 42.0, 480.0, 40.0),
        "SURVIVED",
        crate::ui::TITLE_FONT,
        HAlign::Center,
        VAlign::Middle,
    );
    canvas.text_native(
        Rect::new(80.0, 90.0, 480.0, 26.0),
        &format!("{wave} waves complete — battle report"),
        crate::ui::MFD_FONT,
        HAlign::Center,
        VAlign::Middle,
    );
    for (index, line) in [
        format!("Enemies killed: {}", stats.enemies_killed),
        format!("Damage taken: {}", stats.damage_taken),
        format!("Enemy HP lost: {}", stats.enemy_hp_lost),
    ]
    .iter()
    .enumerate()
    {
        canvas.text_native(
            Rect::new(100.0, 145.0 + index as f32 * 36.0, 440.0, 30.0),
            line,
            crate::ui::MFD_FONT,
            HAlign::Center,
            VAlign::Middle,
        );
    }
    canvas.text_native(
        Rect::new(80.0, 258.0, 480.0, 24.0),
        "Enemy HP lost includes damage from all sources.",
        crate::ui::MFD_FONT,
        HAlign::Center,
        VAlign::Middle,
    );
    for (rect, label) in [
        (CONTINUE, "Continue — endless waves"),
        (QUIT, "Return to main menu"),
    ] {
        let hovered = pointer.is_some_and(|p| rect.contains(p));
        canvas.fill(rect, if hovered { [22, 92, 98] } else { [12, 45, 52] });
        canvas.text_native(
            rect,
            label,
            crate::ui::MFD_FONT,
            HAlign::Center,
            VAlign::Middle,
        );
    }
    canvas.text_native(
        Rect::new(80.0, 430.0, 480.0, 24.0),
        "Endless waves keep increasing. Your equipment carries over.",
        crate::ui::MFD_FONT,
        HAlign::Center,
        VAlign::Middle,
    );
}
