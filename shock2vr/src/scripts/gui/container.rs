use cgmath::{Vector2, Vector3, vec2};
use dark::properties::{
    FrobFlag, Link, PropFrobInfo, PropInventoryDimensions, PropObjIcon, ReceptronEffect,
};

use shipyard::{EntityId, Get, View, World};

use crate::{
    gui::{Gui, GuiComponent, GuiConfig, GuiCursor},
    inventory::Inventory,
    scripts::{Message, script_util},
};

use crate::gui;

use crate::scripts::{Effect, MessagePayload};

pub struct ContainerGui {
    background_image: String,
    width: f32,
    height: f32,
    inv_offset_x: f32,
    inv_offset_y: f32,
    num_slots_x: usize,
    num_slots_y: usize,
    /// Loot semantics: clicking an item takes it into the player's backpack
    /// ("left clicking on the contents picks them up", manual p.7). The
    /// player's own backpack keeps click = use-the-item (`Frob`) instead.
    take_on_click: bool,
    /// Gate a creature's inventory (`creaturecontainer`) behind
    /// [`creature_is_lootable`]: a live, killable hostile must be slain first;
    /// a corpse or an invulnerable posed NPC is lootable on frob. Off for
    /// plain containers (corpses/desks) and the player's own backpack, which
    /// always open.
    require_lootable_creature: bool,
}

/// The basic weapon-impact stim archetype (`Standard Impact`, shock2.gam
/// template -385). Every killable creature carries a `Damage` receptron for
/// it; only an invulnerable set-piece NPC `Abort`s it.
const STANDARD_IMPACT_STIM: i32 = -385;

/// Whether a `creaturecontainer` creature may be looted by frobbing it.
///
/// Faithful gate: a creature's inventory opens only when it is a corpse
/// (dead/incapacitated) OR an invulnerable, posed non-combatant (e.g. Dr.
/// Watts). A live, killable hostile stays sealed until slain. Invulnerability
/// is read straight off the creature's own receptrons - an invulnerable NPC
/// `Abort`s the basic weapon-impact stim, which no killable hostile ever does
/// - so this predicate can never open a live threat's inventory.
fn creature_is_lootable(world: &World, entity_id: EntityId) -> bool {
    if crate::scripts::ai::ai_util::is_killed(entity_id, world) {
        return true;
    }
    script_util::get_all_links_with_template(world, entity_id, |link| match link {
        Link::Receptron(options) => Some(options.clone()),
        _ => None,
    })
    .into_iter()
    .any(|(stim_template_id, options)| {
        stim_template_id == STANDARD_IMPACT_STIM && matches!(options.effect, ReceptronEffect::Abort)
    })
}

/// One inventory cell, in panel pixels. Both the backpack strip and the loot
/// panel step their item grids by this pitch - it is the spacing of the cell
/// separators drawn into `invback.pcx` and `contain.pcx`, whose grid lines sit
/// 35px apart horizontally and 34px apart vertically.
const SLOT_PITCH: (f32, f32) = (35.0, 34.0);

/// Top-left of the loot panel's 4x4 item grid, in `contain.pcx` pixels: its
/// cell separators run at x = 13 + 35n and y = 150 + 34n, and items are
/// authored just inside that first line.
const LOOT_GRID_ORIGIN: (f32, f32) = (15.0, 153.0);

/// Top-left of the backpack strip's 15x3 item grid, in `invback.pcx` pixels
/// (separators at x = 2 + 35n, y = 15 + 34n; the EQUIP paperdoll owns
/// everything right of x = 527).
const BACKPACK_GRID_ORIGIN: (f32, f32) = (4.0, 17.0);

impl ContainerGui {
    pub fn loot_container() -> ContainerGui {
        ContainerGui {
            background_image: "contain.pcx".to_owned(),
            width: 188.0,
            height: 296.0,
            inv_offset_x: LOOT_GRID_ORIGIN.0,
            inv_offset_y: LOOT_GRID_ORIGIN.1,
            num_slots_x: 4,
            num_slots_y: 4,
            take_on_click: true,
            require_lootable_creature: false,
        }
    }

