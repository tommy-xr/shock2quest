//! Modify mode on the shared HRM board.
//!
//! A gun can be improved twice - a bigger clip, a faster reload, a thriftier
//! shot - by playing the same node-connecting board the keypads are hacked and
//! broken guns are repaired on, against the Modify skill and the gun's own
//! `P$ModifyDif` (first modification) or `P$Modify2Di` (second, always the
//! harder of the two). Winning raises `PropGunState.modification`, which is
//! what [`gun_modifications`](crate::scripts::gun_modifications) reads to
//! decide how the gun fires; losing breaks the gun outright, which then needs
//! repair before it will fire again.
//!
//! The board is hosted the way the repair board is - a synthetic panel with no
//! world object of its own, presenting whichever gun the player asked about in
//! [`WeaponModifySubject`] - so flat's MFD slot and VR's cyber interface both
//! present the one canvas.
//!
//! Retail put this board up automatically, as a second overlay beside the
//! weapon-settings MFD, whenever that MFD opened on a gun that could still be
//! modified. Both presentations here have a single panel slot, so the settings
//! panel offers a MODIFY control instead and the board takes the slot when it
//! is pressed.

use cgmath::{Vector2, Vector3, vec2};
use dark::properties::{ObjectState, PropGunState, PropHackDiff, PropModify2Diff, PropModifyDiff};
use shipyard::{EntityId, Get, Unique, UniqueView, View, World};

use crate::gui::{self, Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::scripts::Effect;
use crate::scripts::gun_modifications::{MAX_MODIFICATION, modification_level};

use super::keypad::{
    HackOutcomeEffects, HackState, HrmMode, KeyPadMsg, draw_hack_board, handle_hack_msg,
    object_state,
};

/// The board's effect text - what this modification will do - in the slot
/// retail draws the operation's description in, above the node grid (which
/// starts at y 48).
const EFFECT_TEXT: (f32, f32) = (15.0, 12.0);
const EFFECT_TEXT_W: f32 = 137.0;
const LINE_H: f32 = 11.0;
/// Lines of effect text the slot above the grid holds.
const EFFECT_TEXT_LINES: usize = 3;
/// Characters per line at 137 px, the same conservative greedy-wrap budget the
/// log reader and the settings rows use.
const EFFECT_WRAP: usize = 29;

/// The second modification is harder to qualify for than the first: retail
/// adds two levels to whatever minimum the gun authors.
const SECOND_MODIFICATION_SKILL_PREMIUM: i32 = 2;

/// The gun the modify panel is presenting. The panel is synthetic and shared,
/// so the subject rides beside it rather than being the panel's own entity.
#[derive(Unique, Clone, Copy, Debug)]
pub struct WeaponModifySubject(pub EntityId);

pub struct WeaponModifyGui;

#[derive(Clone, Debug, Default)]
pub struct WeaponModifyState {
    hack: HackState,
}

#[derive(Clone)]
pub enum WeaponModifyMsg {
    Board(KeyPadMsg),
}

/// The terms `weapon`'s *next* modification is played on: `P$ModifyDif` for the
/// first, `P$Modify2Di` for the second. A gun already at the last modification
/// - or one whose data gives no terms for the level it is on - has none.
pub(crate) fn next_modify_diff(world: &World, weapon: EntityId) -> Option<PropHackDiff> {
    match modification_level(world, weapon) {
        0 => world
            .borrow::<View<PropModifyDiff>>()
            .ok()
            .and_then(|diffs| diffs.get(weapon).ok().map(|diff| diff.0)),
        1 => world
            .borrow::<View<PropModify2Diff>>()
            .ok()
            .and_then(|diffs| diffs.get(weapon).ok().map(|diff| diff.0)),
        _ => None,
    }
}

/// Whether `weapon` is a gun modify mode acts on at all: one that tracks gun
/// state and authors a modify script. That script is also the table of what a
/// modification does, so a gun without one has nothing modification could
/// change. The psi amp and the melee weapons are excluded by the same test -
/// neither authors one.
fn is_modifiable_gun(world: &World, weapon: EntityId) -> bool {
    world
        .borrow::<View<PropGunState>>()
        .map(|states| states.get(weapon).is_ok())
        .unwrap_or(false)
        && crate::scripts::gun_modifications::has_modify_script(world, weapon)
}

/// The minimum Modify level `weapon` demands for its next modification. The
/// second is two levels harder than the first, on top of whatever the gun
/// authors.
fn required_modify_level(world: &World, weapon: EntityId) -> i32 {
    let authored =
        crate::scripts::script_util::required_tech_level(world, weapon, |tech| tech.modify());
    if modification_level(world, weapon) >= 1 {
        authored + SECOND_MODIFICATION_SKILL_PREMIUM
    } else {
        authored
    }
}

/// What asking to modify a gun comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifyEntry {
    /// Open the board on this gun.
    Board,
    /// The gun has had every modification it can take.
    AlreadyFullyModified,
    /// The player's Modify skill is below what this modification demands.
    SkillRequired { level: i32 },
    /// Not a gun modification does anything to, or one that has given out and
    /// needs repair before anything else.
    NotModifiable,
}

