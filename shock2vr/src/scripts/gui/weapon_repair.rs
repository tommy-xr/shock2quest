//! Repair mode on the shared HRM board.
//!
//! A gun that has broken cannot be fired, and no amount of maintenance brings
//! it back - it has to be repaired, which is the same node-connecting board the
//! keypads, computers, crates and replicators are hacked on, played against the
//! Repair skill and the object's own `P$RepairDif` terms instead of Hack and
//! `P$HackDiff`. Winning returns the gun to working order and gives ten
//! condition points back; landing on a mine destroys it outright.
//!
//! The board is presented by a synthetic panel that belongs to no world object,
//! the way the weapon-settings MFD is: the subject is whichever gun the player
//! used, held in [`WeaponRepairSubject`]. That is what gives it a way in from
//! both presentations at once - flat docks the panel in the MFD slot, VR
//! presents the same canvas in the cyber interface.

use cgmath::{Vector2, Vector3};
use dark::properties::{ObjectState, PropGunState, PropHackDiff, PropRepairDiff};
use shipyard::{EntityId, Get, Unique, UniqueView, View, World};

use crate::gui::{Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::scripts::Effect;

use super::keypad::{
    HackOutcomeEffects, HackPhase, HackState, HrmMode, KeyPadMsg, draw_hack_board, handle_hack_msg,
    object_state,
};

/// Condition points a successful repair gives back, on top of returning the
/// gun to working order.
const REPAIR_CONDITION_BONUS: f32 = 10.0;

/// What the repair panel is presenting. The panel itself is synthetic and
/// shared, so the subject rides beside it rather than being the panel's own
/// entity.
///
/// The terms are pinned here at the moment the board is dealt rather than
/// re-read from the gun every frame, because a critical failure destroys that
/// gun: a board that re-derived itself from the subject would lose everything
/// it needs to draw in the same instant it lost, and the player would never
/// see the result it lost with.
#[derive(Unique, Clone, Copy, Debug)]
pub struct WeaponRepairSubject {
    pub gun: EntityId,
    /// The terms the repair is played on.
    pub terms: PropHackDiff,
}

/// The subject a board opened on `gun` right now would be dealt for, or `None`
/// when a board is not what asking to repair it comes to. Decided by
/// [`repair_entry`], the one place that judgement lives.
pub fn repair_subject(world: &World, gun: EntityId) -> Option<WeaponRepairSubject> {
    if repair_entry(world, gun) != RepairEntry::Board {
        return None;
    }
    Some(WeaponRepairSubject {
        gun,
        terms: repair_diff(world, gun)?,
    })
}

pub struct WeaponRepairGui;

#[derive(Clone, Debug, Default)]
pub struct WeaponRepairState {
    hack: HackState,
}

#[derive(Clone)]
pub enum WeaponRepairMsg {
    Board(KeyPadMsg),
}

/// The object's authored `P$RepairDif` - the terms it is repaired on.
pub(crate) fn repair_diff(world: &World, entity_id: EntityId) -> Option<PropHackDiff> {
    world
        .borrow::<View<PropRepairDiff>>()
        .ok()
        .and_then(|diffs| diffs.get(entity_id).ok().map(|diff| diff.0))
}

/// Whether the world still holds `entity_id`.
fn is_alive(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<shipyard::EntitiesView>()
        .map(|entities| entities.is_alive(entity_id))
        .unwrap_or(false)
}

/// Whether `entity_id` has given out - the working order repair mode applies
/// to, and the one a broken icon is drawn for.
fn is_out_of_order(world: &World, entity_id: EntityId) -> bool {
    matches!(
        object_state(world, entity_id),
        ObjectState::Broken | ObjectState::Destroyed
    )
}

/// Whether `entity_id` is a gun that has given out - the one thing repair mode
/// applies to. The condition itself does not matter: a gun breaks by state,
/// not by wearing all the way down. The psi amp is excluded on the same terms
/// the maintenance tool excludes it: it inherits a gun state that means
/// nothing, so it is not a ranged weapon for this purpose.
fn is_broken_gun(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropGunState>>()
        .map(|v| v.get(entity_id).is_ok())
        .unwrap_or(false)
        && !crate::wielded_weapon::is_psi_amp(world, entity_id)
        && is_out_of_order(world, entity_id)
}

/// What using a broken gun comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairEntry {
    /// Open the board on this gun.
    Board,
    /// The player's Repair skill is below the level the gun authors.
    SkillRequired { level: i32 },
    /// Not a broken gun, or one the data gives no repair terms for - there is
    /// nothing to open a board over.
    NotRepairable,
}

