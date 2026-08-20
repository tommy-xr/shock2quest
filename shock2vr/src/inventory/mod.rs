//! Inventory grids: which cell each contained item occupies.
//!
//! A containment link carries the item's cell as its data - the `Contains`
//! ordinal is `y * width + x` - so an item keeps the cell it was put in
//! instead of being repacked every time the panel is drawn. That ordinal is
//! authored in the mission data and round-trips through save/load with the
//! link, so this is also what makes a container's layout stable across a
//! reload.

use std::collections::HashMap;

use dark::properties::{Link, PropContainDimensions, PropInventoryDimensions};
use shipyard::{EntityId, Get, UniqueView, View, ViewMut, World};

use crate::{player_stats::PlayerStats, quest_info::QuestInfo};

pub mod player_inventory_entity;
pub use player_inventory_entity::*;

/// The player's maximum backpack grid, in cells (`invback.pcx` draws 15x3).
pub const BACKPACK_GRID: (usize, usize) = (15, 3);

/// Strength 1 exposes ten columns; every effective level through 6 adds one.
pub const BACKPACK_MIN_WIDTH: usize = 10;

/// Effective Strength is capped at 6 for carrying capacity.
pub const BACKPACK_MAX_STRENGTH: i32 = 6;

/// A container's loot grid, in cells (`contain.pcx` draws 4x4).
pub const CONTAINER_GRID: (usize, usize) = (4, 4);

/// Ordinals at or above this are not grid cells - the original reserves the
/// range for equipped/paperdoll slots. We do not model those yet; such an
/// item simply has no cell and gets first-fit like an unplaced one.
const EQUIP_SLOT_BASE: u32 = 1000;

/// Unique, deliberately non-cell ordinals used while a true resize overflow
/// still belongs to the backpack. The mission normally physicalizes and
/// detaches those items immediately. Keeping an exceptional modelless item at
/// a unique pending ordinal avoids both data loss and a duplicate cell claim,
/// and lets a later grow recover it.
const PENDING_OVERFLOW_SLOT_BASE: u32 = 0x8000_0000;

/// Which grid `container_entity` is drawn with. The player's backpack is
/// wider than a container's loot panel, and the ordinal encodes `y * width +
/// x`, so both sides of the containment link must agree on the width.
///
/// A container's grid is authored as `P$ContainDims`; every container in the
/// shipped game declares 4x4, which is also the fallback for anything that
/// does not declare one. The backpack is the exception: its usable width is
/// derived from the persistent character sheet's Strength and Pack-Rat trait.
pub fn grid_for(world: &World, container_entity: EntityId) -> (usize, usize) {
    let is_backpack = world
        .borrow::<View<PlayerInventoryEntity>>()
        .map(|v| v.get(container_entity).is_ok())
        .unwrap_or(false);
    if is_backpack {
        return world
            .borrow::<UniqueView<QuestInfo>>()
            .map(|quests| (backpack_width(quests.player_stats()), BACKPACK_GRID.1))
            // Minimal/debug scenes normally install QuestInfo too. Keep the
            // old maximum-size behavior as a graceful fallback for any scene
            // that deliberately has no character sheet.
            .unwrap_or(BACKPACK_GRID);
    }
    world
        .borrow::<View<PropContainDimensions>>()
        .ok()
        .and_then(|v| v.get(container_entity).ok().map(|d| (d.width, d.height)))
        // A zero dimension would make every cell decode to nothing; treat it
        // as unauthored rather than trusting it.
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map(|(w, h)| (w as usize, h as usize))
        .unwrap_or(CONTAINER_GRID)
}

/// The usable backpack width for a character sheet.
///
/// Retail treats Pack-Rat as one extra effective Strength for inventory only,
/// clamps the result to 1..=6, and maps that to 10..=15 columns.
pub fn backpack_width(stats: &PlayerStats) -> usize {
    let pack_rat = i32::from(stats.has_os_trait(crate::scripts::gui::TRAIT_PACK_RAT));
    let effective_strength = (stats.strength + pack_rat).clamp(1, BACKPACK_MAX_STRENGTH);
    BACKPACK_MIN_WIDTH + (effective_strength - 1) as usize
}

/// Result of changing the width used to encode a container's cell ordinals.
/// Overflow remains linked until the mission successfully gives it world
/// presence, so even a modelless item cannot be silently lost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidthRemapOutcome {
    pub placed: usize,
    pub overflow: Vec<EntityId>,
}

