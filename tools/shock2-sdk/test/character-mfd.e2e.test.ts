import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement, requirePanelPose } from "./helpers/ui.js";
import { aimVrHandAtCanvas } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`character MFD and access cards: ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1",
    timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: vr ? ["--vr"] : [],
    });
    await game.player.setStats({ strength: 4, skills: { hack: 3, standard_weapons: 2 } });
    await game.step({ frames: 10 });
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 3 });

    const clickUtility = async (label: string) => {
      const ui = await game.ui.state();
      const element = ui.utilities.find((e) => e.label === label);
      assert.ok(element, `missing ${label}`);
      if (vr) {
        await clickCanvasWithRay(game, requirePanelPose(ui), canvasCenter(element));
      } else {
        await clickUiElement(game, element);
      }
    };
    const renderedText = async () => (await game.ui.state()).utilities
      .filter((e) => e.kind === "text").map((e) => e.text).join(" ");

    await clickUtility("character_stats");
    assert.match(await renderedText(), /STRENGTH/);
    const stats = (await game.ui.state()).utilities;
    assert.equal(stats.filter((e) => e.texture?.toLowerCase().includes("skilstat")).length, 8,
      "four strength and four baseline attributes must render eight chevrons");

    // Hover STR using each presentation's production pointer path.
    if (vr) {
      await aimVrHandAtCanvas(game, requirePanelPose(await game.ui.state()), [520, 142]);
    } else {
      await game.input.set("pointer.position", [520 / 640, 142 / 480]);
    }
    await game.step({ frames: 2 });
    assert.match(await renderedText(), /minimum STR requirement/,
      "complete authored strength help must fit in its native box");

    await clickUtility("character_tab_1");
    assert.match(await renderedText(), /HACK.*REPAIR.*MODIFY.*MAINTAIN.*RESEARCH/);
    await clickUtility("character_tab_2");
    assert.match(await renderedText(), /STANDARD.*ENERGY.*HEAVY.*EXOTIC/);
    await clickUtility("character_tab_3");
    const selectedBefore = (await game.info()).player.selected_psi_power;
    for (let tier = 1; tier <= 5; tier++) {
      await clickUtility(`psi_tier_${tier}`);
      const power = (await game.ui.state()).utilities.find((e) => e.label?.startsWith("psi_power_"));
      assert.ok(power, `tier ${tier} must show its available power cells`);
      await clickUtility(power.label!);
    }
    assert.equal((await game.info()).player.selected_psi_power, selectedBefore,
      "browsing the sheet must not change the equipped amp's selection");

    await clickUtility("access_cards");
    assert.match(await renderedText(), /No access cards collected/);
    // Provision an authentic credential, then use its production Frob script.
    // The panel reads the resulting keyring, not a debug-only state override.
    const card = await game.player.spawnItem(-1495); // Hydro Card B archetype
    await game.entities.sendMessage(card.entity_id, { type: "Frob" });
    await game.step({ frames: 3 });
    assert.match(await renderedText(), /Hydroponics B/);
    assert.doesNotMatch(await renderedText(), /No access cards/);
    await clickUtility("utility_close");
    assert.ok(!(await game.ui.state()).utilities.some((e) => e.label === "utility_close"));
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });
    assert.equal((await game.ui.state()).utilities.length, 0,
      "cyber-interface controls must disappear in shooter mode");
  });
}
