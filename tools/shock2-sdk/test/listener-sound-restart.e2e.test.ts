import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { physicallyOpenEarthReplicator } from "./helpers/earth-replicator.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";
import { clickUiElement } from "./helpers/ui.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable authored Earth objects: a 250-nanite pile and the replicator.
const EARTH_NANITES = 257;
const EARTH_REPLICATOR = 262;

// A second vend restarts the replicator's "thank you" line rather than
// stacking a second copy over the first.
test(
  "a repeated non-spatial sound restarts rather than overlapping",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis" });
    await game.step({ frames: 5 });

    const [nanites] = await game.entities.byTemplate(EARTH_NANITES);
    const [replicator] = await game.entities.byTemplate(EARTH_REPLICATOR);
    assert.ok(nanites && replicator, "Earth should author the nanites and replicator");
    await earthWorldUse(game, nanites);
    await physicallyOpenEarthReplicator(game, replicator);

    for (let vend = 0; vend < 2; vend++) {
      const panel = (await game.ui.state()).active_panel;
      const chips = panel?.elements.find(
        (element) => element.kind === "button" && element.label === "buy:chips",
      );
      assert.ok(chips, "the replicator panel should offer chips");
      await clickUiElement(game, chips);
      await game.step({ frames: 10 });
    }

    const thanks = (await game.audio.recent({ sample: "replic2e" })).sounds;
    assert.equal(thanks.length, 2, `expected two vend lines, got ${JSON.stringify(thanks)}`);
    assert.equal(thanks[0]!.still_playing, false, "the second vend stops the first line");
    assert.notEqual(thanks[0]!.stopped_at_sim_time, null);
    assert.equal(thanks[1]!.still_playing, true);
  },
);
