use super::*;
use crate::{weapon_installation as installation, weapon_upgrades::WeaponUpgrade};

#[derive(Clone, Debug)]
pub(super) struct UpgradeChooser {
    pub tier: usize,
    pub selected: Option<WeaponUpgrade>,
}

fn button(
    msg: WeaponSettingsGuiMsg,
    label: &str,
    text: &str,
    rect: Rect,
) -> Vec<GuiComponent<WeaponSettingsGuiMsg>> {
    vec![
        gui::button(msg)
            .with_rect(rect)
            .with_label(label)
            .with_alpha(0.0),
        super::super::PanelText::text(
            text,
            Rect::new(rect.x + 4.0, rect.y + 3.0, rect.w - 8.0, rect.h - 4.0),
        ),
    ]
}

pub(super) fn draw(
    world: &World,
    weapon: EntityId,
    chooser: &UpgradeChooser,
) -> Vec<GuiComponent<WeaponSettingsGuiMsg>> {
    use super::super::PanelText;
    let upgrades = installation::state(world, weapon);
    let mut out = vec![
        GuiComponent::Fill {
            position: vec2(0.0, 0.0),
            size: vec2(PANEL_W, PANEL_H),
            color: [0, 35, 32],
            alpha: 1.0,
        },
        PanelText::text(
            &format!("MODIFY - tier {}/4", upgrades.tier()),
            Rect::new(8.0, 8.0, 172.0, 15.0),
        ),
    ];
    for (i, choice) in WeaponUpgrade::ALL.into_iter().enumerate() {
        let installed = upgrades.has(choice);
        let prefix = if installed {
            "+ "
        } else if chooser.selected == Some(choice) {
            "> "
        } else {
            "  "
        };
        out.extend(button(
            WeaponSettingsGuiMsg::ChooseUpgrade(choice),
            &format!("upgrade_{choice:?}"),
            &format!("{prefix}{}", installation::label(choice)),
            Rect::new(6.0, 30.0 + i as f32 * 20.0, 176.0, 20.0),
        ));
    }
    let info = match chooser.selected {
        Some(choice) if upgrades.has(choice) => {
            "Already installed. Choices are permanent.".to_owned()
        }
        Some(choice) => installation::preview(world, weapon, choice, chooser.tier),
        None => {
            "+ marks installed upgrades. Select an upgrade to preview it. Choices are permanent."
                .into()
        }
    };
    out.extend(PanelText::paragraph(
        world,
        &info,
        Rect::new(10.0, 176.0, 168.0, 65.0),
    ));
    if let Some(choice) = chooser.selected {
        match installation::quote(world, weapon, choice, chooser.tier) {
            Ok(diff) => out.extend(button(
                WeaponSettingsGuiMsg::ConfirmUpgrade,
                "upgrade_confirm",
                &format!("Attempt: {} nanites", diff.cost as i32),
                Rect::new(6.0, 245.0, 176.0, 22.0),
            )),
            Err(reason) => out.extend(PanelText::paragraph(
                world,
                &reason,
                Rect::new(10.0, 242.0, 168.0, 29.0),
            )),
        }
    }
    out.extend(button(
        WeaponSettingsGuiMsg::CancelUpgrade,
        "upgrade_cancel",
        "Back",
        Rect::new(6.0, 275.0, 176.0, 22.0),
    ));
    out
}
