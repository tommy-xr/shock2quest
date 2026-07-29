import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "crouched flat interaction collects SHODAN Delacroix log 4",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8231),
    });
    await game.step({ frames: 5 });

    const log = (await game.entities.list({ filter: "Audio Log", limit: 80 }))
      .entities.find((entity) => entity.template_id === 290);
    assert.ok(log, "shodan should contain Delacroix deck 9/log 4");

    // Setup only: reproduce the campaign's stable crouched landing on the
    // authored hidden platform. Runtime entity ids remain launch-local.
    await game.input.set("crouch", 1);
    await game.step({ frames: 5 });
    await teleportVerified(game, {
      x: 31.9264,
      y: -27.315,
      z: 28.5662,
    });
    await game.step({ frames: 30 });

    const aim = await game.player.aimAt(log, {
      hitbox: "surface",
      visibility: "required",
    });
    assert.ok(
      aim.target_confirmed,
      `production camera ray should select log 4 (${JSON.stringify(aim)})`,
    );

    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });

    assert.ok(
      (await game.info()).player.collected_logs.some(
        (entry) => entry.deck === 9 && entry.log === 4,
      ),
      "production squeeze should collect Delacroix deck 9/log 4",
    );
  },
);