    /// Loot panel for a creature's inventory (`creaturecontainer`). Same layout
    /// as [`loot_container`], but gated: frobbing only opens it once the
    /// creature is a corpse or an invulnerable posed NPC (see
    /// [`creature_is_lootable`]).
    pub fn loot_creature() -> ContainerGui {
        ContainerGui {
            require_lootable_creature: true,
            ..ContainerGui::loot_container()
        }
    }

    pub fn inv_container() -> ContainerGui {
        ContainerGui {
            background_image: "invback.pcx".to_owned(),
            width: 635.0,
            height: 120.0,
            inv_offset_x: BACKPACK_GRID_ORIGIN.0,
            inv_offset_y: BACKPACK_GRID_ORIGIN.1,
            num_slots_x: 15,
            num_slots_y: 3,
            take_on_click: false,
            require_lootable_creature: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ContainerGuiState {}

#[derive(Clone)]
pub enum ContainerGuiMsg {
    GrabbedWithLeftHand(EntityId),
    GrabbedWithRightHand(EntityId),
    Frob(EntityId),
    /// Take the item out of this container into the player's backpack.
    Take(EntityId),
}

impl Gui<ContainerGuiState, ContainerGuiMsg> for ContainerGui {
    fn get_components(
        &self,
        maybe_cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        _state: &ContainerGuiState,
    ) -> Vec<GuiComponent<ContainerGuiMsg>> {
        let mut components: Vec<GuiComponent<ContainerGuiMsg>> = vec![
            gui::image(self.background_image.as_str())
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(self.width, self.height)),
        ];

        let mut contained_entities =
            script_util::get_all_links_with_data(world, entity_id, |link| match link {
                Link::Contains(ordinal) => Some(*ordinal),
                _ => None,
            });

        contained_entities.sort_by(|a, b| a.1.cmp(&b.1));

        let mut inventory = Inventory::new(self.num_slots_x, self.num_slots_y);

        let v_inv_dims = world.borrow::<View<PropInventoryDimensions>>().unwrap();
        for entity in contained_entities {
            let inv_dims = v_inv_dims
                .get(entity.0)
                .map(|dims| (dims.width, dims.height))
                .unwrap_or((1, 1));
            inventory.insert_first_available(entity.0, inv_dims.0 as usize, inv_dims.1 as usize);
        }

        let slot_pixel_width = SLOT_PITCH.0;
        let slot_pixel_height = SLOT_PITCH.1;
        let initial_offset_y = self.inv_offset_y;
        let initial_offset_x = self.inv_offset_x;

        let v_obj_icon = world.borrow::<View<PropObjIcon>>().unwrap();
        for contained_entity_info in inventory.all_items() {
            let ent = contained_entity_info.entity;
            let maybe_obj_icon = v_obj_icon.get(ent);

            if maybe_obj_icon.is_err() {
                continue;
            }

            let inv_dims = (contained_entity_info.width, contained_entity_info.height);
            let position_x = slot_pixel_width * contained_entity_info.x as f32;
            let position_y = slot_pixel_height * contained_entity_info.y as f32;

            let obj_icon = &maybe_obj_icon.unwrap().0;

            // TODO: Fix this issue:
            // thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: Custom { kind: InvalidInput, error: "pcx::Reader::next_row_paletted called on non-paletted image" }', engine/src/texture_format.rs:81:45
            if obj_icon.contains("upgrade") {
                continue;
            }

            let on_click = if self.take_on_click {
                ContainerGuiMsg::Take(ent)
            } else {
                ContainerGuiMsg::Frob(ent)
            };
            components.push(
                gui::grabbable(
                    ContainerGuiMsg::GrabbedWithLeftHand(ent),
                    ContainerGuiMsg::GrabbedWithRightHand(ent),
                )
                .with_onclick(on_click)
                .with_entity(ent)
                .with_image(&format!("{}.pcx", obj_icon))
                .with_position(vec2(
                    initial_offset_x + position_x,
                    initial_offset_y + position_y,
                ))
                .with_size(vec2(
                    slot_pixel_width * inv_dims.0 as f32,
                    slot_pixel_height * inv_dims.1 as f32,
                )),
            )
        }

        if let Some(cursor) = maybe_cursor {
            if let Some(ent) = cursor.held_entity_id {
                let maybe_obj_icon = v_obj_icon.get(ent);

                if let Ok(obj_icon) = maybe_obj_icon {
                    let inv_dims = v_inv_dims
                        .get(ent)
                        .map(|dims| (dims.width, dims.height))
                        .unwrap_or((1, 1));
                    components.push(
                        gui::image(&format!("{}.pcx", obj_icon.0))
                            .with_position(vec2(cursor.position.x, cursor.position.y))
                            .with_size(vec2(
                                slot_pixel_width * inv_dims.0 as f32,
                                slot_pixel_height * inv_dims.1 as f32,
                            )),
                    );
                }
            }
        }

        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 1.0, 0.0),
            screen_size_in_pixels: Vector2::new(self.width, self.height),
        }
    }

