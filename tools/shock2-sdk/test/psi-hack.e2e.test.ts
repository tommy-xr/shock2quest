import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type UiElement } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";
import { clickUiElement, clickCanvasWithRay, requirePanelPose } from "./helpers/ui.js";

for (const mode of ["flat", "left", "right"] as const) {
  const vr = mode !== "flat";
  const hand = mode === "left" ? "left" : "right";
  test(`psionic hacking opens and pays a real board, then hacks the target (${mode})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 30 });
    if (vr) {
      const [amp] = await game.entities.byTemplate(-247);
      await aimVrHandAt(game, amp.position, .35, 1, 0, { hand });
      await game.step({ frames: 8 });
      assert.equal((await game.info()).player[hand === "left" ? "wielded_entity_id" : "right_hand_entity_id"], amp.id);
    }
    const [target] = await game.entities.byTemplate(-1886);
    assert.ok(target);
    const state = async () => (await game.entities.detail(target.id)).properties.find(p => p.name === "ObjectState")?.value;
    assert.equal(await state(), "Locked");
    await selectPsiPower(game, "CyberHack");
    if (vr) await aimVrHandAt(game, target.position, 2, 1, 0, { hand });
    else await game.input.lookAtWorldPoint(target.position);
    const psi = (await game.info()).player.psi_points!;
    const nanites = await carriedNaniteTotal(game);
    const pointerHand = hand === "left" ? "right" : "left";
    if (vr) await game.input.set(`${pointerHand}_hand.trigger`, 1);
    await pullTrigger(game, hand);
    await game.step({ frames: 3 });
    let panel = (await game.ui.state()).active_panel;
    assert.equal(panel?.name, "Psionic Hacking");
    assert.equal(await state(), "Locked", "casting opens the board without granting success");
    assert.equal((await game.info()).player.psi_points, psi - 4);
    assert.match(panel!.elements.map(e => e.text ?? "").join("\n"), /PSI skill 3/);
    if (vr) {
      const start = panel!.elements.find(e => e.label === "start-hack")!;
      const pose = requirePanelPose(await game.ui.state());
      await aimVrHandAtCanvas(game, pose, [start.rect[0]+start.rect[2]/2, start.rect[1]+start.rect[3]/2], { hand: pointerHand, trigger: 1 });
      await game.step({ frames: 3 });
      assert.equal((await game.info()).player.psi_points, psi - 4, "held offhand trigger at entry must not press START");
      assert.equal((await game.ui.state()).active_panel?.name, "Psionic Hacking", "held trigger must not frob the console behind the board");
      await game.input.set(`${pointerHand}_hand.trigger`, 0);
      await game.step({ frames: 3 });
      assert.equal((await game.ui.state()).active_panel?.name, "Psionic Hacking", "held-entry gesture and release must preserve the board");
    }
    async function click(el: UiElement) {
      if (vr) {
        const ui = await game.ui.state();
        await clickCanvasWithRay(game, requirePanelPose(ui), [el.rect[0]+el.rect[2]/2, el.rect[1]+el.rect[3]/2], hand === "left" ? "right" : "left");
      } else await clickUiElement(game, el);
    }
    let attempts = 0;
    for (; attempts < 8 && await state() === "Locked"; attempts++) {
      panel = (await game.ui.state()).active_panel;
      assert.ok(panel);
      const start = panel.elements.find(e => e.label === "start-hack" || e.label === "reset-hack")!;
      assert.ok(start);
      const before = (await game.info()).player.psi_points!;
      await click(start);
      assert.equal((await game.info()).player.psi_points, before - 5, "START spends full authored cost in PSI");
      assert.equal(await carriedNaniteTotal(game), nanites, "psionic START never spends nanites");
      for (let y = 0; y < 4 && await state() === "Locked"; y++) {
        for (let x = 0; x < 5 && await state() === "Locked"; x++) {
          panel = (await game.ui.state()).active_panel;
          assert.ok(panel);
          assert.ok(!panel.elements.some(e => e.texture === "payh.pcx"));
          const node = panel.elements.find(e => e.label === `node-${x}-${y}`);
          if (!node || panel.elements.some(e => ["hrmmine.pcx","hrmburn.pcx","hrmon.pcx"].includes(e.texture ?? "") && e.rect[0] === node.rect[0] && e.rect[1] === node.rect[1])) continue;
          await click(node);
        }
      }
    }
    assert.equal(await state(), "Hacked");
    assert.equal((await game.ui.state()).active_panel, null, "successful hack dismisses projection");
    const after = (await game.info()).player.psi_points;
    await pullTrigger(game, hand);
    assert.equal((await game.info()).player.psi_points, after, "already-hacked target costs nothing");
    const [securityComputer] = await game.entities.byTemplate(-1250);
    async function openConsole() {
      if (vr) await aimVrHandAt(game, securityComputer.position, 2, 1, 0, { hand });
      else await game.input.lookAtWorldPoint(securityComputer.position);
      await pullTrigger(game, hand);
      await game.step({ frames: 3 });
      assert.equal((await game.ui.state()).active_panel?.name, "Psionic Hacking");
    }
    await openConsole();
    const close = (await game.ui.state()).active_panel!.elements.find(e => e.label === "close")!;
    assert.ok(close, "projection includes the ordinary visible close button");
    await click(close);
    await game.step({ frames: 2 });
    assert.equal((await game.ui.state()).active_panel, null, "close cancels without hacking");
    await openConsole();
    if (vr) await game.input.set(`${hand}_hand.squeeze`, 0);
    else await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    assert.equal((await game.ui.state()).active_panel, null, "amp loss or cyber-interface entry dismisses the board");
  });
}
