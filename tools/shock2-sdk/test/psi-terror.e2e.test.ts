import assert from "node:assert/strict";
import { readFile, writeFile, unlink } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { fireOnce } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
const enabled = process.env.SHOCK2_E2E === "1";
async function prop(game: GameServer, id: number, name: string) {
  return (await game.entities.detail(id)).properties.find(p => p.name === name)?.value;
}
async function assertFx(game: GameServer, count: number) {
  assert.equal((await game.entities.byTemplate(-608)).length, count, "Stun Cloud lifetime follows the motion");
  assert.equal((await game.entities.byTemplate(-2406)).length, count, "Tinkling Lights lifetime follows the motion");
}
async function aim(game: GameServer, id: number, vr: boolean) {
  if (vr) {
    const target = await game.entities.detail(id);
    const torso = target.aim_points?.find(p => p.classification === "torso")?.position ?? target.position;
    await aimVrHandAt(game, torso, 2, 1);
  } else await game.player.aimAt(id, { hitbox: "torso", visibility: "required" });
  await game.step({ frames: 1 });
}
for (const vr of [false, true]) {
 test(`Terror impact, recast and recovery (${vr ? "VR" : "flat"})`, { skip: !enabled, timeout: 600_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
  await game.step({ frames: 10 });
  if (vr) {
    const [amp] = await game.entities.byTemplate(-247);
    await aimVrHandAt(game, amp.position, .35);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
  }
  await selectPsiPower(game, "Terror");
  await game.input.set("head.look", [0, 0]);
  await game.input.trigger("SpawnDebugMonster");
  await game.step({ frames: 2 });
  const [target] = await game.entities.byTemplate(-397);
  assert.ok(target);
  await aim(game, target.id, vr);
  const psi = (await game.info()).player.psi_points!;
  await fireOnce(game);
  await game.step({ frames: 60 });
  assert.equal((await game.info()).player.psi_points, psi - 3);
  assert.equal(Number(await prop(game, target.id, "HitPoints")), 12);
  assert.equal(await prop(game, target.id, "AIBehavior"), "Stunned");
  await assertFx(game, 1);
  assert.equal((await game.entities.animation(target.id))?.clip, "ogsrwnd3");
  await game.step({ frames: 540 });
  assert.equal(await prop(game, target.id, "AIBehavior"), "Stunned");
  await aim(game, target.id, vr);
  await fireOnce(game);
  await game.step({ frames: 720 });
  assert.equal((await game.info()).player.psi_points, psi - 6);
  assert.equal(await prop(game, target.id, "AIBehavior"), "Stunned", "recast replaces the 20-second timer");
  await assertFx(game, 1); // Recasting must not duplicate the attachments.
  await game.step({ frames: 660 });
  assert.notEqual(await prop(game, target.id, "AIBehavior"), "Stunned", "recovery after timer and motion boundary");
  assert.equal(Number(await prop(game, target.id, "HitPoints")), 12);
  await assertFx(game, 0);
  await aim(game, target.id, vr);
  await fireOnce(game);
  await game.step({ frames: 60 });
  await assertFx(game, 1);
  await game.entities.sendMessage(target.id, { type: "Damage", amount: 100 });
  await game.step({ frames: 3 });
  await assertFx(game, 0); // Death/corpse conversion cannot strand cosmetic children.
 });
}

test("Terror saves remaining time and robots ignore its real impact", { skip: !enabled, timeout: 600_000 }, async () => {
  assert.ok(process.env.DARK_ASSET_PATH);
  const slot = `e2e_terror_${Date.now()}`;
  const path = join(process.env.DARK_ASSET_PATH, "saves", `${slot}.sav`);
  await using game = await GameServer.launch({ mission: "earth.mis" });
  try {
    await game.step({ frames: 10 });
    await game.player.setStats({ psionic_ability: 6, psi_tier: 5 });
    await game.player.spawnItem("Psi Amp");
    await game.input.trigger("EquipPsiAmp");
    await game.step({ frames: 5 });
    await game.save(slot);
    // Provision learned power and currency only. Stun and the tested save below
    // are produced by the real cast, collision, receiver and script lifecycle.
    const fixture = JSON.parse(await readFile(path, "utf8"));
    fixture.global_data.quest_info.player_stats.purchased_psi_powers = [-3156];
    fixture.global_data.player_vitals.psi_points.current = fixture.global_data.player_vitals.psi_points.maximum;
    await writeFile(path, JSON.stringify(fixture));
    await game.load(slot);
    await game.step({ frames: 5 });
    await selectPsiPower(game, "Terror");
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 2 });
    let [target] = await game.entities.byTemplate(-397);
    await aim(game, target.id, false);
    await fireOnce(game);
    await game.step({ frames: 360 });
    assert.equal(await prop(game, target.id, "AIBehavior"), "Stunned");
    await assertFx(game, 1);
    await game.save(slot);
    await game.load(slot);
    await game.step({ frames: 1 });
    [target] = await game.entities.byTemplate(-397);
    assert.ok(target);
    assert.equal(await prop(game, target.id, "AIBehavior"), "Stunned");
    await assertFx(game, 1); // Recreated once from saved AI state.
    await game.step({ frames: 600 });
    assert.equal(await prop(game, target.id, "AIBehavior"), "Stunned");
    await game.step({ frames: 360 });
    assert.notEqual(await prop(game, target.id, "AIBehavior"), "Stunned", "saved remaining time is preserved, not restarted");

    await assertFx(game, 0);

    const [droid] = await game.entities.byTemplate(593);
    assert.ok(droid);
    const [x, y, z] = (await game.entities.detail(droid.id)).position;
    await game.player.teleport({ x: x + 4, y: y + 1, z });
    await game.step({ frames: 70 });
    await aim(game, droid.id, false);
    const hp = await prop(game, droid.id, "HitPoints");
    const psi = (await game.info()).player.psi_points!;
    await fireOnce(game);
    await game.step({ frames: 90 });
    assert.equal((await game.info()).player.psi_points, psi - 3);
    assert.equal(await prop(game, droid.id, "HitPoints"), hp);
    assert.notEqual(await prop(game, droid.id, "AIBehavior"), "Stunned", "Robots do not inherit Stunable");
    await assertFx(game, 0);
  } finally { await unlink(path).catch(() => {}); }
});
