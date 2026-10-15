use super::*;
use rand::{SeedableRng, rngs::StdRng};

fn arena() -> (OverWorld, StdRng) {
    let mut rng = StdRng::seed_from_u64(7);
    let mut game = OverWorld::new(false, &mut rng);
    game.actors.fill(None);
    for p in Map::positions() {
        *game.map.tile_mut(p) = Tile::default();
    }
    game.player.position = Position { x: 10, y: 10 };
    game.player.hp = 100;
    game.player.max_hp = 100;
    (game, rng)
}

#[test]
fn generated_worlds_have_reachable_unique_objectives_and_valid_occupants() {
    for seed in 0..64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let game = OverWorld::new(seed % 2 == 0, &mut rng);
        assert_eq!(game.map.towns.len(), 14);
        let reachable = game.map.reachable(game.player.position);
        for (i, town) in game.map.towns.iter().enumerate() {
            assert_eq!(game.map.tile(town.position).town, Some(i));
            assert!(
                game.map.is_reachable(&reachable, town.position),
                "seed {seed}, town {i}"
            );
        }
        for p in game.map.shrines {
            assert_eq!(game.map.tile(p).terrain, Terrain::Shrine);
            assert!(game.map.is_reachable(&reachable, p));
        }
        let mut positions = std::collections::HashSet::new();
        for (_, a) in game.actors() {
            assert!(positions.insert((a.position.x, a.position.y)));
            assert_ne!(a.position, game.player.position);
        }
        assert_eq!(
            game.actors()
                .filter(|(_, a)| a.kind == Kind::Dragon)
                .count(),
            1
        );
        assert_eq!(game.player.potions, u16::from(seed % 2 == 0));
        assert!(game.map.is_reachable(&reachable, game.exotic.unwrap()));
        let dragon = game
            .actors()
            .find(|(_, a)| a.kind == Kind::Dragon)
            .unwrap()
            .1;
        assert!(game.map.can_reach_neighbor(&reachable, dragon.position));
    }
}

#[test]
fn movement_wraps_and_blocked_terrain_still_costs_a_turn() {
    let (mut game, mut rng) = arena();
    game.player.position = Position { x: 0, y: 0 };
    game.act(Action::Move(-1, -1), false, &mut rng);
    assert_eq!(game.player.position, Position { x: 63, y: 63 });
    let blocked = game.player.position.offset(1, 0);
    game.map.tile_mut(blocked).terrain = Terrain::Water;
    let turns = game.player.turns;
    game.act(Action::Move(1, 0), false, &mut rng);
    assert_eq!(game.player.position, Position { x: 63, y: 63 });
    assert_eq!(game.player.turns, turns + 1);
    assert_eq!(
        Position { x: 63, y: 0 }.delta(Position { x: 0, y: 63 }),
        (1, -1)
    );
}

#[test]
fn equipment_upgrades_do_not_stack_and_potions_are_consumed_once() {
    let (mut game, mut rng) = arena();
    for loot in [
        Loot::Shield,
        Loot::Armor,
        Loot::Shield,
        Loot::Sword,
        Loot::Hoe,
        Loot::Sword,
        Loot::GoldChest,
        Loot::Potion,
    ] {
        let p = game.player.position.offset(1, 0);
        game.map.tile_mut(p).loot = Some(loot);
        game.act(Action::Move(1, 0), false, &mut rng);
        assert_eq!(game.map.tile(p).loot, None);
    }
    assert_eq!(game.player.armor(), 10);
    assert_eq!(game.player.damage_dice(), Dice(2, 4, 11));
    assert_eq!(game.player.gold, 17);
    game.player.hp = 50;
    game.player.healing = 64;
    game.act(Action::Potion, false, &mut rng);
    assert_eq!(game.player.hp, 60);
    assert_eq!(game.player.potions, 0);
    game.act(Action::Potion, false, &mut rng);
    assert_eq!(game.player.hp, 60);
}