/// Decide what using `item` from the inventory does about its being broken.
pub fn repair_entry(world: &World, item: EntityId) -> RepairEntry {
    if !is_broken_gun(world, item) || repair_diff(world, item).is_none() {
        return RepairEntry::NotRepairable;
    }
    let required =
        crate::scripts::script_util::required_tech_level(world, item, |tech| tech.repair());
    // Judged with the board's own definition of the player's repair level, so
    // the door and the odds behind it can never disagree.
    if HrmMode::Repair.player_level(world) < required {
        return RepairEntry::SkillRequired { level: required };
    }
    RepairEntry::Board
}

/// The effect using a carried `item` has when it is a broken gun, or `None`
/// when it is not one and the ordinary use gesture applies. This is the entry
/// point both presentations share: the flat inventory's use gesture and the VR
/// cyber interface both run it through
/// [`maintenance::use_carried_item`](crate::scripts::maintenance::use_carried_item).
pub fn use_broken_weapon(world: &World, item: EntityId) -> Option<Effect> {
    match repair_entry(world, item) {
        RepairEntry::NotRepairable => None,
        RepairEntry::Board => Some(Effect::OpenWeaponRepair { entity_id: item }),
        RepairEntry::SkillRequired { level } => Some(Effect::ShowMessage {
            text: crate::hud::hud_strings(world)
                .repair_skill_req
                .replace("%d", &level.to_string()),
        }),
    }
}

/// A repaired gun works again, and comes back ten condition points better off
/// than it broke.
fn repair_success(entity_id: EntityId, _world: &World) -> Effect {
    Effect::combine(vec![
        Effect::SetObjectState {
            entity_id,
            state: ObjectState::Normal,
        },
        Effect::AdjustWeaponCondition {
            entity_id,
            delta: REPAIR_CONDITION_BONUS,
        },
    ])
}

/// A critical failure destroys what was being repaired, at the moment it
/// fails. Destroying the entity also takes it out of whatever hand or backpack
/// held it, so a gun cannot be left wielded after it ceases to exist. The
/// board stands over the wreck showing the loss because it holds everything it
/// draws in [`WeaponRepairSubject`] rather than reading it back off the gun.
fn repair_critical_failure(entity_id: EntityId, _world: &World) -> Effect {
    Effect::DestroyEntity { entity_id }
}

/// What the panel is presenting, as it was pinned when the board was dealt.
fn subject(world: &World) -> Option<WeaponRepairSubject> {
    world
        .borrow::<UniqueView<WeaponRepairSubject>>()
        .ok()
        .map(|subject| *subject)
}

impl Gui<WeaponRepairState, WeaponRepairMsg> for WeaponRepairGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        world: &World,
        state: &WeaponRepairState,
    ) -> Vec<GuiComponent<WeaponRepairMsg>> {
        let Some(subject) = subject(world) else {
            return Vec::new();
        };
        // A gun that no longer exists was destroyed by a critical failure -
        // the one thing that destroys one - so the board wears the terminal
        // loss face and offers no deal, driven by the world rather than the
        // in-memory phase (the crate's ruined face works the same way). That
        // covers a subject taken away by anything else too: a board that can
        // never act again must not look live.
        let hack = if is_alive(world, subject.gun) {
            state.hack.clone()
        } else {
            HackState {
                phase: HackPhase::Lost,
                ..state.hack.clone()
            }
        };
        draw_hack_board(
            &hack,
            subject.terms,
            HrmMode::Repair,
            WeaponRepairMsg::Board,
        )
    }

    /// One host serves every gun, so its board must not outlive the opening it
    /// was dealt for: without this, a won or lost board would still be sitting
    /// there - terminal, and refusing to start - when the next broken gun
    /// opened the panel, and a part-played board would carry its paid nodes
    /// onto a different gun. Reopening on the same gun therefore deals afresh
    /// and charges again, which is how the board behaves everywhere else.
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
        state: &WeaponRepairState,
        msg: &WeaponRepairMsg,
    ) -> (WeaponRepairState, Effect) {
        let WeaponRepairMsg::Board(board_msg) = msg;
        let Some(subject) = subject(world) else {
            return (state.clone(), Effect::NoEffect);
        };
        // A gun that is no longer broken - repaired, or destroyed by the
        // failure this board is still showing - has nothing left to repair;
        // ignore stale board input rather than charging for another attempt.
        if !is_broken_gun(world, subject.gun) {
            return (state.clone(), Effect::NoEffect);
        }
        // The board is played against the *gun*, not the panel presenting it:
        // that is what keys the deal off the gun's own stable id and sources
        // the board's sounds at the object being worked on.
        let (hack, effect) = handle_hack_msg(
            subject.gun,
            world,
            &state.hack,
            board_msg,
            subject.terms,
            HrmMode::Repair,
            HackOutcomeEffects {
                success: repair_success,
                critical_failure: repair_critical_failure,
            },
        );
        (WeaponRepairState { hack }, effect)
    }
}