    /// A creature's loot panel opens on frob only when it is lootable (corpse
    /// or invulnerable posed NPC); a live hostile stays sealed. Plain
    /// containers and the backpack always open.
    fn opens_on_frob(&self, entity_id: EntityId, world: &World) -> bool {
        !self.require_lootable_creature || creature_is_lootable(world, entity_id)
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        world: &World,
        state: &ContainerGuiState,
        msg: &ContainerGuiMsg,
    ) -> (ContainerGuiState, Effect) {
        match msg {
            ContainerGuiMsg::GrabbedWithLeftHand(ent) => (
                state.clone(),
                Effect::GrabEntity {
                    entity_id: *ent,
                    hand: crate::vr_config::Handedness::Left,
                    // TODO: Set this up to break link
                    current_parent_id: None,
                },
            ),
            ContainerGuiMsg::GrabbedWithRightHand(ent) => (
                state.clone(),
                Effect::GrabEntity {
                    entity_id: *ent,
                    hand: crate::vr_config::Handedness::Right,
                    // TODO: Set this up to break link
                    current_parent_id: None,
                },
            ),
            // Wield-or-use (backpack click): a carried weapon - gun
            // (PropPlayerGun) or melee (PropLimbModel) - is wielded through
            // `GrabEntity` (flat: the first-person viewmodel wield path,
            // which also restores world refs and clears the Contains link;
            // VR: a grab into the hand). Anything else gets its own Frob
            // (use) action, e.g. a hypo consumes.
            ContainerGuiMsg::Frob(ent) => {
                let is_weapon = crate::virtual_hand::is_wieldable_weapon(world, *ent);
                if is_weapon {
                    (
                        state.clone(),
                        Effect::GrabEntity {
                            entity_id: *ent,
                            hand: crate::vr_config::Handedness::Right,
                            current_parent_id: None,
                        },
                    )
                } else {
                    (
                        state.clone(),
                        Effect::Send {
                            msg: Message {
                                payload: MessagePayload::Frob,
                                to: *ent,
                            },
                        },
                    )
                }
            }
            // Transfer the clicked item's `Contains` link to the player's
            // backpack - the same `DropEntityInfo` path used when a VR hand
            // feeds an item into a container (drop_entity_into_container).
            // Guarded by the same grabbability check as the debug give
            // lever: reparenting a non-grabbable entity would corrupt it.
            // Use-only contained objects (notably corpse audio logs) still
            // need their normal Frob behavior when clicked; they are consumed
            // or recorded in place rather than moved into the backpack.
            ContainerGuiMsg::Take(ent) => {
                if !crate::virtual_hand::can_grab_item(world, *ent) {
                    let is_use_only = world
                        .borrow::<View<PropFrobInfo>>()
                        .map(|frob| {
                            frob.get(*ent).is_ok_and(|frob| {
                                frob.world_action.contains(FrobFlag::SCRIPT)
                                    || frob.inventory_action.contains(FrobFlag::SCRIPT)
                            })
                        })
                        .unwrap_or(false);
                    return if is_use_only {
                        (
                            state.clone(),
                            Effect::Send {
                                msg: Message {
                                    payload: MessagePayload::Frob,
                                    to: *ent,
                                },
                            },
                        )
                    } else {
                        (state.clone(), Effect::NoEffect)
                    };
                }
                let inventory_entity = world
                    .borrow::<shipyard::UniqueView<crate::mission::PlayerInfo>>()
                    .map(|player| player.inventory_entity_id)
                    .ok();
                match inventory_entity {
                    Some(inventory_entity) => (
                        state.clone(),
                        Effect::DropEntityInfo {
                            parent_entity_id: inventory_entity,
                            dropped_entity_id: *ent,
                        },
                    ),
                    None => (state.clone(), Effect::NoEffect),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::GuiInputInfo;
    use crate::mission::PlayerInfo;
    use cgmath::{Quaternion, point2, vec3};
    use dark::properties::{FrobFlag, Links, PropFrobInfo, ToLink, WrappedEntityId};

    /// A world with a loot container holding one iconed, grabbable item,
    /// plus the player-info unique the Take path resolves the backpack
    /// through.
    fn loot_world() -> (World, EntityId, EntityId, EntityId) {
        let mut world = World::new();
        let item = world.add_entity((
            PropObjIcon("icn_psi".to_owned()),
            PropFrobInfo {
                world_action: FrobFlag::MOVE,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
        ));
        let container = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(item)),
                link: Link::Contains(0),
            }],
        });
        let player = world.add_entity(());
        let inventory = world.add_entity(Links::empty());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        (world, container, item, inventory)
    }

