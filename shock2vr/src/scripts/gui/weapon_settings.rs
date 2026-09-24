//! Weapon settings MFD - the panel the AMMOFULL readout's SETTING button opens.
//!
//! The original's weapon-settings overlay: a `SETTINGS.PCX` backdrop naming the
//! wielded gun and its modification level, over two selectable rows - one per
//! fire setting - each reading "{header}: {description}" from the gun's
//! `P$SHead1`/`P$Sett1` and `P$SHead2`/`P$Sett2` object strings. `SETSEL.PCX`
//! highlights the row the gun is currently set to; clicking the other row
//! switches to it. A gun that takes clips also carries an UNLOAD button that
//! ejects the magazine back to the backpack.
//!
//! The canvas is presentation-agnostic - placement is decided once here, in
//! panel pixels (AGENTS.md section 3), and `FlatUiHost` presents that one
//! component list in flat's MFD slot and in VR's cyber-interface panel slot
//! alike. What VR does not have yet is a *way in*: the forearm readout draws no
//! buttons, because the VR pointer only ever reaches the cyber-interface panel
//! and never the forearm quad. When that readout gains its buttons, its SETTING
//! button emits the same `Effect::OpenWeaponSettings` and this panel works with
//! no further wiring.

use cgmath::{Vector2, Vector3, vec2};
use dark::properties::PropGunState;
use shipyard::{EntityId, Get, View, World};

use super::hrm_plug::{self, PlugKind, draw_plug, plug_sidecar};
use super::keypad::{
    HackOutcomeEffects, HackPhase, HackState, HrmContext, KeyPadMsg, draw_hack_board,
    draw_hrm_text, handle_hrm_msg,
};
use crate::gui::{
    self, ButtonHoverBehavior, Gui, GuiComponent, GuiConfig, GuiCursor, PanelSidecar,
};
use crate::scripts::Effect;
use crate::scripts::script_util;
use crate::ui::Rect;
use crate::weapon_modification;
use crate::weapon_repair;

/// A panel-local rect's upper-left corner / extent, as the component builders
/// want them.
fn origin_of(rect: Rect) -> Vector2<f32> {
    vec2(rect.x, rect.y)
}
fn extent_of(rect: Rect) -> Vector2<f32> {
    vec2(rect.w, rect.h)
}

/// `SETTINGS.PCX` is 188x300 - the MFD slot's 188 width, four pixels taller
/// than the 188x296 reader/keypad art. The panel is drawn at the art's own size.
const PANEL_W: f32 = 188.0;
const PANEL_H: f32 = 300.0;

/// The gun's short name, on the art's name bar (y 127..147).
const NAME_POS: (f32, f32) = (24.0, 133.0);
/// The two fire-setting rows: the selection highlight covers the whole row and
/// the row is the click target.
///
/// Both are 164x68 - `SETSEL.PCX`'s native size - because that is what the
/// backdrop authors. Scanning a column of `SETTINGS.PCX` gives the boxes as
/// y 150..217 and y 220..287, 68 px each, separated by a 2 px rule; drawing the
/// highlight at any other height squashes the art non-uniformly.
const ROWS: [Rect; 2] = [
    Rect::new(11.0, 150.0, 164.0, HIGHLIGHT_SIZE.1),
    Rect::new(11.0, 220.0, 164.0, HIGHLIGHT_SIZE.1),
];
/// `SETSEL.PCX`'s authored size. A row that is not exactly this is a bug.
const HIGHLIGHT_SIZE: (f32, f32) = (164.0, 68.0);
/// Where each row's text starts, inset from the row box.
const ROW_TEXT_POS: [(f32, f32); 2] = [(22.0, 153.0), (22.0, 223.0)];
/// Row text runs to the row's right edge.
const ROW_TEXT_RIGHT: f32 = 175.0;
/// The modification line sits 50 px above the first row, which puts it at the
/// foot of the art's large top box. That box is the weapon-icon area; the panel
/// draws no icon into it today, so the line has it to itself.
const MOD_LEVEL_Y: f32 = ROWS[0].y - 50.0;
/// `UNLOAD0.PCX` is 142x22, centred exactly ((188 - 142) / 2 = 23) and flush to
/// the panel's bottom edge. The backdrop authors no strip of its own for it, so
/// it necessarily overlays something: the thin bottom bezel and row 1's last
/// 10 px, where row 1's text stops (`row_text_bottom`). It is pushed last, and
/// the hit test takes the last match, so the overlap resolves to UNLOAD - the
/// control actually drawn there.
const UNLOAD_RECT: Rect = Rect::new(23.0, 278.0, 142.0, 22.0);

const BACKDROP: &str = "iface/settings.pcx";

