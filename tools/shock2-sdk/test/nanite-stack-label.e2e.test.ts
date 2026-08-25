import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { stackCount } from "./helpers/nanites.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "Earth nanite world-label visual smoke preserves the authored stack",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });
    await game.step({ frames: 5 });

    // Earth mission object 257 is the Technical Training nanite supply.
    const pile = (await game.entities.list()).entities.find(
      (entity) => entity.template_id === 257,
    );
    assert.ok(pile, "expected Earth mission object 257 (Big Nanite Pile)");

    const before = await game.entities.detail(pile.id);
    assert.equal(stackCount(before.properties), 250);

    // The fresh Earth spawn faces opposite the Technical lesson. Stage two
    // units west of the pile and turn the head 270 degrees; twenty degrees down
    // centers the physical nanite canister against a clear wall.
    // Do not squeeze: highlighting renders the label without picking it up.
    const [x, y, z] = before.position;
    await game.player.teleport({ x: x - 2.0, y: y - 1.2, z });
    await game.input.set("head.look", [270, 20]);
    await game.step({ frames: 2 });

    // This is a real-mission render smoke and artifact capture, not a
    // pixel/OCR assertion. The negative-first semantic assertion that `%d`
    // becomes `250` lives in item_outline.rs, beside the formatter used here.
    const shot = await game.screenshot("earth-nanite-stack-label.png");
    assert.deepEqual(shot.resolution, [800, 600]);
    assert.ok(shot.size_bytes > 10_000);

    const after = await game.entities.detail(pile.id);
    assert.equal(
      stackCount(after.properties),
      250,
      "rendering the stack-aware label must not alter pickup quantity",
    );
  },
);