/// Decide what asking to modify `weapon` does.
pub fn modify_entry(world: &World, weapon: EntityId) -> ModifyEntry {
    if !is_modifiable_gun(world, weapon)
        || matches!(
            object_state(world, weapon),
            ObjectState::Broken | ObjectState::Destroyed
        )
    {
        return ModifyEntry::NotModifiable;
    }
    // Two things end a gun's modifications: reaching the last one, and data
    // that gives no terms for the next. Both are "there is no more to do here"
    // rather than a refusal the player can act on.
    if modification_level(world, weapon) >= MAX_MODIFICATION
        || next_modify_diff(world, weapon).is_none()
    {
        return ModifyEntry::AlreadyFullyModified;
    }
    let required = required_modify_level(world, weapon);
    // Judged with the board's own definition of the player's modify level, so
    // the door and the odds behind it can never disagree.
    if HrmMode::Modify.player_level(world) < required {
        return ModifyEntry::SkillRequired { level: required };
    }
    ModifyEntry::Board
}

/// The effect the settings panel's MODIFY control has, or `None` when the gun
/// offers no such control at all.
pub fn use_modify_control(world: &World, weapon: EntityId) -> Option<Effect> {
    match modify_entry(world, weapon) {
        ModifyEntry::NotModifiable => None,
        ModifyEntry::Board => Some(Effect::OpenWeaponModify { entity_id: weapon }),
        ModifyEntry::AlreadyFullyModified => Some(Effect::ShowMessage {
            text: crate::hud::hud_strings(world).modify_maxed.clone(),
        }),
        ModifyEntry::SkillRequired { level } => Some(Effect::ShowMessage {
            text: crate::hud::hud_strings(world)
                .modify_skill_req
                .replace("%d", &level.to_string()),
        }),
    }
}

/// Whether the settings panel draws a MODIFY control for `weapon` - it does
/// whenever modification means anything to that gun, so a gun already fully
/// modified still has somewhere to be told so.
pub fn offers_modify_control(world: &World, weapon: EntityId) -> bool {
    modify_entry(world, weapon) != ModifyEntry::NotModifiable
}

/// A won board is one more modification, up to the last one a gun can take.
fn modify_success(entity_id: EntityId, _world: &World) -> Effect {
    Effect::ModifyWeapon { entity_id }
}

/// A critical failure breaks the gun, which is repair's problem from then on.
/// Unlike a failed repair, nothing is destroyed - the gun is recoverable.
fn modify_critical_failure(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetObjectState {
        entity_id,
        state: ObjectState::Broken,
    }
}

/// The gun the panel is presenting and the terms its next modification is
/// played on - both, or neither.
fn subject(world: &World) -> Option<(EntityId, PropHackDiff)> {
    let gun = world
        .borrow::<UniqueView<WeaponModifySubject>>()
        .ok()
        .map(|subject| subject.0)?;
    Some((gun, next_modify_diff(world, gun)?))
}