const HIGHLIGHT: &str = "iface/setsel.pcx";
/// The UNLOAD button's rest and lit art, the pair every shipped button ships as.
const UNLOAD_ART: &str = "iface/unload0.pcx";
const UNLOAD_ART_HOVER: &str = "iface/unload1.pcx";

/// `/v1/ui` labels, so a client clicks a row by meaning rather than by pixel.
const ROW_LABELS: [&str; 2] = ["setting_0", "setting_1"];
const UNLOAD_LABEL: &str = "unload";

#[derive(shipyard::Unique, Clone, Copy, Default)]
pub(crate) struct WeaponSettingsTarget(pub Option<EntityId>);

impl WeaponSettingsTarget {
    pub(crate) fn select(world: &World, weapon: EntityId) {
        world.add_unique(Self::default());
        world.borrow::<shipyard::UniqueViewMut<Self>>().unwrap().0 = Some(weapon);
    }

    fn resolve(world: &World) -> Option<EntityId> {
        let target = world.borrow::<shipyard::UniqueView<Self>>().ok()?.0?;
        crate::wielded_weapon::resolve_weapon_target(world, Some(target))
    }
}

pub struct WeaponSettingsGui;

#[derive(Clone, Debug, Default)]
pub struct WeaponSettingsGuiState {
    board: Option<(HrmJob, dark::properties::PropHackDiff, HackState)>,
}

/// The plug the settings panel raises for `weapon`, if any, with its button's
/// message and label.
fn plug_for(
    world: &World,
    weapon: EntityId,
) -> Option<(PlugKind, WeaponSettingsGuiMsg, &'static str)> {
    if weapon_repair::is_broken(world, weapon) {
        weapon_repair::supported(world, weapon).then_some((
            PlugKind::Repair,
            WeaponSettingsGuiMsg::Repair,
            "repair",
        ))
    } else {
        weapon_modification::supported(world, weapon).then_some((
            PlugKind::Modify,
            WeaponSettingsGuiMsg::Modify,
            "modify",
        ))
    }
}

/// Whether `weapon` can ever raise a plug. Unlike `plug_for` this does not
/// flip with the gun's condition, so the canvas width stays fixed while the
/// panel is open (a repair win on an unmodifiable gun removes its plug).
fn has_plug_room(world: &World, weapon: EntityId) -> bool {
    weapon_repair::supported(world, weapon) || weapon_modification::supported(world, weapon)
}

/// What the open HRM board is doing to the gun.
#[derive(Clone, Copy, Debug, PartialEq)]
enum HrmJob {
    /// Modifying from this level; a change of level mid-board aborts it.
    Modify(i32),
    Repair,
}

impl HrmJob {
    fn context(self) -> HrmContext {
        match self {
            HrmJob::Modify(_) => HrmContext::Modify,
            HrmJob::Repair => HrmContext::Repair,
        }
    }

    fn quote(
        self,
        world: &World,
        weapon: EntityId,
    ) -> Result<dark::properties::PropHackDiff, String> {
        match self {
            HrmJob::Modify(_) => weapon_modification::quote(world, weapon),
            HrmJob::Repair => weapon_repair::quote(world, weapon),
        }
    }
}

#[derive(Clone, Debug)]
pub enum WeaponSettingsGuiMsg {
    /// Switch the gun to fire setting 0 or 1.
    SelectSetting(i32),
    /// Eject the magazine back to the backpack.
    Unload,
    Modify,
    Repair,
    Board(KeyPadMsg),
}

/// Whether an open settings panel must now close. The panel is opened for the
/// gun that was wielded at the time and shows only that gun, so unwielding it -
/// dropping it, holstering it, or cycling to another weapon - dismisses it.
pub fn should_close_settings_panel(opened_for: EntityId, wielded: Option<EntityId>) -> bool {
    wielded != Some(opened_for)
}

/// The line each row shows: "{header}: {description}". A gun that names no
/// header for a setting has no such setting and draws no row.
fn row_text(header: Option<&str>, description: Option<&str>) -> Option<String> {
    let header = header?;
    Some(match description {
        Some(description) => format!("{header}: {description}"),
        None => header.to_owned(),
    })
}

/// Substitute the modification level into the MISC.STR `ModLevel` format
/// ("Modification Level %d"); a string without the placeholder draws as is.
fn mod_level_text(format: &str, modification: i32) -> String {
    super::PanelText::format(format, &[modification])
}

