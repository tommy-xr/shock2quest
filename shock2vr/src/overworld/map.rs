use super::{Kind, Loot, Terrain};
use rand::Rng;
use serde::{Deserialize, Serialize};

pub const WORLD_SIZE: i32 = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}
impl Position {
    pub fn offset(self, x: i32, y: i32) -> Self {
        Self {
            x: (self.x + x).rem_euclid(WORLD_SIZE),
            y: (self.y + y).rem_euclid(WORLD_SIZE),
        }
    }
    pub fn delta(self, other: Self) -> (i32, i32) {
        (
            (other.x - self.x + 32).rem_euclid(64) - 32,
            (other.y - self.y + 32).rem_euclid(64) - 32,
        )
    }
    pub(super) fn distance(self, other: Self) -> i32 {
        let (x, y) = self.delta(other);
        x.abs() + y.abs()
    }
    pub fn on_screen(self, center: Self) -> bool {
        let (x, y) = center.delta(self);
        x.abs() <= 5 && y.abs() <= 8
    }
    fn index(self) -> usize {
        (self.y.rem_euclid(64) * 64 + self.x.rem_euclid(64)) as usize
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tile {
    pub terrain: Terrain,
    pub loot: Option<Loot>,
    pub town: Option<usize>,
    pub camp: Option<Kind>,
}
impl Default for Tile {
    fn default() -> Self {
        Self {
            terrain: Terrain::Plain,
            loot: None,
            town: None,
            camp: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Town {
    pub position: Position,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map {
    tiles: Vec<Tile>,
    pub towns: Vec<Town>,
    /// Mountain shrine, then water shrine; these later become the lord and temple.
    pub shrines: [Position; 2],
}
impl Map {
    pub fn tile(&self, at: Position) -> &Tile {
        &self.tiles[at.index()]
    }
    pub(super) fn tile_mut(&mut self, at: Position) -> &mut Tile {
        &mut self.tiles[at.index()]
    }
    pub(super) fn positions() -> impl Iterator<Item = Position> {
        (0..4096).map(|i| Position {
            x: i % 64,
            y: i / 64,
        })
    }
    pub(super) fn find(
        rng: &mut impl Rng,
        mut valid: impl FnMut(Position) -> bool,
    ) -> Option<Position> {
        let start = rng.gen_range(0..4096);
        (0..4096)
            .map(|i| {
                let n = (start + i) % 4096;
                Position {
                    x: n % 64,
                    y: n / 64,
                }
            })
            .find(|&p| valid(p))
    }
    pub(super) fn generate(rng: &mut impl Rng) -> Self {
        let mut map = Self {
            tiles: vec![Tile::default(); 4096],
            towns: Vec::new(),
            shrines: [Position::default(); 2],
        };
        for (terrain, count, low, high, linear) in [
            (Terrain::Forest, 24, 2, 7, false),
            (Terrain::Mountain, 12, 4, 10, true),
            (Terrain::Water, 3, 3, 6, false),
        ] {
            for _ in 0..count {
                let center = Position {
                    x: rng.gen_range(0..64),
                    y: rng.gen_range(0..64),
                };
                let radius = rng.gen_range(low..high);
                let angle = rng.gen_range(0.0..std::f32::consts::PI);
                let scale = if linear {
                    rng.gen_range(0.08..0.33)
                } else {
                    1.0
                };
                for y in -radius..=radius {
                    for x in -radius..=radius {
                        let a = x as f32 + rng.gen_range(0.0..1.0);
                        let b = y as f32 + rng.gen_range(0.0..1.0);
                        let u = (a * angle.cos() + b * angle.sin()) / scale;
                        let v = -a * angle.sin() + b * angle.cos();
                        if u * u + v * v < (radius * radius) as f32 {
                            map.tile_mut(center.offset(x, y)).terrain = terrain;
                        }
                    }
                }
            }
        }
        for _ in 0..80 {
            if let Some(p) = Self::find(rng, |p| map.tile(p).terrain == Terrain::Plain) {
                map.tile_mut(p).terrain = Terrain::Forest;
            }
        }
        for _ in 0..3 {
            let start = Self::find(rng, |p| {
                map.tile(p).town.is_none() && map.tile(p).terrain.allows(super::rules::WALK)
            })
            .unwrap();
            map.add_town(start, Terrain::City, rng);
            let dx = if rng.gen_bool(0.5) { 1 } else { -1 };
            let dy = rng.gen_range(-1..=1);
            let mut end = start;
            for _ in 0..rng.gen_range(9..27) {
                end = end.offset(dx, dy);
                if map.tile(end).town.is_none() {
                    map.tile_mut(end).terrain = Terrain::Road;
                }
            }
            // Keep all six destinations distinct even if roads meet.
            let end =
                Self::find(rng, |p| p.distance(end) < 5 && map.tile(p).town.is_none()).unwrap();
            map.add_town(end, Terrain::City, rng);
        }
        for _ in 0..8 {
            let p = Self::find(rng, |p| {
                map.tile(p).town.is_none()
                    && matches!(map.tile(p).terrain, Terrain::Plain | Terrain::Forest)
            })
            .unwrap();
            map.add_town(p, Terrain::Village, rng);
        }
        for (i, ring) in [Terrain::Mountain, Terrain::Water].into_iter().enumerate() {
            let p = Self::find(rng, |p| {
                (-2..=2).all(|y| {
                    (-2..=2).all(|x| {
                        let t = map.tile(p.offset(x, y));
                        t.town.is_none() && matches!(t.terrain, Terrain::Plain | Terrain::Forest)
                    })
                })
            })
            .unwrap();
            for y in -1..=1 {
                for x in -1..=1 {
                    map.tile_mut(p.offset(x, y)).terrain = ring;
                }
            }
            map.tile_mut(p.offset(1, 0)).terrain = Terrain::Road;
            map.tile_mut(p.offset(2, 0)).terrain = Terrain::Road;
            map.tile_mut(p).terrain = Terrain::Shrine;
            map.shrines[i] = p;
        }
        // Every quest and shrine must be reachable on foot. Connect only isolated destinations.
        let anchor = map.towns[0].position;
        let destinations: Vec<_> = map
            .towns
            .iter()
            .map(|t| t.position)
            .chain(map.shrines)
            .collect();
        for destination in destinations {
            if !map.reachable(anchor)[destination.index()] {
                let mut p = destination;
                while p != anchor {
                    let (x, y) = p.delta(anchor);
                    p = p.offset(x.signum(), y.signum());
                    if !map.tile(p).terrain.allows(super::rules::WALK) {
                        map.tile_mut(p).terrain = Terrain::Road;
                    }
                }
            }
        }
        for _ in 0..64 {
            let p = Self::find(rng, |p| {
                map.tile(p).terrain.allows(super::rules::WALK) && map.tile(p).loot.is_none()
            })
            .unwrap();
            map.tile_mut(p).loot = Some(if rng.gen_ratio(1, 8) {
                if rng.gen_ratio(1, 16) {
                    Loot::GoldChest
                } else {
                    Loot::GoldPile
                }
            } else {
                Loot::Gold
            });
        }
        map
    }
    pub(super) fn reachable(&self, start: Position) -> Vec<bool> {
        let mut seen = vec![false; 4096];
        let mut queue = std::collections::VecDeque::from([start]);
        seen[start.index()] = true;
        while let Some(p) = queue.pop_front() {
            for y in -1..=1 {
                for x in -1..=1 {
                    let n = p.offset(x, y);
                    if !seen[n.index()] && self.tile(n).terrain.allows(super::rules::WALK) {
                        seen[n.index()] = true;
                        queue.push_back(n);
                    }
                }
            }
        }
        seen
    }
    pub(super) fn is_reachable(&self, reached: &[bool], position: Position) -> bool {
        reached[position.index()]
    }
    pub(super) fn can_reach_neighbor(&self, reached: &[bool], p: Position) -> bool {
        (-1..=1).any(|y| (-1..=1).any(|x| self.is_reachable(reached, p.offset(x, y))))
    }

    pub(super) fn repaint(&mut self, p: Position, terrain: Terrain, player: Position) {
        let previous = self.tile(p).terrain;
        if !matches!(
            previous,
            Terrain::Plain | Terrain::Forest | Terrain::Mountain | Terrain::Water | Terrain::Road
        ) {
            return;
        }
        if terrain.allows(super::rules::WALK) || !previous.allows(super::rules::WALK) {
            self.tile_mut(p).terrain = terrain;
            return;
        }
        // A dragon may change terrain, but cannot cut the player's only route to a quest or exit.
        let before = self.reachable(player);
        self.tile_mut(p).terrain = terrain;
        let after = self.reachable(player);
        if Self::positions().any(|q| q != p && before[q.index()] && !after[q.index()]) {
            self.tile_mut(p).terrain = previous;
        }
    }

    fn add_town(&mut self, p: Position, terrain: Terrain, rng: &mut impl Rng) {
        let length = if terrain == Terrain::City {
            rng.gen_range(4..8)
        } else {
            rng.gen_range(3..5)
        };
        let mut name = String::new();
        for i in 0..length {
            let letters: &[u8] = if i % 2 == 0 {
                b"bcdfghjklmnpqrstvwxz"
            } else {
                b"aeiouy"
            };
            let c = letters[rng.gen_range(0..letters.len())] as char;
            name.push(if i == 0 { c.to_ascii_uppercase() } else { c });
        }
        let id = self.towns.len();
        self.towns.push(Town { position: p, name });
        let tile = self.tile_mut(p);
        tile.terrain = terrain;
        tile.town = Some(id);
    }
}
