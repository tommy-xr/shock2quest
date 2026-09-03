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
use dark::properties::{PropGunState, PropObjShortName, PropSymName};
use shipyard::{EntityId, Get, View, World};

use crate::gui::{self, ButtonHoverBehavior, Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::scripts::Effect;
use crate::scripts::script_util;
use crate::ui::Rect;

use super::research::localized_fallback;

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
const LINE_H: f32 = 11.0;
/// `UNLOAD0.PCX` is 142x22, centred exactly ((188 - 142) / 2 = 23) and flush to
/// the panel's bottom edge. The backdrop authors no strip of its own for it, so
/// it necessarily overlays something; sitting it below the last line row 1 can
/// hold (223 + 5 * 11 = 278) costs only the thin bottom bezel, where any
/// higher placement would cover the row's own text. It is pushed last, and the
/// hit test takes the last match, so the overlap resolves to UNLOAD - the
/// control actually drawn there.
const UNLOAD_RECT: Rect = Rect::new(23.0, 278.0, 142.0, 22.0);

/// Approximate characters per line at the row text width. `mainfont` is
/// variable-width; this is the same conservative greedy-wrap budget the log
/// reader uses (26 chars at 136 px), scaled to this rect.
const ROW_WRAP: usize = 29;

const BACKDROP: &str = "iface/settings.pcx";
const HIGHLIGHT: &str = "iface/setsel.pcx";
/// The UNLOAD button's rest and lit art, the pair every shipped button ships as.
const UNLOAD_ART: &str = "iface/unload0.pcx";
const UNLOAD_ART_HOVER: &str = "iface/unload1.pcx";

/// `/v1/ui` labels, so a client clicks a row by meaning rather than by pixel.
const ROW_LABELS: [&str; 2] = ["setting_0", "setting_1"];
const UNLOAD_LABEL: &str = "unload";

pub struct WeaponSettingsGui;

#[derive(Clone, Debug, Default)]
pub struct WeaponSettingsGuiState;

#[derive(Clone, Debug)]
pub enum WeaponSettingsGuiMsg {
    /// Switch the gun to fire setting 0 or 1.
    SelectSetting(i32),
    /// Eject the magazine back to the backpack.
    Unload,
}

/// Whether an open settings panel must now close. The panel is opened for the
/// gun that was wielded at the time and shows only that gun, so unwielding it -
/// dropping it, holstering it, or cycling to another weapon - dismisses it.
pub fn should_close_settings_panel(opened_for: EntityId, wielded: Option<EntityId>) -> bool {
    wielded != Some(opened_for)
}

/// The gun's display name: its authored short name (an object string), falling
/// back to the symbolic name every entity has.
fn display_name(world: &World, weapon: EntityId) -> Option<String> {
    let short = world
        .borrow::<View<PropObjShortName>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().map(|n| localized_fallback(&n.0)))
        .filter(|name| !name.is_empty());
    short.or_else(|| {
        world
            .borrow::<View<PropSymName>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|n| n.0.clone()))
    })
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
/// ("Modification Level %d") - the same `replace` the HUD's own `%d` item
/// labels use, so a data install whose string lacks the placeholder still draws
/// its own text.
fn mod_level_text(format: &str, modification: i32) -> String {
    format.replace("%d", &modification.to_string())
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

/// How many wrapped lines fit inside `row` starting at `text_y`.
fn row_line_budget(row: Rect, text_y: f32) -> usize {
    ((row.y + row.h - text_y) / LINE_H).floor().max(0.0) as usize
}

impl Gui<WeaponSettingsGuiState, WeaponSettingsGuiMsg> for WeaponSettingsGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        world: &World,
        _state: &WeaponSettingsGuiState,
    ) -> Vec<GuiComponent<WeaponSettingsGuiMsg>> {
        // Archive-qualified for the same reason the log reader qualifies its
        // backdrop: obj.crf and iface.crf collide on plain basenames.
        let mut components: Vec<GuiComponent<WeaponSettingsGuiMsg>> = vec![
            gui::image(BACKDROP)
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(PANEL_W, PANEL_H)),
        ];

        // The panel always shows the wielded gun; the host closes it when that
        // changes (`should_close_settings_panel`), so this cannot draw a stale weapon.
        let Some(weapon) = crate::wielded_weapon::wielded_weapon(world) else {
            return components;
        };

        let modification = world
            .borrow::<View<PropGunState>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|state| state.modification))
            .unwrap_or(0);
        components.push(
            gui::text(&mod_level_text(
                &crate::hud::hud_strings(world).mod_level_label,
                modification,
            ))
            .with_position(vec2(NAME_POS.0, MOD_LEVEL_Y))
            .with_size(vec2(ROW_TEXT_RIGHT - NAME_POS.0, LINE_H)),
        );

        if let Some(name) = display_name(world, weapon) {
            components.push(
                gui::text(&name)
                    .with_position(vec2(NAME_POS.0, NAME_POS.1))
                    .with_size(vec2(ROW_TEXT_RIGHT - NAME_POS.0, LINE_H)),
            );
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
            for (idx, wrapped) in super::media::wrap_text(&line, ROW_WRAP)
                .iter()
                .take(row_line_budget(row, text_y))
                .enumerate()
            {
                if wrapped.is_empty() {
                    continue;
                }
                components.push(
                    gui::text(wrapped)
                        .with_position(vec2(text_x, text_y + idx as f32 * LINE_H))
                        .with_size(vec2(ROW_TEXT_RIGHT - text_x, LINE_H)),
                );
            }
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

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.1),
            screen_size_in_pixels: Vector2::new(PANEL_W, PANEL_H),
        }
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        world: &World,
        state: &WeaponSettingsGuiState,
        msg: &WeaponSettingsGuiMsg,
    ) -> (WeaponSettingsGuiState, Effect) {
        let Some(weapon) = crate::wielded_weapon::wielded_weapon(world) else {
            return (state.clone(), Effect::NoEffect);
        };
        let effect = match msg {
            // `SetGunSetting` is a no-op for a gun with no second mode and
            // plays the mode-switch cue itself.
            WeaponSettingsGuiMsg::SelectSetting(setting) => Effect::SetGunSetting {
                entity_id: weapon,
                setting: *setting,
            },
            WeaponSettingsGuiMsg::Unload => Effect::UnloadWeapon { entity_id: weapon },
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
        PropGunSettingText1, PropGunSettingText2, PropScripts, ToLink,
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

    fn components(world: &World) -> Vec<GuiComponent<WeaponSettingsGuiMsg>> {
        let host = EntityId::dead();
        WeaponSettingsGui.get_components(&None, host, world, &WeaponSettingsGuiState)
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
        let (row1_text_x, row1_text_y) = ROW_TEXT_POS[1];
        let _ = row1_text_x;
        let last_line_bottom = row1_text_y + row_line_budget(ROWS[1], row1_text_y) as f32 * LINE_H;
        assert!(
            UNLOAD_RECT.y >= last_line_bottom,
            "UNLOAD ({}) must start below row 1's last line ({last_line_bottom})",
            UNLOAD_RECT.y
        );
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
            &WeaponSettingsGuiState,
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
            &WeaponSettingsGuiState,
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

    #[test]
    fn the_row_line_budget_keeps_text_inside_its_row() {
        for (row, (_, text_y)) in ROWS.iter().zip(ROW_TEXT_POS) {
            let lines = row_line_budget(*row, text_y);
            assert!(lines > 0);
            assert!(
                text_y + lines as f32 * LINE_H <= row.y + row.h,
                "a full page of row text must not spill past the row"
            );
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
