import assert from "node:assert/strict";
import { test } from "node:test";
import { rmSync } from "node:fs";
import { GameServer } from "../src/index.js";
import { savedGamePath } from "./helpers/save.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) {
  test(`GamePig ${vr ? "VR" : "flat"}: cartridges install cumulatively and survive save/load and deck changes`,
    { skip: !enabled, timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 2 });
      const handheld = await game.player.spawnItem("Gameboy");
      await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
      await game.step({ frames: 2 });
      let ui = await game.ui.state();
      assert.ok(ui.active_panel, "the handheld opens its MFD");
      assert.ok(!ui.active_panel.elements.some(e => e.text?.includes("Recreation")), "GamePig must not display elevator stops");
      for (const cart of ["Swinekeeper", "TTT Cart", "Swinekeeper"]) {
        const item = await game.player.spawnItem(cart);
        await game.entities.sendMessage(item.entity_id, { type: "Frob" });
        await game.step({ frames: 2 });
        assert.ok(!(await game.entities.list()).entities.some(e => e.id === item.entity_id), "installing consumes even a duplicate cartridge");
      }
      await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
      await game.step({ frames: 2 });
      ui = await game.ui.state();
      const labels = () => ui.active_panel!.elements.map(e => e.text);
      assert.ok(labels().includes("Swinekeeper"));
      assert.ok(labels().includes("Tic-Tac-Triop"));
      assert.ok(!labels().includes("Golf"));
      const name = `gamepig_${vr}_${Date.now()}`;
      try {
        await game.save(name);
        await game.load(name);
        await game.step({ frames: 2 });
        const reloaded = (await game.entities.list({ filter: "Gameboy" })).entities.find(e => e.template_id === -3864);
        assert.ok(reloaded, "carried handheld survives reload");
        await game.entities.sendMessage(reloaded.id, { type: "Frob" });
        await game.step({ frames: 2 });
        ui = await game.ui.state();
        assert.ok(labels().includes("Swinekeeper"));
        assert.ok(labels().includes("Tic-Tac-Triop"));
        await game.transitionLevel("medsci2.mis");
        await game.step({ frames: 2 });
        const carried = (await game.entities.list({ filter: "Gameboy" })).entities.find(e => e.template_id === -3864);
        assert.ok(carried, "carried handheld survives a deck change");
        await game.entities.sendMessage(carried.id, { type: "Frob" });
        await game.step({ frames: 2 });
        ui = await game.ui.state();
        assert.ok(labels().includes("Swinekeeper"));
        assert.ok(labels().includes("Tic-Tac-Triop"));
      } finally { rmSync(savedGamePath(name), { force: true }); }
    });
}
