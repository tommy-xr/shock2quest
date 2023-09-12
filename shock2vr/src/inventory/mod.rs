use shipyard::EntityId;
pub mod player_inventory_entity;
pub use player_inventory_entity::*;

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

    pub fn insert_first_available(
        &mut self,
        entity: EntityId,
        width: usize,
        height: usize,
    ) -> bool {
        for y in 0..self.height {
            for x in 0..self.width {
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