/// Re-encode stored `Contains` ordinals after a grid width change.
///
/// Existing `(x,y)` coordinates are claimed first. Items falling off the
/// right edge are then re-placed column-major without disturbing the items
/// that still fit. A true capacity overflow is returned to the mission, which
/// mirrors retail by spilling those objects into the world.
pub fn remap_container_width(
    world: &mut World,
    container_entity: EntityId,
    old_grid: (usize, usize),
    new_grid: (usize, usize),
) -> WidthRemapOutcome {
    if old_grid == new_grid {
        let placed = Inventory::from_container(world, container_entity, new_grid)
            .all_items()
            .count();
        return WidthRemapOutcome {
            placed,
            overflow: Vec::new(),
        };
    }

    // Decode every old ordinal before changing any link. Inventory's two-pass
    // ownership logic also makes malformed duplicate cells deterministic.
    let old_layout = Inventory::from_container(world, container_entity, old_grid);
    let mut resized = Inventory::new(new_grid.0, new_grid.1);
    let mut reflow = Vec::new();
    let mut accounted_for = std::collections::HashSet::new();

    for item in old_layout.all_items() {
        accounted_for.insert(item.entity);
        if !resized.insert_if_fits(item.entity, item.x, item.y, item.width, item.height) {
            reflow.push(item.clone());
        }
    }

    // A prior shrink can leave a modelless overflow linked at a deliberately
    // non-cell ordinal because it cannot safely be materialized. Such an item
    // is absent from `old_layout` while the grid is full, but it must join the
    // deterministic first-fit pass so a later grow can recover it.
    let mut pending: Vec<(EntityId, u32)> =
        crate::scripts::script_util::get_all_links_with_data(world, container_entity, |link| {
            match link {
                Link::Contains(ordinal) => Some(*ordinal),
                _ => None,
            }
        });
    pending.sort_by_key(|(entity, ordinal)| (*ordinal, entity.inner()));
    let v_dims = world.borrow::<View<PropInventoryDimensions>>().unwrap();
    for (entity, _) in pending {
        if accounted_for.insert(entity) {
            let (width, height) = v_dims
                .get(entity)
                .map(|dims| (dims.width as usize, dims.height as usize))
                .unwrap_or((1, 1));
            reflow.push(ContainedEntityInfo {
                entity,
                x: 0,
                y: 0,
                width,
                height,
            });
        }
    }
    drop(v_dims);

    let mut overflow = Vec::new();
    for item in reflow {
        if !resized.insert_first_available(item.entity, item.width, item.height) {
            overflow.push(item.entity);
        }
    }

    let placements: HashMap<EntityId, u32> = resized
        .all_items()
        .map(|item| (item.entity, resized.slot_at(item.x, item.y)))
        .collect();
    let pending_overflow: HashMap<EntityId, u32> = overflow
        .iter()
        .enumerate()
        .map(|(index, entity)| {
            (
                *entity,
                PENDING_OVERFLOW_SLOT_BASE.saturating_add(index as u32),
            )
        })
        .collect();
    if let Ok(mut links) = world.borrow::<ViewMut<dark::properties::Links>>()
        && let Ok(container_links) = (&mut links).get(container_entity)
    {
        for link in &mut container_links.to_links {
            let Some(target) = link.to_entity_id.map(|wrapped| wrapped.0) else {
                continue;
            };
            if matches!(link.link, Link::Contains(_))
                && let Some(slot) = placements.get(&target)
            {
                link.link = Link::Contains(*slot);
            } else if matches!(link.link, Link::Contains(_))
                && let Some(slot) = pending_overflow.get(&target)
            {
                link.link = Link::Contains(*slot);
            }
        }
    }

    WidthRemapOutcome {
        placed: placements.len(),
        overflow,
    }
}

#[derive(Clone, Debug)]
pub struct ContainedEntityInfo {
    pub entity: EntityId,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Debug)]
pub struct Inventory {
    grid: Vec<Option<EntityId>>,
    items: Vec<ContainedEntityInfo>,
    width: usize,
    height: usize,
}

impl Inventory {
    pub fn new(width: usize, height: usize) -> Inventory {
        Inventory {
            grid: vec![None; width * height],
            items: Vec::new(),
            width,
            height,
        }
    }

    pub fn has_capacity(&self, x: usize, y: usize, width: usize, height: usize) -> bool {
        for y in y..y + height {
            for x in x..x + width {
                if x >= self.width || y >= self.height {
                    return false;
                }

                if self.grid[self.get_index(x, y)].is_some() {
                    return false;
                }
            }
        }

        true
    }

