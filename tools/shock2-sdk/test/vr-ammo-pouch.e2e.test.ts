import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const hand of ["left", "right"] as const) {
  test(`${hand} pouch draws real reserve and returns it without duplication`, { skip: !enabled, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const gunHand = hand === "left" ? "right" : "left";
    const owner = hand === "left" ? "wielded_entity_id" : "right_hand_entity_id";
    const pistol = (await game.entities.list()).entities.find(e => e.template_id === -17);
    assert.ok(pistol);
    await aimVrHandAt(game, pistol.position, 0.2, 1, 0, { hand: gunHand });
    await game.step({ frames: 5 });
    const reserve = await game.player.spawnItem(-31);
    if (hand === "right") {
      let full = false;
      for (let i = 0; i < 50; i++) {
        try { await game.player.spawnItem(-1221); }
        catch (error) {
          assert.match(String(error), /could not add item to inventory/);
          full = true;
          break;
        }
      }
      assert.ok(full, "test must fill the backpack");
      await game.player.spawnItem(-31); // Matching ammo merges into the full pack.
    }
    await game.step({ frames: 5 });
    const player = (await game.info()).player;
    const center = player.hand_feedback?.ammo_pouch?.center;
    assert.ok(center);
    const i = hand === "left" ? 0 : 1;
    const offer = player.hand_feedback?.ammo_pouch?.offers[i];
    assert.ok(offer, "opposite gun must offer compatible reserve");
    assert.equal(offer.reserve, reserve.entity_id);
    await game.input.set(`${hand}_hand.position`, quatRotate(quatConjugate(player.rotation), sub(center, player.position)));
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player[owner], null, "reach alone must not draw");
    await game.input.set(`${hand}_hand.squeeze`, 1);
    await game.step({ frames: 8 });
    const drawn = (await game.info()).player[owner];
    assert.ok(drawn !== null);
    if (hand === "left") assert.equal(drawn, reserve.entity_id, "one clip moves the exact reserve entity");
    else {
      assert.ok(offer.stock > offer.rounds, "large reserve must be split");
      assert.notEqual(drawn, reserve.entity_id);
      const remaining = await game.entities.detail(reserve.entity_id);
      assert.equal(Number(remaining.properties.find(x => x.name === "StackCount")?.value), offer.stock - offer.rounds);
    }
    assert.equal((await game.player.inventory()).items.find(x => x.entity_id === drawn)?.location, `${hand}_hand`);
    await game.step({ frames: 20 });
    assert.equal((await game.info()).player[owner], drawn, "held squeeze does not duplicate");
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player[owner], null);
    assert.equal((await game.player.inventory()).items.filter(x => x.entity_id === reserve.entity_id && x.location === "inventory").length, 1);
    if (hand === "right") assert.ok(!(await game.entities.list()).entities.some(x => x.id === drawn), "returned split merges into the full pack");
    const detail = await game.entities.detail(reserve.entity_id);
    assert.equal(Number(detail.properties.find(x => x.name === "StackCount")?.value), offer.stock);
  });
}

test("empty pouch refuses selected ammo without grabbing nearby objects", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const pistol = (await game.entities.list()).entities.find(e => e.template_id === -17)!;
  await aimVrHandAt(game, pistol.position, 0.2, 1);
  await game.step({ frames: 5 });
  // An incompatible reserve must never be silently substituted.
  await game.player.spawnItem(-42);
  await game.step({ frames: 5 });
  const player = (await game.info()).player;
  const center = player.hand_feedback?.ammo_pouch?.center;
  assert.ok(center);
  assert.equal(player.hand_feedback?.ammo_pouch?.offers[0], null);
  const stock = await game.player.inventory();
  await game.input.set("left_hand.position", quatRotate(quatConjugate(player.rotation), sub(center, player.position)));
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 3 });
  const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  await game.input.set("left_hand.squeeze", 1);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.wielded_entity_id, null);
  assert.deepEqual(await game.player.inventory(), stock);
  const sounds = (await game.audio.recent()).sounds.filter(s => s.sequence > before);
  assert.ok(sounds.length > 0, "empty pouch must provide audible feedback");
});