/// An item's inventory art: its broken icon while it has given out and authors
/// one, and its ordinary `P$ObjIcon` otherwise. The backpack grid, the loot
/// panel and the drag cursor all resolve their art here, so an item cannot
/// look intact in the backpack and broken on the cursor. (The VR arm HUD's
/// ammo icons come from a per-template table and are not item art.)
pub fn inventory_icon(world: &World, entity_id: EntityId) -> Option<String> {
    if is_out_of_order(world, entity_id) {
        if let Some(icon) = world
            .borrow::<View<dark::properties::PropObjBrokenIcon>>()
            .ok()
            .and_then(|v| v.get(entity_id).ok().map(|icon| icon.0.clone()))
            .filter(|icon| !icon.is_empty())
        {
            return Some(icon);
        }
    }
    world
        .borrow::<View<dark::properties::PropObjIcon>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|icon| icon.0.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_stats::Skill;
    use dark::properties::{
        PropObjBrokenIcon, PropObjIcon, PropObjState, PropRepairDiff, PropRequiredTechDesc,
        TechSkillValues,
    };

    /// A pistol at full condition with the shipped repair terms (success 20,
    /// four criticals, three nanites) and the shipped required Repair level of
    /// one, plus a player trained to `repair`.
    fn fixture(repair: i32) -> (World, EntityId) {
        let mut world = World::new();
        let gun = world.add_entity((
            PropGunState {
                ammo: 12,
                condition: 40.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            PropRepairDiff(PropHackDiff {
                success_chance: 20,
                critical_chance: 4,
                cost: 3.0,
            }),
            PropRequiredTechDesc(TechSkillValues([0, 1, 1, 1, 0])),
        ));
        let mut quests = crate::quest_info::QuestInfo::new();
        for _ in 0..repair {
            quests.player_stats_mut().raise_skill(Skill::Repair);
        }
        world.add_unique(quests);
        (world, gun)
    }

    fn break_gun(world: &mut World, gun: EntityId) {
        world.add_component(gun, (PropObjState(ObjectState::Broken),));
    }

    fn effects(effect: Effect) -> Vec<Effect> {
        Effect::flatten(vec![effect])
    }

    /// The dispatch: a working gun is used normally, a broken one goes to the
    /// board.
    #[test]
    fn only_a_broken_gun_opens_the_repair_board() {
        let (mut world, gun) = fixture(1);
        assert_eq!(repair_entry(&world, gun), RepairEntry::NotRepairable);
        assert!(use_broken_weapon(&world, gun).is_none());

        break_gun(&mut world, gun);
        assert_eq!(repair_entry(&world, gun), RepairEntry::Board);
        assert!(matches!(
            use_broken_weapon(&world, gun),
            Some(Effect::OpenWeaponRepair { entity_id }) if entity_id == gun
        ));
    }

    /// A broken object the data gives no repair terms for cannot be worked on
    /// at all - the board would have nothing to charge or roll against.
    #[test]
    fn a_broken_object_with_no_repair_terms_is_not_repairable() {
        let (mut world, _) = fixture(1);
        let junk = world.add_entity((
            PropGunState {
                ammo: 0,
                condition: 0.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            PropObjState(ObjectState::Broken),
        ));
        assert_eq!(repair_entry(&world, junk), RepairEntry::NotRepairable);
    }

    #[test]
    fn repairing_below_the_authored_skill_requirement_refuses() {
        let (mut world, gun) = fixture(0);
        break_gun(&mut world, gun);

        assert_eq!(
            repair_entry(&world, gun),
            RepairEntry::SkillRequired { level: 1 }
        );
        assert!(
            matches!(use_broken_weapon(&world, gun), Some(Effect::ShowMessage { text })
                if text == "Repair skill 1 required."),
        );
    }

    #[test]
    fn a_won_repair_restores_working_order_and_ten_condition_points() {
        let (mut world, gun) = fixture(1);
        break_gun(&mut world, gun);

        let effects = effects(repair_success(gun, &world));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::SetObjectState { entity_id, state: ObjectState::Normal } if *entity_id == gun
        )));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::AdjustWeaponCondition { entity_id, delta }
                if *entity_id == gun && *delta == 10.0
        )));
    }

    /// A critical failure destroys the gun. Destroying it is also what takes
    /// it out of the hand holding it, so no unequip step of its own is needed.
    #[test]
    fn a_critical_failure_destroys_the_gun() {
        let (mut world, gun) = fixture(1);
        break_gun(&mut world, gun);

        assert!(matches!(
            repair_critical_failure(gun, &world),
            Effect::DestroyEntity { entity_id } if entity_id == gun
        ));
    }

    /// Every image a board in `state` would draw, in draw order.
    fn drawn(world: &World, panel: EntityId, state: &WeaponRepairState) -> Vec<String> {
        WeaponRepairGui
            .get_components(&None, panel, world, state)
            .into_iter()
            .filter_map(|component| match component {
                crate::ui::UiElement::Image { texture, .. } => Some(texture),
                _ => None,
            })
            .collect()
    }

    /// The whole point: playing a mine destroys the gun *and* leaves a board
    /// that still draws the loss art over the wreck. The board draws from what
    /// it pinned when it was dealt, so losing its subject does not blank it -
    /// re-reading the gun would, and then the player would never see the one
    /// result that matters.
    #[test]
    fn a_lost_board_still_draws_its_loss_art_once_the_gun_is_gone() {
        let (mut world, gun) = fixture(1);
        break_gun(&mut world, gun);
        // No node can survive, so the mine below is played for certain.
        world.add_component(
            gun,
            (PropRepairDiff(PropHackDiff {
                success_chance: 0,
                critical_chance: 4,
                cost: 3.0,
            }),),
        );
        world.add_unique(repair_subject(&world, gun).expect("a broken gun is a repair job"));
        let panel = world.add_entity((PropObjState(ObjectState::Normal),));

        let mut dealt = WeaponRepairState::default();
        dealt.hack.phase = super::super::keypad::HackPhase::Playing;
        dealt.hack.nodes[super::super::keypad::board_index(0, 0)] =
            super::super::keypad::HackNode::Mine;

        let (after, effect) = WeaponRepairGui.handle_msg(
            panel,
            &world,
            &dealt,
            &WeaponRepairMsg::Board(KeyPadMsg::PlayNode { x: 0, y: 0 }),
        );

        assert_eq!(after.hack.phase, super::super::keypad::HackPhase::Lost);
        assert!(
            effects(effect)
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity { entity_id } if *entity_id == gun)),
            "a lost repair should destroy the gun"
        );

        // The effect is applied by the mission loop after the script returns;
        // do here what it does, then ask the panel what it draws.
        world.delete_entity(gun);
        let drawn = drawn(&world, panel, &after);
        assert!(
            !WeaponRepairGui
                .get_components(&None, panel, &world, &after)
                .iter()
                .any(|component| matches!(
                    component,
                    crate::ui::UiElement::Button { label, .. } if label.as_deref() == Some("start-hack")
                )),
            "a board that can never act again must offer no deal"
        );
        assert!(
            drawn.contains(&HrmMode::Repair.art().lost.to_owned()),
            "the lost board should still draw the repair loss art (got {drawn:?})"
        );
        assert!(
            drawn.contains(&HrmMode::Repair.art().backdrop.to_owned()),
            "and its backdrop with it (got {drawn:?})"
        );
    }

    /// The board over a destroyed gun accepts nothing: it is a picture of a
    /// result, not a live board that could charge for another attempt.
    #[test]
    fn a_lost_board_accepts_no_further_input() {
        let (mut world, gun) = fixture(1);
        break_gun(&mut world, gun);
        world.add_unique(repair_subject(&world, gun).expect("a broken gun is a repair job"));
        let panel = world.add_entity((PropObjState(ObjectState::Normal),));
        world.delete_entity(gun);

        let (_, effect) = WeaponRepairGui.handle_msg(
            panel,
            &world,
            &WeaponRepairState::default(),
            &WeaponRepairMsg::Board(KeyPadMsg::StartHack),
        );
        assert!(matches!(effect, Effect::NoEffect));
    }

    /// Board input against a gun that is no longer broken is ignored, so a
    /// repaired gun cannot be charged for another attempt.
    #[test]
    fn the_board_ignores_input_once_the_gun_works_again() {
        let (mut world, gun) = fixture(1);
        break_gun(&mut world, gun);
        world.add_unique(repair_subject(&world, gun).expect("a broken gun is a repair job"));
        let panel = world.add_entity((PropObjState(ObjectState::Normal),));
        // Repaired between the board being dealt and this input landing.
        world.add_component(gun, (PropObjState(ObjectState::Normal),));

        let (_, effect) = WeaponRepairGui.handle_msg(
            panel,
            &world,
            &WeaponRepairState::default(),
            &WeaponRepairMsg::Board(KeyPadMsg::StartHack),
        );
        assert!(matches!(effect, Effect::NoEffect));
    }

    /// One panel host serves every gun, so a board must not survive the
    /// opening it was dealt for. A finished board is terminal - it refuses to
    /// start again - so a board left standing would make the *next* broken gun
    /// unrepairable for the rest of the mission.
    #[test]
    fn each_opening_deals_a_fresh_board() {
        assert!(
            WeaponRepairGui.resets_state_on_frob(),
            "a shared panel must reset per open"
        );

        let mut finished = WeaponRepairState::default();
        finished.hack.phase = super::super::keypad::HackPhase::Won;
        let mut carried = finished.clone();
        WeaponRepairGui.prepare_state_on_frob(&mut carried);
        assert_eq!(
            carried.hack.phase,
            super::super::keypad::HackPhase::Unpaid,
            "the next gun's board must start unpaid, not on the last gun's win"
        );
    }

    /// The psi amp inherits a gun state that means nothing, so it is not a gun
    /// repair mode acts on - the same exclusion the maintenance tool makes.
    #[test]
    fn a_broken_psi_amp_is_not_a_repair_job() {
        /// Any template the class tags mark `weapontype psiamp`.
        const PSI_AMP_TEMPLATE: i32 = -42;

        let (mut world, _) = fixture(1);
        let amp = world.add_entity((
            PropGunState {
                ammo: 0,
                condition: 0.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            PropRepairDiff(PropHackDiff {
                success_chance: 20,
                critical_chance: 4,
                cost: 3.0,
            }),
            PropObjState(ObjectState::Broken),
            dark::properties::PropTemplateId {
                template_id: PSI_AMP_TEMPLATE,
            },
        ));
        world.add_unique(crate::mission::mission_core::GlobalTemplateClassTags(
            std::collections::HashMap::from([(
                PSI_AMP_TEMPLATE,
                std::collections::HashMap::from([("weapontype".to_owned(), "psiamp".to_owned())]),
            )]),
        ));

        assert_eq!(repair_entry(&world, amp), RepairEntry::NotRepairable);
    }

    /// A gun the data has marked `Destroyed` is out of order too: every other
    /// working-order check in the game treats the two states alike, and a gun
    /// that repair refused would be an item nothing could act on at all.
    #[test]
    fn a_destroyed_gun_is_repairable_and_draws_its_broken_icon() {
        let (mut world, gun) = fixture(1);
        world.add_component(
            gun,
            (
                PropObjState(ObjectState::Destroyed),
                PropObjIcon("icn_pist".to_owned()),
                PropObjBrokenIcon("icn_pistb".to_owned()),
            ),
        );

        assert_eq!(repair_entry(&world, gun), RepairEntry::Board);
        assert_eq!(inventory_icon(&world, gun).as_deref(), Some("icn_pistb"));
    }

    /// Installed repair software counts toward the entry requirement exactly
    /// as it counts toward the board's odds - one definition of "how good is
    /// the player at this", so the door and the board cannot disagree.
    #[test]
    fn repair_software_counts_toward_the_skill_gate() {
        let (mut world, gun) = fixture(0);
        break_gun(&mut world, gun);
        assert_eq!(
            repair_entry(&world, gun),
            RepairEntry::SkillRequired { level: 1 }
        );

        world
            .borrow::<shipyard::UniqueViewMut<crate::quest_info::QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .install_software(crate::player_stats::Software::Repair, 1);
        assert_eq!(repair_entry(&world, gun), RepairEntry::Board);
    }

    /// The inventory art follows the object's working order.
    #[test]
    fn a_broken_gun_draws_its_broken_icon() {
        let (mut world, gun) = fixture(1);
        world.add_component(
            gun,
            (
                PropObjIcon("icn_pist".to_owned()),
                PropObjBrokenIcon("icn_pistb".to_owned()),
            ),
        );

        assert_eq!(inventory_icon(&world, gun).as_deref(), Some("icn_pist"));
        break_gun(&mut world, gun);
        assert_eq!(inventory_icon(&world, gun).as_deref(), Some("icn_pistb"));
    }

    /// A broken object that authors no broken art keeps its ordinary icon
    /// rather than drawing nothing.
    #[test]
    fn a_broken_object_with_no_broken_art_keeps_its_icon() {
        let (mut world, gun) = fixture(1);
        world.add_component(gun, (PropObjIcon("icn_pist".to_owned()),));
        break_gun(&mut world, gun);

        assert_eq!(inventory_icon(&world, gun).as_deref(), Some("icn_pist"));
    }
}
