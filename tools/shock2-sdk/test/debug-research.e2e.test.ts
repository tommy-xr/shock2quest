import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { clickUiElement } from "./helpers/ui.js";

const specimens = [-1095, -1341, -148, -229, -220, -28];
const chemicals = [-20, -139, -143, -145, -135, -130, -144, -141, -138, -137, -140, -129, -146, -981, -979, -131, -980, -142, -136];

for (const vr of [false, true]) test(`research lab supplies, interaction, and reset (${vr ? "VR" : "flat"})`, {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: vr ? ["--vr"] : [] });
  await game.step({ frames: 60 });
  assert.equal((await game.info()).player.stats?.skills.research, 6);
  for (const template of [...specimens, ...chemicals]) {
    const matches = await game.entities.byTemplate(template);
    assert.equal(matches.length, 1, `one station for ${template}`);
    assert.ok((await game.scene.objects({ entityId: matches[0].id })).objects.length > 0,
      `station ${template} renders its real model`);
    const properties = (await game.entities.detail(matches[0].id)).properties;
    if (chemicals.includes(template)) {
      assert.equal(Number(properties.find(p => p.name === "StackCount")?.value), 5);
    } else {
      assert.equal(properties.find(p => p.name === "ObjectState")?.value, "Unresearched");
    }
  }
  for (let cycle = 0; cycle < 2; cycle++) {
    const [organ] = await game.entities.byTemplate(-1095);
    await game.player.teleport({ x: organ.position[0] + 1, y: 1.25, z: 0 });
    if (vr) {
      await aimVrHandAt(game, organ.position, .2, 0);
      await game.input.set("right_hand.squeeze", 1);
      await game.step({ frames: 3 });
      assert.equal((await game.info()).player.right_hand_entity_id, organ.id);
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 1 });
      await game.input.set("right_hand.trigger", 0);
    } else {
      await game.player.aimAt(organ, { hitbox: "center", visibility: "required" });
      await game.input.set("right_hand.squeeze", 1);
      await game.step({ frames: 1 });
      await game.input.set("right_hand.squeeze", 0);
      await game.step({ frames: 2 });
      assert.ok((await game.player.inventory()).items.some(i => i.entity_id === organ.id));
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 2 });
      const item = (await game.ui.state()).strip?.elements.find(e => e.entity_id === organ.id);
      assert.ok(item, "the specimen is available in the inventory strip");
      await clickUiElement(game, item);
      await clickUiElement(game, item);
    }
    const sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await game.step({ frames: 300 });
    assert.equal((await game.audio.recent()).sounds.filter(s => s.sequence > sequence && s.sample === "bb06").length, 1,
      "the fresh specimen completes through ordinary research");
    if (cycle === 0) {
      // Every station must actually support research, not merely carry its
      // data: some retail specimens use special scripts we do not implement.
      for (const template of specimens.filter(t => t !== -1095 && t !== -1341).concat(-1341)) {
        const [sample] = await game.entities.byTemplate(template);
        await game.player.give(sample.id);
        await game.entities.sendMessage(sample.id, { type: "Frob" });
        await game.step({ frames: 2 });
        assert.equal((await game.ui.state()).active_panel?.entity_id, sample.id,
          `${sample.name} opens its research panel`);
      }
      // Toxin is now active; consume one dose from the lab's actual Sb stack.
      await game.step({ frames: 150 });
      const [antimony] = await game.entities.byTemplate(-145);
      await game.player.give(antimony.id);
      await game.entities.sendMessage(antimony.id, { type: "Frob" });
      await game.step({ frames: 2 });
      assert.equal(Number((await game.entities.detail(antimony.id)).properties.find(p => p.name === "StackCount")?.value), 4);
      await game.input.set("right_hand.squeeze", 0);
      await game.input.set("right_hand.position", [0, 5, 0]);
      await game.input.trigger("DebugReloadLevel");
      await game.step({ frames: 60 });
      assert.equal((await game.player.inventory()).count, 0);
      assert.equal((await game.info()).player.right_hand_entity_id, null);
      for (const template of [...specimens, ...chemicals]) {
        const matches = await game.entities.byTemplate(template);
        assert.equal(matches.length, 1, "reset replenishes every station");
        const properties = (await game.entities.detail(matches[0].id)).properties;
        if (chemicals.includes(template)) {
          assert.equal(Number(properties.find(p => p.name === "StackCount")?.value), 5, "reset restores chemical doses");
        } else {
          assert.equal(properties.find(p => p.name === "ObjectState")?.value, "Unresearched");
        }
      }
    }
  }
});
