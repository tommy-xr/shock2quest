//! OverWorld Zero's serializable adventure simulation. Rendering and campaign rewards live outside it.
mod map;
mod rules;
mod simulation;
#[cfg(test)]
mod tests;

pub use map::{Map, Position, Tile, Town, WORLD_SIZE};
use rand::Rng;
pub use rules::{Dice, Kind, Loot, Terrain, level_for_xp};
use rules::{WALK, dropped_loot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Playing,
    Won,
    Lost,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Move(i32, i32),
    Wait,
    Potion,
    Missile(i32, i32),
    Inspect(Position),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Behavior {
    Wander,
    Guard,
    Attack,
    Camp,
    Talk,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: Kind,
    pub position: Position,
    pub hp: i32,
    behavior: Behavior,
    home: Option<Position>,
    pub element: Option<Terrain>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub position: Position,
    pub hp: i32,
    pub max_hp: i32,
    pub level: u16,
    pub xp: u32,
    pub gold: u32,
    pub potions: u16,
    pub equipment: u8,
    pub quest: Option<usize>,
    pub quests: u32,
    pub dragons: u8,
    pub turns: u32,
    pub spell_cooldown: u16,
    hit: Dice,
    damage: Dice,
    prayer: u16,
    healing: u16,
}
impl Player {
    pub fn armor(&self) -> i32 {
        if self.has(Loot::Armor) {
            10
        } else if self.has(Loot::Shield) {
            4
        } else {
            0
        }
    }
    pub fn damage_dice(&self) -> Dice {
        let mut dice = self.damage;
        dice.2 += if self.has(Loot::Hoe) {
            10
        } else if self.has(Loot::Sword) {
            4
        } else {
            0
        };
        dice
    }
    pub fn hit_dice(&self) -> Dice {
        self.hit
    }
    pub fn has(&self, loot: Loot) -> bool {
        self.equipment & (1 << loot as u8) != 0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub key: String,
    pub arguments: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visual {
    Hit,
    Miss,
    Fireball,
    Missile,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Animation {
    pub position: Position,
    pub visual: Visual,
    direction: (i32, i32),
    steps: u8,
    remaining_ms: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub won: bool,
    pub score: i32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverWorld {
    pub map: Map,
    pub player: Player,
    pub phase: Phase,
    actors: Vec<Option<Actor>>,
    pub animations: Vec<Animation>,
    pub messages: Vec<Message>,
    exotic: Option<Position>,
    lord_stage: u8,
    merchant_greeted: bool,
    wise_stage: u8,
}
impl OverWorld {
    pub fn new(hacked: bool, rng: &mut impl Rng) -> Self {
        let map = Map::generate(rng);
        let reachable = map.reachable(map.towns[0].position);
        let position = Map::find(rng, |p| {
            map.is_reachable(&reachable, p) && map.tile(p).terrain == Terrain::Plain
        })
        .unwrap();
        let hp = rng.gen_range(5..=10);
        let mut game = Self {
            map,
            player: Player {
                position,
                hp,
                max_hp: hp,
                level: 1,
                xp: 0,
                gold: 0,
                potions: u16::from(hacked),
                equipment: 0,
                quest: None,
                quests: 0,
                dragons: 0,
                turns: 0,
                spell_cooldown: 0,
                hit: Dice(1, 50, 15),
                damage: Dice(2, 4, 1),
                prayer: 0,
                healing: 0,
            },
            phase: Phase::Playing,
            actors: vec![None; 128],
            animations: Vec::new(),
            messages: Vec::new(),
            exotic: None,
            lord_stage: 0,
            merchant_greeted: false,
            wise_stage: 0,
        };
        let dragon = game
            .find_square(Kind::Dragon, false, Some(Terrain::Mountain), rng)
            .or_else(|| game.find_square(Kind::Dragon, false, None, rng))
            .unwrap();
        game.spawn(1, Kind::Dragon, dragon, Behavior::Guard, rng);
        for id in 2..128 {
            if rng.gen_ratio(83, 256) {
                let kind = game.choose_monster(rng);
                game.spawn_random(id, kind, false, rng);
            }
        }
        for _ in 0..2 {
            game.spawn_any(Kind::Squid, false, rng);
        }
        game.create_camp(Kind::Goblin, rng);
        game.create_camp(Kind::Goblin, rng);
        game.exotic = game.near_terrain(Terrain::Water, false, rng);
        game.say("ow_restart");
        game
    }
    pub fn actors(&self) -> impl Iterator<Item = (usize, &Actor)> {
        self.actors
            .iter()
            .enumerate()
            .filter_map(|(i, a)| a.as_ref().map(|a| (i, a)))
    }
    pub fn actor_at(&self, p: Position) -> Option<(usize, &Actor)> {
        self.actors().find(|(_, a)| a.position == p)
    }
    fn occupied(&self, p: Position) -> bool {
        p == self.player.position || self.actor_at(p).is_some()
    }
    fn say(&mut self, key: &str) {
        self.tell(key, Vec::new());
    }
    fn tell(&mut self, key: &str, arguments: Vec<String>) {
        self.messages.push(Message {
            key: key.into(),
            arguments,
        });
        if self.messages.len() > 4 {
            self.messages.remove(0);
        }
    }
    fn outcome(&self, previous: Phase) -> Option<Outcome> {
        (previous == Phase::Playing && self.phase != Phase::Playing).then(|| {
            let p = &self.player;
            let score = if self.phase == Phase::Won {
                1_000_000_000i64 - i64::from(p.turns) * 1000
                    + i64::from(p.level) * 500
                    + i64::from(p.gold) * 10
            } else {
                i64::from(p.turns) * 50 + i64::from(p.level) * 1000 + i64::from(p.gold) * 10
            };
            Outcome {
                won: self.phase == Phase::Won,
                score: score.clamp(0, i64::from(i32::MAX)) as i32,
            }
        })
    }
    /// Only the transition to a terminal state returns an outcome, including after a restore.
    pub fn act(&mut self, action: Action, hacked: bool, rng: &mut impl Rng) -> Option<Outcome> {
        if self.phase != Phase::Playing {
            return None;
        }
        if let Action::Inspect(p) = action {
            self.inspect(p);
            return None;
        }
        match action {
            Action::Move(x, y) => {
                if x != 0 || y != 0 {
                    self.player_move(self.player.position.offset(x.signum(), y.signum()), rng);
                }
            }
            Action::Potion => {
                if self.player.potions > 0 {
                    self.player.potions -= 1;
                    self.heal(10);
                } else {
                    self.say("ow_nopotion");
                }
            }
            Action::Missile(x, y) => {
                if !hacked || self.player.spell_cooldown != 0 || (x == 0 && y == 0) {
                    return None;
                }
                if self.launch(
                    self.player.position,
                    (x.signum(), y.signum()),
                    Visual::Missile,
                ) {
                    self.player.spell_cooldown = 64;
                }
            }
            Action::Wait => (),
            Action::Inspect(_) => unreachable!(),
        }
        if self.phase == Phase::Playing {
            self.run_turn(rng);
        }
        self.outcome(Phase::Playing)
    }
    fn player_move(&mut self, p: Position, rng: &mut impl Rng) {
        if let Some((id, actor)) = self.actor_at(p) {
            if actor.kind.friendly() {
                self.talk(id, rng);
            } else {
                self.combat(None, id, rng);
            }
        } else if self.map.tile(p).terrain.allows(WALK) {
            self.player.position = p;
            if let Some(loot) = self.map.tile_mut(p).loot.take() {
                match loot {
                    Loot::Gold | Loot::GoldPile | Loot::GoldChest => {
                        self.player.gold = self.player.gold.saturating_add(loot.gold());
                        self.tell("ow_gotgold", vec![loot.gold().to_string()]);
                    }
                    Loot::Potion => self.player.potions = self.player.potions.saturating_add(1),
                    _ => {
                        self.player.equipment |= 1 << loot as u8;
                        self.say(&format!("ow_find_loot_{}", loot as u8));
                    }
                }
            }
        } else {
            self.say("ow_impass");
        }
    }
    fn combat(&mut self, attacker: Option<usize>, target: usize, rng: &mut impl Rng) {
        let (hit, damage, cause) = if let Some(id) = attacker {
            let a = self.actors[id].as_ref().unwrap();
            let stats = a.kind.stats(a.element.is_some());
            (
                stats.hit,
                stats.damage,
                format!("ow_monster_{:02}", a.kind as u8),
            )
        } else {
            (
                self.player.hit,
                self.player.damage_dice(),
                "ow_monster_00".into(),
            )
        };
        let (armor, position) = if target == 0 {
            (self.player.armor(), self.player.position)
        } else {
            let a = self.actors[target].as_mut().unwrap();
            if a.kind.friendly() {
                return;
            }
            a.behavior = Behavior::Attack;
            let stats = a.kind.stats(a.element.is_some());
            (stats.armor, a.position)
        };
        if hit.roll(rng) - armor / 2 < 40 {
            self.flash(position, Visual::Miss);
            self.tell("ow_missed", vec![cause]);
        } else {
            let amount = (damage.roll(rng) - armor).max(0);
            if amount > 0 {
                self.flash(position, Visual::Hit);
                self.damage(target, amount, attacker.is_none(), &cause, rng);
            } else {
                self.tell("ow_nodamage", vec![cause]);
            }
        }
    }
    fn damage(
        &mut self,
        id: usize,
        amount: i32,
        player_credit: bool,
        cause: &str,
        rng: &mut impl Rng,
    ) {
        if self.phase != Phase::Playing {
            return;
        }
        if id == 0 {
            self.player.hp = (self.player.hp - amount).max(0);
            if self.player.hp == 0 {
                self.phase = Phase::Lost;
                self.tell("ow_youdied", vec![cause.into()]);
            } else {
                self.tell("ow_damage", vec![cause.into(), amount.to_string()]);
            }
            return;
        }
        let Some(actor) = self.actors[id].as_mut() else {
            return;
        };
        if actor.kind.friendly() {
            return;
        }
        actor.hp -= amount;
        actor.behavior = Behavior::Attack;
        if actor.hp > 0 {
            return;
        }
        let actor = self.actors[id].take().unwrap();
        let stats = actor.kind.stats(actor.element.is_some());
        if player_credit {
            self.gain_xp(stats.xp);
        }
        self.tell(
            "ow_itdied",
            vec![format!("ow_monster_{:02}", actor.kind as u8)],
        );
        if rng.gen_range(0..100) < stats.chance {
            if let Some(mut loot) = dropped_loot(stats.table, rng) {
                if matches!(loot, Loot::Armor | Loot::Hoe | Loot::Shield | Loot::Sword)
                    && self.player.has(loot)
                {
                    loot = Loot::GoldPile;
                }
                self.map.tile_mut(actor.position).loot = Some(loot);
            }
        }
        if actor.kind == Kind::Dragon {
            self.player.dragons += 1;
            if self.player.dragons == 1 {
                self.say("ow_dragondead");
                for p in self.map.shrines {
                    self.map.tile_mut(p).terrain = Terrain::Plain;
                }
                let p = self.map.shrines[0];
                // Quest actors must never be lost to population pressure or an occupied shrine.
                self.place_quest_actor(Kind::Lord, p, rng);
            } else if self.player.dragons == 5 {
                self.phase = Phase::Won;
                self.say("ow_win");
            }
        } else if actor.kind == Kind::Slime
            && rng.gen_range(0..100) < (20 + u32::from(self.player.level)).min(40)
        {
            for p in [actor.position, actor.position.offset(1, 0)] {
                if self.can_enter(Kind::Slime, p) {
                    if let Some(slot) = self.free_slot() {
                        self.spawn(slot, Kind::Slime, p, Behavior::Attack, rng);
                    }
                }
            }
        }
    }
    fn gain_xp(&mut self, amount: u32) {
        let before = level_for_xp(self.player.xp);
        self.player.xp = self.player.xp.saturating_add(amount);
        self.tell("ow_gainxp", vec![amount.to_string()]);
        if level_for_xp(self.player.xp) > before {
            self.say("ow_goshrine");
        }
    }
    fn heal(&mut self, amount: i32) {
        self.player.hp = (self.player.hp + amount).min(self.player.max_hp);
    }
    fn level_up(&mut self, rng: &mut impl Rng) {
        let hp = if self.player.level < 12 {
            rng.gen_range(2..8)
        } else {
            rng.gen_range(1..5)
        };
        let p = &mut self.player;
        p.level += 1;
        if p.max_hp < 222 {
            let hp = if p.max_hp >= 200 { 1 } else { hp };
            p.max_hp += hp;
            p.hp += hp;
        }
        if p.level <= 6 {
            p.damage.1 += 1;
            p.damage.2 += 1;
            p.hit.1 += 2;
            p.hit.2 += 1;
        } else {
            if p.level <= 12 || p.level % 2 == 1 {
                p.damage.1 += 1;
            }
            if p.hit.2 < 30 || (p.hit.2 < 35 && p.level % 2 == 1) {
                p.hit.2 += 1;
            }
        }
        let level = p.level;
        self.tell("ow_newlevel", vec![level.to_string()]);
        match level {
            3 | 9 => self.create_camp(Kind::Troll, rng),
            4 => self.create_camp(Kind::Headless, rng),
            5 => self.add_npc(Kind::Merchant, Terrain::Road, rng),
            7 => self.add_npc(Kind::Snickers, Terrain::City, rng),
            10 => self.add_npc(Kind::Wise, Terrain::Water, rng),
            15 => {
                if let Some(a) = &mut self.actors[1] {
                    a.behavior = Behavior::Attack;
                }
                self.say("ow_dragon");
            }
            18 | 21 => self.create_camp(Kind::Demon, rng),
            _ => (),
        }
    }
    fn talk(&mut self, id: usize, rng: &mut impl Rng) {
        let kind = self.actors[id].as_ref().unwrap().kind;
        match kind {
            Kind::Merchant => {
                if !self.merchant_greeted {
                    self.say("ow_merchant_1");
                    self.merchant_greeted = true;
                } else {
                    if self.player.gold >= 50 {
                        self.player.gold -= 50;
                        self.player.potions = self.player.potions.saturating_add(1);
                        self.say("ow_merchant_2");
                    } else {
                        self.say("ow_merchant_0");
                    }
                    self.merchant_greeted = false;
                    if let Some(p) = self.near_terrain(Terrain::Road, true, rng) {
                        self.actors[id].as_mut().unwrap().position = p;
                    }
                }
            }
            Kind::Wise => {
                match self.wise_stage {
                    0 => self.say("ow_hello"),
                    1 => self.say(&format!("ow_wiseman_{}", rng.gen_range(0..=7))),
                    _ => {
                        self.say("ow_wiseman_bye");
                        self.actors[id] = None;
                    }
                }
                self.wise_stage += 1;
            }
            Kind::Snickers => {
                self.say(&format!("ow_snickers_{}", rng.gen_range(0..=20)));
                if let Some(p) = self.near_terrain(Terrain::City, true, rng) {
                    self.actors[id].as_mut().unwrap().position = p;
                }
            }
            Kind::Lord => {
                while self.player.level < level_for_xp(self.player.xp) {
                    self.level_up(rng);
                }
                self.heal(self.player.max_hp);
                if self.lord_stage == 0 {
                    self.map.tile_mut(self.map.shrines[1]).terrain = Terrain::Temple;
                    self.lord_stage = 1;
                }
                self.say(if self.lord_stage == 1 {
                    "ow_lb_1"
                } else {
                    "ow_lb_2"
                });
            }
            _ => (),
        }
    }
    fn inspect(&mut self, p: Position) {
        if !p.on_screen(self.player.position) {
            return;
        }
        if self.exotic == Some(p) && self.player.level >= 10 {
            let (x, y) = p.delta(self.player.position);
            if x.abs() < 2 && y.abs() < 2 {
                self.map.tile_mut(p).loot = Some(Loot::Armor);
                self.exotic = None;
                self.say("ow_find_loot_3");
                return;
            }
        }
        if p == self.player.position {
            self.tell("ow_stats_ac", vec![self.player.armor().to_string()]);
            let d = self.player.damage_dice();
            self.tell(
                "ow_stats_dmg",
                vec![d.0.to_string(), d.1.to_string(), d.2.to_string()],
            );
            return;
        }
        if let Some((_, a)) = self.actor_at(p) {
            self.tell(
                "ow_id_monst",
                vec![format!("ow_monster_{:02}", a.kind as u8)],
            );
        }
        let t = self.map.tile(p).clone();
        if let Some(loot) = t.loot {
            if loot.gold() > 0 {
                self.tell("ow_id_loot", vec![loot.gold().to_string()]);
            } else {
                self.say(&format!("ow_id_loot_{}", loot as u8));
            }
        }
        if let Some(town) = t.town {
            self.tell("ow_id_city", vec![self.map.towns[town].name.clone()]);
        }
        self.tell(
            "ow_id_terr",
            vec![format!("ow_terrain_{}", t.terrain as u8)],
        );
    }
}
