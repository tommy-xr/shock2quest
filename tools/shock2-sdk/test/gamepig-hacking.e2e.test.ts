import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";
import { winHack } from "./helpers/hack.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";
import { savedGamePath } from "./helpers/save.js";

for (const vr of [false, true]) {
  test(`GamePig paid hack unlocks its library and survives saves (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
    const { handheld, click } = await openGamePig(game, vr, "Swinekeeper");
    const elements = async () => (await game.ui.state()).active_panel!.elements;
    const button = async (label: string) => {
      const found = (await elements()).find(e => e.label === label);
      assert.ok(found, `missing ${label}`);
      return found;
    };
    const state = async () => (await game.entities.detail(handheld.entity_id)).properties.find(p => p.name === "ObjectState")?.value;
    const original = (await elements()).filter(e => e.label?.startsWith("game-")).map(e => e.label);
    assert.deepEqual(original, ["game-3"]);
    await click(game, await button("hack-gamepig"));
    assert.ok((await elements()).some(e => e.texture === "hack.pcx"));
    const empty = await carriedNaniteTotal(game);
    assert.ok(empty < 50);
    await click(game, await button("start-hack"));
    assert.equal(await carriedNaniteTotal(game), empty, "an unaffordable attempt charges nothing");
    assert.ok((await elements()).some(e => e.texture === "payh.pcx"));
    await click(game, await button("gamepig-back"));
    assert.deepEqual((await elements()).filter(e => e.label?.startsWith("game-")).map(e => e.label), original,
      "canceling an unpaid board unlocks nothing");

    await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });
    for (let i = 0; i < 25; i++) {
      const nanites = await game.player.spawnItem(-89);
      await game.entities.sendMessage(nanites.entity_id, { type: "Frob" });
      await game.step({ frames: 1 });
    }
    const wallet = await carriedNaniteTotal(game);
    assert.ok(wallet >= 400);
    if (vr) {
      // Only Swinekeeper is installed, so down selects the HACK row.
      await game.input.set("left_hand.thumbstick", [0, -1]);
      await game.step({ frames: 2 });
      await game.input.set("left_hand.thumbstick", [0, 0]);
      await game.input.trigger("LeftHandLowerButton");
      await game.step({ frames: 2 });
    } else await click(game, await button("hack-gamepig"));
    await button("start-hack");
    assert.equal(await carriedNaniteTotal(game), wallet, "opening is free");
    if (vr) {
      await game.input.trigger("LeftHandLowerButton");
      await game.step({ frames: 2 });
    } else await click(game, await button("start-hack"));
    assert.equal(await carriedNaniteTotal(game), wallet - 50, "START charges the authored 50 nanites");
    assert.notEqual(await state(), "Hacked", "payment is not a win");
    await winHack(game, click);
    assert.equal(await state(), "Hacked");
    await click(game, await button("gamepig-back"));
    assert.ok(!(await elements()).some(e => e.label === "hack-gamepig"), "a hacked device cannot charge again");
    for (const label of ["game-2", "game-3", "game-5", "game-8", "game-10"]) await button(label);
    assert.ok((await elements()).some(e => /overworld/i.test(e.text ?? "")), "unimplemented titles are also unlocked");

    const name = `gamepig_hack_${vr}_${Date.now()}`;
    try {
      const bits = () => JSON.parse(readFileSync(savedGamePath(name), "utf8")).global_data.quest_info.mini_games;
      await game.save(name);
      assert.equal(bits(), 0x1ffc, "all eleven actual games are installed without the empty Slots entry");
      const balance = await carriedNaniteTotal(game);
      await game.load(name);
      await game.step({ frames: 2 });
      await game.save(name);
      assert.equal(bits(), 0x1ffc);
      assert.equal(await carriedNaniteTotal(game), balance);
      const [restored] = await game.entities.byTemplate(-3864);
      assert.ok(restored);
      assert.equal((await game.entities.detail(restored.id)).properties.find(p => p.name === "ObjectState")?.value, "Hacked");
    } finally { rmSync(savedGamePath(name), { force: true }); }
  });
}
