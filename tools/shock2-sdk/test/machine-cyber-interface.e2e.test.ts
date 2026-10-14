import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { installEarthCyberInterface } from "./helpers/cyber-interface.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement, requirePanelPose } from "./helpers/ui.js";
import { drawPersonalCard } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  for (const machine of ["trainer", "replicator"]) {
    test(`${machine} frob opens the cyber interface (${vr ? "VR" : "flat"})`,
      { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 }, async () => {
        await using game = await GameServer.launch({
          mission: machine === "trainer" ? "medsci1.mis" : "earth.mis",
          debugFlags: vr ? ["--vr"] : [],
        });
        await game.step({ frames: 5 });
        await game.devParams.set("cheat", 1);
        if (machine === "replicator") await installEarthCyberInterface(game);
        const machines = machine === "trainer"
          ? (await game.entities.list({ filter: "Trainer" })).entities
          : await game.entities.byTemplate(262);
        assert.equal(machines.length, machine === "trainer" ? 4 : 1);
        for (const target of machines) {
          const [x, y, z] = target.position;
          // Walkway beside each wall; the middle of the trainer room is a pit.
          const side = target.template_id === 1352 ? [0, 1.2]
            : target.template_id === 1355 ? [1.2, 0]
            : target.template_id === 1354 ? [0, -1.2] : [-1.2, 0];
          await game.player.teleport(machine === "trainer"
            ? { x: x + side[0]!, y: y + 0.5, z: z + side[1]! }
            : { x: x - 1.59, y: 21.404, z: z - 2.23 });
          await game.step({ frames: 30 });
          assert.equal((await game.ui.state()).mode, "shooter");
          await game.entities.sendMessage(target.id, { type: "Frob" });
          await game.step({ frames: 30 });
          const opened = await game.ui.state();
          assert.equal(opened.mode, "use", `${target.name} must open the full interface`);
          assert.ok(opened.strip);
          assert.equal(opened.active_panel?.entity_id, target.id, `${target.name} stays in range`);
          assert.ok(opened.active_panel.elements.some(e => e.kind === "button"));
          if (vr) {
            requirePanelPose(opened);
            assert.equal((await game.physics.bodies()).bodies.filter(b => b.collision_groups.includes("ui")).length, 0,
              "the machine must not also create a world panel");
          }
          // Frobbing again keeps the interface open instead of toggling it.
          await game.entities.sendMessage(target.id, { type: "Frob" });
          await game.step({ frames: 3 });
          assert.equal((await game.ui.state()).mode, "use");
          const exit = opened.readout.find(e => e.label === "close_use_mode");
          assert.ok(exit);
          if (vr) await clickCanvasWithRay(game, requirePanelPose(opened), canvasCenter(exit));
          else await clickUiElement(game, exit);
          assert.equal((await game.ui.state()).mode, "shooter");
          assert.equal((await game.ui.state()).active_panel, null);
        }
        if (vr) {
          await game.devParams.set("vr_mfd_focus_scan", 0);
          await drawPersonalCard(game, "left");
          await game.entities.sendMessage(machines.at(-1)!.id, { type: "Frob" });
          await game.step({ frames: 3 });
          const held = await game.ui.state();
          assert.equal(held.mode, "device", "a held MFD retains its machine panel");
          assert.equal(held.active_panel?.entity_id, machines.at(-1)!.id);
          assert.equal(held.strip, null);
        }
      });
  }
}