    fn input_at(cursor: cgmath::Point2<f32>, pressed: bool) -> GuiInputInfo {
        GuiInputInfo {
            held_entity_id: None,
            cursor_position: cursor,
            is_pressed: pressed,
            is_grabbed: false,
            hand: crate::vr_config::Handedness::Right,
        }
    }

    /// Clicking a loot-panel item yields `Take`, and `Take` transfers the
    /// item to the player's backpack through the shared `DropEntityInfo`
    /// path ("left clicking on the contents picks them up", manual p.7).
    #[test]
    fn loot_container_click_takes_the_item_into_the_backpack() {
        let (world, container, item, inventory) = loot_world();
        let gui = ContainerGui::loot_container();
        let components = gui.get_components(&None, container, &world, &ContainerGuiState {});

        // The item button carries its entity for /v1/ui introspection.
        let item_button = components
            .iter()
            .find(|c| matches!(c, GuiComponent::Button { entity, .. } if *entity == Some(item)))
            .expect("loot panel should expose a button bound to the contained item");

        // Simulate a press edge on the item's slot (first slot at the loot
        // panel's inventory offset).
        let (position, size) = match item_button {
            GuiComponent::Button { position, size, .. } => (*position, *size),
            _ => unreachable!(),
        };
        let center = point2(position.x + size.x / 2.0, position.y + size.y / 2.0);
        let event = item_button
            .get_event(&input_at(center, false), &input_at(center, true))
            .expect("a press edge on the item should produce an event");
        assert!(
            matches!(event, ContainerGuiMsg::Take(e) if e == item),
            "clicking a loot item should Take it"
        );

        let (_state, effect) = gui.handle_msg(container, &world, &ContainerGuiState {}, &event);
        match effect {
            Effect::DropEntityInfo {
                parent_entity_id,
                dropped_entity_id,
            } => {
                assert_eq!(parent_entity_id, inventory);
                assert_eq!(dropped_entity_id, item);
            }
            other => panic!("Take should transfer via DropEntityInfo, got {:?}", other),
        }
    }

    /// Both panels' item grids must land on the cell separators authored into
    /// their backdrops - `contain.pcx` (loot) and `invback.pcx` (backpack) -
    /// stepping by the shared 35x34 cell pitch. A 1x1 item therefore occupies
    /// exactly one cell at the grid origin.
    #[test]
    fn item_grids_sit_on_the_authored_backdrop_cells() {
        let (world, container, item, _inventory) = loot_world();

        for (gui, expected_origin) in [
            (ContainerGui::loot_container(), vec2(15.0, 153.0)),
            (ContainerGui::inv_container(), vec2(4.0, 17.0)),
        ] {
            let components = gui.get_components(&None, container, &world, &ContainerGuiState {});
            let (position, size) = components
                .iter()
                .find_map(|c| match c {
                    GuiComponent::Button {
                        entity: Some(e),
                        position,
                        size,
                        ..
                    } if *e == item => Some((*position, *size)),
                    _ => None,
                })
                .expect("the panel should expose a button bound to the contained item");

            assert_eq!(position, expected_origin, "grid origin");
            assert_eq!(size, vec2(35.0, 34.0), "one cell of the shared pitch");
        }
    }

