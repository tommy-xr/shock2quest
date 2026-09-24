import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { AIM_AT_PANEL, clickMenuEntry, menuEntry, panelPoint } from "./helpers/frontend-menu.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const SURVIVE_INDEX = 4;

for (const vr of [false, true]) {
  test(
    `Survive launches earth_horde from the ${vr ? "VR" : "flat"} main menu`,
    { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
    async () => {
      await using game = await GameServer.launch({
        mission: "main_menu",
        debugFlags: vr ? ["--vr"] : [],
      });
      await game.step({ frames: 10 });
      assert.equal((await game.info()).mission, "main_menu");

      if (vr) {
        const [, y, z] = panelPoint(menuEntry(SURVIVE_INDEX));
        await game.input.set("right_hand.rotation", AIM_AT_PANEL);
        await game.input.set("right_hand.position", [0, y, z]);
        await game.step({ frames: 10 });
        await game.input.set("right_hand.trigger", 1);
        await game.step({ frames: 3 });
        await game.input.set("right_hand.trigger", 0);
        await game.step({ frames: 5 });
      } else {
        await clickMenuEntry(game, SURVIVE_INDEX);
      }

      assert.equal((await game.info()).mission, "earth_horde");
    },
  );
}
