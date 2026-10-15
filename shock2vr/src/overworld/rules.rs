use rand::Rng;
use serde::{Deserialize, Serialize};

pub(super) const WALK: u8 = 1;
pub(super) const CLIMB: u8 = 2;
pub(super) const SWIM: u8 = 4;
pub(super) const FLY: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Terrain {
    Plain,
    Forest,
    Mountain,
    Water,
    Village,
    City,
    Road,
    Shrine,
    Camp,
    Temple,
}
impl Terrain {
    pub(super) fn allows(self, mobility: u8) -> bool {
        let allowed = match self {
            Self::Forest => WALK,
            Self::Mountain => CLIMB | FLY,
            Self::Water => SWIM | FLY,
            _ => WALK | FLY,
        };
        mobility & allowed != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Loot {
    Gold,
    GoldPile,
    GoldChest,
    Armor,
    Hoe,
    Shield,
    Sword,
    Potion,
}
impl Loot {
    pub(super) fn gold(self) -> u32 {
        match self {
            Self::Gold => 1,
            Self::GoldPile => 6,
            Self::GoldChest => 17,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Player,
    Goblin,
    Headless,
    Bat,
    Pig,
    Troll,
    Ghost,
    Squid,
    Demon,
    Dragon,
    Wise,
    Gazer,
    Reaper,
    Mimic,
    Lurker,
    Slime,
    Mongbat,
    Snickers,
    Lord,
    Merchant,
}
impl Kind {
    pub(super) const HOSTILE: [Self; 14] = [
        Self::Goblin,
        Self::Headless,
        Self::Bat,
        Self::Pig,
        Self::Troll,
        Self::Ghost,
        Self::Squid,
        Self::Demon,
        Self::Gazer,
        Self::Reaper,
        Self::Mimic,
        Self::Lurker,
        Self::Slime,
        Self::Mongbat,
    ];
    pub fn friendly(self) -> bool {
        matches!(
            self,
            Self::Wise | Self::Snickers | Self::Lord | Self::Merchant
        )
    }
    pub(super) fn stats(self, elemental: bool) -> Stats {
        // Hit dice, health dice, damage dice, armor, XP, tier, sight, loot chance/table, mobility.
        let (hit, hp, damage, armor, xp, tier, sight, chance, table, mobility) = match self {
            Self::Player => ((1, 50, 15), (0, 0, 0), (2, 4, 1), 0, 0, 0, 0, 0, 0, WALK),
            Self::Goblin => ((1, 50, 2), (1, 6, 1), (1, 4, 0), 0, 3, 1, 8, 25, 1, WALK),
            Self::Headless => ((1, 50, 8), (2, 6, 4), (1, 8, 1), 1, 7, 2, 4, 40, 2, WALK),
            Self::Bat => ((1, 50, 0), (1, 4, 0), (1, 3, 0), 0, 2, 1, 8, 0, 0, FLY),
            Self::Pig => ((1, 50, 4), (4, 5, 5), (2, 3, 0), 4, 9, 2, 6, 20, 1, WALK),
            Self::Troll => ((1, 50, 17), (3, 8, 4), (2, 8, 2), 2, 13, 3, 10, 50, 2, WALK),
            Self::Ghost => (
                (1, 50, 12),
                (2, 8, 8),
                (3, 8, 1),
                0,
                15,
                4,
                10,
                25,
                2,
                WALK | FLY,
            ),
            Self::Squid => ((1, 50, 20), (2, 8, 1), (3, 8, 1), 1, 9, 2, 6, 15, 0, SWIM),
            Self::Demon => (
                (1, 50, 25),
                (5, 8, 8),
                (4, 8, 1),
                3,
                28,
                6,
                12,
                50,
                3,
                WALK | CLIMB,
            ),
            Self::Dragon if elemental => (
                (1, 70, 34),
                (9, 12, 10),
                (6, 8, 8),
                10,
                150,
                99,
                18,
                75,
                4,
                WALK | FLY,
            ),
            Self::Dragon => (
                (1, 60, 30),
                (6, 10, 10),
                (5, 6, 4),
                5,
                75,
                99,
                18,
                75,
                4,
                WALK | FLY,
            ),
            Self::Gazer => ((1, 50, 21), (6, 6, 1), (4, 4, 1), 2, 21, 5, 15, 15, 2, FLY),
            Self::Reaper => (
                (1, 50, 24),
                (5, 8, 8),
                (3, 12, 2),
                4,
                25,
                5,
                10,
                25,
                3,
                WALK,
            ),
            Self::Mimic => ((1, 50, 17), (4, 8, 4), (4, 3, 0), 3, 15, 3, 6, 50, 2, WALK),
            Self::Lurker => ((1, 50, 17), (7, 8, 5), (5, 3, 1), 4, 19, 4, 6, 50, 2, WALK),
            Self::Slime => ((1, 50, 15), (1, 3, 1), (1, 2, 1), 0, 2, 1, 4, 0, 0, WALK),
            Self::Mongbat => ((1, 50, 11), (2, 10, 2), (3, 3, 2), 1, 9, 3, 12, 5, 1, FLY),
            _ => (
                (1, 50, 50),
                (9, 10, 10),
                (9, 10, 10),
                0,
                0,
                0,
                0,
                0,
                0,
                WALK,
            ),
        };
        Stats {
            hit: Dice(hit.0, hit.1, hit.2),
            hp: Dice(hp.0, hp.1, hp.2),
            damage: Dice(damage.0, damage.1, damage.2),
            armor,
            xp,
            tier,
            sight,
            chance,
            table,
            mobility,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dice(pub u16, pub u16, pub u16);
impl Dice {
    pub(super) fn roll(self, rng: &mut impl Rng) -> i32 {
        i32::from(self.2)
            + (0..self.0)
                .map(|_| rng.gen_range(1..=i32::from(self.1)))
                .sum::<i32>()
    }
}
pub(super) struct Stats {
    pub hit: Dice,
    pub hp: Dice,
    pub damage: Dice,
    pub armor: i32,
    pub xp: u32,
    pub tier: u8,
    pub sight: i32,
    pub chance: u32,
    pub table: usize,
    pub mobility: u8,
}

pub fn level_for_xp(xp: u32) -> u16 {
    let (mut level, mut threshold) = (1u16, 16u64);
    while threshold <= u64::from(xp) {
        let n = u64::from(level);
        threshold += 16 + 8 * n + 9 * n.saturating_sub(6) + 10 * n.saturating_sub(14);
        level += 1;
    }
    level
}

pub(super) fn dropped_loot(table: usize, rng: &mut impl Rng) -> Option<Loot> {
    let weights = match table {
        1 => [78, 10, 0, 0, 0, 1, 1, 10],
        2 => [10, 40, 20, 0, 0, 5, 5, 20],
        3 => [0, 30, 50, 0, 0, 5, 5, 10],
        4 => [0, 0, 0, 0, 10, 30, 30, 30],
        _ => return None,
    };
    let mut roll = rng.gen_range(0..100);
    for (loot, weight) in [
        Loot::Gold,
        Loot::GoldPile,
        Loot::GoldChest,
        Loot::Armor,
        Loot::Hoe,
        Loot::Shield,
        Loot::Sword,
        Loot::Potion,
    ]
    .into_iter()
    .zip(weights)
    {
        if roll < weight {
            return Some(loot);
        }
        roll -= weight;
    }
    None
}