    pub fn all_items(&self) -> impl Iterator<Item = &ContainedEntityInfo> {
        self.items.iter()
    }

    /// Lay out `container_entity`'s contents: every item that has a usable
    /// stored cell keeps it, and only the rest are packed into what is left.
    ///
    /// Placement is in two passes on purpose - a first-fit item claiming a
    /// cell before the item that actually owns it would evict the owner and
    /// reintroduce exactly the shuffling the stored ordinal exists to stop.
    pub fn from_container(world: &World, container_entity: EntityId, grid: (usize, usize)) -> Self {
        let mut inventory = Inventory::new(grid.0, grid.1);

        let mut contents =
            crate::scripts::script_util::get_all_links_with_data(world, container_entity, |link| {
                match link {
                    Link::Contains(ordinal) => Some(*ordinal),
                    _ => None,
                }
            });
        // Ascending slot order, so an unplaced item fills the first hole
        // rather than landing wherever link iteration happened to put it.
        contents.sort_by_key(|(_entity, ordinal)| *ordinal);

        let v_dims = world.borrow::<View<PropInventoryDimensions>>().unwrap();
        let dims_of = |entity: EntityId| {
            v_dims
                .get(entity)
                .map(|d| (d.width as usize, d.height as usize))
                .unwrap_or((1, 1))
        };

        let mut unplaced = Vec::new();
        for (entity, ordinal) in contents {
            let (w, h) = dims_of(entity);
            match inventory.cell_of(ordinal) {
                Some((x, y)) if inventory.insert_if_fits(entity, x, y, w, h) => {}
                // No cell, or the stored one no longer fits (a smaller grid,
                // or another item already there): fall back to first-fit.
                _ => unplaced.push((entity, w, h)),
            }
        }
        for (entity, w, h) in unplaced {
            inventory.insert_first_available(entity, w, h);
        }

        inventory
    }

    /// The cell an ordinal names, or `None` if it is not a cell in this grid
    /// (an equip slot, or beyond the grid's bounds).
    fn cell_of(&self, ordinal: u32) -> Option<(usize, usize)> {
        if ordinal >= EQUIP_SLOT_BASE {
            return None;
        }
        let slot = ordinal as usize;
        match slot < self.width * self.height {
            true => Some((slot % self.width, slot / self.width)),
            false => None,
        }
    }

    /// The ordinal naming `(x, y)` in this grid.
    pub fn slot_at(&self, x: usize, y: usize) -> u32 {
        (y * self.width + x) as u32
    }

    /// The first cell a `width` x `height` item fits in, scanning column by
    /// column - the order the original packs in, so a tall item takes a fresh
    /// column instead of straddling the top row.
    pub fn first_free_slot(&self, width: usize, height: usize) -> Option<u32> {
        for x in 0..self.width {
            for y in 0..self.height {
                if self.has_capacity(x, y, width, height) {
                    return Some(self.slot_at(x, y));
                }
            }
        }
        None
    }

    pub fn insert_first_available(
        &mut self,
        entity: EntityId,
        width: usize,
        height: usize,
    ) -> bool {
        for x in 0..self.width {
            for y in 0..self.height {
                if self.insert_if_fits(entity, x, y, width, height) {
                    return true;
                }
            }
        }
        false
    }

