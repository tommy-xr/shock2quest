import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { tagValue } from "./helpers/audio.js";

const enabled = process.env.SHOCK2_E2E === "1";

interface PipeAttack {
  active: boolean;
  consumed: boolean;
  recovering: boolean;
  parries: number;
}

async function attackState(game: GameServer, entity: number): Promise<PipeAttack | undefined> {
  const property = (await game.entities.detail(entity)).properties.find(p => p.name === "PipeAttack");
  return property ? JSON.parse(property.value) as PipeAttack : undefined;
}

async function stage(game: GameServer, guard: boolean, motion = "bh413001", hand: "left" | "right" = "right") {
  // Let the unarmed actors settle before bringing the player and wrench in.
  // Spawning the weapon first alerts the approaching hybrids prematurely.
  await game.step({ frames: 61 });
  const enemies = await game.entities.byTemplate(-397);
  enemies.sort((a, b) => b.position[0] - a.position[0]);
  const enemy = enemies[0];
  assert.ok(enemy, "debug_melee must contain a live pipe hybrid");
  await game.player.teleport({ x: -6, y: 1.244, z: 0 });
  await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.input.set(`${hand}_hand.position`, guard ? (motion === "bh413004" ? [-1, 0.65, -0.4] : [-1, -0.4, 0.4]) : [-0.15, 0.2, -0.7]);
  const wrench = await game.player.spawnItem("Wrench", { hand });
  await game.entities.sendMessage(enemy.id, { type: "SetAlertness", level: "High" });
  await game.step({ frames: 2 });
  // Pin the authored motion, not its damage: natural schema selection can
  // choose any of several attack directions. The live AI, animation flags,
  // swept contacts, damage, sound, and recovery all run normally.
  await game.entities.sendMessage(enemy.id, { type: "PlayMotion", name: motion });
  await game.step({ frames: 1 });
  assert.equal((await game.entities.animation(enemy.id))?.clip, motion);
  return { enemy, wrench };
}

for (const [motion, hand] of [["bh413001", "right"], ["bh413004", "right"], ["bh413001", "left"]] as const) {
test(`a stationary ${hand} VR wrench blocks ${motion}, clangs once and recoils without damage`,
  { skip: !enabled, timeout: 600_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_melee", debugFlags: ["--vr"] });
    const { enemy } = await stage(game, true, motion, hand);
    const before = await game.info();
    const audioBefore = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    let blocked = false;
    for (let frame = 0; frame < 180; frame++) {
      await game.step({ frames: 1 });
      if ((await attackState(game, enemy.id))?.parries === 1) { blocked = true; break; }
    }
    // Negative-first: retail's active pipe visually crosses this guard in the
    // pre-change runtime, but HP drops 35 -> 25 and no recoil is reported.
    assert.ok(blocked, "the incoming pipe must meet the held wrench");
    assert.equal((await game.info()).player.hit_points, before.player.hit_points);
    const impact = await game.entities.animation(enemy.id);
    assert.ok(impact);
    assert.equal(impact.recoil, true);
    await game.step({ frames: 6 });
    const reversing = await game.entities.animation(enemy.id);
    assert.ok(reversing);
    assert.equal(reversing.clip, impact.clip);
    assert.ok(reversing.frame < impact.frame, "the pipe must travel back toward its opening pose");
    await game.step({ frames: 12 });
    assert.equal((await game.entities.animation(enemy.id))?.recoil, false);
    assert.equal((await game.info()).player.hit_points, before.player.hit_points, "recovery cannot deal deferred damage");
    assert.equal((await attackState(game, enemy.id))?.parries, 1, "sustained contact bills one block");
    const clangs = (await game.audio.recent()).sounds.filter(sound => sound.sequence > audioBefore
      && tagValue(sound, "event") === "collision" && tagValue(sound, "weapontype") === "wrench"
      && tagValue(sound, "material") === "metal");
    assert.equal(clangs.length, 1, "one parry must produce one metal clang");
    assert.match(clangs[0].sample, /^hmetmet[34]$/);
    const feedback = (await game.info()).player.hand_feedback?.haptics;
    assert.ok(feedback, "runtime must expose controller feedback");
    const slot = hand === "left" ? 0 : 1;
    assert.equal(feedback.sequence[slot], (before.player.hand_feedback?.haptics?.sequence[slot] ?? 0) + 1);
    assert.equal(feedback.sequence[1 - slot], before.player.hand_feedback?.haptics?.sequence[1 - slot] ?? 0);
  });

}

for (const placement of ["side", "dropped"] as const) {
test(`a ${placement} VR wrench does not block the player hit`,
  { skip: !enabled, timeout: 600_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_melee", debugFlags: ["--vr"] });
    const { enemy } = await stage(game, placement === "dropped");
    if (placement === "dropped") {
      await game.input.set("right_hand.squeeze", 0);
      await game.step({ frames: 1 });
      assert.equal((await game.info()).player.right_hand_entity_id, null);
    }
    const hp = (await game.info()).player.hit_points;
    assert.ok(hp !== null);
    let damaged = false;
    for (let frame = 0; frame < 180; frame++) {
      await game.step({ frames: 1 });
      if ((await game.info()).player.hit_points! < hp) { damaged = true; break; }
    }
    assert.ok(damaged, "an unblocked physical pipe must still reach and damage the player");
    const damagedHp = (await game.info()).player.hit_points;
    assert.equal(damagedHp, hp - 10, "contact uses the Lead Pipe's authored damage");
    await game.step({ frames: 10 });
    assert.equal((await game.info()).player.hit_points, damagedHp, "the active window cannot bill twice");
    assert.equal((await attackState(game, enemy.id))?.parries, 0);
    assert.equal((await game.entities.animation(enemy.id))?.recoil, false);
  });

}