#[test]
fn city_quest_healing_and_shrine_prayer_progress_without_free_levels() {
    let (mut game, mut rng) = arena();
    let p = game.player.position;
    let t = game.map.tile_mut(p);
    t.terrain = Terrain::City;
    t.town = Some(0);
    game.player.hp = 50;
    game.player.gold = 5;
    game.player.healing = 64;
    game.act(Action::Wait, false, &mut rng);
    assert_eq!(game.player.hp, 60);
    assert_eq!(game.player.gold, 0);
    let target = game.player.quest.unwrap();
    assert_ne!(target, 0);
    game.map.tile_mut(p).town = Some(target);
    game.act(Action::Wait, false, &mut rng);
    assert_eq!(game.player.quests, 1);
    assert_eq!(game.player.xp, 5);
    assert_eq!(game.player.quest, None);
    game.player.xp = 16;
    game.map.tile_mut(p).terrain = Terrain::Shrine;
    game.act(Action::Wait, false, &mut rng);
    game.act(Action::Wait, false, &mut rng);
    assert_eq!(game.player.level, 1);
    game.act(Action::Move(1, 0), false, &mut rng);
    assert_eq!(game.player.prayer, 0);
    game.act(Action::Move(-1, 0), false, &mut rng);
    game.act(Action::Wait, false, &mut rng);
    game.act(Action::Wait, false, &mut rng);
    assert_eq!(game.player.level, 2);
    assert_eq!(level_for_xp(15), 1);
    assert_eq!(level_for_xp(16), 2);
    assert_eq!(level_for_xp(40), 3);
}

#[test]
fn combat_requires_a_hit_and_preserves_npc_immunity() {
    let (mut game, mut rng) = arena();
    let p = game.player.position.offset(1, 0);
    game.spawn(2, Kind::Goblin, p, Behavior::Guard, &mut rng);
    game.actors[2].as_mut().unwrap().hp = 1;
    game.player.hit = Dice(0, 0, 0);
    game.combat(None, 2, &mut rng);
    assert!(game.actors[2].is_some());
    assert_eq!(game.player.xp, 0);
    game.player.hit = Dice(0, 0, 100);
    game.combat(None, 2, &mut rng);
    assert!(game.actors[2].is_none());
    assert_eq!(game.player.xp, 3);
    game.spawn(2, Kind::Merchant, p, Behavior::Talk, &mut rng);
    let hp = game.actors[2].as_ref().unwrap().hp;
    game.damage(2, 10000, true, "test", &mut rng);
    assert_eq!(game.actors[2].as_ref().unwrap().hp, hp);
    game.player.gold = 50;
    game.act(Action::Move(1, 0), false, &mut rng);
    assert_eq!(game.player.potions, 0);
    game.act(Action::Move(1, 0), false, &mut rng);
    assert_eq!(game.player.potions, 1);
    assert_eq!(game.player.gold, 0);
}

#[test]
fn first_dragon_lord_temple_and_four_elemental_dragons_produce_one_victory() {
    let (mut game, mut rng) = arena();
    game.player.hit = Dice(0, 0, 100);
    game.player.damage = Dice(0, 0, 1000);
    game.map.shrines = [Position { x: 12, y: 10 }, Position { x: 15, y: 10 }];
    for p in game.map.shrines {
        game.map.tile_mut(p).terrain = Terrain::Shrine;
    }
    game.spawn(
        1,
        Kind::Dragon,
        Position { x: 11, y: 10 },
        Behavior::Guard,
        &mut rng,
    );
    assert_eq!(game.act(Action::Move(1, 0), false, &mut rng), None);
    assert_eq!(game.player.dragons, 1);
    let lord = game
        .actors()
        .find(|(_, a)| a.kind == Kind::Lord)
        .unwrap()
        .1
        .position;
    game.player.position = lord.offset(-1, 0);
    game.act(Action::Move(1, 0), false, &mut rng);
    assert_eq!(game.map.tile(game.map.shrines[1]).terrain, Terrain::Temple);
    game.player.position = game.map.shrines[1];
    game.act(Action::Wait, false, &mut rng);
    let dragons: Vec<_> = game
        .actors()
        .filter(|(_, a)| a.kind == Kind::Dragon)
        .map(|(id, a)| (id, a.element))
        .collect();
    assert_eq!(dragons.len(), 4);
    assert!(dragons.iter().all(|(_, e)| e.is_some()));
    for (n, (id, _)) in dragons.into_iter().enumerate() {
        let p = game.actors[id].as_ref().unwrap().position;
        game.player.position = p.offset(-1, 0);
        game.map.tile_mut(game.player.position).terrain = Terrain::Plain;
        let result = game.act(Action::Move(1, 0), false, &mut rng);
        if n == 3 {
            assert!(result.unwrap().won);
        } else {
            assert_eq!(result, None);
        }
    }
    assert_eq!(game.phase, Phase::Won);
    assert_eq!(game.player.dragons, 5);
    let before = game.clone();
    assert_eq!(game.act(Action::Wait, true, &mut rng), None);
    assert_eq!(game.advance(10000, &mut rng), None);
    assert_eq!(game, before);
    let mut restored: OverWorld =
        serde_json::from_str(&serde_json::to_string(&game).unwrap()).unwrap();
    assert_eq!(restored.act(Action::Move(1, 0), true, &mut rng), None);
}

