//! Inventory grids: which cell each contained item occupies.
//!
//! A containment link carries the item's cell as its data - the `Contains`
//! ordinal is `y * width + x` - so an item keeps the cell it was put in
//! instead of being repacked every time the panel is drawn. That ordinal is
//! authored in the mission data and round-trips through save/load with the
//! link, so this is also what makes a container's layout stable across a
//! reload.

use dark::properties::{Link, PropContainDimensions, PropInventoryDimensions};
use shipyard::{EntityId, Get, View, World};

pub mod player_inventory_entity;
pub use player_inventory_entity::*;

/// The player's backpack grid, in cells (`invback.pcx` draws 15x3).
pub const BACKPACK_GRID: (usize, usize) = (15, 3);

/// A container's loot grid, in cells (`contain.pcx` draws 4x4).
pub const CONTAINER_GRID: (usize, usize) = (4, 4);

/// Ordinals at or above this are not grid cells - the original reserves the
/// range for equipped/paperdoll slots. We do not model those yet; such an
/// item simply has no cell and gets first-fit like an unplaced one.
const EQUIP_SLOT_BASE: u32 = 1000;

/// Which grid `container_entity` is drawn with. The player's backpack is
/// wider than a container's loot panel, and the ordinal encodes `y * width +
/// x`, so both sides of the containment link must agree on the width.
///
/// A container's grid is authored as `P$ContainDims`; every container in the
/// shipped game declares 4x4, which is also the fallback for anything that
/// does not declare one. The backpack's width is not read from the world yet -
/// the original derives it from Strength (see the tracking issue), so it stays
/// at its maximum here.
pub fn grid_for(world: &World, container_entity: EntityId) -> (usize, usize) {
    let is_backpack = world
        .borrow::<View<PlayerInventoryEntity>>()
        .map(|v| v.get(container_entity).is_ok())
        .unwrap_or(false);
    if is_backpack {
        return BACKPACK_GRID;
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
}
