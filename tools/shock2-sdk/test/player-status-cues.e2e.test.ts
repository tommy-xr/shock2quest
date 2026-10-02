import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  const presentation = vr ? "VR" : "flat";
  test(`low psi announces once when spending crosses twenty percent (${presentation})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 10 });
    if (vr) {
      const [amp] = await game.entities.byTemplate(-247);
      await aimVrHandAt(game, amp.position, .35, 1);
      await game.step({ frames: 8 });
    }
    const full = (await game.info()).player.psi_points!;
    const threshold = Math.floor(full / 5);
    const sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    const warnings = async () => (await game.audio.recent()).sounds.filter(s => s.sequence > sequence && s.sample === "bb01");
    for (let i = full; i > threshold + 1; i--) await pullTrigger(game);
    assert.equal((await warnings()).length, 0);
    await pullTrigger(game);
    assert.equal((await game.info()).player.psi_points, threshold);
    assert.equal((await warnings()).length, 1, "crossing the threshold announces low psi");
    await pullTrigger(game);
    await game.step({ frames: 120 });
    assert.equal((await warnings()).length, 1, "staying below the threshold does not repeat");
  });

  test(`research announces each chemical gate and completion once (${presentation})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 10 });
    await game.player.setStats({ skills: { research: 6 } });
    const organ = await game.player.spawnItem(-1095);
    await game.entities.sendMessage(organ.entity_id, { type: "Frob" });
    await game.step({ frames: 300 });
    assert.equal((await game.audio.recent()).sounds.filter(s => s.sample === "bb06").length, 1,
      "a project without chemicals also announces completion");
    await game.entities.sendMessage(organ.entity_id, { type: "Frob" });
    await game.step({ frames: 30 });
    assert.equal((await game.audio.recent()).sounds.filter(s => s.sample === "bb06").length, 1);
    const toxin = await game.player.spawnItem(-1341);
    const sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    const cues = async (sample: string) => (await game.audio.recent()).sounds.filter(s => s.sequence > sequence && s.sample === sample).length;
    await game.entities.sendMessage(toxin.entity_id, { type: "Frob" });
    await game.step({ frames: 150 });
    assert.equal(await cues("bb05"), 1, "first gate announces chemical needed");
    // Reopening the panel and waiting at a blocked gate must remain quiet.
    await game.entities.sendMessage(toxin.entity_id, { type: "Frob" });
    await game.step({ frames: 150 });
    assert.equal(await cues("bb05"), 1);
    const wrong = await game.player.spawnItem(-139);
    await game.entities.sendMessage(wrong.entity_id, { type: "Frob" });
    await game.step({ frames: 2 });
    assert.equal(await cues("bb05"), 1, "wrong chemicals do not repeat the request");
    for (const [index, template] of [-145, -139, -145].entries()) {
      const chemical = await game.player.spawnItem(template);
      await game.entities.sendMessage(chemical.entity_id, { type: "Frob" });
      await game.step({ frames: 1500 });
      assert.equal(await cues("bb05"), Math.min(index + 2, 3));
    }
    assert.equal(await cues("bb06"), 1, "completion announces once");
    await game.entities.sendMessage(toxin.entity_id, { type: "Frob" });
    await game.step({ frames: 150 });
    assert.equal(await cues("bb06"), 1);
  });
}
