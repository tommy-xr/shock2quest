import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";

// Regression test for #431: re-entering a previously visited level rebuilds it
// from the JSON snapshot taken when leaving. earth.mis object 189 ("Grate
// 6x8") ships a P$Scale with an infinite component; serde_json stores that as
// null, and deserializing it used to panic the game-loop thread with
// "invalid type: null, expected f32". A fresh earth load was always fine -
// only the revisit path crashed.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function lootAuthoredItem(
  game: GameServer,
  containerTemplateId: number,
  itemName: string,
): Promise<void> {
  const containers = await game.entities.byTemplate(containerTemplateId);
  assert.equal(containers.length, 1, `expected container ${containerTemplateId}`);
  const container = containers[0];
  await teleportVerified(game, {
    x: container.position[0] + 1,
    y: container.position[1] + 0.5,
    z: container.position[2] + 1,
  });
  await game.entities.sendMessage(container.id, { type: "Frob" });
  await game.step({ frames: 5 });

  const panel = (await game.ui.state()).active_panel;
  assert.equal(panel?.entity_id, container.id, "container should open its loot MFD");
  const item = panel.elements.find(
    (element) => element.kind === "button" && element.label === itemName,
  );
  assert.ok(item, `container ${containerTemplateId} should contain ${itemName}`);
  await clickUiElement(game, item);

  const close = (await game.ui.state()).active_panel?.elements.find(
    (element) => element.label === "close",
  );
  if (close) {
    await clickUiElement(game, close);
  }
}

test(
  "revisiting earth rebuilds it from save data without crashing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });

    await game.step({ frames: 2 });
    assert.equal((await game.info()).mission, "earth.mis");

    // Leaving earth serializes its world into the in-memory save snapshot.
    const away = await game.transitionLevel("station");
    assert.equal(away.mission, "station.mis");

    // Coming back rebuilds earth from that snapshot - the crashing path.
    const back = await game.transitionLevel("earth");
    assert.equal(back.mission, "earth.mis");

    // The runtime is still live: step it and confirm a sane player state.
    await game.step({ frames: 30 });
    const pos = await game.player.position();
    assert.ok(
      Number.isFinite(pos.x) && Number.isFinite(pos.y) && Number.isFinite(pos.z),
      `player should have a finite position back on earth, got ${JSON.stringify(pos)}`,
    );
  },
);

test(
  "revisiting a mission preserves carried items whose saved IDs overlap mission IDs",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
    });

    await game.step({ frames: 2 });
    for (const desk of [675, 713]) {
      await lootAuthoredItem(game, desk, "Anti-Annelid Toxin");
    }
    assert.equal(
      (await game.player.inventory()).items.filter(
        (item) => item.name === "Anti-Annelid Toxin",
      ).length,
      2,
      "both authored Hydro 2 toxin vials should be carried before leaving",
    );

    const away = await game.transitionLevel("hydro1");
    assert.equal(away.mission, "hydro1.mis");
    const back = await game.transitionLevel("hydro2");
    assert.equal(back.mission, "hydro2.mis");

    await game.step({ frames: 30 });
    const carriedToxins = (await game.player.inventory()).items.filter(
      (item) => item.name === "Anti-Annelid Toxin",
    );
    assert.equal(carriedToxins.length, 2, "both toxin vials should survive the revisit");
  },
);
