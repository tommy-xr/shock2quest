use super::*;
use crate::{weapon_installation as installation, weapon_upgrades::WeaponUpgrade};

#[derive(Clone, Debug)]
pub(super) struct UpgradeChooser {
    pub tier: usize,
    pub selected: Option<WeaponUpgrade>,
    pub payment: Payment,
}

pub(super) fn button(
    msg: WeaponSettingsGuiMsg,
    label: &str,
    text: &str,
    rect: Rect,
    selected: bool,
    cursor: &Option<GuiCursor>,
) -> Vec<GuiComponent<WeaponSettingsGuiMsg>> {
    let hovered = cursor
        .as_ref()
        .is_some_and(|c| rect.contains(vec2(c.position.x, c.position.y)));
    let mut out = vec![
        gui::button(msg)
            .with_rect(rect)
            .with_label(label)
            .with_image("iface/tbutmax.pcx")
            .with_nine_slice([90.0, 32.0], [3.0; 4])
            .with_alpha(1.0),
    ];
    // TBUT11 has a baked arrow in its stretchable centre. Use the unlabelled
    // retail bevel and highlight only its interior, leaving every border intact.
    if selected || hovered {
        out.push(GuiComponent::Fill {
            position: vec2(rect.x + 3.0, rect.y + 3.0),
            size: vec2(rect.w - 6.0, rect.h - 6.0),
            color: if selected { [0, 112, 88] } else { [0, 76, 60] },
            alpha: 1.0,
        });
    }
    out.push(super::super::PanelText::text(
        text,
        Rect::new(rect.x + 6.0, rect.y + 4.0, rect.w - 12.0, rect.h - 6.0),
    ));
    out
}

pub(super) fn draw(
    world: &World,
    weapon: EntityId,
    chooser: &UpgradeChooser,
    cursor: &Option<GuiCursor>,
) -> Vec<GuiComponent<WeaponSettingsGuiMsg>> {
    use super::super::PanelText;
    let upgrades = installation::state(world, weapon);
    // Keep the retail MODIFY bezel at its authored 188x296 size. Replace only
    // the board and goal wells; the shared canvas owns all placement in flat/VR.
    let mut out = vec![
        gui::image("iface/modify.pcx")
            .with_rect(Rect::new(0.0, 0.0, 188.0, 296.0))
            .with_alpha(1.0),
        GuiComponent::Fill {
            position: vec2(13.0, 42.0),
            size: vec2(142.0, 135.0),
            color: [0, 58, 47],
            alpha: 1.0,
        },
        // The old board's cost tab extends past its main well.
        GuiComponent::Fill {
            position: vec2(155.0, 148.0),
            size: vec2(21.0, 29.0),
            color: [0, 58, 47],
            alpha: 1.0,
        },
        GuiComponent::Fill {
            position: vec2(13.0, 181.0),
            size: vec2(163.0, 107.0),
            color: [0, 36, 28],
            alpha: 1.0,
        },
        PanelText::text(
            &format!("UPGRADES  {}/4", upgrades.tier()),
            Rect::new(18.0, 14.0, 134.0, 12.0),
        ),
        PanelText::text(
            if chooser.payment == Payment::Modify {
                "Modify skill"
            } else {
                "French-Epstein device"
            },
            Rect::new(18.0, 28.0, 134.0, 11.0),
        ),
    ];
    for (i, choice) in installation::available(world, weapon)
        .into_iter()
        .filter(|choice| !upgrades.has(*choice))
        .filter(|choice| {
            *choice != WeaponUpgrade::LowMaintenanceII
                || upgrades.has(WeaponUpgrade::LowMaintenanceI)
        })
        .enumerate()
    {
        out.extend(button(
            WeaponSettingsGuiMsg::ChooseUpgrade(choice),
            &format!("upgrade_{choice:?}"),
            installation::label(choice),
            Rect::new(14.0, 44.0 + i as f32 * 19.0, 140.0, 18.0),
            chooser.selected == Some(choice),
            cursor,
        ));
    }
    let info = match chooser.selected {
        Some(choice) if upgrades.has(choice) => {
            "Already installed. Choices are permanent.".to_owned()
        }
        Some(choice) => installation::preview(world, weapon, choice, chooser.tier, chooser.payment),
        None => "Select an upgrade to preview it. Choices are permanent.".into(),
    };
    out.extend(PanelText::paragraph(
        world,
        &info,
        Rect::new(17.0, 185.0, 154.0, 53.0),
    ));
    if let Some(choice) = chooser.selected {
        let confirmation = match chooser.payment {
            Payment::Modify => installation::quote(world, weapon, choice, chooser.tier)
                .map(|diff| format!("Attempt: {} nanites", diff.cost as i32)),
            Payment::Device(_) => {
                installation::validate(world, weapon, choice, chooser.tier, chooser.payment)
                    .map(|_| "Install: use 1 device".into())
            }
        };
        match confirmation {
            Ok(text) => out.extend(button(
                WeaponSettingsGuiMsg::ConfirmUpgrade,
                "upgrade_confirm",
                &text,
                Rect::new(15.0, 242.0, 158.0, 21.0),
                false,
                cursor,
            )),
            Err(reason) if reason != info => out.extend(PanelText::paragraph(
                world,
                &reason,
                Rect::new(17.0, 240.0, 154.0, 25.0),
            )),
            Err(_) => {}
        }
    }
    out.extend(button(
        WeaponSettingsGuiMsg::CancelUpgrade,
        "upgrade_cancel",
        "Back",
        Rect::new(15.0, 267.0, 54.0, 21.0),
        false,
        cursor,
    ));
    let switch = match chooser.payment {
        Payment::Modify => {
            installation::available_device(world).map(|d| (Payment::Device(d), "Use device"))
        }
        Payment::Device(_) => Some((Payment::Modify, "Use skill")),
    };
    if let Some((payment, text)) = switch {
        out.extend(button(
            WeaponSettingsGuiMsg::UpgradePayment(payment),
            "upgrade_payment",
            text,
            Rect::new(73.0, 267.0, 100.0, 21.0),
            false,
            cursor,
        ));
    }
    out
}