    /// Use-only objects can be contained too. Audio logs are the critical
    /// retail case: they deliberately cannot be grabbed, but clicking their
    /// corpse-loot icon must still run `LogDiscScript`'s normal Frob path.
    #[test]
    fn loot_container_click_frobs_a_non_grabbable_item() {
        let mut world = World::new();
        let item = world.add_entity((
            PropObjIcon("disc".to_owned()),
            PropFrobInfo {
                world_action: FrobFlag::SCRIPT,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
        ));
        let container = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(item)),
                link: Link::Contains(0),
            }],
        });
        let player = world.add_entity(());
        let inventory = world.add_entity(Links::empty());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });

        let gui = ContainerGui::loot_container();
        let (_state, effect) = gui.handle_msg(
            container,
            &world,
            &ContainerGuiState {},
            &ContainerGuiMsg::Take(item),
        );

        assert!(
            matches!(
                effect,
                Effect::Send {
                    msg: Message {
                        to,
                        payload: MessagePayload::Frob,
                    },
                } if to == item
            ),
            "non-grabbable contained objects should be used in place"
        );
    }

    /// A contained entity that is not grabbable (no MOVE/USE_AMMO frob
    /// action) must not be reparented - same guard as the debug give lever.
    #[test]
    fn take_refuses_a_non_grabbable_entity() {
        let (mut world, container, _item, _inventory) = loot_world();
        let stuck = world.add_entity(PropObjIcon("icn_junk".to_owned()));
        let gui = ContainerGui::loot_container();
        let (_state, effect) = gui.handle_msg(
            container,
            &world,
            &ContainerGuiState {},
            &ContainerGuiMsg::Take(stuck),
        );
        assert!(
            matches!(effect, Effect::NoEffect),
            "taking a non-grabbable entity must be a no-op, got {:?}",
            effect
        );
    }

    /// Clicking a carried WEAPON in the backpack strip wields it: gun
    /// (PropPlayerGun) and melee (PropLimbModel) items route through
    /// `Effect::GrabEntity` (the flat grab path wields; it also restores
    /// world refs and clears the Contains link, so the strip updates live).
    #[test]
    fn backpack_click_on_a_weapon_wields_it() {
        let (mut world, container, _item, _inventory) = loot_world();
        let gui = ContainerGui::inv_container();

        let wrench = world.add_entity((
            PropObjIcon("icn_wrench".to_owned()),
            dark::properties::PropLimbModel("atek_h".to_owned()),
        ));
        let pistol = world.add_entity((
            PropObjIcon("icn_pist".to_owned()),
            dark::properties::PropPlayerGun {
                flags: 0,
                hand_model: "pis_h".to_owned(),
                icon_file: String::new(),
                model_offset: vec3(0.0, 0.0, 0.0),
                fire_offset: vec3(0.0, 0.0, 0.0),
                heading: 0,
                reload_pitch: 0,
                reload_rate: 0,
                gun_type: 0,
            },
        ));
        for weapon in [wrench, pistol] {
            let (_state, effect) = gui.handle_msg(
                container,
                &world,
                &ContainerGuiState {},
                &ContainerGuiMsg::Frob(weapon),
            );
            assert!(
                matches!(
                    effect,
                    Effect::GrabEntity { entity_id, .. } if entity_id == weapon
                ),
                "clicking a carried weapon should wield it via GrabEntity, got {:?}",
                effect
            );
        }
    }

    /// The player's own backpack keeps the original click semantics (use the
    /// item), NOT take-into-self.
    #[test]
    fn backpack_click_frobs_the_item() {
        let (world, container, item, _inventory) = loot_world();
        let gui = ContainerGui::inv_container();
        let components = gui.get_components(&None, container, &world, &ContainerGuiState {});
        let item_button = components
            .iter()
            .find(|c| matches!(c, GuiComponent::Button { entity, .. } if *entity == Some(item)))
            .expect("backpack should expose a button bound to the carried item");
        let (position, size) = match item_button {
            GuiComponent::Button { position, size, .. } => (*position, *size),
            _ => unreachable!(),
        };
        let center = point2(position.x + size.x / 2.0, position.y + size.y / 2.0);
        let event = item_button
            .get_event(&input_at(center, false), &input_at(center, true))
            .expect("a press edge on the item should produce an event");
        assert!(
            matches!(event, ContainerGuiMsg::Frob(e) if e == item),
            "clicking a backpack item should Frob (use) it"
        );
    }
}
