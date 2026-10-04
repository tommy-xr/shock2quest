use super::*;
use crate::{weapon_installation as installation, weapon_upgrades::WeaponUpgrade};

#[derive(Clone, Debug)]
pub(super) struct UpgradeChooser {
    pub tier: usize,
    pub selected: Option<WeaponUpgrade>,
    pub payment: Payment,
}

fn button(
    msg: WeaponSettingsGuiMsg,
    label: &str,
    text: &str,
    rect: Rect,
    cursor: &Option<GuiCursor>,
) -> Vec<GuiComponent<WeaponSettingsGuiMsg>> {
    let hovered = cursor.as_ref().is_some_and(|c| {
        c.position.x >= rect.x
            && c.position.x < rect.x + rect.w
            && c.position.y >= rect.y
            && c.position.y < rect.y + rect.h
    });
    vec![
        GuiComponent::Fill {
            position: origin_of(rect),
            size: extent_of(rect),
            color: if hovered { [0, 85, 70] } else { [0, 48, 42] },
            alpha: 1.0,
        },
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
    cursor: &Option<GuiCursor>,
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
            &format!(
                "{} - tier {}/4",
                if chooser.payment == Payment::Modify {
                    "MODIFY"
                } else {
                    "DEVICE"
                },
                upgrades.tier()
            ),
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
            &format!(
                "{prefix}{}{}",
                installation::label(choice),
                if installation::AVAILABLE.contains(&choice) {
                    ""
                } else {
                    " (later)"
                }
            ),
            Rect::new(6.0, 30.0 + i as f32 * 20.0, 176.0, 19.0),
            cursor,
        ));
        if !installed && !installation::AVAILABLE.contains(&choice) {
            if let Some(GuiComponent::Text { alpha, .. }) = out.last_mut() {
                *alpha = 0.45;
            }
        }
    }
    let info = match chooser.selected {
        Some(choice) if upgrades.has(choice) => {
            "Already installed. Choices are permanent.".to_owned()
        }
        Some(choice) => installation::preview(world, weapon, choice, chooser.tier, chooser.payment),
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
                Rect::new(6.0, 245.0, 176.0, 22.0),
                cursor,
            )),
            Err(reason) if reason != info => out.extend(PanelText::paragraph(
                world,
                &reason,
                Rect::new(10.0, 242.0, 168.0, 29.0),
            )),
            Err(_) => {}
        }
    }
    out.extend(button(
        WeaponSettingsGuiMsg::CancelUpgrade,
        "upgrade_cancel",
        "Back",
        Rect::new(6.0, 275.0, 58.0, 22.0),
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
            Rect::new(68.0, 275.0, 114.0, 22.0),
            cursor,
        ));
    }
    out
}
