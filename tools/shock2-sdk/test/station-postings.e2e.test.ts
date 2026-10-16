import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Walking up to each training room's doorway puts that room's mission posting
// on the HUD. The doorway traps author no text: ChooseMission gives each one
// this year's posting for the career, so with none assigned the walk-up was
// silent.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Doorway tripwire centres (station.mis 771, 772, 136), at the player's height.
const DOORWAYS = [
  { x: 1.16, y: -6.0, z: 8.29, posting: "Ordnance Master's Mate" },
  { x: 1.22, y: -6.0, z: 0.22, posting: "Survival Training" },
  { x: 0.93, y: -6.0, z: -7.85, posting: "Zero-G Training" },
];

test(
  "station: each doorway shows its room's mission posting",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "station.mis" });
    await game.step({ frames: 10 });

    // Postings are assigned at level start for the career, so reload as a Marine.
    await game.quests.set("career_marine", "complete");
    await game.input.trigger("DebugReloadLevel");
    await game.step({ frames: 60 });

    for (const { posting, ...position } of DOORWAYS) {
      await game.player.teleport(position);
      await game.step({ frames: 10 });
      const { messages } = await game.ui.state();
      assert.ok(
        messages.some((m) => m.startsWith("Mission Posting:") && m.includes(posting)),
        `expected the "${posting}" posting, HUD showed ${JSON.stringify(messages)}`,
      );
    }
  },
);
