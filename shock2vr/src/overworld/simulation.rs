use super::*;

impl OverWorld {
    pub(super) fn free_slot(&self) -> Option<usize> {
        (2..128).find(|&i| self.actors[i].is_none())
    }
    fn quest_slot(&mut self) -> usize {
        self.free_slot().unwrap_or_else(|| {
            let id = self
                .actors()
                .filter(|(_, a)| !a.kind.friendly() && a.kind != Kind::Dragon)
                .max_by_key(|(_, a)| a.position.distance(self.player.position))
                .unwrap()
                .0;
            self.actors[id] = None;
            id
        })
    }
    pub(super) fn spawn(
        &mut self,
        id: usize,
        kind: Kind,
        position: Position,
        behavior: Behavior,
        rng: &mut impl Rng,
    ) {
        self.actors[id] = Some(Actor {
            kind,
            position,
            hp: kind.stats(false).hp.roll(rng),
            behavior,
            home: None,
            element: None,
        });
    }
    pub(super) fn can_enter(&self, kind: Kind, p: Position) -> bool {
        !self.occupied(p) && self.map.tile(p).terrain.allows(kind.stats(false).mobility)
    }
    pub(super) fn find_square(
        &self,
        kind: Kind,
        offscreen: bool,
        terrain: Option<Terrain>,
        rng: &mut impl Rng,
    ) -> Option<Position> {
        let reachable = (kind.friendly() || matches!(kind, Kind::Player | Kind::Dragon))
            .then(|| self.map.reachable(self.player.position));
        Map::find(rng, |p| {
            self.can_enter(kind, p)
                && reachable.as_ref().is_none_or(|r| {
                    if kind == Kind::Dragon {
                        self.map.can_reach_neighbor(r, p)
                    } else {
                        self.map.is_reachable(r, p)
                    }
                })
                && (!offscreen || !p.on_screen(self.player.position))
                && terrain.is_none_or(|t| self.map.tile(p).terrain == t)
        })
    }
    pub(super) fn spawn_random(
        &mut self,
        id: usize,
        kind: Kind,
        offscreen: bool,
        rng: &mut impl Rng,
    ) {
        if let Some(p) = self.find_square(kind, offscreen, None, rng) {
            let behavior = if kind == Kind::Mimic {
                Behavior::Guard
            } else {
                Behavior::Wander
            };
            self.spawn(id, kind, p, behavior, rng);
        }
    }
    pub(super) fn spawn_any(&mut self, kind: Kind, offscreen: bool, rng: &mut impl Rng) {
        if let Some(id) = self.free_slot() {
            self.spawn_random(id, kind, offscreen, rng);
        }
    }
    pub(super) fn choose_monster(&self, rng: &mut impl Rng) -> Kind {
        let tier =
            (Dice(6, 2, 0).roll(rng) + i32::from(self.player.level) / 2 - 9).clamp(1, 6) as u8;
        let choices: Vec<_> = Kind::HOSTILE
            .into_iter()
            .filter(|k| k.stats(false).tier == tier)
            .collect();
        choices[rng.gen_range(0..choices.len())]
    }
    pub(super) fn near_terrain(
        &self,
        terrain: Terrain,
        offscreen: bool,
        rng: &mut impl Rng,
    ) -> Option<Position> {
        let reachable = self.map.reachable(self.player.position);
        Map::find(rng, |p| {
            self.can_enter(Kind::Player, p)
                && self.map.is_reachable(&reachable, p)
                && (!offscreen || !p.on_screen(self.player.position))
                && (-1..=1).any(|y| {
                    (-1..=1).any(|x| {
                        (x != 0 || y != 0) && self.map.tile(p.offset(x, y)).terrain == terrain
                    })
                })
        })
    }
    pub(super) fn place_quest_actor(&mut self, kind: Kind, position: Position, rng: &mut impl Rng) {
        let slot = self.quest_slot();
        if let Some((id, _)) = self.actor_at(position) {
            if let Some(p) =
                self.find_square(self.actors[id].as_ref().unwrap().kind, true, None, rng)
            {
                self.actors[id].as_mut().unwrap().position = p;
            }
        }
        let position = if self.can_enter(kind, position) {
            position
        } else {
            let reachable = self.map.reachable(self.player.position);
            Map::find(rng, |p| {
                p.distance(position) <= 4
                    && self.can_enter(kind, p)
                    && self.map.is_reachable(&reachable, p)
            })
            .or_else(|| self.find_square(kind, false, None, rng))
            .unwrap()
        };
        self.spawn(slot, kind, position, Behavior::Talk, rng);
    }
    pub(super) fn add_npc(&mut self, kind: Kind, terrain: Terrain, rng: &mut impl Rng) {
        if let Some(p) = self
            .near_terrain(terrain, true, rng)
            .or_else(|| self.find_square(kind, false, None, rng))
        {
            self.place_quest_actor(kind, p, rng);
        }
    }
    pub(super) fn create_camp(&mut self, kind: Kind, rng: &mut impl Rng) {
        if let Some(p) = self.find_square(kind, true, Some(Terrain::Plain), rng) {
            let t = self.map.tile_mut(p);
            t.terrain = Terrain::Camp;
            t.camp = Some(kind);
            self.camp_actor(kind, p, p, 2.0, false, rng);
            for (x, y) in [(-1, -1), (1, 1), (-1, 1), (1, -1)]
                .into_iter()
                .take(2 + usize::from(kind.stats(false).tier) / 3)
            {
                self.camp_actor(kind, p.offset(x, y), p, 1.5, false, rng);
            }
        }
    }
    fn camp_actor(
        &mut self,
        kind: Kind,
        p: Position,
        home: Position,
        factor: f32,
        active: bool,
        rng: &mut impl Rng,
    ) {
        if !self.can_enter(kind, p) {
            return;
        }
        if let Some(id) = self.free_slot() {
            self.spawn(
                id,
                kind,
                p,
                if active {
                    Behavior::Camp
                } else {
                    Behavior::Guard
                },
                rng,
            );
            let a = self.actors[id].as_mut().unwrap();
            a.hp = (a.hp as f32 * factor) as i32;
            a.home = Some(home);
        }
    }
    fn update_camps(&mut self, rng: &mut impl Rng) {
        let camps: Vec<_> = Map::positions()
            .filter_map(|p| self.map.tile(p).camp.map(|k| (p, k)))
            .collect();
        for (p, kind) in camps {
            let tier = i32::from(kind.stats(false).tier);
            let level = i32::from(self.player.level);
            let delta = level - 2 * tier;
            let charging = delta > 0;
            self.camp_actor(kind, p, p, 2.0, charging, rng);
            if level <= 2 || level > 4 + 3 * tier {
                continue;
            }
            let target = (3 + (tier + delta) / 2).min(8);
            let mut count = self
                .actors()
                .filter(|(_, a)| a.kind == kind && a.position.distance(p) <= 7)
                .count() as i32;
            for y in -1..=1 {
                for x in -1..=1 {
                    let at = p.offset(x, y);
                    if let Some((id, a)) = self.actor_at(at) {
                        if charging
                            && a.home == Some(p)
                            && a.behavior == Behavior::Guard
                            && rng.gen_bool(0.5)
                        {
                            self.actors[id].as_mut().unwrap().behavior = Behavior::Camp;
                        }
                    } else if count < target
                        && self.actors().count() < 90
                        && rng.gen_range(0..100) < 30 + 4 * (target - count) + delta * 6
                        && self.can_enter(kind, at)
                    {
                        self.camp_actor(kind, at, p, 1.4, charging, rng);
                        count += 1;
                    }
                }
            }
        }
    }
    pub(super) fn move_actor(&mut self, id: usize, target: Position) {
        let a = self.actors[id].as_ref().unwrap();
        let pos = a.position;
        let kind = a.kind;
        let (dx, dy) = pos.delta(target);
        let (x, y) = (dx.signum(), dy.signum());
        let reachable = (kind == Kind::Dragon).then(|| self.map.reachable(self.player.position));
        for p in [pos.offset(x, y), pos.offset(0, y), pos.offset(x, 0)] {
            if self.can_enter(kind, p)
                && reachable
                    .as_ref()
                    .is_none_or(|r| self.map.can_reach_neighbor(r, p))
            {
                self.actors[id].as_mut().unwrap().position = p;
                break;
            }
        }
    }
    fn ai(&mut self, id: usize, rng: &mut impl Rng) {
        let Some(a) = self.actors[id].clone() else {
            return;
        };
        if a.kind.friendly() {
            return;
        }
        let (x, y) = a.position.delta(self.player.position);
        let distance = x.abs() + y.abs();
        match a.behavior {
            Behavior::Attack => {
                if x.abs() <= 1 && y.abs() <= 1 {
                    self.combat(Some(id), 0, rng);
                } else if rng.gen_range(0..100) < 2 * distance - 1 {
                    self.actors[id].as_mut().unwrap().behavior = if a.home.is_some() {
                        Behavior::Camp
                    } else {
                        Behavior::Wander
                    };
                } else {
                    self.move_actor(id, self.player.position);
                }
            }
            Behavior::Wander | Behavior::Camp => {
                let target = if let Some(home) = a.home {
                    if rng.gen_range(0..100) < 20 + 10 * a.position.distance(home) {
                        home
                    } else {
                        a.position
                            .offset(rng.gen_range(-4..=4), rng.gen_range(-4..=4))
                    }
                } else {
                    a.position
                        .offset(rng.gen_range(-4..=4), rng.gen_range(-4..=4))
                };
                self.move_actor(id, target);
                if rng.gen_ratio(3, 16)
                    && rng.gen_range(0..100) < (a.kind.stats(false).sight - distance) * 10
                {
                    self.actors[id].as_mut().unwrap().behavior = Behavior::Attack;
                    self.say("ow_wereseen");
                } else {
                    let p = self.actors[id].as_ref().unwrap().position;
                    let tile = self.map.tile(p);
                    let chance = match (tile.loot, tile.terrain) {
                        (Some(Loot::Gold), _) => 15,
                        (Some(_), _) => 75,
                        (_, Terrain::City) => 50,
                        (_, Terrain::Village) => 20,
                        (_, Terrain::Shrine) => 99,
                        _ => 0,
                    };
                    if a.home.is_none() && a.kind != Kind::Dragon && rng.gen_range(0..100) < chance
                    {
                        self.actors[id].as_mut().unwrap().behavior = Behavior::Guard;
                    }
                }
            }
            _ => (),
        }
    }
    pub(super) fn run_turn(&mut self, rng: &mut impl Rng) {
        // Process the existing population before replenishment and progression spawns.
        let ids: Vec<_> = self.actors().map(|(id, _)| id).collect();
        for id in ids {
            self.ai(id, rng);
            if self.phase != Phase::Playing {
                return;
            }
        }
        let count = self.actors().count();
        if count < 30
            || (count < 50 && rng.gen_ratio(8, 256))
            || (count < 100 && rng.gen_ratio(4, 256))
        {
            let kind = self.choose_monster(rng);
            self.spawn_any(kind, true, rng);
        }
        if self.player.turns % 16 == 8 {
            self.update_camps(rng);
        }
        if self.player.turns % 32 == 0 {
            if let Some(p) = self.find_square(Kind::Player, true, None, rng) {
                let roll = Dice(4, 2, 0).roll(rng) - 4;
                self.map.tile_mut(p).loot = Some(match if roll >= 2 { roll - 2 } else { roll } {
                    0 => Loot::Gold,
                    1 => Loot::GoldPile,
                    _ => Loot::GoldChest,
                });
            }
        }
        let tile = self.map.tile(self.player.position).clone();
        if tile.terrain == Terrain::Shrine && self.player.level < level_for_xp(self.player.xp) {
            self.player.prayer += 1;
            if self.player.prayer == 1 {
                self.say("ow_startpray");
            } else {
                self.say("ow_keeppray");
            }
            if self.player.prayer > (self.player.level * 2).min(16) {
                self.level_up(rng);
                self.player.prayer = 0;
            }
        } else {
            self.player.prayer = 0;
        }
        if self.player.healing > 0 {
            self.player.healing -= 1;
        } else if self.player.hp < self.player.max_hp {
            self.heal((self.player.max_hp * 5 / 100).max(1));
            self.player.healing = 64;
        }
        self.player.spell_cooldown = self.player.spell_cooldown.saturating_sub(1);
        match tile.terrain {
            Terrain::City | Terrain::Village => {
                let cost = if tile.terrain == Terrain::City { 5 } else { 15 };
                if self.player.hp < self.player.max_hp {
                    if self.player.gold >= cost {
                        self.player.gold -= cost;
                        self.heal((self.player.max_hp / 10).max(1));
                        self.tell("ow_payheal", vec![cost.to_string()]);
                    } else {
                        self.tell("ow_nocash", vec![cost.to_string()]);
                    }
                }
                if let Some(town) = tile.town {
                    if self.player.quest == Some(town) {
                        let xp = (self.player.quests * (2 + u32::from(self.player.level) / 10) + 5)
                            .min(20 + 2 * u32::from(self.player.level));
                        self.gain_xp(xp);
                        self.player.quests += 1;
                        self.player.quest = None;
                        self.say("ow_doquest");
                    } else if self.player.quest.is_none() && tile.terrain == Terrain::City {
                        let options: Vec<_> =
                            (0..self.map.towns.len()).filter(|&i| i != town).collect();
                        let target = options[rng.gen_range(0..options.len())];
                        self.player.quest = Some(target);
                        self.tell("ow_getquest", vec![self.map.towns[target].name.clone()]);
                    }
                }
            }
            Terrain::Camp => {
                if let Some(kind) = tile.camp {
                    let tier = u32::from(kind.stats(false).tier);
                    self.gain_xp(15 + 3 * tier * tier);
                }
                let t = self.map.tile_mut(self.player.position);
                t.camp = None;
                t.terrain = Terrain::Plain;
                self.say("ow_destroycamp");
            }
            Terrain::Temple if self.lord_stage == 1 => {
                self.lord_stage = 2;
                self.gain_xp(222);
                self.say("ow_temple");
                for element in [
                    Terrain::Mountain,
                    Terrain::Water,
                    Terrain::Forest,
                    Terrain::Road,
                ] {
                    let id = self.quest_slot();
                    let p = self.find_square(Kind::Dragon, true, None, rng).unwrap();
                    self.spawn(id, Kind::Dragon, p, Behavior::Wander, rng);
                    let a = self.actors[id].as_mut().unwrap();
                    a.element = Some(element);
                    a.hp = Kind::Dragon.stats(true).hp.roll(rng);
                }
            }
            _ => (),
        }
        let dragons: Vec<_> = self
            .actors()
            .filter(|(_, a)| a.kind == Kind::Dragon)
            .map(|(_, a)| (a.position, a.element))
            .collect();
        for (p, element) in dragons {
            if let Some(terrain) = element {
                self.map.repaint(p, terrain, self.player.position);
            }
            let shoots = if element.is_some() {
                rng.gen_ratio(3, 32)
            } else {
                self.player.level > 7 && rng.gen_ratio(3, 64)
            };
            if shoots {
                let (x, y) = p.delta(self.player.position);
                let direction = if x.abs() + y.abs() < 18 {
                    if x.abs() * 2 < y.abs() {
                        (0, y.signum())
                    } else if y.abs() * 2 < x.abs() {
                        (x.signum(), 0)
                    } else {
                        (x.signum(), y.signum())
                    }
                } else if rng.gen_ratio(1, 8) {
                    (rng.gen_range(-1..=1), rng.gen_range(-1..=1))
                } else {
                    (0, 0)
                };
                if direction != (0, 0) {
                    self.launch(p, direction, Visual::Fireball);
                }
            }
        }
        self.player.turns = self.player.turns.saturating_add(1);
        if self.player.turns % 256 == 0 {
            if self.player.max_hp < 180 {
                self.player.max_hp += 1;
            }
            self.say("ow_moreturn");
        }
    }
    pub(super) fn flash(&mut self, p: Position, visual: Visual) {
        if self.animations.len() < 32 {
            self.animations.push(Animation {
                position: p,
                visual,
                direction: (0, 0),
                steps: 0,
                remaining_ms: 500,
            });
        }
    }
    pub(super) fn launch(&mut self, p: Position, direction: (i32, i32), visual: Visual) -> bool {
        let next = p.offset(direction.0, direction.1);
        if self.occupied(next) || self.animations.len() >= 32 {
            return false;
        }
        self.animations.push(Animation {
            position: next,
            visual,
            direction,
            steps: if visual == Visual::Missile { 7 } else { 8 },
            remaining_ms: 400,
        });
        true
    }
    fn hit_projectile(&mut self, index: usize, rng: &mut impl Rng) -> bool {
        let animation = &self.animations[index];
        if !matches!(animation.visual, Visual::Missile | Visual::Fireball) {
            return false;
        }
        let victim = if animation.position == self.player.position {
            Some(0)
        } else {
            self.actor_at(animation.position).map(|(id, _)| id)
        };
        let Some(id) = victim else {
            return false;
        };
        let animation = self.animations.remove(index);
        if id == 0 || self.actors[id].as_ref().unwrap().kind != Kind::Dragon {
            let (dice, cause) = if animation.visual == Visual::Fireball {
                (Dice(3, 8, 3), "ow_spell_1")
            } else {
                (Dice(1, 8, 2), "ow_spell_2")
            };
            self.damage(id, dice.roll(rng), false, cause, rng);
        }
        true
    }

