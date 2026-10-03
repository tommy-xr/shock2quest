import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type Vec3 } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { fireOnce } from "./helpers/weapon.js";
const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) {
  test(`Metacreative Barrier blocks, takes damage, expires and refuses overlap (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 10 });
      if (vr) {
        const [amp] = await game.entities.byTemplate(-247);
        await aimVrHandAt(game, amp.position, .35);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
        const p = (await game.info()).player.position;
        await aimVrHandAt(game, [p[0] - 5, p[1] + 1, p[2]], 3.5, 1);
      }
      await selectPsiPower(game, "ForceWall");
      const before = (await game.info()).player;
      await fireOnce(game);
      await game.step({ frames: 3 });
      const [wall] = await game.entities.byTemplate(-3450);
      assert.ok(wall, "casting creates the authored ForceWallStructure");
      assert.equal((await game.info()).player.psi_points, before.psi_points! - 5);
      const bodies = (await game.physics.bodies({ entityId: wall.id })).bodies;
      assert.ok(bodies.some(b => b.blocks_player && b.blocks_actor && !b.is_sensor && b.body_type !== "dynamic"));
      const hp = (await game.entities.detail(wall.id)).properties.find(p => p.name === "HitPoints")?.value;
      assert.equal(Number(hp), 150 + 50 * Math.max(0, before.stats!.psionic_ability - 5));
      const start: Vec3 = [before.position[0], wall.position[1], before.position[2]];
      const end: Vec3 = [wall.position[0] * 2 - start[0], wall.position[1], wall.position[2] * 2 - start[2]];
      const ray = await game.raycast({ start, end, collision_groups: ["entity", "world"] });
      assert.equal(ray.entity_id, wall.id, "the same solid collider blocks incoming fire");
      await fireOnce(game);
      assert.equal((await game.entities.byTemplate(-3450)).length, 1, "overlap refuses a second wall");
      assert.equal((await game.info()).player.psi_points, before.psi_points! - 5, "rejected placement is free");
      await selectPsiPower(game, "Cryokinesis");
      await fireOnce(game);
      await game.step({ frames: 30 });
      const shotHp = Number((await game.entities.detail(wall.id)).properties.find(p => p.name === "HitPoints")?.value);
      assert.ok(shotHp < Number(hp), "an actual cryokinesis projectile hits and damages the wall");
      await selectPsiPower(game, "ForceWall");
      await game.input.set("head.look", [0, 0]);
      await game.input.set("right_hand.thumbstick", [0, 1]);
      await game.step({ frames: 120 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      const stopped = (await game.info()).player.position;
      assert.ok(stopped[0] > wall.position[0], `player stops before wall: ${stopped} / ${wall.position}`);
      await game.entities.sendMessage(wall.id, { type: "Damage", amount: 10 });
      await game.step({ frames: 1 });
      assert.equal(Number((await game.entities.detail(wall.id)).properties.find(p => p.name === "HitPoints")?.value), shotHp - 10);
      await game.entities.sendMessage(wall.id, { type: "Damage", amount: Number(hp) });
      await game.step({ frames: 2 });
      assert.equal((await game.entities.byTemplate(-3450)).length, 0);
      await game.input.set("right_hand.thumbstick", [0, 1]);
      await game.step({ frames: 25 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      assert.ok((await game.info()).player.position[0] < wall.position[0], "destroying the barrier clears the way");
      // Return to the same clear floor for the expiry/refusal checks.
      await game.player.teleport({ x: before.position[0], y: before.position[1], z: before.position[2] });
      await game.step({ frames: 2 });
      await fireOnce(game);
      assert.equal((await game.entities.byTemplate(-3450)).length, 1);
      for (let i = 0; i < 24; i++) await game.step({ frames: 600 });
      await game.step({ frames: 60 });
      assert.equal((await game.entities.byTemplate(-3450)).length, 0, "expires at four minutes");
      await game.player.teleport({ x: -13.5, y: before.position[1], z: 0 });
      await game.step({ frames: 2 });
      const obstructedPoints = (await game.info()).player.psi_points;
      await fireOnce(game);
      assert.equal((await game.entities.byTemplate(-3450)).length, 0, "refuses intersection with the backstop world geometry");
      assert.equal((await game.info()).player.psi_points, obstructedPoints);
    });
}

test("Metacreative Barrier retains health, solid physics and remaining lifetime through a mission save", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  const { readFile, writeFile, unlink } = await import("node:fs/promises");
  const { join } = await import("node:path");
  assert.ok(process.env.DARK_ASSET_PATH);
  const slot = `e2e_barrier_${Date.now()}`;
  const path = join(process.env.DARK_ASSET_PATH, "saves", `${slot}.sav`);
  await using game = await GameServer.launch({ mission: "earth.mis" });
  try {
    await game.step({ frames: 10 });
    await game.player.setStats({ psionic_ability: 6, psi_tier: 5 });
    await game.player.spawnItem("Psi Amp");
    await game.input.trigger("EquipPsiAmp");
    await game.step({ frames: 5 });
    await game.save(slot);
    const fixture = JSON.parse(await readFile(path, "utf8"));
    fixture.global_data.quest_info.player_stats.purchased_psi_powers = [-3162];
    fixture.global_data.player_vitals.psi_points.current = fixture.global_data.player_vitals.psi_points.maximum;
    await writeFile(path, JSON.stringify(fixture));
    await game.load(slot);
    await game.step({ frames: 5 });
    // A broad authored rooftop isolates serialization from the starting room's
    // low ceiling. Real mission geometry is required: generated scenes cannot save.
    await game.player.teleport({ x: 0, y: 54.5, z: 0 });
    await game.step({ frames: 60 });
    await selectPsiPower(game, "ForceWall");
    await fireOnce(game);
    await game.step({ frames: 3 });
    const [wall] = await game.entities.byTemplate(-3450);
    assert.ok(wall, "the isolated roof fixture has room for the authored barrier");
    await game.entities.sendMessage(wall.id, { type: "Damage", amount: 25 });
    await game.step({ frames: 1 });
    for (let i = 0; i < 23; i++) await game.step({ frames: 600 });
    await game.save(slot);
    await game.entities.sendMessage(wall.id, { type: "Damage", amount: 1000 });
    await game.step({ frames: 1 });
    assert.equal((await game.entities.byTemplate(-3450)).length, 0);
    await game.load(slot);
    await game.step({ frames: 1 });
    const [restored] = await game.entities.byTemplate(-3450);
    assert.ok(restored);
    assert.equal(Number((await game.entities.detail(restored.id)).properties.find(p => p.name === "HitPoints")?.value), 175);
    assert.ok((await game.physics.bodies({ entityId: restored.id })).bodies.some(b => b.blocks_player && !b.is_sensor));
    await game.step({ frames: 660 });
    assert.equal((await game.entities.byTemplate(-3450)).length, 0, "loading keeps the original expiry rather than restarting four minutes");
  } finally { await unlink(path).catch(() => {}); }
});