impl Gui<WeaponModifyState, WeaponModifyMsg> for WeaponModifyGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        world: &World,
        state: &WeaponModifyState,
    ) -> Vec<GuiComponent<WeaponModifyMsg>> {
        let Some((gun, diff)) = subject(world) else {
            return Vec::new();
        };
        let mut components = draw_hack_board(&state.hack, diff, WeaponModifyMsg::Board);

        // What this modification will do, which is the gun's own
        // `P$Modify1`/`P$Modify2` text - the only place the game ever shows it.
        if let Some(effect) = crate::scripts::script_util::modification_description(
            world,
            gun,
            modification_level(world, gun) + 1,
        ) {
            for (index, line) in super::media::wrap_text(&effect, EFFECT_WRAP)
                .iter()
                .take(EFFECT_TEXT_LINES)
                .enumerate()
            {
                components.push(
                    gui::text(line)
                        .with_position(vec2(EFFECT_TEXT.0, EFFECT_TEXT.1 + index as f32 * LINE_H))
                        .with_size(vec2(EFFECT_TEXT_W, LINE_H)),
                );
            }
        }
        components
    }

    /// One host serves every gun, so its board must not outlive the opening it
    /// was dealt for - the same contract the repair board has. Without it a
    /// finished board would still be sitting there, terminal, when the next
    /// gun opened the panel.
    fn resets_state_on_frob(&self) -> bool {
        true
    }

    fn get_config(&self) -> GuiConfig {
        // The board's own 188x296 canvas, the size every HRM presentation uses.
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.1),
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        world: &World,
        state: &WeaponModifyState,
        msg: &WeaponModifyMsg,
    ) -> (WeaponModifyState, Effect) {
        let WeaponModifyMsg::Board(board_msg) = msg;
        let Some((gun, diff)) = subject(world) else {
            return (state.clone(), Effect::NoEffect);
        };
        // A gun that has since been modified to the last level, or broken by a
        // failed attempt, has nothing left to play for; ignore stale board
        // input rather than charging for another attempt.
        if modify_entry(world, gun) != ModifyEntry::Board {
            return (state.clone(), Effect::NoEffect);
        }
        let (hack, effect) = handle_hack_msg(
            gun,
            world,
            &state.hack,
            board_msg,
            diff,
            HrmMode::Modify,
            HackOutcomeEffects {
                success: modify_success,
                critical_failure: modify_critical_failure,
            },
        );
        (WeaponModifyState { hack }, effect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_stats::Skill;
    use dark::properties::{
        PropObjState, PropRequiredTechDesc, PropScripts, PropSymName, TechSkillValues,
    };

    /// The shipped pistol: a modify script, both modify difficulties, and the
    /// authored Modify requirement of one.
    fn fixture(modify_skill: i32, modification: i32) -> (World, EntityId) {
        let mut world = World::new();
        let gun = world.add_entity((
            PropGunState {
                ammo: 12,
                condition: 100.0,
                setting: 0,
                modification,
                silence_value: 0.0,
            },
            PropScripts {
                scripts: vec!["PistolModify".to_owned()],
                inherits: true,
            },
            PropSymName("Pistol".to_owned()),
            PropModifyDiff(PropHackDiff {
                success_chance: 40,
                critical_chance: 2,
                cost: 20.0,
            }),
            PropModify2Diff(PropHackDiff {
                success_chance: 30,
                critical_chance: 4,
                cost: 20.0,
            }),
            PropRequiredTechDesc(TechSkillValues([0, 1, 1, 1, 0])),
        ));
        let mut quests = crate::quest_info::QuestInfo::new();
        for _ in 0..modify_skill {
            quests.player_stats_mut().raise_skill(Skill::Modify);
        }
        world.add_unique(quests);
        (world, gun)
    }

    /// The first modification is played on `P$ModifyDif` and the second on the
    /// harder `P$Modify2Di`; there is no third.
    #[test]
    fn each_modification_is_played_on_its_own_terms() {
        let (world, gun) = fixture(3, 0);
        assert_eq!(next_modify_diff(&world, gun).unwrap().success_chance, 40);

        let (world, gun) = fixture(3, 1);
        assert_eq!(next_modify_diff(&world, gun).unwrap().success_chance, 30);

        let (world, gun) = fixture(3, 2);
        assert_eq!(next_modify_diff(&world, gun), None);
    }

    /// A gun at the last modification is refused rather than charged for an
    /// attempt that could not do anything.
    #[test]
    fn a_fully_modified_gun_cannot_be_modified_again() {
        let (world, gun) = fixture(9, 2);
        assert_eq!(modify_entry(&world, gun), ModifyEntry::AlreadyFullyModified);
        assert!(matches!(
            use_modify_control(&world, gun),
            Some(Effect::ShowMessage { text }) if text == "This weapon cannot be modified any further."
        ));
        // The control is still offered: it is where the player is told so.
        assert!(offers_modify_control(&world, gun));
    }

    /// The second modification demands two levels more than the first.
    #[test]
    fn the_second_modification_is_two_levels_harder() {
        let (world, gun) = fixture(1, 0);
        assert_eq!(modify_entry(&world, gun), ModifyEntry::Board);

        let (world, gun) = fixture(1, 1);
        assert_eq!(
            modify_entry(&world, gun),
            ModifyEntry::SkillRequired { level: 3 }
        );

        let (world, gun) = fixture(3, 1);
        assert_eq!(modify_entry(&world, gun), ModifyEntry::Board);
    }

    /// Installed modify software counts toward the entry requirement exactly
    /// as it counts toward the board's odds.
    #[test]
    fn modify_software_counts_toward_the_skill_gate() {
        let (world, gun) = fixture(0, 0);
        assert_eq!(
            modify_entry(&world, gun),
            ModifyEntry::SkillRequired { level: 1 }
        );

        world
            .borrow::<shipyard::UniqueViewMut<crate::quest_info::QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .install_software(crate::player_stats::Software::Modify, 1);
        assert_eq!(modify_entry(&world, gun), ModifyEntry::Board);
    }

    /// A gun that has given out is repair's problem: modification offers
    /// nothing at all until it works again.
    #[test]
    fn a_broken_gun_is_not_modifiable() {
        let (mut world, gun) = fixture(9, 0);
        world.add_component(gun, (PropObjState(ObjectState::Broken),));

        assert_eq!(modify_entry(&world, gun), ModifyEntry::NotModifiable);
        assert!(use_modify_control(&world, gun).is_none());
        assert!(!offers_modify_control(&world, gun));
    }

    /// A weapon that authors no modify script - a melee weapon, the psi amp -
    /// offers no control at all, even where the data gives modify terms.
    #[test]
    fn a_weapon_with_no_modify_script_offers_nothing() {
        let (mut world, _) = fixture(9, 0);
        let wrench = world.add_entity((
            PropGunState {
                ammo: 0,
                condition: 100.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            PropModifyDiff(PropHackDiff {
                success_chance: 40,
                critical_chance: 2,
                cost: 20.0,
            }),
        ));
        assert_eq!(modify_entry(&world, wrench), ModifyEntry::NotModifiable);
    }

    /// A won board is one more modification; a critical failure breaks the gun
    /// rather than destroying it, so it can still be repaired.
    #[test]
    fn winning_modifies_and_a_critical_failure_breaks_the_gun() {
        let (world, gun) = fixture(3, 0);
        assert!(matches!(
            modify_success(gun, &world),
            Effect::ModifyWeapon { entity_id } if entity_id == gun
        ));
        assert!(matches!(
            modify_critical_failure(gun, &world),
            Effect::SetObjectState { entity_id, state: ObjectState::Broken } if entity_id == gun
        ));
    }

    /// One panel host serves every gun, so a board must not survive the
    /// opening it was dealt for.
    #[test]
    fn each_opening_deals_a_fresh_board() {
        assert!(
            WeaponModifyGui.resets_state_on_frob(),
            "a shared panel must reset per open"
        );

        let mut carried = WeaponModifyState::default();
        carried.hack.phase = super::super::keypad::HackPhase::Won;
        WeaponModifyGui.prepare_state_on_frob(&mut carried);
        assert_eq!(
            carried.hack.phase,
            super::super::keypad::HackPhase::Unpaid,
            "the next gun's board must start unpaid, not on the last gun's win"
        );
    }

    /// Board input against a gun that can no longer be modified is ignored, so
    /// a gun cannot be charged for a third attempt.
    #[test]
    fn the_board_ignores_input_once_the_gun_is_fully_modified() {
        let (mut world, gun) = fixture(9, 2);
        world.add_unique(WeaponModifySubject(gun));
        let panel = world.add_entity((PropObjState(ObjectState::Normal),));

        let (_, effect) = WeaponModifyGui.handle_msg(
            panel,
            &world,
            &WeaponModifyState::default(),
            &WeaponModifyMsg::Board(KeyPadMsg::StartHack),
        );
        assert!(matches!(effect, Effect::NoEffect));
    }
}