/// Whether the settings panel offers an UNLOAD button for `weapon`. The button
/// appears only when pressing it would do something, the way the readout's own
/// controls do: an energy weapon has no magazine at all - it recharges - and
/// neither an empty magazine nor one whose projectile has no clip archetype to
/// return to has rounds this could hand back.
fn shows_unload(world: &World, weapon: EntityId) -> bool {
    if crate::wielded_weapon::is_energy_weapon(world, weapon) {
        return false;
    }
    let loaded = world
        .borrow::<View<PropGunState>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().map(|state| state.ammo > 0))
        .unwrap_or(false);
    loaded && crate::mission::reload::can_unload(world, weapon)
}

/// Where a row's text must stop: the row's bottom, or UNLOAD's top where the
/// button overlays the row.
fn row_text_bottom(row: Rect) -> f32 {
    (row.y + row.h).min(UNLOAD_RECT.y)
}

impl Gui<WeaponSettingsGuiState, WeaponSettingsGuiMsg> for WeaponSettingsGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        world: &World,
        state: &WeaponSettingsGuiState,
    ) -> Vec<GuiComponent<WeaponSettingsGuiMsg>> {
        // Archive-qualified for the same reason the log reader qualifies its
        // backdrop: obj.crf and iface.crf collide on plain basenames.
        let mut components: Vec<GuiComponent<WeaponSettingsGuiMsg>> = vec![
            gui::image(BACKDROP)
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(PANEL_W, PANEL_H)),
        ];

        // The panel owns an explicit gun target. Losing it makes the panel
        // inert immediately; a second held gun is never a fallback.
        let Some(weapon) = WeaponSettingsTarget::resolve(world) else {
            return components;
        };

        if let Some((job, diff, board)) = &state.board {
            let shown_diff = if matches!(board.phase, HackPhase::Won | HackPhase::Lost) {
                *diff
            } else {
                job.quote(world, weapon).unwrap_or(*diff)
            };
            // Archive-qualified: obj.crf also ships a repair.pcx.
            let (backdrop, goal) = match job {
                // The level the board was opened at: after a win the gun
                // already carries the next one.
                HrmJob::Modify(level) => (
                    "modify.pcx",
                    weapon_modification::description(world, weapon, *level),
                ),
                HrmJob::Repair => (
                    "iface/repair.pcx",
                    super::PanelText::string(
                        world,
                        "hrm",
                        "RepairText",
                        "Return this item to normal functionality.",
                    ),
                ),
            };
            let mut components = draw_hack_board(board, shown_diff, WeaponSettingsGuiMsg::Board);
            // The authored Modify and Repair boards share HRM geometry with Hack.
            if let Some(GuiComponent::Image { texture, .. }) = components.first_mut() {
                *texture = backdrop.into();
            }
            components.extend(draw_hrm_text(world, &goal, shown_diff, job.context()));
            return components;
        }
        // Retail raises its HRM plug beside the settings MFD: repair for a
        // Broken gun, modify for a working one.
        if let Some((kind, msg, label)) = plug_for(world, weapon) {
            components.extend(draw_plug(kind, Some((msg, label))));
        }

        let modification = world
            .borrow::<View<PropGunState>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|state| state.modification))
            .unwrap_or(0);
        // Retail draws every settings line in the MFD font (MAINAA, cyan).
        let line_h = super::PanelText::line_height(world);
        components.push(super::PanelText::text(
            &mod_level_text(
                &crate::hud::hud_strings(world).mod_level_label,
                modification,
            ),
            Rect::new(NAME_POS.0, MOD_LEVEL_Y, ROW_TEXT_RIGHT - NAME_POS.0, line_h),
        ));

        if let Some(name) = script_util::object_short_name(world, weapon) {
            components.push(super::PanelText::text(
                &name,
                Rect::new(NAME_POS.0, NAME_POS.1, ROW_TEXT_RIGHT - NAME_POS.0, line_h),
            ));
        }

        let current = script_util::current_gun_setting(world, weapon);
        for setting in 0..2i32 {
            let row = ROWS[setting as usize];
            let header = script_util::gun_setting_header(world, weapon, setting);
            let Some(line) = row_text(
                header.as_deref(),
                script_util::gun_setting_description(world, weapon, setting).as_deref(),
            ) else {
                continue;
            };
            // The highlight and the click target are one rect: both are `row`.
            if setting == current {
                components.push(
                    gui::image(HIGHLIGHT)
                        .with_position(origin_of(row))
                        .with_size(extent_of(row)),
                );
            }
            // A zero-alpha button is the established "hit target over backdrop
            // art" convention (`flat_ui_host::draw_components`): the row boxes
            // are painted into SETTINGS.PCX, so the row must be clickable
            // without painting anything of its own over them.
            components.push(
                gui::button(WeaponSettingsGuiMsg::SelectSetting(setting))
                    .with_image(HIGHLIGHT)
                    .with_alpha(0.0)
                    .with_label(ROW_LABELS[setting as usize])
                    .with_position(origin_of(row))
                    .with_size(extent_of(row)),
            );
            let (text_x, text_y) = ROW_TEXT_POS[setting as usize];
            components.extend(super::PanelText::paragraph(
                world,
                &line,
                Rect::new(
                    text_x,
                    text_y,
                    ROW_TEXT_RIGHT - text_x,
                    row_text_bottom(row) - text_y,
                ),
            ));
        }

        if shows_unload(world, weapon) {
            components.push(
                gui::button(WeaponSettingsGuiMsg::Unload)
                    .with_image(UNLOAD_ART)
                    .with_hover(ButtonHoverBehavior::Texture(UNLOAD_ART_HOVER.to_owned()))
                    .with_label(UNLOAD_LABEL)
                    .with_position(origin_of(UNLOAD_RECT))
                    .with_size(extent_of(UNLOAD_RECT)),
            );
        }

        components
    }

    /// Each open starts at the settings: a finished (or abandoned) board must
    /// not greet the next gun the panel is opened for.
    fn resets_state_on_frob(&self) -> bool {
        true
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.1),
            screen_size_in_pixels: Vector2::new(PANEL_W, PANEL_H),
        }
    }

    /// A gun with a plug widens the canvas for it for as long as the panel is
    /// open - board included - so the VR quad never resizes mid-session.
    fn get_config_for(
        &self,
        _entity_id: EntityId,
        world: &World,
        _state: &WeaponSettingsGuiState,
    ) -> GuiConfig {
        let mut config = self.get_config();
        if WeaponSettingsTarget::resolve(world).is_some_and(|w| has_plug_room(world, w)) {
            config.screen_size_in_pixels.x = hrm_plug::CANVAS_W;
        }
        config
    }

    /// The plug's room is kept while the panel is open; it shows (and takes
    /// clicks) only beside the settings, not the board.
    fn sidecar(
        &self,
        _entity_id: EntityId,
        world: &World,
        state: &WeaponSettingsGuiState,
    ) -> Option<PanelSidecar> {
        let weapon = WeaponSettingsTarget::resolve(world)?;
        if !has_plug_room(world, weapon) {
            return None;
        }
        Some(plug_sidecar(
            state.board.is_none() && plug_for(world, weapon).is_some(),
        ))
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        world: &World,
        state: &WeaponSettingsGuiState,
        msg: &WeaponSettingsGuiMsg,
    ) -> (WeaponSettingsGuiState, Effect) {
        let Some(weapon) = WeaponSettingsTarget::resolve(world) else {
            return (state.clone(), Effect::NoEffect);
        };
        match msg {
            WeaponSettingsGuiMsg::Modify | WeaponSettingsGuiMsg::Repair => {
                let job = if matches!(msg, WeaponSettingsGuiMsg::Repair) {
                    HrmJob::Repair
                } else {
                    HrmJob::Modify(weapon_modification::level(world, weapon).unwrap_or(-1))
                };
                return match job.quote(world, weapon) {
                    Ok(diff) => (
                        WeaponSettingsGuiState {
                            board: Some((job, diff, HackState::default())),
                        },
                        Effect::NoEffect,
                    ),
                    Err(text) => (state.clone(), Effect::ShowMessage { text }),
                };
            }
            WeaponSettingsGuiMsg::Board(msg) => {
                let Some((job, _, board)) = &state.board else {
                    return (state.clone(), Effect::NoEffect);
                };
                if matches!(board.phase, HackPhase::Won | HackPhase::Lost) {
                    return (state.clone(), Effect::NoEffect);
                }
                let quote = job.quote(world, weapon);
                let level_changed = matches!(job, HrmJob::Modify(level)
                    if weapon_modification::level(world, weapon) != Some(*level));
                if level_changed || quote.is_err() {
                    return (
                        WeaponSettingsGuiState::default(),
                        Effect::ShowMessage {
                            text: quote
                                .err()
                                .unwrap_or_else(|| "Weapon modification changed.".into()),
                        },
                    );
                }
                let diff = quote.unwrap();
                let outcomes = match job {
                    HrmJob::Modify(_) => HackOutcomeEffects {
                        success: |entity_id, world| {
                            Effect::combine(vec![
                                Effect::ShowMessage {
                                    text: super::PanelText::hrm(
                                        world,
                                        "ModifyResult1",
                                        "Modification completed!",
                                        &[],
                                    ),
                                },
                                Effect::ModifyWeapon {
                                    entity_id,
                                    expected_level: weapon_modification::level(world, entity_id)
                                        .unwrap_or(-1),
                                },
                            ])
                        },
                        critical_failure: |entity_id, world| {
                            Effect::combine(vec![
                                Effect::ShowMessage {
                                    text: super::PanelText::hrm(
                                        world,
                                        "ModifyResult2",
                                        "Modification Failed!",
                                        &[],
                                    ),
                                },
                                Effect::SetObjectState {
                                    entity_id,
                                    state: dark::properties::ObjectState::Broken,
                                },
                            ])
                        },
                    },
                    HrmJob::Repair => HackOutcomeEffects {
                        success: weapon_repair::success,
                        critical_failure: weapon_repair::critical_failure,
                    },
                };
                let (board, effect) =
                    handle_hrm_msg(weapon, world, board, msg, diff, job.context(), outcomes);
                return (
                    WeaponSettingsGuiState {
                        board: Some((*job, diff, board)),
                    },
                    effect,
                );
            }
            _ => {}
        }
        let effect = match msg {
            // `SetGunSetting` is a no-op for a gun with no second mode and
            // plays the mode-switch cue itself.
            WeaponSettingsGuiMsg::SelectSetting(setting) => Effect::SetGunSetting {
                entity_id: weapon,
                setting: *setting,
            },
            WeaponSettingsGuiMsg::Unload => Effect::UnloadWeapon { entity_id: weapon },
            _ => Effect::NoEffect,
        };
        (state.clone(), effect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission::PlayerInfo;
    use crate::mission::mission_core::{GlobalGunSettingHeaders, GlobalGunSettingTexts};
    use crate::mission::reload::GlobalProjectileClips;
    use cgmath::{Quaternion, vec3};
    use dark::properties::{
        Link, Links, ProjectileOptions, PropGunSettingHeader1, PropGunSettingHeader2,
        PropGunSettingText1, PropGunSettingText2, PropObjShortName, PropScripts, PropSymName,
        ToLink,
    };
    use std::collections::HashMap;

    /// A world with `weapon` wielded in the right hand, and empty gun-setting
    /// string tables (every setting resolves from the entity's own properties).
    fn wield(world: &mut World, weapon: EntityId) {
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: Some(weapon),
            inventory_entity_id: inventory,
        });
        WeaponSettingsTarget::select(world, weapon);
        world.add_unique(GlobalGunSettingHeaders([HashMap::new(), HashMap::new()]));
        world.add_unique(GlobalGunSettingTexts([HashMap::new(), HashMap::new()]));
    }

    fn gun_state(setting: i32, modification: i32) -> PropGunState {
        PropGunState {
            ammo: 6,
            condition: 100.0,
            setting,
            modification,
            silence_value: 0.0,
        }
    }

    /// The pistol's standard projectile and the clip archetype its rounds go
    /// back to - all `can_unload` needs to say the magazine is returnable.
    const STD_PROJECTILE: i32 = -37;
    const STD_CLIP: i32 = -31;

    /// A two-mode pistol, wielded, at fire setting `setting`, holding `ammo`
    /// returnable rounds.
    fn pistol_world_with_ammo(setting: i32, ammo: i32) -> (World, EntityId) {
        let mut world = World::new();
        let weapon = world.add_entity((
            PropGunState {
                ammo,
                ..gun_state(setting, 2)
            },
            PropSymName("Pistol".to_owned()),
            PropObjShortName("name_pistol: \"Pistol\"".to_owned()),
            PropGunSettingHeader1("pistol: \"NORM\"".to_owned()),
            PropGunSettingHeader2("pistol: \"BURST\"".to_owned()),
        ));
        world.add_component(
            weapon,
            Links {
                to_links: vec![ToLink {
                    link: Link::Projectile(ProjectileOptions {
                        order: 0,
                        setting: -1,
                    }),
                    to_entity_id: None,
                    to_template_id: STD_PROJECTILE,
                }],
            },
        );
        world.add_unique(GlobalProjectileClips {
            clips: HashMap::from([(STD_PROJECTILE, vec![STD_CLIP])]),
            clip_sizes: HashMap::from([(STD_CLIP, 12)]),
        });
        world.add_component(
            weapon,
            PropGunSettingText1(
                "Pistol: \"This is the normal single-shot firing mode.\"".to_owned(),
            ),
        );
        world.add_component(
            weapon,
            PropGunSettingText2("Pistol: \"This is the 3-shot rapid burst mode.\"".to_owned()),
        );
        wield(&mut world, weapon);
        (world, weapon)
    }

    /// The pistol as the bench hands it out: loaded.
    fn pistol_world(setting: i32) -> (World, EntityId) {
        pistol_world_with_ammo(setting, 6)
    }

    /// A Broken gun raises the repair plug in the modify plug's place, and
    /// its button opens the board on the repair art.
    #[test]
    fn a_broken_gun_offers_repair_instead_of_modify() {
        use dark::properties::{ObjectState, PropHackDiff, PropObjState, PropRepairDiff};

        let (mut world, weapon) = pistol_world(0);
        world.add_component(
            weapon,
            (
                PropScripts {
                    scripts: vec!["PistolModify".to_owned()],
                    inherits: false,
                },
                PropRepairDiff(PropHackDiff {
                    success_chance: 20,
                    critical_chance: 4,
                    cost: 3.0,
                }),
            ),
        );
        let mut quests = crate::quest_info::QuestInfo::new();
        quests.player_stats_mut().skills.repair = 1;
        world.add_unique(quests);
        let plug = |world: &World| {
            components(world).into_iter().find_map(|c| match c {
                GuiComponent::Button {
                    label: Some(label),
                    texture,
                    ..
                } if label == "modify" || label == "repair" => Some((label, texture)),
                _ => None,
            })
        };

        assert_eq!(plug(&world), Some(("modify".into(), "plugm0.pcx".into())));

        world.add_component(weapon, PropObjState(ObjectState::Broken));
        assert_eq!(plug(&world), Some(("repair".into(), "plugr0.pcx".into())));
        assert_eq!(
            WeaponSettingsGui
                .get_config_for(EntityId::dead(), &world, &WeaponSettingsGuiState::default())
                .screen_size_in_pixels
                .x,
            hrm_plug::CANVAS_W,
            "the plug widens the canvas beside the settings",
        );

        let (state, _) = WeaponSettingsGui.handle_msg(
            EntityId::dead(),
            &world,
            &WeaponSettingsGuiState::default(),
            &WeaponSettingsGuiMsg::Repair,
        );
        let board = WeaponSettingsGui.get_components(&None, EntityId::dead(), &world, &state);
        assert!(matches!(
            board.first(),
            Some(GuiComponent::Image { texture, .. }) if texture == "iface/repair.pcx"
        ));
    }

    /// Repairing a gun that cannot be modified removes its plug; the canvas
    /// must keep its width anyway, or the VR quad resizes mid-session.
    #[test]
    fn the_canvas_keeps_its_width_when_a_repair_removes_the_plug() {
        use dark::properties::{ObjectState, PropHackDiff, PropObjState, PropRepairDiff};

        let (mut world, weapon) = pistol_world(0);
        world.add_component(
            weapon,
            (
                PropRepairDiff(PropHackDiff {
                    success_chance: 20,
                    critical_chance: 4,
                    cost: 3.0,
                }),
                PropObjState(ObjectState::Broken),
            ),
        );
        let state = WeaponSettingsGuiState::default();
        let width = |world: &World| {
            WeaponSettingsGui
                .get_config_for(EntityId::dead(), world, &state)
                .screen_size_in_pixels
                .x
        };
        assert_eq!(width(&world), hrm_plug::CANVAS_W);

        world.add_component(weapon, PropObjState(ObjectState::Normal));
        assert_eq!(width(&world), hrm_plug::CANVAS_W);
        let sidecar = WeaponSettingsGui.sidecar(EntityId::dead(), &world, &state);
        assert_eq!(sidecar.map(|s| s.rect), Some(None), "no plug to click");
    }

    #[test]
    fn settings_stay_bound_to_the_selected_left_gun_and_go_inert_when_dropped() {
        let (mut world, left) = pistol_world(0);
        let right = world.add_entity((gun_state(0, 0),));
        {
            let mut player = world
                .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
                .unwrap();
            player.left_hand_entity_id = Some(left);
            player.right_hand_entity_id = Some(right);
        }
        let click = || {
            WeaponSettingsGui
                .handle_msg(
                    EntityId::dead(),
                    &world,
                    &WeaponSettingsGuiState::default(),
                    &WeaponSettingsGuiMsg::SelectSetting(1),
                )
                .1
        };
        assert!(
            matches!(click(), Effect::SetGunSetting { entity_id, setting: 1 } if entity_id == left)
        );
        WeaponSettingsTarget::select(&world, right);
        assert!(
            matches!(click(), Effect::SetGunSetting { entity_id, setting: 1 } if entity_id == right)
        );
        WeaponSettingsTarget::select(&world, left);
        assert!(
            matches!(click(), Effect::SetGunSetting { entity_id, setting: 1 } if entity_id == left)
        );
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .left_hand_entity_id = None;
        assert!(matches!(click(), Effect::NoEffect));
        assert!(labels(&components(&world)).is_empty());
    }

    fn components(world: &World) -> Vec<GuiComponent<WeaponSettingsGuiMsg>> {
        let host = EntityId::dead();
        WeaponSettingsGui.get_components(&None, host, world, &WeaponSettingsGuiState::default())
    }

    /// Every placed (texture, position, size, interactive) - images and buttons.
    fn placed(
        components: &[GuiComponent<WeaponSettingsGuiMsg>],
    ) -> Vec<(String, Vector2<f32>, Vector2<f32>, bool)> {
        components
            .iter()
            .filter_map(|c| match c {
                GuiComponent::Image {
                    texture,
                    position,
                    size,
                    ..
                } => Some((texture.clone(), *position, *size, false)),
                GuiComponent::Button {
                    texture,
                    position,
                    size,
                    ..
                } => Some((texture.clone(), *position, *size, true)),
                _ => None,
            })
            .collect()
    }

    fn texts(components: &[GuiComponent<WeaponSettingsGuiMsg>]) -> Vec<String> {
        components
            .iter()
            .filter_map(|c| match c {
                GuiComponent::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn labels(components: &[GuiComponent<WeaponSettingsGuiMsg>]) -> Vec<String> {
        components
            .iter()
            .filter_map(|c| match c {
                GuiComponent::Button { label, .. } => label.clone(),
                _ => None,
            })
            .collect()
    }

    /// The rect of the (single) drawn `SETSEL` highlight, if any.
    fn highlight(
        components: &[GuiComponent<WeaponSettingsGuiMsg>],
    ) -> Option<(Vector2<f32>, Vector2<f32>)> {
        let mut found = placed(components)
            .into_iter()
            .filter(|(texture, _, _, interactive)| texture == HIGHLIGHT && !interactive)
            .map(|(_, position, size, _)| (position, size));
        let first = found.next();
        assert!(found.next().is_none(), "at most one row is highlighted");
        first
    }

    #[test]
    fn the_rows_do_not_overlap_and_sit_inside_the_panel() {
        assert!(ROWS[0].y + ROWS[0].h <= ROWS[1].y, "rows must not overlap");
        for row in ROWS {
            assert!(row.x >= 0.0 && row.x + row.w <= PANEL_W);
            assert!(row.y >= 0.0 && row.y + row.h <= PANEL_H);
        }
        // The UNLOAD strip is centred and flush to the panel's bottom edge, and
        // starts below the last line of text row 1 can hold - so it covers only
        // the bezel, never a row's own words.
        assert_eq!(UNLOAD_RECT.y + UNLOAD_RECT.h, PANEL_H);
        assert_eq!(UNLOAD_RECT.x, (PANEL_W - UNLOAD_RECT.w) / 2.0);
    }

    /// The selection highlight is blitted at its authored size, so a row that is
    /// not exactly `SETSEL.PCX`'s 164x68 squashes the art. Both of the
    /// backdrop's setting boxes measure 68 px tall (y 150..217 and y 220..287).
    #[test]
    fn every_row_is_exactly_the_highlight_arts_native_size() {
        for row in ROWS {
            assert_eq!(
                (row.w, row.h),
                HIGHLIGHT_SIZE,
                "row {row:?} does not match SETSEL.PCX"
            );
        }
    }

    #[test]
    fn the_highlight_covers_exactly_the_current_setting_row() {
        for setting in 0..2usize {
            let (world, _) = pistol_world(setting as i32);
            let components = components(&world);
            assert_eq!(
                highlight(&components),
                Some((origin_of(ROWS[setting]), extent_of(ROWS[setting]))),
                "setting {setting}'s row must carry the highlight"
            );
        }
    }

    #[test]
    fn each_row_is_clickable_over_exactly_its_highlight_rect() {
        let (world, _) = pistol_world(0);
        let components = components(&world);
        let rows: Vec<_> = placed(&components)
            .into_iter()
            .filter(|(texture, _, _, interactive)| texture == HIGHLIGHT && *interactive)
            .map(|(_, position, size, _)| (position, size))
            .collect();
        assert_eq!(
            rows,
            vec![
                (origin_of(ROWS[0]), extent_of(ROWS[0])),
                (origin_of(ROWS[1]), extent_of(ROWS[1])),
            ],
            "the hit target and the highlight are the same rect, so they cannot diverge"
        );
        assert_eq!(labels(&components)[0..2], ROW_LABELS.map(str::to_owned));
    }

    #[test]
    fn a_row_reads_its_header_and_description() {
        let (world, _) = pistol_world(0);
        let drawn = texts(&components(&world)).join(" ");
        assert!(
            drawn.contains("NORM: This is the normal"),
            "row 0 must read \"{{header}}: {{description}}\", got {drawn:?}"
        );
        assert!(drawn.contains("BURST: This is the 3-shot"));
        assert!(drawn.contains("Pistol"), "the panel names the gun");
        assert!(
            drawn.contains("Modification Level 2"),
            "the panel reports the gun's modification level, got {drawn:?}"
        );
    }

    #[test]
    fn a_gun_with_one_fire_mode_draws_one_row() {
        let mut world = World::new();
        let weapon = world.add_entity((
            gun_state(0, 0),
            PropSymName("Rick Turret Gun".to_owned()),
            PropGunSettingHeader1("turret: \"NORM\"".to_owned()),
        ));
        wield(&mut world, weapon);
        let components = components(&world);
        assert_eq!(
            labels(&components)
                .into_iter()
                .filter(|label| label.starts_with("setting_"))
                .collect::<Vec<_>>(),
            vec![ROW_LABELS[0]],
            "a gun that names no second header offers no second row"
        );
    }

    #[test]
    fn an_energy_weapon_has_no_unload_button() {
        let (world, _) = pistol_world(0);
        assert!(
            labels(&components(&world)).contains(&UNLOAD_LABEL.to_owned()),
            "a loaded gun whose rounds have a clip to go back to can eject them"
        );

        // An empty magazine has nothing to hand back, so the button that would
        // hand it back is not drawn dead.
        let (empty, _) = pistol_world_with_ammo(0, 0);
        assert!(
            !labels(&components(&empty)).contains(&UNLOAD_LABEL.to_owned()),
            "an empty magazine offers no UNLOAD"
        );

        let mut world = World::new();
        let weapon = world.add_entity((
            gun_state(0, 0),
            PropSymName("laser".to_owned()),
            PropGunSettingHeader1("laser: \"NORM\"".to_owned()),
            PropGunSettingHeader2("laser: \"OVER\"".to_owned()),
            PropScripts {
                scripts: vec!["EnergyWeapon".to_owned()],
                inherits: true,
            },
        ));
        wield(&mut world, weapon);
        assert!(
            !labels(&components(&world)).contains(&UNLOAD_LABEL.to_owned()),
            "a rechargeable weapon has no magazine to eject"
        );
    }

    #[test]
    fn nothing_wielded_draws_only_the_backdrop() {
        let mut world = World::new();
        let empty = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: empty,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: empty,
        });
        let components = components(&world);
        assert_eq!(placed(&components).len(), 1);
        assert!(texts(&components).is_empty());
    }

    #[test]
    fn clicking_a_row_sets_that_fire_mode_on_the_wielded_gun() {
        let (world, weapon) = pistol_world(0);
        let (_, effect) = WeaponSettingsGui.handle_msg(
            EntityId::dead(),
            &world,
            &WeaponSettingsGuiState::default(),
            &WeaponSettingsGuiMsg::SelectSetting(1),
        );
        assert!(matches!(
            effect,
            Effect::SetGunSetting {
                entity_id,
                setting: 1
            } if entity_id == weapon
        ));

        let (_, effect) = WeaponSettingsGui.handle_msg(
            EntityId::dead(),
            &world,
            &WeaponSettingsGuiState::default(),
            &WeaponSettingsGuiMsg::Unload,
        );
        assert!(matches!(
            effect,
            Effect::UnloadWeapon { entity_id } if entity_id == weapon
        ));
    }

    #[test]
    fn the_panel_closes_when_its_gun_stops_being_wielded() {
        let mut world = World::new();
        let gun = world.add_entity(());
        let other = world.add_entity(());
        assert!(!should_close_settings_panel(gun, Some(gun)));
        assert!(should_close_settings_panel(gun, Some(other)));
        assert!(should_close_settings_panel(gun, None));
    }

    /// Row text stays inside its row and above UNLOAD, with room for at least
    /// four MFD-font (13 px) lines.
    #[test]
    fn row_text_stays_inside_its_row_and_clear_of_unload() {
        for (row, (_, text_y)) in ROWS.iter().zip(ROW_TEXT_POS) {
            let bottom = row_text_bottom(*row);
            assert!(bottom <= row.y + row.h && bottom <= UNLOAD_RECT.y);
            assert!(bottom - text_y >= 4.0 * 13.0);
        }
    }

    #[test]
    fn mod_level_substitutes_the_level_and_survives_a_string_without_it() {
        assert_eq!(
            mod_level_text("Modification Level %d", 3),
            "Modification Level 3"
        );
        assert_eq!(mod_level_text("Modification", 3), "Modification");
    }

    #[test]
    fn a_row_without_a_header_has_no_line_at_all() {
        assert_eq!(row_text(None, Some("described")), None);
        assert_eq!(row_text(Some("NORM"), None), Some("NORM".to_owned()));
        assert_eq!(
            row_text(Some("NORM"), Some("single shot")),
            Some("NORM: single shot".to_owned())
        );
    }
}
