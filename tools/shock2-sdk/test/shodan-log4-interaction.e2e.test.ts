import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "crouched flat interaction collects SHODAN Delacroix log 4, and reads it back",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
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

    const staged = await game.info();
    assert.ok(
      Math.abs(staged.player.camera_offset[1] - 0.48) < 1e-5,
      `debug runtime should report the live crouched eye (got ${staged.player.camera_offset[1]})`,
    );
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

    const collected = (await game.info()).player.collected_logs;
    assert.ok(
      collected.some((entry) => entry.deck === 9 && entry.log === 4 && !entry.read),
      "production squeeze should collect Delacroix deck 9/log 4, unread",
    );
    // Collecting files the log; it does not raise the reader over the world.
    assert.ok(
      !(await game.ui.state()).active_panel,
      "collecting a log must not open the reader",
    );

    // The disc is consumed by collection, so the world pickup is gone.
    assert.equal(
      (await game.physics.bodies({ entityId: log.id })).bodies.length,
      0,
      "the collected disc should lose its world presence",
    );

    // Reading it back is an explicit act: the newest unread log opens, and is
    // marked read.
    await game.input.trigger("ReadLastUnreadLog");
    await game.step({ frames: 5 });
    assert.equal(
      (await game.ui.state()).active_panel?.template_id,
      290,
      "ReadLastUnreadLog should open Delacroix log 4 in the reader",
    );
    assert.ok(
      (await game.info()).player.collected_logs.some(
        (entry) => entry.deck === 9 && entry.log === 4 && entry.read,
      ),
      "playing the log back should mark it read",
    );

    // And Tab closes it again, as it does any MFD panel.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    assert.ok(
      !(await game.ui.state()).active_panel,
      "Tab should dismiss the reader",
    );

    t.diagnostic(
      `SHODAN log 4: camera ${JSON.stringify(staged.player.camera_offset)}, ` +
        `aim target ${aim.interaction_target_id}, collected 9/4`,
    );
  },
);