#[test]
fn game_over_is_terminal_and_does_not_run_remaining_monsters() {
    let (mut game, mut rng) = arena();
    game.player.hp = 1;
    game.player.max_hp = 1;
    for (id, (x, y)) in (2..6).zip([(-1, 0), (1, 0), (0, -1), (0, 1)]) {
        game.spawn(
            id,
            Kind::Demon,
            game.player.position.offset(x, y),
            Behavior::Attack,
            &mut rng,
        );
    }
    let mut result = None;
    for _ in 0..100 {
        result = game.act(Action::Wait, false, &mut rng);
        if result.is_some() {
            break;
        }
    }
    assert!(!result.unwrap().won);
    assert_eq!(game.player.hp, 0);
    let turns = game.player.turns;
    assert_eq!(game.act(Action::Potion, true, &mut rng), None);
    assert_eq!(game.player.turns, turns);
}

#[test]
fn missiles_require_hacking_cool_down_and_advance_in_elapsed_time() {
    let (mut game, mut rng) = arena();
    let before = game.clone();
    assert_eq!(game.act(Action::Missile(1, 0), false, &mut rng), None);
    assert_eq!(game, before);
    game.act(Action::Missile(1, 0), true, &mut rng);
    assert_eq!(game.animations.len(), 1);
    assert_eq!(game.player.spell_cooldown, 63);
    let target = game.player.position.offset(3, 0);
    game.spawn(2, Kind::Goblin, target, Behavior::Guard, &mut rng);
    game.actors[2].as_mut().unwrap().hp = 1;
    game.advance(399, &mut rng);
    assert_eq!(
        game.animations[0].position,
        game.player.position.offset(1, 0)
    );
    game.advance(1, &mut rng);
    assert_eq!(
        game.animations[0].position,
        game.player.position.offset(2, 0)
    );
    game.advance(400, &mut rng);
    assert!(game.actors[2].is_none());
    assert!(game.animations.is_empty());
    // Magic does not grant melee XP, and dragons are immune to both projectile types.
    assert_eq!(game.player.xp, 0);
    game.spawn(1, Kind::Dragon, target, Behavior::Guard, &mut rng);
    let hp = game.actors[1].as_ref().unwrap().hp;
    game.launch(game.player.position, (1, 0), Visual::Missile);
    game.advance(800, &mut rng);
    assert_eq!(game.actors[1].as_ref().unwrap().hp, hp);
}

#[test]
fn serialized_game_resumes_identically_with_the_same_random_stream() {
    let mut rng = StdRng::seed_from_u64(81);
    let mut game = OverWorld::new(true, &mut rng);
    game.act(Action::Missile(1, 0), true, &mut rng);
    game.advance(123, &mut rng);
    let mut restored: OverWorld =
        serde_json::from_str(&serde_json::to_string(&game).unwrap()).unwrap();
    let mut other_rng = rng.clone();
    for i in 0..80 {
        let action = match i % 4 {
            0 => Action::Move(1, 0),
            1 => Action::Wait,
            2 => Action::Move(0, -1),
            _ => Action::Potion,
        };
        assert_eq!(
            game.act(action, true, &mut rng),
            restored.act(action, true, &mut other_rng)
        );
        assert_eq!(
            game.advance(250, &mut rng),
            restored.advance(250, &mut other_rng)
        );
        assert_eq!(game, restored);
    }
}

#[test]
fn inspecting_exotic_armor_is_free_but_requires_proximity_and_level() {
    let (mut game, mut rng) = arena();
    let p = game.player.position.offset(1, 0);
    game.exotic = Some(p);
    game.act(Action::Inspect(p), false, &mut rng);
    assert_eq!(game.map.tile(p).loot, None);
    game.player.level = 10;
    game.act(Action::Inspect(p), false, &mut rng);
    assert_eq!(game.map.tile(p).loot, Some(Loot::Armor));
    assert_eq!(game.exotic, None);
    assert_eq!(game.player.turns, 0);
    game.act(Action::Move(1, 0), false, &mut rng);
    assert_eq!(game.player.armor(), 10);
}

#[test]
fn projectile_results_do_not_depend_on_frame_batching() {
    let (mut game, mut rng) = arena();
    game.launch(game.player.position.offset(-3, 0), (1, 0), Visual::Fireball);
    game.launch(game.player.position.offset(0, 2), (0, -1), Visual::Missile);
    let mut small = game.clone();
    let mut small_rng = rng.clone();
    let large_result = game.advance(1200, &mut rng);
    let mut small_result = None;
    for _ in 0..3 {
        small_result = small.advance(400, &mut small_rng).or(small_result);
    }
    assert_eq!(large_result, small_result);
    assert_eq!(game, small);
    // Actor movement into an existing projectile must also be resolved before it moves away.
    let (mut game, mut rng) = arena();
    game.launch(game.player.position.offset(-2, 0), (1, 0), Visual::Fireball);
    game.act(Action::Move(-1, 0), false, &mut rng);
    let hp = game.player.hp;
    game.advance(0, &mut rng);
    assert!(game.player.hp < hp);
    assert!(game.animations.is_empty());
}

