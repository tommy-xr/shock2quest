import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, type UiElement } from "../src/index.js";
import { aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// Every use-mode interface button (bottom readouts + MFD utilities) shares one
// hover outline, reported by /v1/ui as `pointer.hovered`, in flat and VR.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) {
  test(
    `every use-mode button reports the hover outline (${vr ? "vr" : "flat"})`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "medsci1.mis",
        debugFlags: vr ? ["--vr"] : [],
      });
      await game.step({ frames: 5 });
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 5 });

      const pointAt = async (x: number, y: number) => {
        if (vr) {
          const pose = (await game.ui.state()).panel_pose;
          assert.ok(pose, "the VR interface must report its panel pose");
          await aimVrHandAtCanvas(game, pose, [x, y]);
        } else {
          await game.input.set("pointer.position", [x / 640, y / 480]);
        }
        await game.step({ frames: 2 });
        return (await game.ui.state()).pointer?.hovered ?? null;
      };

      const ui = await game.ui.state();
      const buttons: UiElement[] = [...ui.readout, ...ui.utilities].filter(
        (e) => e.kind === "button",
      );
      const labels = buttons.map((b) => b.label);
      for (const label of ["logs", "system_menu", "close_use_mode", "character_stats", "map"]) {
        assert.ok(labels.includes(label), `expected ${label} in ${JSON.stringify(labels)}`);
      }
      for (const button of buttons) {
        const [x, y, w, h] = button.rect;
        assert.deepEqual(await pointAt(x + w / 2, y + h / 2), button.rect, `hovering ${button.label}`);
      }
      assert.equal(await pointAt(320, 250), null, "no outline off every button");
    },
  );
}
