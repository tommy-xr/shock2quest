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

async function stage(game: GameServer, guard: boolean, motion = "bh413001", hand: "left" | "right" = "right", weapon = "Wrench", playerX = -6, guardHeight?: number) {
  // Let the unarmed actors settle before bringing the player and wrench in.
  // Spawning the weapon first alerts the approaching hybrids prematurely.
  await game.step({ frames: 61 });
  const enemies = await game.entities.byTemplate(-397);
  enemies.sort((a, b) => b.position[0] - a.position[0]);
  const enemy = enemies[0];
  assert.ok(enemy, "debug_melee must contain a live pipe hybrid");
  await game.player.teleport({ x: playerX, y: 1.244, z: 0 });
  await game.input.set(`${hand}_hand.squeeze`, 1);
  // Keep long gun guards out of the way until the selected attack is playing;
  // windup parries can otherwise occur during setup, before PlayMotion.
  const guardPose: [number, number, number] = motion === "bh413004" ? [-1, 0.65, -0.4] : [-1, guardHeight ?? (weapon === "Wrench" ? -0.4 : 0), 0.4];
  await game.input.set(`${hand}_hand.position`, guard && weapon === "Wrench" ? guardPose : [-0.15, 0.2, -0.7]);
  const held = await game.player.spawnItem(weapon, { hand });
  await game.entities.sendMessage(enemy.id, { type: "SetAlertness", level: "High" });
  await game.step({ frames: 2 });
  // Pin the authored motion, not its damage: natural schema selection can
  // choose any of several attack directions. The live AI, animation flags,
  // swept contacts, damage, sound, and recovery all run normally.
  await game.entities.sendMessage(enemy.id, { type: "PlayMotion", name: motion });
  await game.step({ frames: 1 });
  assert.equal((await game.entities.animation(enemy.id))?.clip, motion);
  if (guard && weapon !== "Wrench") await game.input.set(`${hand}_hand.position`, guardPose);
  return { enemy, held };
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

async function gunState(game: GameServer, entity: number) {
  const properties = (await game.entities.detail(entity)).properties;
  return Object.fromEntries(properties.filter(p => ["Condition", "Ammo"].includes(p.name)).map(p => [p.name, Number(p.value)]));
}

for (const [condition, hand, physical] of [[100, "right", false], [100, "left", false], [2.5, "right", false], [0, "right", false], [100, "right", true]] as const) {
  test(`VR ${hand} shotgun parry charges condition once from ${condition} (physical=${physical})`, { skip: !enabled, timeout: 600_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_melee", debugFlags: physical ? ["--vr", "--experimental", "physical_held_items"] : ["--vr"] });
    const { enemy, held: gun } = await stage(game, true, "bh413001", hand, "Shotgun");
    await game.entities.sendMessage(gun.entity_id, { type: "SetGunCondition", condition });
    const otherHand = hand === "right" ? "left" : "right";
    await game.input.set(`${otherHand}_hand.squeeze`, 1);
    await game.input.set(`${otherHand}_hand.position`, [-0.15, 0.2, -0.7]);
    const other = await game.player.spawnItem("Pistol", { hand: otherHand });
    const initial = await gunState(game, gun.entity_id);
    const audioBefore = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    const hp = (await game.info()).player.hit_points!;
    for (let frame = 0; frame < 180; frame++) {
      await game.step({ frames: 1 });
      if ((await attackState(game, enemy.id))?.consumed) break;
    }
    assert.equal((await attackState(game, enemy.id))?.parries, condition > 0 ? 1 : 0);
    assert.equal((await game.info()).player.hit_points, condition > 0 ? hp : hp - 10);
    // Stay inside this attack's recoil: once it completes the live AI may
    // begin a separate attack, which legitimately costs another block.
    await game.step({ frames: 12 });
    const after = await gunState(game, gun.entity_id);
    assert.equal(after.Condition, Math.max(0, condition - 5));
    assert.equal(after.Ammo, initial.Ammo, "blocking cannot consume ammunition");
    assert.equal((await gunState(game, other.entity_id)).Condition, 100, "the other hand is not charged");
    if (condition > 0) {
      const clangs = (await game.audio.recent()).sounds.filter(s => s.sequence > audioBefore && /^hmetmet[34]$/.test(s.sample));
      assert.equal(clangs.length, 1, "gun parries must resolve one metal clang");
    }
  });
}

for (const placement of ["side", "dropped"] as const) {
  test(`a ${placement} shotgun neither blocks nor loses condition`, { skip: !enabled, timeout: 600_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_melee", debugFlags: ["--vr"] });
    const { enemy, held } = await stage(game, placement === "dropped", "bh413001", "right", "Shotgun");
    if (placement === "dropped") {
      await game.input.set("right_hand.squeeze", 0);
      await game.step({ frames: 1 });
      assert.equal((await game.info()).player.right_hand_entity_id, null);
    }
    const hp = (await game.info()).player.hit_points!;
    for (let frame = 0; frame < 180; frame++) {
      await game.step({ frames: 1 });
      if ((await attackState(game, enemy.id))?.consumed) break;
    }
    assert.equal((await attackState(game, enemy.id))?.parries, 0);
    assert.equal((await game.info()).player.hit_points, hp - 10);
    assert.equal((await gunState(game, held.entity_id)).Condition, 100);
  });
}

for (const weapon of ["Wrench", "Shotgun"]) {
  test(`a close ${weapon} guard intercepts the pipe during windup`, { skip: !enabled, timeout: 600_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_melee", debugFlags: ["--vr"] });
    const { enemy, held } = await stage(game, true, "bh413001", "right", weapon, -7, 0);
    const hp = (await game.info()).player.hit_points;
    let blockedAt: number | undefined;
    for (let tick = 0; tick < 84; tick++) {
      await game.step({ frames: 1 });
      if ((await attackState(game, enemy.id))?.parries === 1) {
        blockedAt = (await game.entities.animation(enemy.id))?.frame;
        break;
      }
    }
    // On the parent, the inactive pipe visibly crosses this guard near frame
    // 13 but is ignored. Frame 42 opens damage with the pipe past the guard.
    assert.ok(blockedAt !== undefined && blockedAt < 42, "visible windup contact must parry before the damage window");
    assert.equal((await game.info()).player.hit_points, hp);
    assert.equal((await game.entities.animation(enemy.id))?.recoil, true);
    // Check this recoil before the AI can start a separate attack.
    await game.step({ frames: 12 });
    assert.equal((await game.info()).player.hit_points, hp, "rewinding across the guard must not damage the player");
    assert.equal((await attackState(game, enemy.id))?.parries, 1);
    if (weapon === "Shotgun") assert.equal((await gunState(game, held.entity_id)).Condition, 95);
  });
}

test("an unguarded close pipe still waits for its damage window", { skip: !enabled, timeout: 600_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_melee", debugFlags: ["--vr"] });
  const { enemy } = await stage(game, false, "bh413001", "right", "Wrench", -7);
  const hp = (await game.info()).player.hit_points!;
  let damaged = false;
  for (let tick = 0; tick < 110; tick++) {
    await game.step({ frames: 1 });
    const animation = await game.entities.animation(enemy.id);
    const health = (await game.info()).player.hit_points;
    if (animation!.frame < 42) assert.equal(health, hp, "windup may parry but cannot hurt the player");
    if (health! < hp) { damaged = true; break; }
  }
  assert.ok(damaged, "missing the guard still permits the authored hit");
  assert.equal((await attackState(game, enemy.id))?.parries, 0);
  assert.equal((await game.info()).player.hit_points, hp - 10);
});

test("a moving wrench intercepts a close windup", { skip: !enabled, timeout: 600_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_melee", debugFlags: ["--vr"] });
  const { enemy } = await stage(game, true, "bh413001", "right", "Wrench", -7, 0);
  const hp = (await game.info()).player.hit_points;
  // Lower the guard continuously as the pipe winds up. This is a defensive
  // movement, slow enough not to strike/stagger the hybrid's body first.
  for (let step = 1; step <= 24; step++) {
    await game.input.set("right_hand.position", [-1, -0.2 * step / 24, 0.4]);
    await game.step({ frames: 1 });
    if ((await attackState(game, enemy.id))?.parries === 1) break;
  }
  assert.equal((await attackState(game, enemy.id))?.parries, 1, "a moving guard must intercept the windup");
  assert.equal((await game.info()).player.hit_points, hp);
});