#[test]
fn elemental_repainting_cannot_cut_a_bridge_or_erase_a_destination() {
    let (mut game, _) = arena();
    for p in Map::positions() {
        game.map.tile_mut(p).terrain = Terrain::Water;
    }
    for x in 10..=15 {
        game.map.tile_mut(Position { x, y: 10 }).terrain = Terrain::Road;
    }
    let bridge = Position { x: 12, y: 10 };
    game.map
        .repaint(bridge, Terrain::Mountain, game.player.position);
    assert_eq!(game.map.tile(bridge).terrain, Terrain::Road);
    game.map.tile_mut(bridge).terrain = Terrain::Temple;
    game.map
        .repaint(bridge, Terrain::Forest, game.player.position);
    assert_eq!(game.map.tile(bridge).terrain, Terrain::Temple);
}

#[test]
fn isolated_islands_do_not_receive_npcs_or_dragon_spawns() {
    let (mut game, mut rng) = arena();
    for p in Map::positions() {
        game.map.tile_mut(p).terrain = Terrain::Water;
    }
    for y in 8..=12 {
        for x in 8..=12 {
            game.map.tile_mut(Position { x, y }).terrain = Terrain::Plain;
        }
    }
    game.map.tile_mut(Position { x: 40, y: 40 }).terrain = Terrain::Plain;
    let reachable = game.map.reachable(game.player.position);
    for _ in 0..50 {
        let p = game.near_terrain(Terrain::Water, false, &mut rng).unwrap();
        assert!(game.map.is_reachable(&reachable, p));
        let dragon = game
            .find_square(Kind::Dragon, false, None, &mut rng)
            .unwrap();
        assert!(game.map.can_reach_neighbor(&reachable, dragon));
        let npc = game.find_square(Kind::Wise, false, None, &mut rng).unwrap();
        assert!(game.map.is_reachable(&reachable, npc));
    }
    game.spawn(
        1,
        Kind::Dragon,
        Position { x: 12, y: 10 },
        Behavior::Wander,
        &mut rng,
    );
    for _ in 0..30 {
        game.move_actor(1, Position { x: 40, y: 40 });
        assert!(
            game.map
                .can_reach_neighbor(&reachable, game.actors[1].as_ref().unwrap().position)
        );
    }
}

#[test]
fn quest_actors_survive_population_pressure() {
    let (mut game, mut rng) = arena();
    for id in 2..128 {
        let p = game
            .find_square(Kind::Goblin, true, None, &mut rng)
            .unwrap();
        game.spawn(id, Kind::Goblin, p, Behavior::Wander, &mut rng);
    }
    assert_eq!(game.free_slot(), None);
    game.place_quest_actor(Kind::Lord, game.map.shrines[0], &mut rng);
    assert_eq!(
        game.actors().filter(|(_, a)| a.kind == Kind::Lord).count(),
        1
    );
    game.lord_stage = 1;
    game.map.tile_mut(game.player.position).terrain = Terrain::Temple;
    game.act(Action::Wait, false, &mut rng);
    assert_eq!(
        game.actors().filter(|(_, a)| a.element.is_some()).count(),
        4
    );
    assert_eq!(
        game.actors().filter(|(_, a)| a.kind == Kind::Lord).count(),
        1
    );
}

#[test]
fn slime_splits_resolve_new_projectile_overlaps_in_the_same_instant() {
    let (fixture, _) = arena();
    for seed in 0..64 {
        let mut game = fixture.clone();
        let mut rng = StdRng::seed_from_u64(seed);
        let slime = game.player.position.offset(3, 0);
        game.spawn(2, Kind::Slime, slime, Behavior::Guard, &mut rng);
        game.actors[2].as_mut().unwrap().hp = 1;
        game.launch(slime.offset(-2, 0), (1, 0), Visual::Missile);
        game.launch(slime.offset(1, -1), (0, 1), Visual::Missile);
        game.animations[1].remaining_ms = 800;
        let mut split = game.clone();
        let mut split_rng = rng.clone();
        game.advance(800, &mut rng);
        split.advance(400, &mut split_rng);
        split.advance(400, &mut split_rng);
        assert_eq!(game, split, "seed {seed}");
    }
}