    pub fn insert_if_fits(
        &mut self,
        entity: EntityId,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> bool {
        if !self.has_capacity(x, y, width, height) {
            return false;
        }

        self.items.push(ContainedEntityInfo {
            entity,
            x,
            y,
            width,
            height,
        });

        for y in y..y + height {
            for x in x..x + width {
                let idx = self.get_index(x, y);
                let item = self.grid.get_mut(idx).unwrap();
                *item = Some(entity);
            }
        }

        true
    }

    pub fn remove_entity(&mut self, entity: EntityId) {
        for item in self.grid.iter_mut() {
            if let Some(contained_entity) = item {
                if *contained_entity == entity {
                    *item = None;
                }
            }
        }

        self.items.retain(|item| item.entity != entity);
    }

    fn get_index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{Links, ToLink, WrappedEntityId};

    /// A container holding `(entity, stored ordinal)` pairs, every item 1x1.
    fn container_with(slots: &[(EntityId, u32)]) -> (World, EntityId) {
        let mut world = World::new();
        let to_links = slots
            .iter()
            .map(|(entity, ordinal)| ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(*entity)),
                link: Link::Contains(*ordinal),
            })
            .collect();
        let container = world.add_entity(Links { to_links });
        (world, container)
    }

    fn entities(n: usize) -> Vec<EntityId> {
        let mut world = World::new();
        (0..n).map(|_| world.add_entity(())).collect()
    }

    fn cell_of_item(inv: &Inventory, entity: EntityId) -> (usize, usize) {
        inv.all_items()
            .find(|i| i.entity == entity)
            .map(|i| (i.x, i.y))
            .expect("item should be laid out")
    }

    /// An item keeps the cell stored on its containment link - it is not
    /// repacked into reading order.
    #[test]
    fn stored_slots_place_items() {
        let ids = entities(2);
        // Deliberately out of reading order: the second link names cell 0.
        let (world, container) = container_with(&[(ids[0], 7), (ids[1], 0)]);
        let inv = Inventory::from_container(&world, container, CONTAINER_GRID);

        assert_eq!(
            cell_of_item(&inv, ids[0]),
            (3, 1),
            "slot 7 in a 4-wide grid"
        );
        assert_eq!(cell_of_item(&inv, ids[1]), (0, 0));
    }

    /// The reported bug: taking an item out must not shuffle the others.
    /// Without stored slots every item repacks from reading order, so
    /// removing the first one slides all the rest up a cell.
    #[test]
    fn removing_an_item_leaves_the_others_where_they_were() {
        let ids = entities(3);
        let (world, container) = container_with(&[(ids[0], 0), (ids[1], 5), (ids[2], 9)]);

        let before = Inventory::from_container(&world, container, CONTAINER_GRID);
        let kept: Vec<(usize, usize)> = ids[1..]
            .iter()
            .map(|id| cell_of_item(&before, *id))
            .collect();

        // Take the first item out of the container.
        let (world, container) = container_with(&[(ids[1], 5), (ids[2], 9)]);
        let after = Inventory::from_container(&world, container, CONTAINER_GRID);

        let now: Vec<(usize, usize)> = ids[1..]
            .iter()
            .map(|id| cell_of_item(&after, *id))
            .collect();
        assert_eq!(now, kept, "the remaining items must not move");
    }

    /// An item with no usable cell is packed into a hole, and must never
    /// evict an item that does own its cell.
    #[test]
    fn unplaced_items_fill_holes_without_evicting_owners() {
        let ids = entities(2);
        // Ordinal 0 is "unset" for a runtime-inserted item; the other item
        // genuinely owns cell 0.
        let (world, container) = container_with(&[(ids[0], 0), (ids[1], 0)]);
        let inv = Inventory::from_container(&world, container, CONTAINER_GRID);

        let first = cell_of_item(&inv, ids[0]);
        let second = cell_of_item(&inv, ids[1]);
        assert_eq!(first, (0, 0), "the first claim on cell 0 keeps it");
        assert_ne!(
            second, first,
            "the loser must be packed elsewhere, not stacked"
        );
    }

    /// A container's authored `ContainDims` decides the width its ordinals
    /// decode against - get that wrong and every stored cell lands somewhere
    /// else. Shipped containers all declare 4x4, but the value is data.
    #[test]
    fn container_grid_comes_from_authored_contain_dims() {
        use dark::properties::PropContainDimensions;

        let mut world = World::new();
        let plain = world.add_entity(());
        assert_eq!(grid_for(&world, plain), CONTAINER_GRID, "fallback");

        let wide = world.add_entity(PropContainDimensions {
            width: 6,
            height: 2,
        });
        assert_eq!(grid_for(&world, wide), (6, 2));

        // Slot 7 is (3,1) in a 4-wide grid but (1,1) in a 6-wide one.
        let six = Inventory::new(6, 2);
        assert_eq!(six.cell_of(7), Some((1, 1)));
        let four = Inventory::new(4, 4);
        assert_eq!(four.cell_of(7), Some((3, 1)));

        // A degenerate authored value must not swallow the grid.
        let zero = world.add_entity(PropContainDimensions {
            width: 0,
            height: 4,
        });
        assert_eq!(grid_for(&world, zero), CONTAINER_GRID);
    }

    /// Equip-range ordinals are not cells; such an item still gets laid out.
    #[test]
    fn equip_slots_are_not_grid_cells() {
        let ids = entities(1);
        let (world, container) = container_with(&[(ids[0], EQUIP_SLOT_BASE + 2)]);
        let inv = Inventory::from_container(&world, container, CONTAINER_GRID);
        assert_eq!(
            cell_of_item(&inv, ids[0]),
            (0, 0),
            "first-fit, not cell 1002"
        );
    }

    /// A stored cell outside the grid (a container drawn smaller than the one
    /// the ordinal was authored for) falls back rather than vanishing.
    #[test]
    fn out_of_range_slots_fall_back_to_first_fit() {
        let ids = entities(1);
        let (world, container) = container_with(&[(ids[0], 99)]);
        let inv = Inventory::from_container(&world, container, CONTAINER_GRID);
        assert_eq!(cell_of_item(&inv, ids[0]), (0, 0));
    }

    /// First-fit scans column by column, so a 1x3 item takes a fresh column
    /// instead of straddling the top row (the original's packing order).
    #[test]
    fn first_free_slot_scans_by_column() {
        let mut inv = Inventory::new(CONTAINER_GRID.0, CONTAINER_GRID.1);
        let ids = entities(2);
        assert_eq!(inv.first_free_slot(1, 3), Some(0));
        inv.insert_if_fits(ids[0], 0, 0, 1, 3);
        // Column 0 has one cell left, too short for another 1x3.
        assert_eq!(inv.first_free_slot(1, 3), Some(inv.slot_at(1, 0)));
        assert_eq!(inv.first_free_slot(1, 1), Some(inv.slot_at(0, 3)));
        inv.insert_if_fits(ids[1], 0, 3, 1, 1);
        assert_eq!(inv.first_free_slot(1, 1), Some(inv.slot_at(1, 0)));
    }

    #[test]
    fn backpack_width_follows_strength_and_pack_rat() {
        let mut stats = crate::player_stats::PlayerStats::new();
        assert_eq!(backpack_width(&stats), 10, "Strength 1");

        stats.strength = 4;
        assert_eq!(backpack_width(&stats), 13, "Strength 4");

        assert!(
            stats.add_os_trait(crate::scripts::gui::TRAIT_PACK_RAT),
            "Pack-Rat is an authored O/S trait"
        );
        assert_eq!(backpack_width(&stats), 14, "Pack-Rat adds one column");

        stats.strength = 99;
        assert_eq!(backpack_width(&stats), 15, "effective Strength clamps at 6");
        stats.strength = -99;
        assert_eq!(backpack_width(&stats), 10, "effective Strength clamps at 1");
    }

    #[test]
    fn player_backpack_grid_reads_the_live_character_sheet() {
        let mut world = World::new();
        let backpack = world.add_entity(PlayerInventoryEntity {});
        world.add_unique(crate::quest_info::QuestInfo::new());

        assert_eq!(grid_for(&world, backpack), (10, 3));
        world
            .borrow::<shipyard::UniqueViewMut<crate::quest_info::QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .strength = 5;
        assert_eq!(grid_for(&world, backpack), (14, 3));
    }

    fn remap_world(width: usize, height: usize) -> (World, EntityId, Vec<EntityId>) {
        let mut world = World::new();
        let item_count = width * height;
        let items: Vec<_> = (0..item_count).map(|_| world.add_entity(())).collect();
        let links = items
            .iter()
            .enumerate()
            .map(|(slot, item)| ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(*item)),
                link: Link::Contains(slot as u32),
            })
            .collect();
        let container = world.add_entity(Links { to_links: links });
        (world, container, items)
    }

    fn stored_slot(world: &World, container: EntityId, item: EntityId) -> u32 {
        world
            .borrow::<View<Links>>()
            .unwrap()
            .get(container)
            .unwrap()
            .to_links
            .iter()
            .find_map(|link| {
                (link.to_entity_id.map(|wrapped| wrapped.0) == Some(item))
                    .then(|| match link.link {
                        Link::Contains(slot) => Some(slot),
                        _ => None,
                    })
                    .flatten()
            })
            .expect("item should retain one Contains link")
    }

    #[test]
    fn width_changes_preserve_coordinates_and_repack_right_edge_items() {
        let (mut world, container, items) = remap_world(11, 3);

        // Make holes in the left side. The rightmost-column items at old
        // slots 10, 21 and 32 cannot retain x=10 after shrinking to width 10,
        // so they should fill these holes deterministically.
        {
            let mut links = world.borrow::<shipyard::ViewMut<Links>>().unwrap();
            let links = (&mut links).get(container).unwrap();
            links
                .to_links
                .retain(|link| !matches!(link.link, Link::Contains(0 | 11 | 22)));
        }
        let outcome = remap_container_width(&mut world, container, (11, 3), (10, 3));
        assert!(outcome.overflow.is_empty());

        assert_eq!(
            stored_slot(&world, container, items[1]),
            1,
            "(1,0) stays put"
        );
        assert_eq!(
            stored_slot(&world, container, items[12]),
            11,
            "(1,1) re-encodes"
        );
        assert_eq!(
            stored_slot(&world, container, items[23]),
            21,
            "(1,2) re-encodes"
        );
        assert_eq!(stored_slot(&world, container, items[10]), 0);
        assert_eq!(stored_slot(&world, container, items[21]), 10);
        assert_eq!(stored_slot(&world, container, items[32]), 20);

        // Growing must keep every (x,y), which means row 1/2 ordinals change
        // back to the wider encoding rather than silently moving the icons.
        let outcome = remap_container_width(&mut world, container, (10, 3), (11, 3));
        assert!(outcome.overflow.is_empty());
        assert_eq!(stored_slot(&world, container, items[12]), 12);
        assert_eq!(stored_slot(&world, container, items[23]), 23);
    }

    #[test]
    fn shrinking_replaces_a_multi_cell_item_that_falls_off_the_right() {
        let mut world = World::new();
        let fixed = world.add_entity(PropInventoryDimensions {
            width: 1,
            height: 3,
        });
        let off_right = world.add_entity(PropInventoryDimensions {
            width: 2,
            height: 2,
        });
        let container = world.add_entity(Links {
            to_links: vec![
                ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(fixed)),
                    link: Link::Contains(0),
                },
                ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(off_right)),
                    link: Link::Contains(9),
                },
            ],
        });

        let outcome = remap_container_width(&mut world, container, (11, 3), (10, 3));
        assert!(outcome.overflow.is_empty());
        assert_eq!(stored_slot(&world, container, fixed), 0);
        assert_eq!(stored_slot(&world, container, off_right), 1);

        let layout = Inventory::from_container(&world, container, (10, 3));
        assert_eq!(cell_of_item(&layout, fixed), (0, 0));
        assert_eq!(cell_of_item(&layout, off_right), (1, 0));
        assert_eq!(
            layout.all_items().count(),
            2,
            "neither item is lost or overlapped"
        );
    }

    #[test]
    fn shrinking_a_full_backpack_reports_true_overflow_without_overlap_or_link_loss() {
        let (mut world, container, items) = remap_world(11, 3);
        let outcome = remap_container_width(&mut world, container, (11, 3), (10, 3));

        assert_eq!(outcome.overflow, vec![items[10], items[21], items[32]]);
        assert_eq!(outcome.placed, 30);

        let placed_slots: std::collections::HashSet<_> = {
            let links = world.borrow::<View<Links>>().unwrap();
            let links = links.get(container).unwrap();
            assert_eq!(
                links.to_links.len(),
                33,
                "overflow remains linked until world spill succeeds"
            );
            links
                .to_links
                .iter()
                .filter(|link| {
                    link.to_entity_id
                        .is_some_and(|target| !outcome.overflow.contains(&target.0))
                })
                .filter_map(|link| match link.link {
                    Link::Contains(slot) => Some(slot),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(placed_slots.len(), 30, "placed items never overlap");
        assert!(placed_slots.iter().all(|slot| *slot < 30));

        let pending_slots: Vec<_> = outcome
            .overflow
            .iter()
            .map(|entity| stored_slot(&world, container, *entity))
            .collect();
        assert_eq!(
            pending_slots,
            vec![
                PENDING_OVERFLOW_SLOT_BASE,
                PENDING_OVERFLOW_SLOT_BASE + 1,
                PENDING_OVERFLOW_SLOT_BASE + 2,
            ],
            "retained overflow has unique non-cell ordinals"
        );

        // A modelless item cannot be spilled into the world. If it remains
        // linked at one of the pending ordinals, a later grow must recover it
        // without duplication, overlap, or loss.
        let recovered = remap_container_width(&mut world, container, (10, 3), (11, 3));
        assert!(recovered.overflow.is_empty());
        assert_eq!(recovered.placed, 33);
        let recovered_layout = Inventory::from_container(&world, container, (11, 3));
        assert_eq!(recovered_layout.all_items().count(), 33);
        let recovered_slots: std::collections::HashSet<_> = items
            .iter()
            .map(|entity| stored_slot(&world, container, *entity))
            .collect();
        assert_eq!(recovered_slots.len(), 33);
        assert!(recovered_slots.iter().all(|slot| *slot < 33));
    }
}