    /// Projectile events run in time order, independent of how the caller batches elapsed time.
    pub fn advance(&mut self, milliseconds: u64, rng: &mut impl Rng) -> Option<Outcome> {
        if self.phase != Phase::Playing {
            return None;
        }
        let mut remaining = milliseconds;
        while self.phase == Phase::Playing {
            // A hit can spawn slime onto another projectile, including an earlier entry.
            let mut index = 0;
            while index < self.animations.len() && self.phase == Phase::Playing {
                if self.hit_projectile(index, rng) {
                    index = 0;
                } else {
                    index += 1;
                }
            }
            if self.phase != Phase::Playing {
                break;
            }
            let Some(next) = self.animations.iter().map(|a| a.remaining_ms).min() else {
                break;
            };
            let delta = remaining.min(next);
            for animation in &mut self.animations {
                animation.remaining_ms -= delta;
            }
            remaining -= delta;
            if delta < next {
                break;
            }
            // Resolve one due event at a time, retaining stable order for ties.
            let index = self
                .animations
                .iter()
                .position(|a| a.remaining_ms == 0)
                .unwrap();
            if self.animations[index].steps == 0 {
                self.animations.remove(index);
            } else {
                let animation = &mut self.animations[index];
                animation.steps -= 1;
                animation.position = animation
                    .position
                    .offset(animation.direction.0, animation.direction.1);
                animation.remaining_ms = 400;
                self.hit_projectile(index, rng);
            }
        }
        self.outcome(Phase::Playing)
    }
}
